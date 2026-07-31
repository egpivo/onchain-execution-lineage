//! Provider-neutral artifact manifest (schema v1).
//!
//! Raw artifacts are referenced by path + SHA-256 and are never rewritten by
//! this module. Unsupported schema versions fail closed.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::evidence::ARTIFACT_SCHEMA_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointType {
    Developer,
    Production,
    App,
    Rpc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SanitizationStatus {
    NotRequired,
    Sanitized,
    PendingReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPresence {
    Absent,
    PresentNull,
    PresentBase64,
    SettledSignatureOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub schema_version: String,
    pub artifact_id: String,
    pub capture_run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_set_id: Option<String>,
    pub provider: String,
    pub surface: String,
    pub endpoint_type: EndpointType,
    pub endpoint_hostname: String,
    pub authentication_mode: String,
    pub captured_at_utc: String,
    pub pair: String,
    pub input_mint: String,
    pub output_mint: String,
    pub raw_input_amount: String,
    pub slippage_configuration: String,
    pub raw_artifact_path: String,
    pub raw_artifact_sha256: String,
    pub sanitized_artifact_path: String,
    pub sanitization_status: SanitizationStatus,
    pub transaction_presence: TransactionPresence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub source_notes: String,
}

impl ArtifactManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != ARTIFACT_SCHEMA_VERSION {
            bail!(
                "unsupported artifact schema_version '{}'; supported '{}'",
                self.schema_version,
                ARTIFACT_SCHEMA_VERSION
            );
        }
        if self.artifact_id.is_empty() {
            bail!("artifact_id must be non-empty");
        }
        if self.raw_artifact_sha256.len() != 64
            || !self
                .raw_artifact_sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            bail!("raw_artifact_sha256 must be 64 lowercase/uppercase hex chars");
        }
        // Fail closed: manifests must never embed auth secrets.
        let blob = serde_json::to_string(self)?;
        for needle in [
            "authorization:",
            "bearer ",
            "cookie:",
            "private_key",
            "api_key=",
        ] {
            if blob.to_lowercase().contains(needle) {
                bail!("manifest appears to contain secret material ({needle})");
            }
        }
        Ok(())
    }

    pub fn load_path(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read manifest {}", path.display()))?;
        let m: Self = serde_json::from_str(&text).context("failed to parse artifact manifest")?;
        m.validate()?;
        Ok(m)
    }

    pub fn to_canonical_json(&self) -> Result<String> {
        self.validate()?;
        // Deterministic: struct field order from derive + pretty print.
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn verify_raw_hash(&self, base_dir: &Path) -> Result<()> {
        let path = resolve_against(base_dir, &self.raw_artifact_path);
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read raw artifact {}", path.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let got = format!("{:x}", hasher.finalize());
        if !got.eq_ignore_ascii_case(&self.raw_artifact_sha256) {
            bail!(
                "raw artifact hash mismatch for {}: manifest={} actual={}",
                self.artifact_id,
                self.raw_artifact_sha256,
                got
            );
        }
        Ok(())
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn resolve_against(base: &Path, maybe_relative: &str) -> PathBuf {
    let p = PathBuf::from(maybe_relative);
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample() -> ArtifactManifest {
        ArtifactManifest {
            schema_version: ARTIFACT_SCHEMA_VERSION.into(),
            artifact_id: "art_test_001".into(),
            capture_run_id: "run_test".into(),
            matched_set_id: None,
            provider: "dflow".into(),
            surface: "dev_quote".into(),
            endpoint_type: EndpointType::Developer,
            endpoint_hostname: "dev-quote-api.dflow.net".into(),
            authentication_mode: "none".into(),
            captured_at_utc: "2026-07-29T12:34:22Z".into(),
            pair: "USDC/SOL".into(),
            input_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            output_mint: "So11111111111111111111111111111111111111112".into(),
            raw_input_amount: "1000000000".into(),
            slippage_configuration: "50_bps".into(),
            raw_artifact_path: "raw.json".into(),
            raw_artifact_sha256: "a".repeat(64),
            sanitized_artifact_path: "sanitized.json".into(),
            sanitization_status: SanitizationStatus::NotRequired,
            transaction_presence: TransactionPresence::Absent,
            signature: None,
            source_notes: "unit test".into(),
        }
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let mut m = sample();
        m.schema_version = "9.9.9".into();
        assert!(m
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unsupported"));
    }

    #[test]
    fn rejects_secret_like_notes() {
        let mut m = sample();
        m.source_notes = "Authorization: Bearer abc".into();
        assert!(m.validate().unwrap_err().to_string().contains("secret"));
    }

    #[test]
    fn verify_raw_hash_detects_mismatch() {
        let dir = std::env::temp_dir().join(format!("art_hash_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("raw.json");
        let mut f = fs::File::create(&raw).unwrap();
        write!(f, "{{}}").unwrap();
        let mut m = sample();
        m.raw_artifact_path = "raw.json".into();
        m.raw_artifact_sha256 = "0".repeat(64);
        assert!(m
            .verify_raw_hash(&dir)
            .unwrap_err()
            .to_string()
            .contains("mismatch"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canonical_json_is_stable() {
        let a = sample().to_canonical_json().unwrap();
        let b = sample().to_canonical_json().unwrap();
        assert_eq!(a, b);
    }
}
