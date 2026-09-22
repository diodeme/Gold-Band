use gold_band::cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if gold_band::memory::mcp::requested() {
        return gold_band::memory::mcp::run().await;
    }
    cli::run().await
}
