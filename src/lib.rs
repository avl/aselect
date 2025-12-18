use futures::stream;
use std::any::Any;
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

macro_rules! safe_select {
    ( captures {$($cap: ident),*},   $($name: ident = |$ctxname:ident| $body: expr  => $handler_body: expr ),*) => {
        {


            struct __SafeSelectCapture<$($cap),*> {
                $($cap: $cap)*
            }


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


            pub struct __SafeSelectImpl<TOUT, TCap, $($name),*> where
                $($name: Factory<TCap> ,)*
                $($name::Output: Future<Output = TOUT>,)*
            {
                __captures: TCap,
                $($name: (Option<$name::Output>, $name),)*
            }

            impl<$($name),*  , TOUT, TCap> $crate::Stream for __SafeSelectImpl<TOUT, TCap,  $($name),*> where
                $($name: Factory<TCap> ,)*
                $($name::Output: Future<Output = TOUT>,)*
            {
                type Item = TOUT;

                fn poll_next(self: ::std::pin::Pin<&mut Self>, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<Option<Self::Item>> {
                    let mut this = unsafe { self.get_unchecked_mut() };
                    $(
                    if this.$name.0.is_none() {
                        this.$name.0 = Some(this.$name.1.invoke(&mut this.__captures));
                    }
                    )*
                    $(
                    match (unsafe { ::std::pin::Pin::new_unchecked(this.$name.0.as_mut().unwrap()) }).poll(cx) {
                        ::std::task::Poll::Ready(val) => {
                            this.$name.0 = None;
                            match $handler_body(&mut this.__captures, val) {
                                ::std::ops::ControlFlow::Break(val) => return ::std::task::Poll::Ready(Some(val)),
                                ::std::ops::ControlFlow::Continue(()) => {}
                            }
                        }
                        _ => {}
                    }
                    )*
                    ::std::task::Poll::Pending
                }
            }

            __SafeSelectImpl {
                __captures: __SafeSelectCapture {
                    $($cap: $cap, )*
                },
                $(
                $name: (None,
                        move |$ctxname: &mut __SafeSelectCapture<_>|{
                            $body
                            /*async move {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                42u64
                            }*/

                        }),
                )*
            }
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
                let mut strm = pin!(safe_select!(
                    captures {
                        tempcap
                    },
                    pred1 = |ctx| {
                        println!("Tempcap:  {}", ctx.tempcap);
                        ctx.tempcap += 2;
                        async move {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            42u64
                        }
                    } => |ctx, val| ControlFlow::Break(val),
                    pred2 = |ctx2| {
                            ctx2.tempcap += 10;
                            async move {
                                tokio::time::sleep(Duration::from_millis(75)).await;
                                43u64
                          }
                    } => |ctx2, val|{
                        compile_error!("USe trick to make ctx2 actually usable!")
                        ControlFlow::Continue(())
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
