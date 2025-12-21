use futures::Stream;
use safeselect::safe_select;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio::signal;

#[tokio::main]
async fn main() {

    let mut smuggle = Arc::new(Mutex::new(None));
    let smuggle2 = smuggle.clone();
    {
        let mut connection_attempts = 0u32;
        safe_select!(
            capture(connection_attempts),
            ctrl_c(async move { signal::ctrl_c().await }, |ctrl_c| {
                println!(
                    "ctrl-c result: {:?}, after {} attempts",
                    ctrl_c,
                    *connection_attempts.get()?
                );
                Some(Some(()))
            }),
            conn(
                {
                    *connection_attempts.get()? += 1;
                    let counter = *connection_attempts.get()?;
                    smuggle.lock().unwrap().replace(counter);
                    async move {
                        let conn = TcpStream::connect(
                            std::env::args()
                                .nth(1)
                                .unwrap_or("127.0.0.1:80".to_string()),
                        )
                        .await;
                        println!("Connection result: {:?}", conn);

                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                },
                |conn| {
                    println!("Continue");
                    None
                }
            )
        )
        .await;
    }
    println!("Smuggled: {:?}", smuggle2.lock().unwrap());
}
