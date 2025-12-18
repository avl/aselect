use std::ops::ControlFlow;
use std::pin::pin;
use std::time::Duration;
pub use futures::Stream;

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
    fn do_poll(&mut self, ctx: &mut CTX, cx: &mut ::std::task::Context<'_>) -> ControlFlow<TOut>;
}

macro_rules! safe_select {
    ( captures {$($cap: ident),*},   $($name: ident =  $body: expr  =>  $handler_body: expr ),*) => {
      safe_select!( inner captures ( __SafeSelectCapture { $($cap,)* }   ), {$($cap),*},   $($name =  $body  =>  $handler_body ),* )
    };
    ( inner captures ($capassign: pat), {$($cap: ident),*},   $($name: ident =  $body: expr  =>  $handler_body: expr ),*) => {
        {

            #[allow(nonstandard_style)]
            struct __SafeSelectCapture<$($cap),*> {
                $($cap: $cap, )*
            }

            $(
                #[allow(nonstandard_style)]
                struct $name<R, TOut, TCap, TFun, TDecide> where
                    TFun: FnMut(&mut TCap) -> R,
                    R: Future,
                    TDecide: FnMut(&mut TCap, R::Output) -> ControlFlow<TOut>,
                {
                    fun: TFun,
                    fut: Option<R>,
                    decide: TDecide,
                    phantom_cap: ::std::marker::PhantomData<TCap>,
                }

                #[allow(nonstandard_style)]
                impl<R, TOut, TCap, TFun,TDecide> $crate::NewFactory<TCap, TOut> for $name<R, TOut, TCap, TFun, TDecide> where
                    TFun: FnMut(&mut TCap) -> R,
                    R: Future<Output = TOut> + 'static,
                    TDecide: FnMut(&mut TCap, R::Output) -> ControlFlow<TOut>,
                {
                    fn do_poll(&mut self, ctx: &mut TCap, cx: &mut ::std::task::Context<'_>) -> ControlFlow<TOut>  {
                        if self.fut.is_none() {
                            self.fut = Some((self.fun)(ctx));
                        }

                        let fut = self.fut.as_mut().unwrap();
                        match unsafe { ::std::pin::Pin::new_unchecked(fut) }.poll(cx) {
                            ::std::task::Poll::Ready(out) => {
                                self.fut = None;
                                let res = (self.decide)(ctx, out);
                                res
                            }
                            ::std::task::Poll::Pending => {
                                ControlFlow::Continue(())
                            },
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

                    $(
                        if let ControlFlow::Break(val) = this.$name.do_poll(&mut this.__captures, cx) {
                            return ::std::task::Poll::Ready(Some(val));
                        }
                    )*
                    ::std::task::Poll::Pending
                }
            }

            #[allow(nonstandard_style)]
            fn assemble<TOut, TCap, $($name,)*> (thecap: TCap, $($name:$name ,)*) -> __SafeSelectImpl<TOut, TCap, $($name),*> where
                $($name: $crate::NewFactory<TCap, TOut> ,)* {
                    __SafeSelectImpl {
                    __captures: thecap,
                    phantom: ::std::marker::PhantomData,
                    $(
                    $name,
                    )*
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
            assemble(
                cap,
                    $(
                    $name {
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
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            )
        }

    }


}

pub async fn test() {
    use futures::StreamExt;
    tokio::time::timeout(Duration::from_secs(1), async move {
        let tempcap = 42u16;
        let temp2 = 43u32;
        let mut strm = pin!(safe_select!(
                captures {
                    tempcap, temp2
                },
                pred1 =  {
                    //println!("Tempcap:  {} : {}", ctx.tempcap, ctx.temp2);
                    *tempcap += 2;
                    async move {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        42u64
                    }
                } =>  ControlFlow::Break(pred1),
                pred2 =  {
                        *tempcap += 10;
                        async move {
                            tokio::time::sleep(Duration::from_millis(75)).await;
                            43u64
                      }
                } => {
                    ControlFlow::Break(pred2)
                }
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
                captures {
                    tempcap, temp2
                },
                pred1 =  {
                    println!("Tempcap:  {} : {}", tempcap, temp2);
                    *tempcap += 2;
                    async move {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        42u64
                    }
                } =>  ControlFlow::Break(pred1),
                pred2 = {
                        *tempcap += 10;
                        async move {
                            tokio::time::sleep(Duration::from_millis(75)).await;
                            43u64
                      }
                } => {
                    println!("Result Tempcap:  {} : {}", tempcap, temp2);
                    ControlFlow::Continue(())
                }
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
