//! bp's wire types as cc reads them: `src/ipc/bp-contract.ts` is generated
//! from bitplane-core, and `bindings.ts` points at it instead of keeping a
//! copy that drifts.

use serde::{Deserialize, Serialize};

/// bp's failure envelope. Only the fields cc reads; the frontend gets the
/// whole type from `bp-contract.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub error: String,
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub problems: Vec<Problem>,
    #[serde(default)]
    pub remedy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    #[serde(default)]
    pub subject: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchIntent {
    Resolve,
    RequireNew,
    RequireExisting,
}
