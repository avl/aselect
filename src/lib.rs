#![no_std]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

//! # aselect
//! Wait on multiple branches, without canceling or starving any futures, while allowing
//! safe access to mutable state. Works in `#[no_std]`, allocates no memory, and has no
//! non-optional dependencies. Tested with miri.
//!
//! ## Background
//!
//! This crate implements [`aselect`], a safer alternative to the tokio `select!`-macro.
//! By using `aselect`, it becomes possible to avoid cancelling futures during normal
//! operations, eliminating a class of bugs. See the excellent RFD 400 from Oxide
//! for a great overview of cancellation safety in rust:
//! <https://rfd.shared.oxide.computer/rfd/400> . `aselect` also avoids the "FutureLock"
//! class of bugs, described (also by Oxide) at <https://rfd.shared.oxide.computer/rfd/0609>,
//! because it doesn't allow async code in handlers (only in the actual concurrent arms).
//!
//! ### Comparison with tokio::select
//! The regular `select!` macro from tokio is very useful, but it has two properties that can
//! be error-prone:
//! * As soon as one select arm completes, all other arms are canceled. Many futures are
//!   not cancellation safe (e.g. `tokio::sync::mpsc::Sender::send`).
//! * When an arm has completed, while the handler is executing, other arms are no longer
//!   polled. This can lead to starvation when `select!` is used in a loop.
//!
//! In contrast to `select!`, aselect has these differences:
//!  * It implements `futures::Stream`, meaning it can be polled multiple times.
//!    When polled repeatedly, it never implicitly cancels any futures; arms are polled until they
//!    become ready. It also implements `core::future::Future`.
//!  * When polled, it *always* polls all active arms.
//!  * It has a different syntax (that allows it to be formatted by rustfmt).
//!
//! ## Tips
//!  * Both the setup and handler blocks return `Option`. This means the `?` operator can be
//!    used in them.
//!  * Use the `std::pin::pin!`-macro to pin the `aselect!` expression when using it as a
//!    `futures::Stream`.
//!
//! ## Implementation
//! [`aselect`] works by creating a set of structs that implement a state machine.
//! Each select arm is its own struct, and consists of two closures and a stored future.
//! One of the closures creates the future, and the other decides if the result of a future
//! should cause `aselect` itself to produce a value.
//!
//! `aselect` does not allocate memory on the heap.
//!
//!

use core::cell::UnsafeCell;
use core::fmt::{Debug, Formatter};
use core::marker::PhantomData;
use core::ptr::null_mut;

#[cfg(feature = "futures")]
pub use futures::Stream;

#[cfg(feature = "std")]
extern crate std;

#[cfg(all(feature = "std", test))]
mod tests;

#[doc(hidden)]
pub trait SelectArm<'a, CTX, TOut> {
    /// Returns Some if user code was run
    /// If it was ready, it may have produced a value `Some(Some(_))` or not `Some(None)`.
    /// It is guaranteed that if this method has run user-code, it returns Some.
    /// If the future was not ready, and no user code was run, `None` is returned.
    fn do_poll(&mut self, ctx: &'a CTX, cx: &mut ::core::task::Context<'_>, canceler: &mut Canceler) -> PollResult<TOut>;
    fn cancel(&mut self);
}

#[doc(hidden)]
pub struct UnsafeCapture<'a, T: 'a> {
    value: UnsafeCell<T>,
    phantom: PhantomData<&'a ()>,
}

impl<'a, T: 'a> UnsafeCapture<'a, T> {
    pub fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            phantom: PhantomData,
        }
    }
    /// # Safety
    /// The underlying captured value must still be alive, and
    /// must be mutably accessible without causing aliasing.
    pub unsafe fn access(&self) -> UnsafeCaptureAccess<T> {
        UnsafeCaptureAccess {
            value: self.value.get(),
        }
    }
}

#[doc(hidden)]
pub struct LockedCapture<'a, T: 'a> {
    locks: UnsafeCell<bool>,
    value: UnsafeCell<T>,
    phantom: PhantomData<&'a ()>,
}
impl<'a, T: Debug> Debug for LockedCapture<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> ::core::fmt::Result {
        write!(f, "Borrowed()")
    }
}
impl<'a, T: 'a> LockedCapture<'a, T> {
    pub fn new(value: T) -> Self {
        Self {
            locks: UnsafeCell::new(false),
            value: UnsafeCell::new(value),
            phantom: PhantomData,
        }
    }
    /// # Safety
    /// The underlying captured value must still be alive, and
    /// must be mutably accessible without causing aliasing, and
    /// also no concurrent access to the lock must be allowed.
    pub unsafe fn access(&self) -> CaptureAccess<T> {
        // SAFETY:
        // No concurrent access to lock, guaranteed by caller
        if unsafe { !*self.locks.get() } {
            CaptureAccess {
                value: self.value.get(),
            }
        } else {
            CaptureAccess { value: null_mut() }
        }
    }
}

#[doc(hidden)]
pub struct CaptureAccess<T> {
    value: *mut T,
}

impl<T> CaptureAccess<T> {
    /// # Safety
    /// The underlying captured value must still be alive, and
    /// must be mutably accessible without causing aliasing.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self) -> Option<&'_ mut T> {
        if self.value.is_null() {
            None
        } else {
            // SAFETY:
            // Caller guarantees captured value is still alive
            Some(unsafe { &mut *self.value })
        }
    }
}

#[doc(hidden)]
pub struct UnsafeCaptureAccess<T> {
    value: *mut T,
}

impl<T> UnsafeCaptureAccess<T> {
    /// # Safety
    /// The underlying captured value must still be alive, and
    /// must be mutably accessible without causing aliasing.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self) -> &'_ mut T {
        // SAFETY:
        // Caller guarantees captured value is still alive
        unsafe { &mut *self.value }
    }
}

#[doc(hidden)]
pub struct CaptureGuard<'a, T> {
    lock: &'a UnsafeCell<bool>,
    #[doc(hidden)]
    value: *mut T,
}

#[doc(hidden)]
pub struct ConstantCapture<'a, T> {
    value: &'a T,
    // We need invariant variance, otherwise lifetime extension
    // will allow some unsound code.
    _variance: *mut T
}

impl<'a, T:'a> ConstantCapture<'a, T> {
    #[doc(hidden)]
    pub fn new(value: &'a T) -> Self {
        Self {
            value,
            _variance: null_mut(),
        }
    }
    pub fn const_access<'b>(&'b self) -> &'b T  where 'a: 'b {
        self.value
    }
}

impl<'a, T> Debug for CaptureGuard<'a, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> ::core::fmt::Result {
        write!(f, "CaptureGuard({:?})", self.value)
    }
}

impl<'a, T> CaptureGuard<'a, T>
where
    T: Debug,
{
    /// # Safety
    /// The underlying captured value must still be alive, and
    /// must be mutably accessible without causing aliasing.
    pub unsafe fn get_mut(&mut self) -> &mut T {
        // SAFETY:
        // Caller guarantees captured value is still alive
        unsafe { &mut *self.value }
    }
}

impl<'a, T: 'a> LockedCapture<'a, T> {
    /// # Safety
    /// The underlying captured value must still be alive, and
    /// must be mutably accessible without causing aliasing, and
    /// also no concurrent access to the lock must be allowed.
    /// Lock must only be accessed from this thread, and must
    /// stay alive for as long as `CaptureGuard` stays alive.
    #[doc(hidden)]
    pub unsafe fn lock(&self) -> Option<CaptureGuard<'_, T>> {
        // SAFETY:
        // Caller guarantees locks is not aliased.
        let locks = unsafe { &mut *self.locks.get() };
        if *locks {
            return None;
        }
        *locks = true;
        Some(CaptureGuard {
            lock: &self.locks,
            // SAFETY:
            // Caller guarantees captured value is still alive
            value: unsafe { &mut *(self.value.get()) },
        })
    }
}

impl<T> Drop for CaptureGuard<'_, T> {
    fn drop(&mut self) {
        // SAFETY:
        // CaptureGuard instances are only creatable in this module, and are only created
        // by `LockedCapture::lock`. This method guarantees `lock` stays alive.
        unsafe { *self.lock.get() = false }
    }
}

#[doc(hidden)]
pub trait SafeResult {
    type Output;
    /// Some(Some(x)) - return value
    /// Some(None) - end stream
    /// None - pending
    fn result(self) -> Option<Option<Self::Output>>;
}

/// Marker type to signal that the stream is to be terminated.
pub struct Terminate<R>(PhantomData<R>);


#[doc(hidden)]
pub enum Output<R> {
    Pending,
    Value(R),
    Terminate
}


/// Terminate the stream
pub fn terminate<R>() -> Output<R> {
    Output::Terminate
}

#[doc(hidden)]
pub fn result<T: SafeResult>(input: Option<T>) -> Option<Option<T::Output>> {
    input?.result()
}
impl<R> SafeResult for Option<R> {
    type Output = R;

    fn result(self) -> Option<Option<Self::Output>> {
        Some(Some(self?))
    }
}
impl<R> SafeResult for Output<R> {
    type Output = R;

    fn result(self) -> Option<Option<Self::Output>> {
        match self {
            Output::Pending => None,
            Output::Value(v) => Some(Some(v)),
            Output::Terminate => Some(None),
        }

    }
}
impl SafeResult for () {
    type Output = ();
    fn result(self) -> Option<Option<Self::Output>> {
        None
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! borrowed_captures0 {
    ( $temp: ident, $($cap: ident,)*) => {
        $(
            // SAFETY:
            // Only called from inside `aselect`-macro, in closures that live shorter
            // than captures.
            // From safety perspective, we do not protect against users calling this
            // hidden macro manually.
            let $cap = unsafe { $temp.$cap.access()};
        )*
    };
}
#[doc(hidden)]
#[macro_export]
macro_rules! borrowed_captures1 {
    ( $($cap: ident,)*) => {
        $(
            // SAFETY:
            // Only called from inside `aselect`-macro, in closures that live shorter
            // than captures.
            // From safety perspective, we do not protect against users calling this
            // hidden macro manually.
            let mut $cap = unsafe { $cap.get() };
        )*
    };
}
#[doc(hidden)]
#[macro_export]
macro_rules! cancelers {
    ( $canceler: ident, $($arm_name: ident,)*) => {
        let mut i = 0;
        $(
            // SAFETY:
            // Only called from inside `aselect`-macro, in closures that live shorter
            // than canceler.
            // From safety perspective, we do not protect against users calling this
            // hidden macro manually.
            let mut $arm_name = unsafe { $crate::CancelerWrapper::new($canceler, i) };
            i+=1;
        )*
    };
}
#[doc(hidden)]
#[macro_export]
macro_rules! mutable_captures0 {
    ( $temp: ident, $($cap: ident,)*) => {
        $(
            // SAFETY:
            // Only called from inside `aselect`-macro, in closures that live shorter
            // than captures.
            // From safety perspective, we do not protect against users calling this
            // hidden macro manually.
            let $cap = unsafe { $temp.$cap.access()};
        )*
    };
}
#[doc(hidden)]
#[macro_export]
macro_rules! mutable_captures1 {
    ( $($cap: ident,)*) => {
        $(
            // SAFETY:
            // Only called from inside `aselect`-macro, in closures that live shorter
            // than captures.
            // From safety perspective, we do not protect against users calling this
            // hidden macro manually.
            let mut $cap = unsafe { $cap.get() };
        )*
    };
}

/// Marker type that is bound to mutable captures in the context of
/// async blocks.
///
/// This makes it clear that mutable captured variables cannot be
/// accessed from within an async block.
///
/// The reason for this limitation is that multiple async blocks can execute
/// concurrently, and the semantics of mutable references in rust forbid concurrent
/// access.
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub struct MutableValueUnavailableInThisAsyncContext;

#[doc(hidden)]
#[macro_export]
macro_rules! mutable_captures2 {
    ( $($cap: ident,)*) => {
        $(
            let mut $cap = $crate::MutableValueUnavailableInThisAsyncContext;
        )*
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! constant_captures0 {
    ( $temp: ident, $($cap: ident,)*) => {
        $(
            let $cap = $crate::ConstantCapture::new(&$temp.$cap);
        )*
    };
}
#[doc(hidden)]
#[macro_export]
macro_rules! constant_captures1 {
    ( $($cap: ident,)*) => {
        $(
            // SAFETY:
            // Only called from inside `aselect`-macro, in closures that live shorter
            // than captures.
            // From safety perspective, we do not protect against users calling this
            // hidden macro manually.
            let $cap = $cap.const_access();
        )*
    };
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "futures")]
macro_rules! define_stream_impl {
    ($($name:ident),*) => {
            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> $crate::Stream for ASelectImpl<'a, TCap, TOut, $($name),*> where
                $($name: $crate::SelectArm<'a, TCap, TOut> ,)*
            {
                type Item = TOut;
                fn poll_next(self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>) -> ::core::task::Poll<Option<Self::Item>> {
                    match self.poll_stream_impl(cx) {
                        ::core::task::Poll::Ready(val) => ::core::task::Poll::Ready(val),
                        _ => ::core::task::Poll::Pending,
                    }
                }
            }

    };
}

#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "futures"))]
macro_rules! define_stream_impl {
    ($($name:ident),*) => {};
}

#[doc(hidden)]
pub enum PollResult<T> {
    Result(T),
    Pending(bool/*future created*/),
    /// Future ran to completion, but no output value was produced
    Inhibited,
    Disabled,
    EndStream
}


#[doc(hidden)]
#[derive(Debug)]
pub struct Canceler {
    #[doc(hidden)]
    pub canceled: UnsafeCell<u64>,
}
impl Canceler {
    #[doc(hidden)]
    pub fn new() -> Canceler {
        Canceler { canceled: UnsafeCell::new(0) }
    }

    /// # Safety
    /// No concurrent access must occur
    #[doc(hidden)]
    pub unsafe fn any(&self) -> bool {
        // Safety:
        // Caller guarantees no concurrent access
        (unsafe {*self.canceled.get()}) != 0
    }

    /// # Safety
    /// No concurrent access must occur
    #[doc(hidden)]
    pub unsafe fn canceled(&self, i: u32) -> bool {
        // Safety:
        // Caller guarantees no concurrent access
        (unsafe {*self.canceled.get()} & (1<<i)) != 0
    }

    /// Cancel the select arm with the given index.
    ///
    /// The first arm has index 0, arms are then numbered consecutively.
    ///
    /// # Safety
    /// The Canceler must not be concurrently accessed.
    pub unsafe fn cancel(&self, index: u32) {
        if index >= 64 {
            panic!("aselect only supports canceling the first 64 arms of a aselect invocation.");
        }
        // Safety:
        // Caller guarantees no concurrent access
        let val = unsafe { &mut *self.canceled.get()};
         *val |=  1 << index;
    }
}


#[doc(hidden)]
pub struct CancelerWrapper<'a>  {
    canceler: &'a Canceler,
    index: u32,
}



impl Debug for CancelerWrapper<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> ::core::fmt::Result {
        write!(f, "Canceler")
    }
}

impl CancelerWrapper<'_> {
    // # Safety
    // Canceler must not be accessed from other threads
    pub unsafe fn new(canceler: &Canceler, index: u32) -> CancelerWrapper<'_> {
        CancelerWrapper { canceler, index }
    }
    pub fn cancel(&mut self) {
        // Safety:
        // CancelerWrapper not constructable in safe code. All constructors
        // promise that `self.canceler` is not accessed concurrently.
        unsafe {
            self.canceler.cancel(self.index);
        }
    }
}


/// Evaluate multiple different async operations concurrently.
///
/// Example:
/// ```rust
/// use aselect::aselect;
/// # use tokio::time::{sleep, Duration, Instant};
///
/// # #[tokio::main]
/// # async fn main() {
/// let counter = 0u32;
/// let result = aselect!(
///     {
///         // Capture variable 'counter'
///         mutable(counter);
///     },
///     // First select arm. A unique name for each arm must be provided (`timer1`).
///     // Sleeps 0.3 seconds, over and over.
///     timer1(
///         { // Setup
///
///             // Print value of counter, then increment
///             println!("Counter = {:?}", counter);
///             *counter += 1;
///             // Create a future. Will be available to async block below.
///             sleep(Duration::from_millis(300))
///         },
///         async |sleep| { // Async block
///             // 'sleep' is the future created above
///             let sleep_start = Instant::now();
///             sleep.await;
///             // Value returned by this async block is given to block below
///             sleep_start.elapsed()
///         },
///         |time_slept| { // Handler
///             // Print value returned from future
///             println!("Slept {:?}", time_slept);
///             // Do not produce a result from the 'aselect' future.
///             None
///         }
///     ),
///     // Second select arm.
///     // Sleeps 1 second, then produces a value.
///     timer2(
///         { // Setup
///             // Similar to above, but now sleep 10 seconds
///             tokio::time::sleep(tokio::time::Duration::from_secs(1))
///         },
///         async |sleep| { // Async block
///             sleep.await;
///         },
///         |time_slept| { // Handler
///             println!("Timer 2 done");
///             // After the 10 seconds have elapsed,
///             Some("finished")
///         }
///     ),
/// ).await;
/// println!("Produced value: {}", result);
/// # }
/// ```
/// The above prints:
/// ```plaintext
/// Counter = 0
/// Slept 300ms
/// Counter = 1
/// Slept 300ms
/// Counter = 2
/// Slept 300ms
/// Counter = 3
/// Timer 2 done
/// Produced value: finished
/// ```
///
/// ## Enabling/disabling arms
/// The setup block returns an Option. The whole setup expression is invisibly wrapped in
/// `Some(..)`, so this is not immediately obvious. By returning `None` from the setup block,
/// the arm can be disabled.
///
/// ## Captures
/// Variables can be captured for use within the three blocks: setup, async_block, and
/// handler.
///
/// There are three types of capture:
///
///  * `constant`: Capture is available in all three blocks, for every arm. Captured variable
///    is immutable.
///  * `mutable`: Capture is available in setup and handler only. Captured variable is mutable.
///  * `borrowed`: Capture is available in all three blocks. The variable can be borrowed by
///    exactly one async_block (at any instant). In setup and handler, borrowed variables
///    are wrapped in an `Option`. If a variable is currently borrowed by another async block,
///    the `Option` is `None`. When a `borrowed` variable is used in an async block, the block
///    will not run if the variable is currently borrowed by another async block.
///
/// All these capture mechanisms always take ownership. Ownership is retained in the
/// select object. At present, there is no built-in way to move the captured variables out of the
/// object.
///
/// # Data flow
/// The setup block expression has access to all captures. The value it evaluates to is
/// forwarded as the first input to the async block. The async block is evaluated, and when it
/// becomes ready, its value is provided to the handler.
///
/// The async block always requires at least one parameter. Each async block can
/// capture an arbitrary number of `borrowed` capture variables by listing them as further
/// parameters, after the initial 'async_input' parameter.
///
/// # Canceling arms
/// While `aselect` never automatically cancels arms (unless the whole object is dropped),
/// arms can still be canceled explicitly. The syntax for this is slightly obscure:
///
/// ```ignore
/// timer1.cancel(1);
/// ```
/// The above call will cancel the arm with index 1. The indexing starts at 0.
/// In the example above, this would be the second arm, the one labeled `timer2`.
/// Canceled arms will immediately restart, unless their setup code disables them.
///
/// NOTE! It would be nice if a syntax like `timer1.cancel()` could be used.
///
/// # Cancellation Safety
/// Dropping the `aselect` object drops all captured variables and any currently executing
/// futures.
///
/// Note, `aselect` objects can be polled multiple times. Using `aselect` in a
/// tokio `select!` arm is fine and will not cause any futures to be canceled (unless the
/// select!-macro takes ownership of `aselect` and thus drops it on cancellation).
///
/// # Precise semantics of the aselect state machine
/// Every time the `aselect` macro object is polled, the following is performed:
/// * Each arm is visited in order (top to bottom)
/// * For each arm:
///   * If a future does not exist:
///     * Evaluate the `setup` block.
///     * If this results in a future: Store the future.
///   * Poll any stored future
///     * If the future is ready, and produced a value: Return the value to the callee.
///     * If the future is pending: Disable the arm for the duration of this poll.
/// * Repeat above for each arm until any of the following conditions are satisfied:
///   * No setup code block produced a new future, and no future completed in this iteration
///     (these conditions exist to ensure any side effects of creating a new future or completing
///     a future are visible to other setup blocks).
///   * The loop has run for more than 10 iterations.
///
///
/// It is expected that user code normally satisfies the condition within one or two
/// iterations. A failure to do so is possibly a programming error: futures keep being ready
/// without producing any output.
///
/// If all arms have been disabled, the future will be pending forever. Since no waker
/// has been registered in this case, the future might never be polled again.
///
/// If the iteration limit has been reached, the poll context waker is awoken,
/// and the poll returns pending. This makes sure `aselect` does not hang the async
/// runtime. In this condition the current CPU core will be occupied 100%, which may be
/// undesirable. However, it's possible that this is desired behavior: It would happen,
/// for example, if `aselect` is used to copy data between two async streams, and
/// both streams are fast enough that all async operations complete immediately.
///
/// # Pitfalls
/// Some things to watch out for:
///  * Make sure at least one arm always yields a pending future. Disabling all arms will
///    sleep forever, which is likely a programming error.
///  * If multiple futures attempt to borrow the same capture of type `borrowed`,
///    only one of them will actually be constructed. The other(s) will be disabled.
///
///
/// # Panics
/// `aselect` does not itself panic. However, user-provided code blocks (setup,
///  async_block, and handler) can panic. Such panics will unwind out of the aselect
///  poll method. Unless the panic is caught at a higher level, of course, the
/// `aselect` object is likely to be dropped. But if it is not dropped, any future
/// that panics *will* be polled again by `aselect`.
///
/// # Troubleshooting
///
/// Since `aselect!` is a pure declarative macro, and generates non-trivial code,
/// using it can sometimes result in very bad compilation errors. Please start with one
/// of the examples, and carefully modify it step-by-step into the desired shape, taking
/// note exactly at what step it stops compiling. Bug reports are welcome.
///
#[macro_export]
macro_rules! aselect {
    (
        {
            $(
                $(constant($($const_capture:ident),*))?
                $(mutable($($mutable_capture:ident),*))?
                $(borrowed($($borrowed_capture:ident),*))?;
            )*
        }$(,)?
        $( $arm_name: ident(
            $setup: expr,
            async |$async_input:ident $(,$borrow:ident)*| $async_block: expr,
            |$async_result:ident| $handler: expr
        )$(,)?  )*
    ) => {
        $crate::safe_select_impl!(constant($($($($const_capture,)*)*)*), mutable($($($($mutable_capture,)*)*)*), borrowed($($($($borrowed_capture,)*)*)*), temp, canceler, $crate::cancelers!(canceler, $($arm_name,)*), $crate::constant_captures0!(temp, $($($($const_capture,)*)*)*),$crate::constant_captures1!($($($($const_capture,)*)*)*), $crate::mutable_captures0!(temp, $($($($mutable_capture,)*)*)*), $crate::mutable_captures1!($($($($mutable_capture,)*)*)*), $crate::mutable_captures2!($($($($mutable_capture,)*)*)*), $crate::borrowed_captures0!(temp, $($($($borrowed_capture,)*)*)*), $crate::borrowed_captures1!($($($($borrowed_capture,)*)*)*), $($arm_name  ( $setup , async |$async_input $(,$borrow)*| $async_block, |$async_result| $handler), )*)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! safe_select_impl {
    ( constant($($const_cap:ident,)*), mutable($($mutable_cap:ident,)*), borrowed($($excl_cap:ident,)*), $temp: ident, $canceler: ident, $cancelers: stmt, $const_captures0:stmt, $const_captures1:stmt, $mutable_captures0: stmt, $mutable_captures1: stmt, $mutable_captures2: stmt,$excl_captures0: stmt, $excl_captures1: stmt,$( $name: ident  ( $body0: expr, async |$fut_input:ident $(,$cap1:ident)*| $body1: expr, |$result:ident| $handler_body: expr),  )* ) => {

        {
            #[allow(nonstandard_style)]
            #[allow(unused)]
            struct Context<'a $(,$const_cap)* $(,$mutable_cap)* $(,$excl_cap)*> {
                phantom: ::core::marker::PhantomData<&'a()>,
                $($excl_cap: $crate::LockedCapture<'a, $excl_cap>, )*
                $($mutable_cap: $crate::UnsafeCapture<'a, $mutable_cap>, )*
                $($const_cap: $const_cap, )*
            }

            let context = Context {
                phantom: ::core::marker::PhantomData,
                $($excl_cap: $crate::LockedCapture::new($excl_cap), )*
                $($mutable_cap: $crate::UnsafeCapture::new($mutable_cap), )*
                $($const_cap: $const_cap, )*
            };

            $(
                #[allow(nonstandard_style)]
                #[allow(unused)]
                struct $name<'a, R, TOut, TCap:'a, TFun, TDecide> where
                    TFun: FnMut(&'a TCap, &mut $crate::Canceler) -> Option<R>,
                    R: ::core::future::Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output, &mut $crate::Canceler) -> Option<Option<TOut>>,

                {
                    fun: TFun,
                    fut: Option<R>,
                    decide: TDecide,
                    phantom_cap: ::core::marker::PhantomData<&'a TCap>,
                }

                /// Return Some if future was ready (and thus must be recreated before next iteration)
                /// Return Some(Some(_)) if it also produced a value
                #[allow(nonstandard_style)]
                impl<'a, R, TOut, TCap:'a, TFun,TDecide> $crate::SelectArm<'a, TCap, TOut> for $name<'a, R, TOut, TCap, TFun, TDecide> where
                    TFun: FnMut(&'a TCap, &mut $crate::Canceler) -> Option<R>,
                    R: ::core::future::Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output, &mut $crate::Canceler) -> Option<Option<TOut>>,
                {

                    fn cancel(&mut self) {
                        self.fut = None;
                    }

                    fn do_poll(&mut self, ctx: &'a TCap, cx: &mut ::core::task::Context<'_>, canceler: &mut $crate::Canceler) -> $crate::PollResult<TOut> {
                        let mut future_created = false;
                        if self.fut.is_none() {
                            if let Some(fut) = (self.fun)(ctx, canceler) {
                                self.fut = Some(fut);
                                future_created = true;
                            } else {
                                return $crate::PollResult::Disabled;
                            }
                        }

                        let fut = self.fut.as_mut().unwrap();
                        // SAFETY:
                        // `do_poll` is not public. `Self` is in fact always pinned.
                        match unsafe { ::core::pin::Pin::new_unchecked(fut) }.poll(cx) {
                            ::core::task::Poll::Ready(out) => {
                                self.fut = None;
                                match (self.decide)(ctx, out, canceler) {
                                    Some(Some(c)) => {
                                        return $crate::PollResult::Result(c);
                                    }
                                    Some(None) => {
                                        return $crate::PollResult::EndStream;
                                    }
                                    _ => {
                                        return $crate::PollResult::Inhibited;
                                    }
                                }
                            }
                            ::core::task::Poll::Pending => {
                                return $crate::PollResult::Pending(future_created);
                            },
                        }

                    }
                }
            )*

            #[allow(nonstandard_style)]
            #[allow(unused)]
            struct ASelectImpl<'a, TCap:'a, TOut, $($name),*> where
                $($name: $crate::SelectArm<'a, TCap, TOut> ,)*
            {
                context: TCap,
                #[allow(unused)]
                $($name: $name,)*
                phantom: ::core::marker::PhantomData<&'a TOut>,
                #[allow(unused)]
                phantom_pinned: ::core::marker::PhantomPinned
            }

            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> ASelectImpl<'a, TCap, TOut,  $($name),*> where
                $($name: $crate::SelectArm<'a, TCap, TOut> ,)*
            {

                fn poll_next(self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>) -> ::core::task::Poll<Option<TOut>> {
                    // SAFETY:
                    // We do not move out of this.
                    let this = unsafe { self.get_unchecked_mut() };


                    const TOTCOUNT: usize = const {
                        let mut totcount = 0;
                        $(
                            #[allow(unused)]
                            let $name = ();
                            totcount += 1;
                        )*
                        totcount
                    };

                    let mut runnable: [bool; TOTCOUNT] = [true; TOTCOUNT];

                    let cap_ptr = (&this.context) as *const _;
                    let mut iteration_count = 0;
                    #[allow(unused_assignments)]
                    loop {
                        // True if all arms are okay with yielding. I.e, thy haven't just
                        // returned pending (in which case other arms may have to be polled),
                        // or returned "inhibited" (in which case they themselves must be
                        // polled again).
                        let mut can_yield = true;
                        let mut i = 0;
                        let mut canceler = $crate::Canceler::new();
                        $(
                            if runnable[i] {
                                match this.$name.do_poll(unsafe{&*cap_ptr}, cx, &mut canceler){
                                    $crate::PollResult::Result(out) => {
                                        return ::core::task::Poll::Ready(Some(out));
                                    }
                                    $crate::PollResult::EndStream => {
                                        return ::core::task::Poll::Ready(None);
                                    }
                                    $crate::PollResult::Pending(future_created) => {
                                        if future_created {
                                            can_yield = false;
                                        }
                                        runnable[i] = false;
                                    }
                                    $crate::PollResult::Disabled => {
                                    }
                                    $crate::PollResult::Inhibited => {
                                        can_yield = false;
                                    }
                                }
                            }
                            i += 1;
                        )*
                        // SAFETY:
                        // Only a single thread executes poll on this future. This is guaranteed
                        // because we take the future by `Pin<&mut Self`
                        if unsafe { canceler.any() } {
                            can_yield = false;
                            let mut i = 0;
                            $(
                                // SAFETY:
                                // Only a single thread executes poll on this future. This is guaranteed
                                // because we take the future by `Pin<&mut Self`
                                if unsafe { canceler.canceled(i as u32) } {
                                    this.$name.cancel();
                                    runnable[i] = true;
                                }
                                i += 1;
                            )*
                        }
                        iteration_count += 1;
                        if can_yield {
                            return ::core::task::Poll::Pending;
                        }
                        if iteration_count > 10 {
                            cx.waker().wake_by_ref();
                            return ::core::task::Poll::Pending;
                        }
                    }
                }
            }

            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> ASelectImpl<'a, TCap, TOut,  $($name),*> where
                $($name: $crate::SelectArm<'a, TCap, TOut> ,)*
            {
                fn poll_impl(self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>) -> ::core::task::Poll<TOut> {
                    match self.poll_next(cx) {
                        ::core::task::Poll::Ready(Some(val)) => ::core::task::Poll::Ready(val),
                        _ => ::core::task::Poll::Pending,
                    }
                }
            }

            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> ASelectImpl<'a, TCap, TOut,  $($name),*> where
                $($name: $crate::SelectArm<'a, TCap, TOut> ,)*
            {
                fn poll_stream_impl(self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>) -> ::core::task::Poll<Option<TOut>> {
                    match self.poll_next(cx) {
                        ::core::task::Poll::Ready(val) => ::core::task::Poll::Ready(val),
                        _ => ::core::task::Poll::Pending,
                    }
                }
            }

            // This helps with type resolution
            // Note, the way this is used, 'a usually ends up being resolved as 'static .
            // It will only be non-static if captured variables contain references (or are references).
            #[allow(nonstandard_style)]
            fn unify_fut<'a, R: 'a $(,$const_cap:'a)* $(,$mutable_cap:'a)* $(,$excl_cap:'a)*, F: FnMut(&'a Context<'a $(,$const_cap)* $(,$mutable_cap)* $(,$excl_cap)*>, &mut $crate::Canceler) -> Option<R>>(_hint: *const Context<'a $(,$const_cap)* $(,$mutable_cap)* $(,$excl_cap)*>, func: F) -> F {
                func
            }


            $crate::define_stream_impl!($($name),*);

            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> ::core::future::Future for ASelectImpl<'a, TCap, TOut, $($name),*> where
                $($name: $crate::SelectArm<'a, TCap, TOut> ,)*
            {
                type Output = TOut;
                fn poll(self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>) -> ::core::task::Poll<Self::Output> {
                    self.poll_impl(cx)
                }
            }


            let context_hint = &context as *const _;

            ASelectImpl {
                context,
                phantom_pinned: ::core::marker::PhantomPinned,
                phantom: ::core::marker::PhantomData,
                $(
                #[allow(unused)]
                $name: $name {
                    fun: unify_fut(context_hint, move|$temp, $canceler: &mut $crate::Canceler|{
                        $cancelers
                        $const_captures0
                        $const_captures1
                        $mutable_captures0
                        $mutable_captures1
                        $excl_captures0
                        $excl_captures1
                        Some({
                            let mut $fut_input = {$body0};
                            $mutable_captures2
                            $(
                                // SAFETY:
                                // Only a single thread executes poll on this future. This is guaranteed
                                // because we take the future by `Pin<&mut Self`
                                let mut $cap1 = unsafe { $temp.$cap1.lock()? };
                            )*
                            async move {
                                $const_captures0
                                $const_captures1
                                $(
                                    // SAFETY:
                                    // Only a single thread executes poll on this future. This is guaranteed
                                    // because we take the future by `Pin<&mut Self`
                                    let $cap1 : &mut _ = unsafe { $cap1.get_mut() };
                                )*

                                $body1
                            }
                        })
                    }),
                    fut: None,
                    decide: move |$temp, $result, $canceler: &mut $crate::Canceler|{
                            $cancelers
                            $const_captures0
                            $const_captures1
                            $mutable_captures0
                            $mutable_captures1
                            $excl_captures0
                            $excl_captures1
                            let t = Some($handler_body);
                            $crate::result(t)
                        },
                    phantom_cap: ::core::marker::PhantomData,
                },
                )*
            }
        }
    }
}
