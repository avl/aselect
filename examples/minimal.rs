use aselect::aselect;

// Minimal (useless) example
#[tokio::main]
async fn main() {
    aselect!(
        {},
        timer1(
            {
            },
            async |_sleep| {
            },
            |_result| {
            }
        ),
    ).await;
}
