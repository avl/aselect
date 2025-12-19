use std::ops::ControlFlow;
use tokio::net::{TcpListener, TcpStream};
use safeselect::safe_select;

#[tokio::main]
async fn main() {

    #[derive(Default)]
    struct State {
        port: Option<u16>,
        server: Option<TcpStream>
    }

    let state = State::default();

    safe_select!(
        capture (state),
        ({
            async move {
                let tcp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                tcp_listener.accept().await.unwrap().0
        }}, |acceptor|{
            state.server = Some(acceptor);
            ControlFlow::<()>::Continue(())
        }
        )
    ).await;


}