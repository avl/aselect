use safeselect::safe_select;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    #[derive(Default)]
    struct State {
        port: Option<u16>,
    }

    let mut state = State::default();

    let listen_factory = || TcpListener::bind("127.0.0.1:0");

    let mut server = None;
    let mut new_conn = None;

    fn staticer<T:'static>(_s: &'static T) {
    }

    let mut temp = safe_select!(
        capture(state, server, new_conn),
        acceptor(
            {
                if state.get()?.port.is_some() {
                    return None;
                };
                listen_factory()
            },
            |acceptor| {
                if let Ok(listener) = acceptor {
                    let listener: TcpListener = listener;
                    println!("New listener");
                    state.get()?.port = Some(listener.local_addr().unwrap().port());
                    *server.get()? = Some(listener);
                }
                None::<Option<()>>
            }
        )accept_result(
            {
                let mut server = server.get_some()?;

                async move { server.accept().await.map(|x| x.0) }
            },
            |accept_result| {
                match accept_result {
                    Ok(connection) => {
                        *new_conn.get()? = Some(connection);
                        println!("Accepted");
                    }
                    Err(_) => {}
                }
                None
            }
        )res2(
            {
                let port = state.get()?.port?;
                async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let mut conn = TcpStream::connect(format!("127.0.0.1:{}", port))
                        .await
                        .unwrap();
                    conn.write_all(b"hello").await
                }
            },
            |res2| {
                println!("Send result: {:?}", res2);
                None
            }
        )res3(
            {
                let mut conn: TcpStream = new_conn.take()?;
                async move {
                    let mut buf = [0u8; 5];
                    conn.read_exact(&mut buf).await.unwrap();
                    String::from_utf8_lossy(&buf).to_string()
                }
            },
            |res3| {
                println!("Server received: {:?}", res3);
                None
            }
        )
    );
    let temp2 = temp;
    temp2.await;

}
