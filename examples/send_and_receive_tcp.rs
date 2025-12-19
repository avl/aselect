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
        server: Option<TcpListener>,
        new_conn: Option<TcpStream>,
    }

    let mut state = State::default();

    let listen_factory = || TcpListener::bind("127.0.0.1:0");

    safe_select!(
        capture(state),
        (
            if (state.get().unwrap().port.is_none()) {
                listen_factory()
            },
            |acceptor| {
                if let Ok(listener) = acceptor {
                    let listener: TcpListener = listener;
                    println!("New listner");
                    state.get().unwrap().port = Some(listener.local_addr().unwrap().port());
                    state.get().unwrap().server = Some(listener);
                }
                ControlFlow::<()>::Continue(())
            }
        )(
            if (state.get().unwrap().server.is_some()) {

                let mut guard = state.get().unwrap();
                async move {
                    let serv = guard.server.as_mut().unwrap();
                    serv.accept().await.map(|x| x.0)
                }
            },
            |res| {
                match res {
                    Ok(conn) => {
                        state.get().unwrap().new_conn = Some(conn);
                        println!("Accepted");
                    }
                    Err(_) => {}
                }
                ControlFlow::<()>::Continue(())
            }
        )(
            if (state.get().unwrap().port.is_some()) {
                let port = state.get().unwrap().port.unwrap();
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
                ControlFlow::<()>::Continue(())
            }
        )(
            if (state.get().unwrap().new_conn.is_some()) {
                let mut guard = state.get().unwrap();
                async move {
                    let mut conn: TcpStream = guard.new_conn.take().unwrap();
                    let mut buf = [0u8; 5];
                    conn.read_exact(&mut buf).await.unwrap();
                    String::from_utf8_lossy(&buf).to_string()
                }
            },
            |res3| {
                println!("Server received: {:?}", res3);
                ControlFlow::<()>::Continue(())
            }
        )
    )
    .await;
}
