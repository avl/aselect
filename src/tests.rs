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

#[test]
fn test_counter() {

    let abc = "abc";
    let def = "def";
    let temp = ord_cap!(abc, def);

    assert_eq!(temp, ((abc, 0), (def, 1)))
}
