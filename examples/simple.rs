use std::pin::pin;
use futures::StreamExt;
use aselect::{safe_select};

#[tokio::main]
async fn main() {
    let counter = 0u32;
    let mut stream = pin!(safe_select!(
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
                Some(*counter)
            }
        ),
    ));
    while let Some(item) = stream.next().await {
        println!("Value: {}", item);
    }
}
