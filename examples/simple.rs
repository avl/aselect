use safeselect::{safe_select, safe_select_context};

#[tokio::main]
async fn main() {
    safe_select_context!(Simple (value: String));

    {
        println!("select");
        safe_select!(
            Simple,
            conn(value)(
                {
                    // TODO: Figure out if Capture is protected enough that a user
                    // can't smuggle one out in unsafe code and violate the rules for
                    // UnsafeCell access
                    println!("{:?}", value);
                    *value? = "Hello".to_string();
                    let value_moved_into_future = "World".to_string();
                },
                async | value | {
                    println!("Message: {}, value: {:?}", value_moved_into_future, value);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    44u64
                },
                |conn| {
                    println!("Continue: {:?}, {:?}", value, conn);
                    Some(())
                }
            ),
        )
        .await;
    }
}
