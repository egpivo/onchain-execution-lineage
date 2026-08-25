//! `onchain-execution-lineage` — read-only execution lineage extraction,
//! tracing and verification.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    onchain_execution_lineage::cli::run().await
}
