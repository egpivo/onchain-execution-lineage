//! Known trading-pair mint allowlist for the CLI quote command.

use anyhow::Result;

/// Spot-checked against this project's existing verified allowlist
/// (Sunday project quote-economics work) -- not re-verified here, reused.
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const JUP_MINT: &str = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";

pub fn resolve_pair(pair: &str) -> Result<(&'static str, &'static str, u8)> {
    match pair {
        "USDC/SOL" => Ok((USDC_MINT, SOL_MINT, 6)),
        "USDC/JUP" => Ok((USDC_MINT, JUP_MINT, 6)),
        _ => anyhow::bail!("unknown pair '{pair}' -- add it to resolve_pair in pairs.rs"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_pair_known_pairs() {
        let (input, output, decimals) = resolve_pair("USDC/SOL").unwrap();
        assert_eq!(input, USDC_MINT);
        assert_eq!(output, SOL_MINT);
        assert_eq!(decimals, 6);

        let (input, output, decimals) = resolve_pair("USDC/JUP").unwrap();
        assert_eq!(input, USDC_MINT);
        assert_eq!(output, JUP_MINT);
        assert_eq!(decimals, 6);
    }

    #[test]
    fn resolve_pair_rejects_unknown() {
        assert!(resolve_pair("SOL/USDC").is_err());
    }
}
