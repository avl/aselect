use safeselect::{safe_select, safe_select_context};
use std::ops::ControlFlow;
use std::time::Duration;

#[tokio::main]
async fn main() {
    safe_select_context!(State (server: bool));

    safe_select!(
        State,
        accepted(
            |server|
            {
                if !*server? {
                    return None;
                };
                println!("Server");
            },
            async | |{
                println!("Do server stuff");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            },
            |result|{
                *server? = false;
                println!("Server future completed");
                None
            }
        ),
        connection_result(
             |server|{
                 if *server? {
                    return None;
                };
                println!("not server");
            },
            async | |{
                println!("Do client stuff");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            },
            |connection_result| {
                *server? = true;
                println!("Client future completed");
                None
            }
        ),
        timer(
            |server|{},
            async | | {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            },
            |timer| {
                println!("Timer");
                Some("finished")
            }
        ),
    )
    .await;
}
