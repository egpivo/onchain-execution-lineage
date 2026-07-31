//! Evidence levels for every attribution claim.
//!
//! No undocumented scalar "confidence" score. Callers must pick an explicit
//! level and keep the source artifact ID on the claim.

use serde::{Deserialize, Serialize};

pub const LINEAGE_SCHEMA_VERSION: &str = "1.0.0";
pub const ARTIFACT_SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    DirectObservation,
    DecodedFromTransaction,
    ResolvedFromRpc,
    ExternalProgramLabel,
    CrossArtifactInference,
    Candidate,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionClaim {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub evidence_level: EvidenceLevel,
    pub source_artifact_id: String,
    pub explanation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedField {
    pub field: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_artifact_id: Option<String>,
}

impl AttributionClaim {
    pub fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
        evidence_level: EvidenceLevel,
        source_artifact_id: impl Into<String>,
        explanation: impl Into<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            evidence_level,
            source_artifact_id: source_artifact_id.into(),
            explanation: explanation.into(),
            source_field: None,
            instruction_index: None,
        }
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.source_field = Some(field.into());
        self
    }

    pub fn with_instruction(mut self, index: usize) -> Self {
        self.instruction_index = Some(index);
        self
    }
}
