use safeselect::{safe_select};
use tokio::net::TcpStream;
use tokio::signal;

#[tokio::main]
async fn main() {
    let connection_attempts = 0u32;
    {
        safe_select!(
            {mutable(connection_attempts);},
            ctrl_c(
                 {},
                async | _fut | { signal::ctrl_c().await },
                |ctrl_c| {
                    println!(
                        "ctrl-c result: {:?}, after {} attempts",
                        ctrl_c,
                        *connection_attempts
                    );
                    Some(())
                }
            ),
            conn(
                 {
                    *connection_attempts += 1;
                },
                async | _fut | {
                    let conn = TcpStream::connect(
                        std::env::args()
                            .nth(1)
                            .unwrap_or("127.0.0.1:80".to_string()),
                    )
                    .await;
                    println!("Connection result: {:?}", conn);

                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                },
                |conn| {
                    println!("Continue");
                    None
                }
            ),
        )
        .await;
    }
}
