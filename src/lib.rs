//! `onchain-execution-lineage` — a read-only toolkit for reconstructing and
//! verifying execution lineage across provider responses, transaction
//! construction, and settlement.
//!
//! Layering, outermost first:
//!
//! ```text
//! adapters/          provider adapter boundary (provider field names stop here)
//! execution_context  normalized, stage-optional execution state
//! solana/            generic Solana extraction (provider-independent)
//! lineage_builder    cross-stage joins → LineageBundle
//! checks/            verification over context + lineage
//! ```
//!
//! Everything else is a shared primitive (`transaction`, `instruction_map`,
//! `program_registry`, `rpc`), experiment tooling (`experiment`,
//! `route_bracket`, `fingerprint`), or publication tooling
//! (`evidence_extract`, `report`). Publication and experiment modules may
//! consume verifier output; they do not define execution semantics.
//!
//! Read-only throughout: nothing in this crate signs or submits a transaction.

pub mod cli;

// Verifier core.
pub mod adapters;
pub mod checks;
pub mod execution_context;
pub mod extract;
pub mod lineage_builder;
pub mod lineage_model;
pub mod solana;

// Shared primitives.
pub mod artifact;
pub mod evidence;
pub mod instruction_map;
pub mod lookup_tables;
pub mod program_registry;
pub mod rpc;
pub mod settlement;
pub mod trace;
pub mod transaction;
pub mod tx_compare;

// Legacy provider normalization, retained for the `trace` / `experiment`
// paths. New code should use [`adapters`].
pub mod providers;

// Experiment and reference-case tooling.
pub mod api;
pub mod capture;
pub mod diff;
pub mod experiment;
pub mod fingerprint;
pub mod models;
pub mod pairs;
pub mod route_bracket;
pub mod route_fingerprint;

// Publication tooling.
pub mod evidence_extract;
pub mod reference_case;
pub mod report;

/// Deprecated: static DFlow-dev field-lineage CSV. Superseded by `trace`.
pub mod lineage;
