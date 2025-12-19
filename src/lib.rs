pub use futures::Stream;
use std::ops::ControlFlow;
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

pub trait NewFactory<CTX, TOut> {
    /// Returns true if future was ready
    /// If it was ready, it may have produced a value (ControlFlow::Break) or not (ControlFlow::Pending)
    fn do_poll(&mut self, ctx: &mut CTX, cx: &mut ::std::task::Context<'_>) -> Option<ControlFlow<TOut>>;
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
        {

            #[allow(nonstandard_style)]
            struct __SafeSelectCapture<$($cap),*> {
                $($cap: $cap, )*
            }

            $(
                #[allow(nonstandard_style)]
                struct $name<R, TOut, TCap, TFun, TDecide, TCond> where
                    TFun: FnMut(&mut TCap) -> R,
                    R: Future,
                    TDecide: FnMut(&mut TCap, R::Output) -> ::std::ops::ControlFlow<TOut>,
                    TCond: FnMut(&mut TCap) -> bool,
                {
                    fun: TFun,
                    fut: Option<R>,
                    decide: TDecide,
                    cond: TCond,
                    phantom_cap: ::std::marker::PhantomData<TCap>,
                }

                /// Return Some if future was ready
                #[allow(nonstandard_style)]
                impl<R, TOut, TCap, TFun,TDecide,TCond> $crate::NewFactory<TCap, TOut> for $name<R, TOut, TCap, TFun, TDecide,TCond> where
                    TFun: FnMut(&mut TCap) -> R,
                    R: Future + 'static,
                    TDecide: FnMut(&mut TCap, R::Output) -> ::std::ops::ControlFlow<TOut>,
                    TCond: FnMut(&mut TCap) -> bool,
                {
                    fn do_poll(&mut self, ctx: &mut TCap, cx: &mut ::std::task::Context<'_>) -> Option<::std::ops::ControlFlow<TOut>> {
                        println!("Polling: {:?}", self.fut.is_some());
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
                                    println!("Ready!");
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
            pub struct __SafeSelectImpl<TOut, TCap, $($name),*> where
                $($name: $crate::NewFactory<TCap, TOut> ,)*
            {
                __captures: TCap,
                $($name: $name,)*
                phantom: ::std::marker::PhantomData<TOut>,
            }

            #[allow(nonstandard_style)]
            impl<TOut, TCap, $($name),*> $crate::Stream for __SafeSelectImpl<TOut, TCap,  $($name),*> where
                $($name: $crate::NewFactory<TCap, TOut> ,)*
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
                            if let Some(ready) = this.$name.do_poll(&mut this.__captures, cx) {
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
            impl<TOut, TCap, $($name),*> ::std::future::Future for __SafeSelectImpl<TOut, TCap,  $($name),*> where
                $($name: $crate::NewFactory<TCap, TOut> ,)*
            {
                type Output = TOut;

                fn poll(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Self::Output> {
                    println!("Future poll");
                    use $crate::Stream;
                    match self.poll_next(cx) {
                        ::std::task::Poll::Ready(Some(val)) => ::std::task::Poll::Ready(val),
                        _ => ::std::task::Poll::Pending,
                    }
                }
            }

            fn unify<R, TCap, F: FnMut(&mut TCap) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }
            fn unify2<R, V, TCap, F: FnMut(&mut TCap, V) -> R>(func: F, cap: *const TCap) -> F {
                _ = cap;
                func
            }

            let cap = __SafeSelectCapture {
                    $($cap: $cap, )*
                };
            let capptr: *const _ = &cap;
            __SafeSelectImpl{
                __captures: cap,
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
                        cond: unify(move |temp|{
                                #[allow(unused)]
                                let $capassign = temp;
                                let guard_value: bool = $ifexpr;
                                guard_value
                        }, capptr),
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            }
        }

    }


}

#[cfg(test)]
pub async fn test() {
    use futures::StreamExt;
    tokio::time::timeout(Duration::from_secs(1), async move {
        let tempcap = 42u16;
        let temp2 = 43u32;
        let mut strm = pin!(safe_select!(
            capture (tempcap, temp2),
            (
                if (true) {
                    //println!("Tempcap:  {} : {}", ctx.tempcap, ctx.temp2);
                    *tempcap += 2;
                    async move {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        42u64
                    }
                },
                |pred1| ControlFlow::Break(pred1)
            )
            (
                if (true) {
                    *tempcap += 10;
                    async move {
                        tokio::time::sleep(Duration::from_millis(75)).await;
                        43u64
                    }
                },
                |pred2|  ControlFlow::Break(pred2)
            )
        ));

        loop {
            let n = strm.next().await;
            println!("Got: {:?}", n);
        }
    })
    .await
    .unwrap_err();
}

mod tests {
    use futures::Stream;
    use futures::StreamExt;
    use std::ops::ControlFlow;
    use std::pin::pin;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn test() {
        tokio::time::timeout(Duration::from_secs(1), async move {
            let tempcap = 42u16;
            let temp2 = 43u32;
            let mut strm = pin!(safe_select!(
                capture(tempcap, temp2),
                (
                    if (true) {
                        println!("1 Tempcap:  {} : {}", tempcap, temp2);
                        *tempcap += 2;
                        async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            42u64
                        }
                    },
                    |pred1| ControlFlow::Break(pred1)
                )
                (
                    {
                        *tempcap += 10;
                        async move {
                            tokio::time::sleep(Duration::from_millis(75)).await;
                            43u32
                        }
                    },
                    |_pred2| {
                        println!("2 Result Tempcap:  {} : {}", tempcap, temp2);
                        ControlFlow::Continue(())
                    }
                )
            ));

            loop {
                let n = strm.next().await;
                println!("Got: {:?}", n);
            }
        })
        .await
        .unwrap_err();
    }
}
