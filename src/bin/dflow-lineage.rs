//! Deprecated compatibility alias for the `onchain-execution-lineage` binary.
//!
//! The project is provider-neutral now; the old name is kept only so existing
//! scripts, article commands and shell history keep working. Identical
//! behaviour, one extra line on stderr. It will be removed in a later
//! milestone.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!(
        "warning: `dflow-lineage` is a deprecated alias; use `onchain-execution-lineage` instead."
    );
    onchain_execution_lineage::cli::run().await
}
