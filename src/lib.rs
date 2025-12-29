pub use futures::Stream;
use std::cell::UnsafeCell;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ptr::null_mut;

#[cfg(test)]
mod tests;

pub trait NewFactory<'a, CTX, TOut> {
    /// Returns Some if user code was run
    /// If it was ready, it may have produced a value `Some(Some(_))` or not `Some(None)`.
    /// It is guaranteed that if this method has run user-code, it returns Some.
    /// If the future was not ready, and no user code was run, `None` is returned.
    fn do_poll(&mut self, ctx: &'a CTX, cx: &mut ::std::task::Context<'_>) -> PollResult<TOut>;
}

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
    pub unsafe fn access(&self) -> UnsafeCaptureAccess<T> {
        UnsafeCaptureAccess { value: self.value.get() }
    }
}


pub struct LockedCapture<'a, T: 'a> {
    locks: UnsafeCell<bool>,
    value: UnsafeCell<T>,
    phantom: PhantomData<&'a ()>,
}
impl<'a, T: Debug> Debug for LockedCapture<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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
    pub unsafe fn access(&self) -> CaptureAccess<T> {
        if unsafe { !*self.locks.get() } {
            CaptureAccess {
                value: self.value.get(),
            }
        } else {
            CaptureAccess { value: null_mut() }
        }
    }
}

pub struct CaptureAccess<T> {
    value: *mut T,
}

impl<T> CaptureAccess<T> {
    pub unsafe fn get(&self) -> Option<&'_ mut T> {
        if self.value.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.value })
        }
    }
}

pub struct UnsafeCaptureAccess<T> {
    value: *mut T,
}

impl<T> UnsafeCaptureAccess<T> {
    pub unsafe fn get(&self) -> &'_ mut T {
        unsafe { &mut *self.value }
    }
}


pub struct CaptureGuard<'a, T> {
    lock: &'a UnsafeCell<bool>,
    #[doc(hidden)]
    value: *mut T,
}

impl<'a, T> Debug for CaptureGuard<'a, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "CaptureGuard({:?})", self.value)
    }
}

impl<'a, T> CaptureGuard<'a, T>
where
    T: Debug,
{
    pub unsafe fn get_mut(&mut self) -> &mut T {
        unsafe { &mut *self.value }
    }
}

impl<'a, T: 'a> LockedCapture<'a, T> {
    #[doc(hidden)]
    pub unsafe fn lock(&self) -> Option<CaptureGuard<'_, T>> {
        let locks = unsafe { &mut *self.locks.get() };
        if *locks {
            return None;
        }
        *locks = true;
        Some(CaptureGuard {
            lock: &self.locks,
            value: unsafe { &mut *(self.value.get()) },
        })
    }
}

impl<T> Drop for CaptureGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { *self.lock.get() = false }
    }
}

#[macro_export]
macro_rules! ord_cap2 {
    ($typname: ident, $($cap:ident)*) => {
        $crate::ord_cap2!($typname, inner 0, parsed $($cap)*)
    };
    ($typname: ident, inner $depth:expr, $(($cap0:ident, $count:expr))* parsed $cap:ident $($cap1:ident)*) => {
        $crate::ord_cap2!($typname, inner ($depth + 1), $(($cap0, $count) )* ($cap, $depth) parsed $($cap1)* )
    };

    ($typname: ident, inner $depth:expr, $(($cap:ident, $count:expr))* parsed) => {

        $typname {
            $($cap: $crate::LockedCapture::new($cap, ($count)), )*
        }

    };
}

/// A small helper macro to expand a sequence of expressions and statements.
///
// TODO: Is this _really_ needed? Can't we inject our variables in an outer scope?
#[macro_export]
macro_rules! expand_arbitrary {
    () => {

    };
    ( ($e:expr) $($tail:tt)* ) => {
        $e
        expand_arbitrary!( $($tail)* )
    };
    ( ($e:stmt) $( $tail:tt)* ) => {
        $e
        expand_arbitrary!( $($tail)* )
    };
}

pub trait SafeResult {
    type Output;
    fn result(self) -> Option<Self::Output>;
}
pub fn result<T: SafeResult>(input: Option<T>) -> Option<T::Output> {
    input?.result()
}
impl<R> SafeResult for Option<R> {
    type Output = R;

    fn result(self) -> Option<Self::Output> {
        Some(self?)
    }
}
impl SafeResult for () {
    type Output = ();
    fn result(self) -> Option<Self::Output> {
        None
    }
}

/*
end goal

safe_select2!(
    captures(mut a,const b,exclusive c),
    {
        // setup
        *a = 5;
        // Optionally return reusable state
        *a + 1
    },
    |time,c|{
        tokio::sleep(time).await;
        tokio::sleep(*a).await;
        *c = 42;
        // produce result
        42
    },
    |result|{
        *a = 47;
        Some("yielded value".to_string())
    }
)

*/

#[macro_export]
macro_rules! safe_select2 {
    (
        constant($($constcap:ident),*),
        mutable($($mutcap:ident),*),
        exclusive($($excap:ident),*),

    $( $name: ident( $body: expr, async |$fut_input:ident $(,$excap0:ident)*| $body1: expr, |$result:ident| $handler_body: expr)  ),* $(,)? ) => {
        //$crate::safe_select!(inner $contextname, $contexttype, $($name  ( |$($cap0),*|  {$($body0 ;)*}, async |$($cap1),*| $body1, |$result| $handler_body), )*)

        {
            struct ConstState<$($constcap),*> {
                $($constcap: $constcap,)*
            }
            struct State<$($constcap),*> {
                const_state: ConstState<$($constcap),*>,

            }





        }


    };
}


#[macro_export]
macro_rules! borrowed_captures0 {
    ( $temp: ident, $($cap: ident,)*) => {
        $(
            let $cap = unsafe { $temp.$cap.access()};
        )*
    };
}
#[macro_export]
macro_rules! borrowed_captures1 {
    ( $($cap: ident,)*) => {
        $(
            let mut $cap = unsafe { $cap.get() };
        )*
    };
}


#[macro_export]
macro_rules! mutable_captures0 {
    ( $temp: ident, $($cap: ident,)*) => {
        $(
            let $cap = unsafe { $temp.$cap.access()};
        )*
    };
}
#[macro_export]
macro_rules! mutable_captures1 {
    ( $($cap: ident,)*) => {
        $(
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
#[derive(Clone,Copy,Debug)]
pub struct MutableValueUnavailableInThisAsyncContext;

#[macro_export]
macro_rules! mutable_captures2 {
    ( $($cap: ident,)*) => {
        $(
            let mut $cap = $crate::MutableValueUnavailableInThisAsyncContext;
        )*
    };
}

#[macro_export]
macro_rules! constant_captures0 {
    ( $temp: ident, $($cap: ident,)*) => {
        $(
            let mut $cap = &$temp.$cap;
        )*
    };
}


pub enum PollResult<T> {
    Result(T),
    Pending,
    /// Future ran to completion, but no output value was produced
    Inhibited,
    Disabled
}


#[macro_export]
macro_rules! safe_select {
    ( {$( $(constant($($const_cap:ident),*))? $(mutable($($mutable_cap:ident),*))? $(borrowed($($excl_cap:ident),*))? ; )* }, $( $name: ident( $body0: expr, async |$fut_input:ident $(,$cap1:ident)*| $body1: expr, |$result:ident| $handler_body: expr),  )* ) => {
        $crate::safe_select!(inner constant($($($($const_cap,)*)*)*), mutable($($($($mutable_cap,)*)*)*), borrowed($($($($excl_cap,)*)*)*), temp, $crate::constant_captures0!(temp, $($($($const_cap,)*)*)*), $crate::mutable_captures0!(temp, $($($($mutable_cap,)*)*)*), $crate::mutable_captures1!($($($($mutable_cap,)*)*)*), $crate::mutable_captures2!($($($($mutable_cap,)*)*)*), $crate::borrowed_captures0!(temp, $($($($excl_cap,)*)*)*), $crate::borrowed_captures1!($($($($excl_cap,)*)*)*), $($name  ( $body0 , async |$fut_input $(,$cap1)*| $body1, |$result| $handler_body), )*)
    };
    ( inner constant($($const_cap:ident,)*), mutable($($mutable_cap:ident,)*), borrowed($($excl_cap:ident,)*), $temp: ident, $const_captures0:stmt, $mutable_captures0: stmt, $mutable_captures1: stmt, $mutable_captures2: stmt,$excl_captures0: stmt, $excl_captures1: stmt,$( $name: ident  ( $body0: expr, async |$fut_input:ident $(,$cap1:ident)*| $body1: expr, |$result:ident| $handler_body: expr),  )* ) => {

        {
            #[allow(nonstandard_style)]
            pub struct Context<'a $(,$const_cap)* $(,$mutable_cap)* $(,$excl_cap)*> {
                phantom: ::std::marker::PhantomData<&'a()>,
                $($excl_cap: $crate::LockedCapture<'a, $excl_cap>, )*
                $($mutable_cap: $crate::UnsafeCapture<'a, $mutable_cap>, )*
                $($const_cap: $const_cap, )*
            }

            let context = Context {
                phantom: ::std::marker::PhantomData,
                $($excl_cap: $crate::LockedCapture::new($excl_cap), )*
                $($mutable_cap: $crate::UnsafeCapture::new($mutable_cap), )*
                $($const_cap: $const_cap, )*
            };

            $(
                #[allow(nonstandard_style)]
                struct $name<'a, R, TOut, TCap:'a, TFun, TDecide> where
                    TFun: FnMut(&'a TCap) -> Option<R>,
                    R: Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output) -> Option<TOut>,

                {
                    fun: TFun,
                    fut: Option<R>,
                    decide: TDecide,
                    phantom_cap: ::std::marker::PhantomData<&'a TCap>,
                }

                /// Return Some if future was ready (and thus must be recreated before next iteration)
                /// Return Some(Some(_)) if it also produced a value
                #[allow(nonstandard_style)]
                impl<'a, R, TOut, TCap:'a, TFun,TDecide> $crate::NewFactory<'a, TCap, TOut> for $name<'a, R, TOut, TCap, TFun, TDecide> where
                    TFun: FnMut(&'a TCap) -> Option<R>,
                    R: Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output) -> Option<TOut>,
                {


                    fn do_poll(&mut self, ctx: &'a TCap, cx: &mut ::std::task::Context<'_>) -> $crate::PollResult<TOut> {
                        loop {
                            if self.fut.is_none() {
                                if let Some(fut) = (self.fun)(ctx) {
                                    self.fut = Some(fut);
                                } else {
                                    return $crate::PollResult::Disabled;
                                }
                            }

                            let fut = self.fut.as_mut().unwrap();
                            match unsafe { ::std::pin::Pin::new_unchecked(fut) }.poll(cx) {
                                ::std::task::Poll::Ready(out) => {
                                    self.fut = None;
                                    match (self.decide)(ctx, out) {
                                        Some(c) => {
                                            return $crate::PollResult::Result(c);
                                        }
                                        _ => {
                                            return $crate::PollResult::Inhibited;
                                        }
                                    }
                                }
                                ::std::task::Poll::Pending => {
                                    return $crate::PollResult::Pending;
                                },
                            }
                        }
                    }
                }
            )*

            #[allow(nonstandard_style)]
            pub struct __SafeSelectImpl<'a, TCap:'a, TOut, $($name),*>
            {
                context: TCap,
                $($name: $name,)*
                phantom: ::std::marker::PhantomData<&'a TOut>,
                #[allow(unused)]
                phantom_pinned: ::std::marker::PhantomPinned
            }

            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> __SafeSelectImpl<'a, TCap, TOut,  $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {

                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<TOut>> {
                    let this = unsafe { self.get_unchecked_mut() };


                    const TOTCOUNT: usize = const {
                        let mut totcount = 0;
                        $(
                            struct $name;
                            totcount += 1;
                        )*
                        totcount
                    };

                    let mut runnable: [bool; TOTCOUNT] = [true; TOTCOUNT];

                    let cap_ptr = (&this.context) as *const _;
                    let mut iteration_count = 0;
                    // True if at least one future has been polled and returned pending.
                    // This means that a waker has been registered, and we can return pending
                    // without risking starvation. Note however that all arms must be given
                    // a chance to produce and poll a future, otherwise futures will never actually
                    // run concurrently.
                    let mut have_any_waiter = false;
                    loop {
                        // True if all arms are okay with yielding. I.e, thy haven't just
                        // returned pending (in which case other arms may have to be polled),
                        // or returned "inhibited" (in which case they themselves must be
                        // polled again).
                        let mut can_yield = true;
                        let mut i = 0;
                        $(
                            if runnable[i] {
                                match this.$name.do_poll(unsafe{&*cap_ptr}, cx){
                                    $crate::PollResult::Result(out) => {
                                        return ::std::task::Poll::Ready(Some(out));
                                    }
                                    $crate::PollResult::Pending => {
                                        can_yield = false;
                                        have_any_waiter = true;
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
                        iteration_count += 1;
                        if can_yield && have_any_waiter {
                            return ::std::task::Poll::Pending;
                        }
                        if iteration_count > 1000 {
                            panic!("Too many iterations with neither pending futures nor a value produced");
                        }

                    }
                }
            }


            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> __SafeSelectImpl<'a, TCap, TOut,  $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                fn poll_impl(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<TOut> {
                    match self.poll_next(cx) {
                        ::std::task::Poll::Ready(Some(val)) => ::std::task::Poll::Ready(val),
                        _ => ::std::task::Poll::Pending,
                    }
                }
            }

            #[allow(nonstandard_style)]
            fn unify_fut<'a, R: 'a $(,$const_cap:'a)* $(,$mutable_cap:'a)* $(,$excl_cap:'a)*, F: FnMut(&'a Context<'a $(,$const_cap)* $(,$mutable_cap)* $(,$excl_cap)*>) -> R>(_hint: *const Context<'a $(,$const_cap)* $(,$mutable_cap)* $(,$excl_cap)*>, func: F) -> F {
                func
            }


            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> $crate::Stream for __SafeSelectImpl<'a, TCap, TOut, $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                type Item = TOut;
                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<Self::Item>> {
                    match self.poll_impl(cx) {
                        ::std::task::Poll::Ready(val) => ::std::task::Poll::Ready(Some(val)),
                        _ => std::task::Poll::Pending,
                    }
                }
            }

            #[allow(nonstandard_style)]
            impl<'a, TCap, TOut, $($name),*> ::std::future::Future for __SafeSelectImpl<'a, TCap, TOut, $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                type Output = TOut;
                fn poll(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Self::Output> {
                    self.poll_impl(cx)
                }
            }


            let context_hint = &context as *const _;


            #[allow(unused)]
            #[allow(unused_mut)]
            __SafeSelectImpl {
                context,
                    phantom_pinned: ::std::marker::PhantomPinned,
                    phantom: ::std::marker::PhantomData,
                    $(
                    $name: $name {
                        fun: unify_fut(context_hint, move|$temp|{
                            $const_captures0
                            $mutable_captures0
                            $mutable_captures1
                            $excl_captures0
                            $excl_captures1
                            Some({
                                let mut $fut_input = {$body0};
                                $const_captures0
                                $mutable_captures2
                                $(
                                    let mut templ = unsafe { $temp.$cap1.lock()? };
                                )*
                                async move {
                                    $(
                                        let $cap1 : &mut _ = unsafe { templ.get_mut() };
                                    )*

                                    $body1
                                }
                            })
                        }),
                        fut: None,
                        decide: move |$temp, $result|{
                                $const_captures0
                                $mutable_captures0
                                $mutable_captures1
                                $excl_captures0
                                $excl_captures1
                                let t = Some($handler_body);
                                $crate::result(t)
                            },
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            }
        }



    }


}
