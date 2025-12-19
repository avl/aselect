use std::cell::UnsafeCell;
pub use futures::Stream;
use std::ops::{ControlFlow, Deref, DerefMut};
use std::pin::pin;
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

pub trait NewFactory<'a,CTX, TOut> {
    /// Returns true if future was ready
    /// If it was ready, it may have produced a value (ControlFlow::Break) or not (ControlFlow::Pending)
    fn do_poll(&mut self, ctx: &'a CTX, cx: &mut ::std::task::Context<'_>) -> Option<ControlFlow<TOut>>;
}

pub struct Capture<T> {
    #[doc(hidden)]
    pub lock: UnsafeCell<bool>,
    #[doc(hidden)]
    pub value: UnsafeCell<T>,
    pub num: usize,
}
pub struct CaptureGuard<'a, T> {
    #[doc(hidden)]
    pub lock: &'a UnsafeCell<bool>,
    #[doc(hidden)]
    pub value: &'a UnsafeCell<T>
}

impl<T> Capture<T> {
    pub fn get<'a>(&'a self) -> Option<CaptureGuard<'a,T>> {
        let lock = unsafe {&mut *self.lock.get()};
        if *lock {
            return None;
        }
        Some(CaptureGuard {
            lock: &self.lock,
            value: &self.value,
        })
    }
}
impl<'a, T> Deref for CaptureGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.value.get() }
    }
}

impl<'a, T> DerefMut for CaptureGuard<'a, T> {

    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.value.get() }
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
        ord_cap2!($typname, inner 0, parsed $($cap)*)
    };
    ($typname: ident,inner $depth:expr, $(($cap0:ident, $count:expr))* parsed $cap:ident $($cap1:ident)*) => {
        ord_cap2!($typname, inner ($depth + 1), $(($cap0, $count) )* ($cap, $depth) parsed $($cap1),* )
    };

    ($typname: ident,inner $depth:expr, $(($cap:ident, $count:expr))* parsed) => {

        $typname {
            $($cap: $crate::Capture {value: ::std::cell::UnsafeCell::new($cap), lock: ::std::cell::UnsafeCell::new(false), num: ($count)}, )*
        }

    };
}



#[macro_export]
macro_rules! safe_select {
    /*
    ( capture ($($cap: ident),*), ( if ( $ifexpr:expr ) $body: expr, |$name: ident| $handler_body: expr) $(,)?  $( ( if ( $ifexpr2:expr ) $body2: expr, |$name2: ident| $handler_body2: expr) $(,)? )+  $( ( $ifexpr3:expr, $body3:expr,  $name3:ident,  $handler_body3:expr ) )* ) => {
      safe_select!( capture ( $($cap),*), $( ( if ( $ifexpr2 ) $body2, |$name2| $handler_body2)  )*  ( inner, $ifexpr, $body,  $name,  $handler_body ) $( ( inner, $ifexpr3, $body3,  $name3,  $handler_body3 ) )* )
    };
    ( capture ($($cap: ident),*), (                     $body: expr, |$name: ident| $handler_body: expr) $(,)?  $( ( if ( $ifexpr2:expr ) $body2: expr, |$name2: ident| $handler_body2: expr) $(,)? )+  $( ( $ifexpr3:expr, $body3:expr,  $name3:ident,  $handler_body3:expr ) )* ) => {
      safe_select!( capture ( $($cap),*), $( ( if ( $ifexpr2 ) $body2, |$name2| $handler_body2)  )*  ( inner, true,    $body,  $name,  $handler_body ) $( ( inner, $ifexpr3, $body3,  $name3,  $handler_body3 ) )* )
    };
    ( capture ($($cap: ident),*),  $( ( inner, $ifexpr:expr, $body: expr, $name: ident, $handler_body: expr)  )* ) => {
        safe_select!( inner capture ( __SafeSelectCapture { $($cap,)* } ), ($($cap),*), $( ( $ifexpr:expr, $body: expr, $name: ident, $handler_body: expr)  )* )
    };
     */
    ( capture ($($cap: ident),*), $($tail:tt)*  ) => {
      safe_select!(partial ($($cap),*), parsed $($tail)*)
    };
    ( partial ($($cap: ident),*), $( ( $ifexpr0:expr, $body0: expr, $name0: ident, $handler_body0: expr)  )* parsed ( if ( $ifexpr:expr ) $body: expr, |$name: ident| $handler_body: expr) $($tail:tt)*  ) => {
        safe_select!( partial ($($cap),*), $(($ifexpr0, $body0, $name0, $handler_body0))* ($ifexpr, $body, $name, $handler_body) parsed $($tail)*)
    };

    ( partial ($($cap: ident),*), $( ( $ifexpr0:expr, $body0: expr, $name0: ident, $handler_body0: expr)  )* parsed ( $body: expr, |$name: ident| $handler_body: expr) $($tail:tt)*  ) => {
        safe_select!( partial ($($cap),*), $(($ifexpr0, $body0, $name0, $handler_body0))* (true, $body, $name, $handler_body) parsed $($tail)*)
    };

    ( partial ($($cap: ident),*), $( ( $ifexpr:expr,  $body: expr, $name: ident, $handler_body: expr)  )* parsed ) => {
        safe_select!( innerest capture ( __SafeSelectCapture { $($cap,)* } ), ($($cap),*), $( ( $ifexpr, $body, $name, $handler_body)  )* )
    };
    ( innerest capture ($capassign: pat), ($($cap: ident),*), $( ( $ifexpr:expr, $body: expr, $name: ident, $handler_body: expr)  )* ) => {
        async move {

            #[allow(nonstandard_style)]
            struct __SafeSelectCapture<$($cap),*> {
                $($cap: $crate::Capture<$cap>, )*
            }


            $(
                #[allow(nonstandard_style)]
                struct $name<'a, R, TOut, TCap, TFun, TDecide, TCond> where
                    TFun: FnMut(&'a TCap) -> R,
                    R: Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output) -> ::std::ops::ControlFlow<TOut>,
                    TCond: FnMut(&'a TCap) -> bool,
                {
                    fun: TFun,
                    fut: Option<R>,
                    decide: TDecide,
                    cond: TCond,
                    phantom_cap: ::std::marker::PhantomData<&'a TCap>,
                }

                /// Return Some if future was ready
                #[allow(nonstandard_style)]
                impl<'a, R, TOut, TCap, TFun,TDecide,TCond> $crate::NewFactory<'a, TCap, TOut> for $name<'a, R, TOut, TCap, TFun, TDecide,TCond> where
                    TFun: FnMut(&'a TCap) -> R,
                    R: Future+'a,
                    TDecide: FnMut(&'a TCap, R::Output) -> ::std::ops::ControlFlow<TOut>,
                    TCond: FnMut(&'a TCap) -> bool,
                {
                    fn do_poll(&mut self, ctx: &'a TCap, cx: &mut ::std::task::Context<'_>) -> Option<::std::ops::ControlFlow<TOut>> {
                        //println!("Polling: {:?}", self.fut.is_some());
                        let mut was_ready = false;

                        loop {
                            if self.fut.is_none() {
                                if !(self.cond)(ctx) {
                                    return was_ready.then_some(::std::ops::ControlFlow::Continue(()));
                                }
                                self.fut = Some((self.fun)(ctx));
                            }

                            let fut = self.fut.as_mut().unwrap();
                            match unsafe { ::std::pin::Pin::new_unchecked(fut) }.poll(cx) {
                                ::std::task::Poll::Ready(out) => {
                                    was_ready = true;
                                    //println!("Ready!");
                                    self.fut = None;
                                    match (self.decide)(ctx, out) {
                                        c@::std::ops::ControlFlow::Break(_) => {
                                            return Some(c);
                                        }
                                        ::std::ops::ControlFlow::Continue(()) => {
                                            continue;
                                        }
                                    }
                                }
                                ::std::task::Poll::Pending => {
                                    return was_ready.then_some(::std::ops::ControlFlow::Continue(()));
                                },
                            }
                        }
                    }
                }
            )*

            #[allow(nonstandard_style)]
            pub struct __SafeSelectImpl<'a, TOut, TCap, $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                __captures: &'a TCap,
                $($name: $name,)*
                phantom: ::std::marker::PhantomData<TOut>,
            }

            #[allow(nonstandard_style)]
            impl<'a, TOut, TCap, $($name),*> $crate::Stream for __SafeSelectImpl<'a, TOut, TCap,  $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                type Item = TOut;

                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<Self::Item>> {
                    let this = unsafe { self.get_unchecked_mut() };

                    let mut unready = 0;
                    let mut totcount = 0;
                    $(
                        _ = &this.$name;
                        totcount += 1;
                    )*

                    loop {
                        $(
                            let cap = &mut this.__captures;
                            if let Some(ready) = this.$name.do_poll(cap, cx) {
                                if let ::std::ops::ControlFlow::Break(val) = ready {
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
            impl<'a, TOut, TCap, $($name),*> ::std::future::Future for __SafeSelectImpl<'a, TOut, TCap,  $($name),*> where
                $($name: $crate::NewFactory<'a, TCap, TOut> ,)*
            {
                type Output = TOut;

                fn poll(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Self::Output> {
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
            fn unifyb<'a, R, TCap:'a, F: FnMut(&'a TCap) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }
            fn unify2<'a, R, V, TCap:'a, F: FnMut(&'a TCap, V) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }


            let cap = ord_cap2!(__SafeSelectCapture, $($cap)*)
            ;

            let capptr: *const _ = &cap;
            __SafeSelectImpl{
                __captures: &cap,
                phantom: ::std::marker::PhantomData,
                    $(
                    $name: $name {
                        fun: unify(move |temp|{
                                #[allow(unused)]
                                let $capassign = temp;
                                $body
                            }, capptr),
                        fut: None,
                        decide: unify2(move |temp, $name|{
                                #[allow(unused)]
                                let $capassign = temp;
                                $handler_body
                            }, capptr),
                        cond: unifyb(move |temp|{
                                #[allow(unused)]
                                let $capassign = temp;
                                let guard_value: bool = $ifexpr;
                                guard_value
                        }, capptr),
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            }.await
        }

    }


}


mod tests {
    use futures::Stream;
    use futures::StreamExt;
    use std::ops::ControlFlow;
    use std::pin::pin;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;
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
                )
                (
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

        let (tx,rx) = channel();
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

                            //*tempcap += 1;
                            async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            //*tempcap += 1;
                            42u64
                        }
                    },
                    |pred1| ControlFlow::Break(pred1)
                )
                (
                    {
                        //*tempcap += 20;
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

    compile_error!("Continue")

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
        let temp = ord_cap!(abc,def);

        assert_eq!(
            temp,
            ((abc, 0),
            (def, 1))
        )
    }
}
