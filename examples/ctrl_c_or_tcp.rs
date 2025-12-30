use aselect::{safe_select};
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
                |ctrl_c_result| {
                    // Cancel the 'conn' arm.
                    conn.cancel();
                    println!(
                        "ctrl-c result: {:?}, after {} attempts",
                        ctrl_c_result,
                        *connection_attempts
                    );
                    (*connection_attempts > 5).then_some(())
                }
            ),
            conn(
                 {
                    *connection_attempts += 1;
                },
                async | _fut | {
                    println!("Reconnecting");
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
    println!("Done");
}
