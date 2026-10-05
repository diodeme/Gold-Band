use std::process::ExitCode;

use gold_band::cli;

#[tokio::main]
async fn main() -> ExitCode {
    if gold_band::memory::mcp::requested() {
        return match gold_band::memory::mcp::run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error:#}");
                ExitCode::FAILURE
            }
        };
    }
    cli::run().await
}
