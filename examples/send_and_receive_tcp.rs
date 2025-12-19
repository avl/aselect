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
        compile_error!("Generate wrapper for state, containing a try_lock-method. clean up unsafety")
        capture(state),
        (
            if (state.port.is_none()) {
                listen_factory()
            },
            |acceptor| {
                if let Ok(listener) = acceptor {
                    let listener: TcpListener = listener;
                    println!("New listner");
                    state.port = Some(listener.local_addr().unwrap().port());
                    state.server = Some(listener);
                }
                ControlFlow::<()>::Continue(())
            }
        )(
            if (state.server.is_some()) {

                let serv = state.server.as_mut().unwrap();
                async move { serv.accept().await.map(|x| x.0) }
            },
            |res| {
                match res {
                    Ok(conn) => {
                        state.new_conn = Some(conn);
                        println!("Accepted");
                    }
                    Err(_) => {}
                }
                ControlFlow::<()>::Continue(())
            }
        )(
            if (state.port.is_some()) {
                let port = state.port.unwrap();
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
            if (state.new_conn.is_some()) {
                let mut conn: TcpStream = state.new_conn.take().unwrap();
                async move {
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
