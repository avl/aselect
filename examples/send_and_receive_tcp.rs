use futures::StreamExt;
use aselect::safe_select;
use std::pin::pin;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    let port: Option<u16> = None;
    let server: Option<TcpListener> = None;
    let new_conn: Option<TcpStream> = None;

    let listen_factory = || TcpListener::bind("127.0.0.1:0");

    fn staticer<T: 'static>(_s: &T) {}

    let temp = safe_select!(
        {
            mutable(port, new_conn);
            borrowed(server);
        },
        create_listener(
            {
                let port: &mut Option<_> = port;
                if port.is_some() {
                    return None;
                };
                listen_factory()
            },
            async |listen_factory_fut| { listen_factory_fut.await },
            |listener| {
                if let Ok(listener) = listener {
                    let listener: TcpListener = listener;
                    println!("New listener");
                    *port = Some(listener.local_addr().unwrap().port());
                    *server? = Some(listener);
                }
                None
            }
        ),
        accept(
            {
                if server.is_none() {
                    return None;
                }
            },
            async |_input, server| { server.as_mut().unwrap().accept().await.map(|x| x.0) },
            |accept_result| {
                match accept_result {
                    Ok(connection) => {
                        *new_conn = Some(connection);
                        println!("Accepted");
                    }
                    Err(_) => {}
                }
                None
            }
        ),
        connect_and_send(
            {
                let cur_port: u16 = (*port)?;
                cur_port
            },
            async |cur_port| {
                let mut conn = TcpStream::connect(format!("127.0.0.1:{}", cur_port))
                    .await
                    .unwrap();
                let result = conn.write_all(b"hello").await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                result
            },
            |res2| {
                println!("Send result: {:?}", res2);
                None
            }
        ),
        receive(
            {
                let mut conn: TcpStream = new_conn.take()?;
                Some(conn)
            },
            async |conn| {
                let mut buf = [0u8; 5];
                conn.unwrap().read_exact(&mut buf).await.unwrap();
                String::from_utf8_lossy(&buf).to_string()
            },
            |res3| {
                let res3: String = res3;
                println!("Server received: {:?}", res3);
                Some(res3)
            }
        ),
    );
    staticer(&temp);
    let mut temp2 = pin!(temp);
    while let Some(val) = temp2.next().await {
        println!("Stream produced: {:?}", val);
    }
    temp2.await;
}
