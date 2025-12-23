use std::pin::pin;
use safeselect::{safe_select, safe_select_context};
use std::time::Duration;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    safe_select_context!(State (
        port: Option<u16>,
        server: Option<TcpListener>,
        new_conn: Option<TcpStream>
    ));
    
    let listen_factory = || TcpListener::bind("127.0.0.1:0");

    fn staticer<T:'static>(_s: &'static T) {
    }

    let temp = safe_select!(
        State,
        create_listener(server,port)(
            {
                if port?.is_some() {
                    return None;
                };
                let listener = listen_factory();
            },
            async | server, port | {
                listener.await
            },
            |listener| {
                if let Ok(listener) = listener {
                    let listener: TcpListener = listener;
                    println!("New listener");
                    *port? = Some(listener.local_addr().unwrap().port());
                    *server? = Some(listener);
                }
                None
            }
        ),
        accept(new_conn, server)(
            {
                _ = server?.as_mut()?;
            },
            async | server | {
                server.as_mut().unwrap().accept().await.map(|x| x.0)
            },
            |accept_result| {
                match accept_result {
                    Ok(connection) => {
                        *new_conn? = Some(connection);
                        println!("Accepted");
                    }
                    Err(_) => {}
                }
                None
            }
        ),
        connect_and_send(port)(
            {
                let cur_port:u16 = (*(port?))?;
            },
            async | | {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let mut conn = TcpStream::connect(format!("127.0.0.1:{}", cur_port))
                    .await
                    .unwrap();
                conn.write_all(b"hello").await
            },
            |res2| {
                println!("Send result: {:?}", res2);
                None
            }
        ),
        receive(new_conn)(
            {
                let mut conn: TcpStream = new_conn?.take()?;
            },
            async | | {
                let mut buf = [0u8; 5];
                conn.read_exact(&mut buf).await.unwrap();
                String::from_utf8_lossy(&buf).to_string()
            },
            |res3| {
                let res3: String = res3;
                println!("Server received: {:?}", res3);
                Some(res3)
            }
        ),
    );
    let mut temp2 = pin!(temp);
    while let Some(val) = temp2.next().await {
        println!("Stream produced: {:?}", val);
    }
    temp2.await;

}
