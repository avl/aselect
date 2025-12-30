use std::pin::pin;
use std::time::Duration;
use futures::StreamExt;
use tokio::time::{sleep, Instant};
use safeselect::safe_select;

#[tokio::main]
async fn main() {

    let counter = 0u32;
    let mut stream = pin!(safe_select!(
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
                // After the 10 seconds have elapsed,
                Some("finished")
            }
        ),
    ));
    while let Some(value) = stream.next().await {
        println!("Produced value: {}", value);
    }
}
