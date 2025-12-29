use safeselect::{safe_select};

#[tokio::main]
async fn main() {
    let value = "Hello".to_string();
    safe_select!(
        {borrowed(value);},
        conn(
            {
                println!("{:?}", value);
                *value? = "World".to_string();
            },
            async | value | {
                println!("Value: {:?}",  value);
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
