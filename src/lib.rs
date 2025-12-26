pub use futures::Stream;
use std::cell::UnsafeCell;
use std::fmt::{Debug, Formatter};
use std::marker::PhantomData;
use std::ptr::null_mut;

#[cfg(test)]
mod tests;

pub trait Factory<CTX> {
    type Output;
    fn invoke(&mut self, ctx: &mut CTX) -> Self::Output;
}

impl<F, R, CTX> Factory<CTX> for F
where
    F: FnMut(&mut CTX) -> R,
{
    type Output = R;

    fn invoke(&mut self, ctx: &mut CTX) -> Self::Output {
        self(ctx)
    }
}

pub trait NewFactory<'a, CTX, TOut> {
    /// Returns true if future was ready
    /// If it was ready, it may have produced a value (ControlFlow::Break) or not (ControlFlow::Pending)
    fn do_poll(&mut self, ctx: &'a CTX, cx: &mut ::std::task::Context<'_>) -> Option<Option<TOut>>;
}

pub struct Capture<'a, T:'a> {
    #[doc(hidden)]
    locks: UnsafeCell<bool>,
    #[doc(hidden)]
    value: UnsafeCell<T>,
    phantom: PhantomData<&'a ()>,

}
impl<'a, T:Debug> Debug for Capture<'a, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Capture()")
    }
}
impl<'a, T:'a> Capture<'a,T> {
    pub fn new(value: T) -> Self {
        Self {
            locks: UnsafeCell::new(false),
            value: UnsafeCell::new(value),
            phantom: PhantomData,
        }
    }
    pub unsafe fn access(&self) -> CaptureAccess<T> {
        if unsafe { !*self.locks.get() } {
            CaptureAccess{
                value: self.value.get()
            }
        } else {
            CaptureAccess{
                value: null_mut()
            }
        }
    }
}

pub struct CaptureAccess<T> {
    value: *mut T
}

impl<T> CaptureAccess<T> {
    pub unsafe fn get(&self) -> Option<&'_ mut T> {
        if self.value.is_null() {
            None
        } else {
            Some(unsafe {&mut *self.value})
        }
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
        unsafe {&mut *self.value}
    }
}

impl<'a, T:'a> Capture<'a, T> {
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
        unsafe {*self.lock.get() = false }
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
            $($cap: $crate::Capture::new($cap, ($count)), )*
        }

    };
}

#[macro_export]
macro_rules! safe_select_context {
    ( $contextname:ident ($($cap: ident: $capty: ty = $capvalue: expr),*) ) => {


        #[allow(nonstandard_style)]
        pub struct $contextname<'a> {
            $($cap: $crate::Capture<'a, $capty>, )*
        };

        #[allow(nonstandard_style)]
        impl<'a> $contextname<'a> {
            pub fn new() -> Self {
                Self {
                        $($cap: $crate::Capture::new($capvalue), )*
                }
            }
        }
        #[allow(non_snake_case)]
        let $contextname = $contextname::new();


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

#[macro_export]
macro_rules! safe_select {
    ( $contextname:ident, $contexttype:ty, $( $name: ident($($cap0:ident),*) ( {$($body0: stmt ;)*}, async |$($cap1:ident),*| $body1: expr, |$result:ident| $handler_body: expr),  )* ) => {
        safe_select!(inner $contextname, $contexttype, $($name  ( |$($cap0),*|  {$($body0 ;)*}, async |$($cap1),*| $body1, |$result| $handler_body), )*)
    };
    ( inner $contextname:ident, $contexttype:ty, $( $name: ident  ( |$($cap0:ident),*| {$($body0: stmt ;)*}, async |$($cap1:ident),*| $body1: expr, |$result:ident| $handler_body: expr),  )* ) => {

        {

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
                    fn do_poll(&mut self, ctx: &'a TCap, cx: &mut ::std::task::Context<'_>) -> Option<Option<TOut>> {
                        //println!("Polling: {:?}", self.fut.is_some());
                        let mut was_ready = false;

                        loop {
                            if self.fut.is_none() {
                                if let Some(fut) = (self.fun)(ctx) {
                                    self.fut = Some(fut);
                                } else {
                                    return was_ready.then_some(None);
                                }
                            }

                            let fut = self.fut.as_mut().unwrap();
                            match unsafe { ::std::pin::Pin::new_unchecked(fut) }.poll(cx) {
                                ::std::task::Poll::Ready(out) => {
                                    was_ready = true;
                                    //println!("Ready!");
                                    self.fut = None;
                                    match (self.decide)(ctx, out) {
                                        Some(c) => {
                                            return Some(Some(c));
                                        }
                                        _ => {
                                            continue;
                                        }
                                    }
                                }
                                ::std::task::Poll::Pending => {
                                    return was_ready.then_some(None);
                                },
                            }
                        }
                    }
                }
            )*

            #[allow(nonstandard_style)]
            pub struct __SafeSelectImpl<'a, TOut, $($name),*>
            {
                cap: $contextname<'a>,
                $($name: $name,)*
                phantom: ::std::marker::PhantomData<(&'a TOut)>,
                phantom_pinned: ::std::marker::PhantomPinned
            }

            #[allow(nonstandard_style)]
            impl<'a, TOut, $($name),*> __SafeSelectImpl<'a, TOut,  $($name),*> where
                $($name: $crate::NewFactory<'a, $contextname<'a>, TOut> ,)*
            {

                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<TOut>> {
                    let this = unsafe { self.get_unchecked_mut() };

                    let mut unready = 0;
                    let mut totcount = 0;
                    $(
                        _ = &this.$name;
                        totcount += 1;
                    )*

                    let cap_ptr = (&this.cap) as *const _;
                    loop {
                        $(

                            if let Some(ready) = this.$name.do_poll(unsafe{&*cap_ptr}, cx) {
                                if let Some(val) = ready {
                                    return ::std::task::Poll::Ready(Some(val));
                                }
                                unready = 0;
                            } else {
                                unready += 1;
                                if unready == totcount {
                                    break;
                                }
                            }
                        )*
                    }

                    ::std::task::Poll::Pending
                }
            }


            #[allow(nonstandard_style)]
            impl<'a, TOut, $($name),*> __SafeSelectImpl<'a, TOut,  $($name),*> where
                $($name: $crate::NewFactory<'a, $contextname<'a>, TOut> ,)*
            {
                fn poll_impl(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<TOut> {
                    //println!("Future poll");
                    use $crate::Stream;
                    match self.poll_next(cx) {
                        ::std::task::Poll::Ready(Some(val)) => ::std::task::Poll::Ready(val),
                        _ => ::std::task::Poll::Pending,
                    }
                }
            }

            fn unify_fut<'a, R: 'a, TCap:'a, F: FnMut(&'a TCap) -> R>(func: F) -> F {
                func
            }
            fn unifyb<'a, R, TCap:'a, F: FnMut(&TCap) -> R>(func: F) -> F {
                func
            }
            fn unify2<'a, R, V, TCap:'a, F: FnMut(&TCap, V) -> R>(func: F) -> F {
                func
            }




            #[allow(nonstandard_style)]
            impl<'a, TOut, $($name),*> $crate::Stream for __SafeSelectImpl<'a, TOut, $($name),*> where
                $($name: $crate::NewFactory<'a, $contextname<'a>, TOut> ,)*
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
            impl<'a, TOut, $($name),*> ::std::future::Future for __SafeSelectImpl<'a, TOut, $($name),*> where
                $($name: $crate::NewFactory<'a, $contextname<'a>, TOut> ,)*
            {
                type Output = TOut;
                fn poll(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Self::Output> {
                    self.poll_impl(cx)
                }
            }





            __SafeSelectImpl {
                cap: $contextname,
                    phantom_pinned: ::std::marker::PhantomPinned,
                    phantom: ::std::marker::PhantomData,
                    $(
                    $name: $name {
                        fun: unify_fut(move|temp:&$contextname<'_>|{
                            $(
                                let mut tempg = unsafe { temp.$cap0.access()};
                                let mut $cap0 = unsafe { tempg.get() };
                            )*
                            Some({
                                use safeselect::expand_arbitrary;
                                $crate::expand_arbitrary!($( ($body0) )*);
                                    $(
                                        let mut templ = unsafe { temp.$cap1.lock()? };
                                    )*
                                async move {
                                    $(
                                        let $cap1 = unsafe { templ.get_mut() };
                                    )*

                                    $body1
                                }
                            })
                        }),
                        fut: None,
                        decide: unify2(move |temp:&$contextname<'_>, $result|{
                                $(
                                    let mut tempg = unsafe { temp.$cap0.access()};
                                    let mut $cap0 = unsafe { tempg.get() };
                                )*
                                let t = Some($handler_body);
                                $crate::result(t)
                            }),
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            }
        }



    }


}

