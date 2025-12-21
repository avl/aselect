use std::cell::UnsafeCell;
use futures::Stream;
use safeselect::safe_select;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio::signal;

//TODO: Not really a "timer" example any more
#[tokio::main]
async fn main() {

    let mut smuggle = Arc::new(Mutex::new(None));
    let smuggle2 = smuggle.clone();
    {
        let mut connection_attempts = UnsafeCell::new("hello".to_string());
        safe_select!(
            capture(connection_attempts),
            conn(
                {
                    //*connection_attempts.get()? += 1;
                    let counter = connection_attempts.get()?;
                    println!("Connection count: {:?}", counter);
                    smuggle.lock().unwrap().replace(counter);
                    async move {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                },
                |conn| {
                    println!("Continue");
                    Some(Some(())) //TODO: Fix double option here, quite unsightly!
                }
            )
        )
        .await;
    }

    // This should not compile
    println!("Smuggled: {:?}", **smuggle2.lock().unwrap().as_ref().unwrap());
}
