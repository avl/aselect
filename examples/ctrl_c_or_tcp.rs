use futures::Stream;
use safeselect::safe_select;
use std::ops::ControlFlow;
use tokio::net::TcpStream;
use tokio::signal;

#[tokio::main]
async fn main() {
    let mut connection_attempts = 0u32;
    safe_select!(
        capture(connection_attempts),
        (async move { signal::ctrl_c().await }, |ctrl_c| {
            println!(
                "ctrl-c result: {:?}, after {} attempts",
                ctrl_c,
                *connection_attempts.get()?
            );
            ControlFlow::Break(())
        })(
            {
                *connection_attempts.get()? += 1;
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
                ControlFlow::Continue(())
            }
        )
    )
    .await;
}

async fn main2() {
    macro_rules! sel2 {
        (captures ($($id:ident),*), $( (if ($expr1:expr) $whatever: expr, |$name:ident |  $result: expr )$(,)?)*) => {};
    }

    sel2!(
        captures(a, b, c),
        (
            if (42.0) {
                println!(
                    "ctrl-c result: {:?}, after {} attempts",
                    ctrl_c, connection_attempts
                );
                ControlFlow::Break(())
            },
            |name| {
                println!("who");
            }
        ),
        (
            if (true) {
                println!(
                    "ctrl-c result: {:?}, after {} attempts",
                    ctrl_c, connection_attempts
                )
            },
            |name| {
                println!("who");
            }
        )
    );
}
