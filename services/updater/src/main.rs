#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    updater::run().await
}
