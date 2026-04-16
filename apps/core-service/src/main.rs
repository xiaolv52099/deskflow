use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    core_service::run_core_service().await
}
