//! Minimal hosting vocabulary for hosts that execute through compiled TinyBus modules.
pub mod error;
pub mod inputs;
mod launch;
pub mod model;
pub mod preparation;
pub mod rpc;
pub mod tools;
pub use error::{Error, Result};
pub use launch::Launch;
pub use model::*;
/// Well-known hosting interface and bus name.
pub const BUS_NAME: &str = "ai.tinyhumans.tinyhosts.Hosting";
/// Hosting object path.
pub const OBJECT_PATH: &str = "/ai/tinyhumans/tinyhosts/Hosting";
/// Member names in their existing order and arity.
pub const METHODS: [&str; 2] = ["Execute", "Providers"];
/// Additive hosting operation vocabulary, independent of artifact package releases.
/// 1.0 is the original Execute/Providers surface; 1.1 adds authorized preparation; 1.2 adds the pure tool-argument and rendering helpers in `tools`.
pub const WIRE_CONTRACT_VERSION: (u32, u32) = (1, 2);

/// Whether an artifact serves every operation in this vocabulary.
#[must_use]
pub fn is_compatible(module: (u32, u32)) -> bool {
    module.0 == WIRE_CONTRACT_VERSION.0 && module.1 >= WIRE_CONTRACT_VERSION.1
}

/// Contract package version, synchronized with the released module manifest.
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Recorded agent tool declarations, including approval metadata.
pub const TOOL_DECLARATIONS_JSON: &str = include_str!("declarations.json");

/// Hosting provider identifiers, independent of provider connection behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProviderId {
    /// Vercel hosting.
    #[default]
    Vercel,
}

/// Agent-facing declaration, independent of the implementation Tool trait.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolDeclaration {
    /// Stable tool name.
    pub name: String,
    /// Existing model-facing description.
    pub description: String,
    /// Existing JSON argument schema.
    pub parameters_schema: serde_json::Value,
    /// Whether OpenHuman must apply its external-effect approval policy.
    pub external_effect: bool,
    /// Existing approval classification recorded in the tool catalog.
    pub permission_level: PermissionLevel,
}

/// Recorded hosting tool approval vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PermissionLevel {
    /// A tool that only reads provider state.
    ReadOnly,
    /// A tool that changes provider state.
    Write,
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
