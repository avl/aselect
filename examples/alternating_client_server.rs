use aselect::{aselect};

#[tokio::main]
async fn main() {
    let server = false;
    aselect!(
        {
            mutable(server);
        },
        accepted(
            {
                let mut server : &mut bool = server;
                if !*server {
                    println!("Not time to be server");
                    return None;
                };
                println!("Server");
            },
            async | _input |{
                println!("Do server stuff");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                println!("Done server stuff");
            },
            |result|{
                *server = false;
                println!("Server future completed");
                None
            }
        ),
        connection_result(
             {
                 if *server {
                    println!("Not time to be client");
                    return None;
                };
                println!("not server");
            },
            async | _input |{
                println!("Do client stuff");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                println!("Done client stuff");
            },
            |connection_result| {
                *server = true;
                println!("Client future completed");
                None
            }
        ),
        timer(
            {},
            async | _input | {
                tokio::time::sleep(tokio::time::Duration::from_secs(1000)).await;
            },
            |timer| {
                println!("Timer");
                Some("finished")
            }
        ),
    )
    .await;
}
