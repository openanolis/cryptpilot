use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    cryptpilot::run().await
}
