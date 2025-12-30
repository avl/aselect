use std::pin::pin;
use futures::StreamExt;
use aselect::{aselect, Output};

#[tokio::main]
async fn main() {
    let counter = 0u32;
    let mut stream = pin!(aselect!(
        {
            mutable(counter);
        },
        timer(
            {
                tokio::time::sleep(std::time::Duration::from_millis(1))
            },
            async |fut1| {
                fut1.await;
            },
            |result| {
                *counter += 1;
                None
            }
        ),
        output(
            {
                tokio::time::sleep(std::time::Duration::from_millis(3))
            },
            async |fut2| {
                fut2.await;
            },
            |result| {
                if *counter > 10 {
                    Output::Terminate
                } else {
                    Output::Value(*counter)
                }
            }
        ),
    ));
    while let Some(item) = stream.next().await {
        println!("Value: {}", item);
    }
    println!("Done");
}
