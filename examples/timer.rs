
use safeselect::{safe_select, safe_select_context};

//TODO: Not really a "timer" example any more
#[tokio::main]
async fn main() {

    safe_select_context!(Timer (connection_attempts: String));

    {
        println!("select");
        safe_select!(
            Timer,
            conn  (
                |connection_attempts| {
                    println!("1");
                    //*connection_attempts.get()? += 1u32;
                    // TODO: Add wrappers around Capture.
                    // Make it so that Capture has private fields and all unsafe methods.
                    // Then give the wrappers a safe "&mut self" API, and don't allow cloning or anything.
                    // This makes (maybe?) the wrappers inaccessible from multi-threaded
                    // code. We know the actual futures don't outlive the context.
                    // So it should(?) be possible to safely give access to Capture contents
                    // without atomics!
                    let m = connection_attempts.as_mut();
                    println!("m: {:?}", m);
                    println!("Conn: {:?}", connection_attempts.as_mut());
                    //let counter = connection_attempts;
                    //_ = smuggle.lock().unwrap().replace(connection_attempts);
                    println!("Made fut");
                    let temp1 = 47;
                },
                async {
                    println!("Connection count: {:?}", connection_attempts);
                    println!("temp: {}", temp1);

                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    44u64
                },
                |conn| {
                    println!("Continue: {:?}, {:?}", connection_attempts, conn);
                    Some(())
                }
            ),
        )
        .await;
    }
}
