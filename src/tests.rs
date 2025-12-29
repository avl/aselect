use std::pin::pin;
use crate::safe_select;
use futures::{Stream, StreamExt};

#[tokio::test(start_paused = true)]
async fn test_miri_cap() {

    fn subfunc() -> impl Stream<Item = ()> {
        let value = 42u32;
        let constval = 1;
        let mutval = 2;
        let mutval2 = 2;
        safe_select!(
            {
                borrowed(value);
                constant(constval);
                mutable(mutval, mutval2);
            },
            conn(
                {
                    println!("{:?}: {:?} {:?}", value, constval, mutval);
                    *value? = 43;
                    "input to future".to_string()
                },
                async |fut_input, value| {
                    println!(
                        "Future input: {} Value: {:?}, Const val: {}",
                        fut_input, value, constval
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    44u64
                },
                |conn2| {
                    println!("Continue: {:?}, {:?}", value, conn2);
                    Some(())
                }
            ),
        )
    }

    let mut t = pin!(subfunc());

    let _t = t.next().await;
}

#[tokio::test]
#[should_panic(expected = "Too many iterations with neither pending futures nor a value produced")]
async fn test_panic_on_no_pending() {
    let abc = "abc";
    let def = "def";
    let ghi = "ghi";

    safe_select!(
            {
                borrowed(abc);
                constant(def);
                mutable(ghi);
            },
            conn(
                {
                    _ = abc;
                    _ = def;
                    _ = ghi;
                },
                async |fut_input, abc| {
                    let ghi = ghi;
                },
                |conn2| {

                }
            ),
        ).await;
}


//TODO: Remove
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
