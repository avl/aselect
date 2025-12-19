use safeselect::safe_select;
use std::ops::ControlFlow;
use std::time::Duration;

#[tokio::main]
async fn main() {
    struct State {
        server: bool,
    }

    let state = State { server: true };

    safe_select!(
        capture(state),
        (
            if (state.server) {
                println!("Server");
                async move {
                    println!("Do server stuff");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            },
            |accepted| {
                println!("Server future completed");
                state.server = false;
                ControlFlow::<()>::Continue(())
            }
        ),
        (
            if (!state.server) {
                println!("not server");
                async move {
                    println!("Do client stuff");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            },
            |connection_result| {
                state.server = true;
                println!("Client future completed");
                ControlFlow::<()>::Continue(())
            }
        ),
        (
            if (true) {async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }},
            |timer| {
            println!("Timer");
            ControlFlow::<()>::Continue(())
        }
        )
    )
    .await;
}
