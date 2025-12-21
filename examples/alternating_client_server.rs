use safeselect::safe_select;
use std::ops::ControlFlow;
use std::time::Duration;

#[tokio::main]
async fn main() {
    struct State {
        server: bool,
    }

    let mut state = State { server: true };

    safe_select!(
        capture(state),
        accepted(
            {
                if !state.get()?.server {
                    return None;
                }
                println!("Server");
                async move {
                    println!("Do server stuff");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            },
            |accepted| {
                println!("Server future completed");
                state.get()?.server = false;
                None::<Option<()>>
            }
        ),
        connection_result(
             {
                 if (state.get()?.server) {
                    return None
                }
                println!("not server");
                async move {
                    println!("Do client stuff");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            },
            |connection_result| {
                state.get()?.server = true;
                println!("Client future completed");
                None
            }
        ),
        timer(
            {
                async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            },
            |timer| {
                println!("Timer");
                None
            }
        )
    )
    .await;
}
