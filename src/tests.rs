#![deny(clippy::undocumented_unsafe_blocks)]

use core::pin::pin;
use crate::{safe_select};
use futures::{Stream, StreamExt};
use tokio::select;
use tokio::time::{sleep, timeout, Instant};
use std::{dbg, println};
use std::future::Future;
use std::string::ToString;
use std::task::{Context, Waker};
use std::time::Duration;

#[tokio::test(start_paused = true)]
async fn minimal_usecase() {

    let counter = 0u32;
    let result = safe_select!(
        {
            // Capture variable 'counter'
            mutable(counter);
        },
        // First select arm. A unique name for each arm must be provided (`timer1`).
        // Sleeps 0.3 seconds, over and over.
        timer1(
            {
                // Print value of counter, then increment
                println!("Counter = {:?}", counter);
                *counter += 1;
                // Create a future. Will be available to async block below.
                sleep(Duration::from_millis(300))
            },
            async |sleep| {
                // 'sleep' is the future created above
                let sleep_start = Instant::now();
                sleep.await;
                // Value returned by this async block is given to block below
                sleep_start.elapsed()
            },
            |time_slept| {
                // Print value returned from future
                println!("Slept {:?}", time_slept);
                // Do not produce a result from the 'safe_select' future.
                None
            }
        ),
        // Second select arm.
        // Sleeps 1 second, then produces a value.
        timer2(
            {
                // Similar to above, but now sleep 10 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(1))
            },
            async |sleep| {
                sleep.await;
            },
            |time_slept| {
                println!("Timer 2 done");
                // After the 10 seconds have elapsed,
                Some("finished")
            }
        ),
    ).await;
    println!("Produced value: {}", result);
}

#[tokio::test(start_paused = true)]
async fn repeated_cancellation() {
    let mut fut = pin!(safe_select!(
        {
        },
        timer1(
            {
                timer2.cancel();
                sleep(Duration::from_millis(100))
            },
            async |sleep| {
                sleep.await;
            },
            |_temp| {
                timer2.cancel();
                None
            }
        ),
        timer2(
            {
                timer1.cancel();
                sleep(Duration::from_millis(100))
            },
            async |sleep| {
                sleep.await;
            },
            |_temp| {
                timer1.cancel();
                Some("finished")
            }
        ),
    ));
    for _ in 0..10 {
        let mut cx = Context::from_waker(&Waker::noop());
        _ = fut.as_mut().poll_next(&mut cx);
    }
}

#[tokio::test(start_paused = true)]
async fn use_all_capture_types() {

    let counter = 0u32;
    let borrowed = "Borrowed".to_string();
    let constant: u32 = 43;
    let result = safe_select!(
        {
            mutable(counter);
            constant(constant);
            borrowed(borrowed);
        },
        timer1(
            {
                dbg!(&counter, constant, &borrowed);
                (*counter) += 1;
                *borrowed? = "Set".to_string();
            },
            async |_unused, borrowed| {
                *borrowed = "Modified".to_string();
                sleep(Duration::from_secs(1)).await;
            },
            |time_slept| {
                dbg!(&counter, constant, &borrowed);
                (*counter) += 1;
                *borrowed? = "Set2".to_string();
                None
            }
        ),
        timer2(
            {
                tokio::time::sleep(tokio::time::Duration::from_secs(1))
            },
            async |sleep_fut| {
                sleep_fut.await;
                sleep(Duration::from_secs(1)).await;
                sleep(Duration::from_secs(1)).await;
            },
            |_unused| {
                println!("Timer 2 done");
                // After the 10 seconds have elapsed,
                Some("finished")
            }
        ),
    ).await;
    println!("Produced value: {}", result);
}

#[tokio::test(start_paused = true)]
async fn test_return_future() {

    fn subfunc() -> impl Stream<Item = ()> + Future<Output=()> {
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
async fn test_no_hang_if_always_ready_and_produce_no_value() {
    let abc = "abc";
    let def = "def";
    let ghi = "ghi";

    timeout(Duration::from_millis(10), safe_select!(
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
        )).await.unwrap_err();
}


#[tokio::test(start_paused = true)]
async fn test_no_hang_if_all_futures_disabled() {

    timeout(Duration::from_secs(1000), safe_select!(
            {
            },
            conn(
                {
                    return None;
                },
                async |fut_input| {
                },
                |conn2| {
                }
            ),
        )).await.unwrap_err();
}

#[tokio::test(start_paused = true)]
async fn test_regular_select() {
    select!(
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
            println!("Arm 1 start");
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            println!("Arm 2 done");
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {
            println!("Arm 2");
        }
    )
}
