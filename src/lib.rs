pub use futures::Stream;
use std::cell::UnsafeCell;
use std::marker::{PhantomData, PhantomPinned};
use std::ops::{ControlFlow, Deref, DerefMut};
use std::pin::{Pin, pin};
use std::task::{Context, Poll};
use std::time::Duration;

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
    fn do_poll(
        &mut self,
        ctx: &'a CTX,
        cx: &mut ::std::task::Context<'_>,
    ) -> Option<Option<TOut>>;
}

pub struct Capture<'a, T> {
    //TODO: Hide this from user
    #[doc(hidden)]
    pub lock: UnsafeCell<bool>,
    pub phantom: PhantomData<&'a T>,
    #[doc(hidden)]
    pub value: *const T,
    pub num: usize,
}
impl<'a, T> Capture<'a, T> {
    pub fn new(
        value: &mut T,
        lock: UnsafeCell<bool>,
        num: usize,
    ) -> Self {
        Self {
            lock,
            phantom: PhantomData,
            value,
            num,
        }
    }
}
pub struct CaptureGuard<'a, T> {
    #[doc(hidden)]
    pub lock: &'a UnsafeCell<bool>,
    #[doc(hidden)]
    pub value: &'a mut T,
}

impl<'a, T> Capture<'a, T> {
    pub fn get(&'a self) -> Option<CaptureGuard<'a, T>> {
        let lock = unsafe { &mut *self.lock.get() };
        if *lock {
            return None;
        }
        Some(CaptureGuard {
            lock: &self.lock,
            value: unsafe { &mut *(self.value as *mut _) },
        })
    }
}

impl<'a, T> Capture<'a, Option<T>> {
    pub fn get_some(&'a self) -> Option<CaptureGuard<'a, T>> {
        let lock = unsafe { &mut *self.lock.get() };
        if *lock {
            return None;
        }
        let value = unsafe { &mut *(self.value as *mut Option<T>) }.as_mut()?;

        Some(CaptureGuard {
            lock: &self.lock,
            value,
        })
    }
    pub fn take(&'a self) -> Option<T> {
        let lock = unsafe { &mut *self.lock.get() };
        if *lock {
            return None;
        }
        unsafe { (*(self.value as * mut Option<T>)).take() }
    }
}

impl<'a, T> CaptureGuard<'a, T> {
    pub fn map<R>(self, map: impl FnOnce(&mut T) -> Option<&mut R>) -> Option<CaptureGuard<'a, R>> {
        let lptr = self.lock as *const _;
        let vptr = self.value as *mut _;
        let new_value = map(unsafe { &mut *vptr })?;
        std::mem::forget(self);
        Some(CaptureGuard {
            lock: unsafe { &*lptr },
            value: new_value,
        })
    }
}

impl<'a, T> Deref for CaptureGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.value }
    }
}

impl<'a, T> DerefMut for CaptureGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.value }
    }
}

macro_rules! ord_cap {
    ($($cap:ident),*) => {
        ord_cap!(inner 0, parsed $($cap)*)
    };
    (inner $depth:expr, $(($cap0:ident, $count:expr))* parsed $cap:ident $($cap1:ident)*) => {
        ord_cap!(inner ($depth + 1), $(($cap0, $count) )* ($cap, $depth) parsed $($cap1),* )
    };

    (inner $depth:expr, $(($cap0:ident, $count:expr))* parsed) => {
        ($( ($cap0, $count) ),*)
    };
}

#[macro_export]
macro_rules! ord_cap2 {
    ($typname: ident, $($cap:ident)*) => {
        $crate::ord_cap2!($typname, inner 0, parsed $($cap)*)
    };
    ($typname: ident,inner $depth:expr, $(($cap0:ident, $count:expr))* parsed $cap:ident $($cap1:ident)*) => {
        $crate::ord_cap2!($typname, inner ($depth + 1), $(($cap0, $count) )* ($cap, $depth) parsed $($cap1)* )
    };

    ($typname: ident,inner $depth:expr, $(($cap:ident, $count:expr))* parsed) => {

        $typname {
            $($cap: $crate::Capture::new(&mut $cap, ::std::cell::UnsafeCell::new(false), ($count)), )*
        }

    };
}

#[macro_export]
macro_rules! safe_select {

    ( capture ($($cap: ident),*), $($tail:tt)*  ) => {
      safe_select!(partial ($($cap),*), parsed $($tail)*)
    };
    // This is a bit convoluted, because we had a tt-muncher to support slightly different
    // syntax for each arm. However, currently, there is only one syntax.
    ( partial ($($cap: ident),*), $( ( $body0: expr, $name0: ident, $handler_body0: expr)  )* parsed $(,)? $name:ident( $body: expr, $handler_body: expr) $($tail:tt)*  ) => {
        safe_select!( partial ($($cap),*), $(($body0, $name0, $handler_body0))* ($body, $name, $handler_body) parsed $($tail)*)
    };

    ( partial ($($cap: ident),*), $( (  $body: expr, $name: ident, $handler_body: expr)  )* parsed $(,)? ) => {
        safe_select!( innerest capture ( __SafeSelectCapture { $($cap,)* } ), ($($cap),*), $( ( $body, $name, $handler_body)  )* )
    };
    ( innerest capture ($capassign: pat), ($($cap: ident),*), $( (  $body: expr, $name: ident, $handler_body: expr)  )* ) => {
        {

            #[allow(nonstandard_style)]
            struct __SafeSelectCapture<'a, $($cap),*> {
                $($cap: $crate::Capture<'a, $cap>, )*
            }


            $(
                #[allow(nonstandard_style)]
                struct $name<'a, R, TOut, TCap:'a, TFun, TDecide> where
                    TFun: FnMut(&'a TCap) -> Option<R>,
                    R: Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output) -> Option<Option<TOut>>,

                {
                    fun: TFun,
                    fut: Option<R>,
                    decide: TDecide,
                    phantom_cap: ::std::marker::PhantomData<&'a TCap>,
                }

                /// Return Some if future was ready
                #[allow(nonstandard_style)]
                impl<'a, R, TOut, TCap:'a, TFun,TDecide> $crate::NewFactory<'a, TCap, TOut> for $name<'a, R, TOut, TCap, TFun, TDecide> where
                    TFun: FnMut(&'a TCap) -> Option<R>,
                    R: Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output) -> Option<Option<TOut>>,
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
                                        Some(Some(c)) => {
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
            pub struct __SafeSelectImpl<'a, TOut, TCap, $($name),*> //where
                //$($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                cap: TCap,
                $($name: $name,)*
                phantom: ::std::marker::PhantomData<(&'a TCap, TOut)>,
                phantom_pinned: ::std::marker::PhantomPinned
            }

            #[allow(nonstandard_style)]
            impl<'a, TOut, TCap:'a, $($name),*> __SafeSelectImpl<'a, TOut, TCap,  $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {

                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<TOut>> {
                    let this = unsafe { self.get_unchecked_mut() };

                    let mut unready = 0;
                    let mut totcount = 0;
                    $(
                        _ = &this.$name;
                        totcount += 1;
                    )*

                    let cap_ptr = &this.cap as *const _;
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
            impl<'a, TOut, TCap, $($name),*> __SafeSelectImpl<'a, TOut, TCap,  $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
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

            fn unify<'a, R: 'a, TCap:'a, F: FnMut(&'a TCap) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }
            fn unifyb<'a, R, TCap:'a, F: FnMut(&TCap) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }
            fn unify2<'a, R, V, TCap:'a, F: FnMut(&TCap, V) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }


            /*struct Wrapper<'a,TOut, TCap:'a, $($name),*> {
                cap: TCap,
                sel: Option<__SafeSelectImpl<'a,TOut, TCap, $($name),*>>
            }*/
            let cap = $crate::ord_cap2!(__SafeSelectCapture, $($cap)*);

            /*impl<'a, TOut, TCap, $($name),*> Wrapper<'a, TOut, TCap,  $($name),*> {
                fn make_static<'b>(&mut self, sel: __SafeSelectImpl<'b, TOut, TCap, $($name),*>)  {
                    unsafe {
                        self.sel = Some(::std::mem::transmute(sel));
                    }
                }
            }
*/

            impl<'a, TOut, TCap:'a, $($name),*> $crate::Stream for __SafeSelectImpl<'a, TOut, TCap, $($name),*> where
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
            impl<'a, TOut, TCap:'a, $($name),*> ::std::future::Future for __SafeSelectImpl<'a, TOut, TCap, $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                type Output = TOut;
                fn poll(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Self::Output> {
                    self.poll_impl(cx)
                }
            }

/*
            impl<'a, TOut, TCap:'a, $($name),*> $crate::Stream for Wrapper<'a, TOut, TCap, $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                type Item = TOut;
                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<Self::Item>> {
                    let cap = unsafe { &*(&self.cap as *const _ )};
                    let sel = unsafe { self.map_unchecked_mut(|x|x.sel.as_mut().unwrap()) };
                    sel.poll_next(cx, cap)
                }
            }*/


            let capptr: *const _ = &cap; //TODO: Remove, surely not needed any more

            __SafeSelectImpl{
                cap,
                    phantom_pinned: ::std::marker::PhantomPinned,
                phantom: ::std::marker::PhantomData,
                    $(
                    $name: $name {
                        fun: unify(move |temp|{
                                #[allow(unused)]
                                let $capassign = temp;
                                Some($body)
                            }, capptr),
                        fut: None,
                        decide: unify2(move |temp, $name|{
                                #[allow(unused)]
                                let $capassign = temp;
                                $handler_body($name)
                            }, capptr),
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            }

        }

    }


}

mod tests {
    use futures::Stream;
    use futures::StreamExt;
    use std::ops::ControlFlow;
    use std::pin::pin;
    use std::sync::Mutex;
    use std::sync::mpsc::{Sender, channel};
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn test() {
        tokio::time::timeout(Duration::from_secs(1), async move {
            let tempcap = 42u16;
            let temp2 = 43u32;
            let mut strm = safe_select!(
                capture(tempcap, temp2),
                (
                    if (true) {
                        //println!("1 Tempcap:  {} : {}", tempcap, temp2);
                        //*tempcap += 2;
                        async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            42u64
                        }
                    },
                    |pred1| ControlFlow::Break(pred1)
                )(
                    {
                        //*tempcap += 10;
                        async move {
                            tokio::time::sleep(Duration::from_millis(75)).await;
                            43u32
                        }
                    },
                    |_pred2| {
                        //println!("2 Result Tempcap:  {} : {}", tempcap, temp2);
                        ControlFlow::Continue(())
                    }
                )
            );

            {
                let n = strm.await;
                println!("Got: {:?}", n);
            }
        })
        .await
        .unwrap_err();
    }

    static SMUGGLER: Mutex<Option<Sender<&'static u16>>> = Mutex::new(None);

    #[tokio::test(start_paused = true)]
    async fn test_miri_cap() {
        let (tx, rx) = channel();
        SMUGGLER.lock().unwrap().replace(tx);

        tokio::time::timeout(Duration::from_millis(200), async move {
            let tempcap = 42u16;
            let temp2 = 43u32;
            let mut strm = pin!(safe_select!(
                capture(tempcap, temp2),
                (
                    if (true) {
                        //println!("1 Tempcap:  {} : {}", tempcap, temp2);
                        //*tempcap += 1;
                        //SMUGGLER.lock().unwrap().as_mut().unwrap().send(tempcap);
                        //let tx = tx.clone();
                        *tempcap.get()? += 10;

                        println!("tempcap: {}", *tempcap.get()?);

                        //*tempcap += 1;
                        async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            //*tempcap += 1;
                            42u64
                        }
                    },
                    |pred1| ControlFlow::Break(pred1)
                )(
                    {
                        let mut temp = tempcap.get().unwrap();
                        *temp += 1;
                        println!("tempcap: {}", *temp);
                        async move {
                            tokio::time::sleep(Duration::from_millis(75)).await;
                            //*tempcap += 20;
                            43u32
                        }
                    },
                    |_pred2| {
                        //println!("2 Result Tempcap:  {} : {}", tempcap, temp2);

                        ControlFlow::Continue(())
                    }
                )
            ));

            {
                let n = strm.await;
                println!("Got: {:?}", n);
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn test2() {
        let mut a = 4u32;

        let t;
        {
            let mut b = &mut a;
            let t2 = &mut b;
            t = &mut *t2;
            //t = &mut b.0;
        }
        //println!("t: {}", t);
    }

    #[test]
    fn test_counter() {
        let abc = "abc";
        let def = "def";
        let temp = ord_cap!(abc, def);

        assert_eq!(temp, ((abc, 0), (def, 1)))
    }
}
