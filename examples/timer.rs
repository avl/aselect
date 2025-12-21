use futures::Stream;
use safeselect::{safe_select, safe_select_context};
use std::cell::UnsafeCell;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio::signal;

//TODO: Not really a "timer" example any more
#[tokio::main]
async fn main() {
    fn staticer<T: 'static>(t: T) {}

    let mut smuggle = Arc::new(Mutex::new(None));
    let smuggle2 = smuggle.clone();
    {
        safe_select_context!(Timer (connection_attempts: String));

        let mut context = Timer::new();
        let mut guard = context.connection_attempts().unwrap();
        guard.push_str("hello");
        drop(guard);

        //compile_error!("CLean up all todos, but I think this approach might actually work");
        /*struct MyContext {
            hello: String
        }

        let context = MyContext {
            hello: "hello".to_string(),
        };*/

        {
            safe_select!(
                context,
                Timer {
                    connection_attempts
                },
                conn(
                    {
                        //*connection_attempts.get()? += 1u32;
                        // TODO: Add wrappers around Capture.
                        // Make it so that Capture has private fields and all unsafe methods.
                        // Then give the wrappers a safe "&mut self" API, and don't allow cloning or anything.
                        // This makes (maybe?) the wrappers inaccessible from multi-threaded
                        // code. We know the actual futures don't outlive the context.
                        // So it should(?) be possible to safely give access to Capture contents
                        // without atomics!

                        println!("Conn: {:?}", connection_attempts.get()?);
                        //let counter = connection_attempts;

                        smuggle.lock().unwrap().replace(connection_attempts);
                        async move {
                            _ = connection_attempts;
                            //println!("Connection count: {:?}", counter);

                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            44u64
                        }
                    },
                    {
                        println!("Continue: {:?}, {:?}", connection_attempts.get()?, conn);
                        Some(()) //TODO: Fix double option here, quite unsightly!
                    }
                )
            );

        }
    }


    // This should not compile
}
