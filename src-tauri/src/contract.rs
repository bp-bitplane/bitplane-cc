//! bp's wire types as cc reads them: `src/ipc/bp-contract.ts` is generated
//! from bitplane-core, and `bindings.ts` points at it instead of keeping a
//! copy that drifts.

use serde::{Deserialize, Serialize};
use specta::Type;

/// A type `src/ipc/bp-contract.ts` defines.
pub fn contract_type(name: &str) -> specta::datatype::DataType {
    specta_typescript::define(format!("import(\"./bp-contract\").{name}")).into()
}

/// bp's failure envelope. Only the fields cc reads; the frontend gets bp's
/// own `ErrorEnvelope` type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
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

impl Type for Envelope {
    fn definition(_: &mut specta::Types) -> specta::datatype::DataType {
        contract_type("ErrorEnvelope")
    }
}

/// bp's own `BranchIntent`, spelled as bp spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchIntent {
    Resolve,
    RequireNew,
    RequireExisting,
}

impl Type for BranchIntent {
    fn definition(_: &mut specta::Types) -> specta::datatype::DataType {
        contract_type("BranchIntent")
    }
}
