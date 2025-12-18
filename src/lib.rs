use futures::stream;
use std::any::Any;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::pin::{Pin, pin};
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::select;

pub trait SafeSelectable {}

struct SafeSelectFuture<
    C,
    R1,
    FA1: Future<Output = R1>,
    FB1: Future<Output = ()>,
    ARM1: FnMut(R1) -> FB1,
    R2,
    FA2: Future<Output = R2>,
    FB2: Future<Output = ()>,
    ARM2: FnMut(R2) -> FB2,
> {
    ctx: C,
    f1: FA1,
    arm1: ARM1,
    jobs1: Vec<FB1>,
    f2: FA2,
    arm2: ARM2,
    jobs2: Vec<FB2>,
}

/*compile_error!("only support mainloops, or support general case? How avoid frequent reconstruction?\
maybe return a Select type instead of a future? And allow that as subtype?

")*/

static START: Mutex<Option<Instant>> = Mutex::new(None);
async fn select_expanded() {
    let mut cnt1 = 0;
    let mut cnt2 = 0;

    let mut pred1 = None;
    let mut pred2 = None;
    let mut arm1 = None;
    let mut arm2 = None;
    START.lock().unwrap().replace(Instant::now());
    fn now() -> String {
        format!("{}ms", START.lock().unwrap().unwrap().elapsed().as_millis())
    };

    loop {
        let mut pred1ref = if arm1.is_some() {
            None
        } else {
            Some(pred1.get_or_insert_with(move || async move {
                if cnt1 == 10 {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
                tokio::time::sleep(Duration::from_millis(3)).await;
                println!("await1");
                cnt1 += 1;
                let jobcnt = cnt1;
                jobcnt
            }))
        };

        let mut pred2ref = pred2.get_or_insert_with(move || async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            println!("{} await2", now());

            cnt2 += 1;
            let jobcnt = cnt1;
            async move {
                println!("{}, job2 start {}", now(), jobcnt);
                tokio::time::sleep(Duration::from_millis(20)).await;
                println!("{} job2 finish {}", now(), jobcnt);
            }
        });

        let mut pred1pinref = pred1ref.map(|pred1ref| unsafe { Pin::new_unchecked(pred1ref) });
        let mut pred2pinref = unsafe { Pin::new_unchecked(pred2ref) };

        let mut scheduled_new_arm1 = None;
        let mut scheduled_new_arm2 = None;

        select! {
            jobcnt = async { pred1pinref.as_mut().unwrap().await }, if pred1pinref.is_some() => {
                println!("{} Pred1 ready!", now());
                pred1 = None;
                scheduled_new_arm1 = Some(
                    async move {
                        println!("{}, job1 start {}", now(), jobcnt);
                        tokio::time::sleep(Duration::from_millis(3)).await;
                        println!("{} job1 finish {}", now(), jobcnt);
                    }
                );
            },
            new_arm2 = pred2pinref => {
                println!("{} Pred2 ready!", now());
                pred2 = None;
                scheduled_new_arm2 = Some(new_arm2);
            },
            _ = async {
                    println!("{} Poll arm 1!", now());
                    unsafe {  Pin::new_unchecked(arm1.as_mut().unwrap()) }.await
                }, if (arm1.is_some()) => {
                arm1 = None;
            }
            _ = async {
                    println!("{} Poll arm 2!", now());
                    unsafe {  Pin::new_unchecked(arm2.as_mut().unwrap()) }.await
                }, if (arm2.is_some()) => {
                arm2 = None;
            }
        }
        if let Some(new_arm1) = scheduled_new_arm1 {
            println!("{} New arm 1!", now());
            arm1 = Some(new_arm1);
        }
        if let Some(new_arm2) = scheduled_new_arm2 {
            println!("{} New arm 2!", now());
            arm2 = Some(new_arm2);
        }
        /*
                pred1.poll()
                let arm1 = arm1.get_or_insert_with(||async {
                    tokio::time::sleep(Duration::from_millis(3)).await;
                    println!("arm1");
                });
                let arm2 = arm2.get_or_insert_with(||async {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    println!("arm");
                });
        */
    }
}

pub use futures::Stream;
pub trait Factory<CTX> {
    type Output;
    fn invoke(&mut self, ctx: &mut CTX) -> Self::Output;
}

impl<F,R, CTX> Factory<CTX> for F
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
    ( captures {$($cap: ident),*},   $($name: ident = |$ctxname:ident| $body: expr  => |$ctxname2:ident| $handler_body: expr ),*) => {
        {

            struct __SafeSelectCapture<$($cap),*> {
                $($cap: $cap, )*
            }

            $(
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

            pub struct __SafeSelectImpl<TOut, TCap, $($name),*> where
                $($name: $crate::NewFactory<TCap, TOut> ,)*
            {
                __captures: TCap,
                $($name: $name,)*
                phantom: ::std::marker::PhantomData<TOut>,
            }

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
                        fun: unify(move |$ctxname|{
                                $body
                            }, capptr),
                        fut: None,
                        decide: unify2(move |$ctxname2, $name|{
                                $handler_body
                            }, capptr),
                        phantom_cap: ::std::marker::PhantomData,
                    },
                    )*
            )
        }

    }


}


#[cfg(test)]
mod tests {
    use std::ops::ControlFlow;
    use crate::{select_expanded};
    use futures::StreamExt;
    use std::pin::pin;
    use std::time::Duration;

    #[tokio::test(start_paused = true)]
    async fn test() {
        tokio::time::timeout(
            Duration::from_secs(1),
            async move {
                let mut tempcap = 42u16;
                let mut temp2 = 43u32;
                let mut strm = pin!(safe_select!(
                    captures {
                        tempcap, temp2
                    },
                    pred1 = |ctx| {
                        println!("Tempcap:  {} : {}", ctx.tempcap, ctx.temp2);
                        ctx.tempcap += 2;
                        async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            42u64
                        }
                    } => |_ctx| ControlFlow::Break(pred1),
                    pred2 = |ctx2| {
                            ctx2.tempcap += 10;
                            async move {
                                tokio::time::sleep(Duration::from_millis(75)).await;
                                43u64
                          }
                    } => |ctx2|{
                        //ctx2.tempcap += 1;
                        //compile_error!("USe trick to make ctx2 actually usable!")
                        ControlFlow::Break(pred2)
                    }
                ));

                loop {
                    let n = unsafe { strm.next().await };
                    //strm.hej();
                    println!("Got: {:?}", n);
                }
            }, /*
               async move {
                   let mut strm = pin!(select_expanded3());
                   loop {
                       let n = strm.next().await;
                       println!("Got: {:?}", n);
                   }

               }*/
        )
        .await.unwrap_err();
    }
}
