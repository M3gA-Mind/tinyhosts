//! Authorized source-directory input and bounded preparation facts.
use crate::inputs::BundleFile;
use serde::{Deserialize, Serialize};

/// Maximum source bytes collected by one preparation operation.
pub const MAX_PREPARATION_BYTES: u64 = 4 * 1024 * 1024;
/// Conservative JSON snapshot budget, before nested Execute string escaping.
pub const MAX_PREPARATION_JSON_BYTES: usize = 6 * 1024 * 1024;
/// Maximum regular files collected by one preparation operation.
pub const MAX_PREPARATION_FILES: u32 = 4096;
/// Maximum filesystem entries examined, including excluded entries.
pub const MAX_PREPARATION_ENTRIES: u32 = 16_384;
/// Maximum UTF-8 bytes in a relative prepared file path.
pub const MAX_PREPARATION_PATH_BYTES: usize = 1024;
/// Maximum nested directory depth beneath the authorized source.
pub const MAX_PREPARATION_DEPTH: usize = 64;

/// A directory the host has authorized for this read and intended deployment.
///
/// The host must finish workspace/path policy and external-effect approval before
/// submitting this value. It is a scope declaration, not proof of authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedDirectory {
    /// Absolute workspace root approved by the host.
    pub workspace: String,
    /// Relative source directory; absolute paths, traversal and symlinks are refused.
    pub path: String,
    /// Source-byte budget, bounded by MAX_PREPARATION_BYTES.
    #[serde(default = "default_bytes")]
    pub max_bytes: u64,
    /// Regular-file budget, bounded by MAX_PREPARATION_FILES.
    #[serde(default = "default_files")]
    pub max_files: u32,
}
const fn default_bytes() -> u64 {
    MAX_PREPARATION_BYTES
}
const fn default_files() -> u32 {
    MAX_PREPARATION_FILES
}

/// Collected source files and facts; no provider effect has been performed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedBundle {
    /// Operation vocabulary served by the preparing module.
    pub contract_version: (u32, u32),
    /// Source files on the existing standard-base64 deployment wire.
    pub bundle: Vec<BundleFile>,
    /// Number of regular source files collected.
    pub file_count: u32,
    /// Total unencoded source bytes.
    pub total_bytes: u64,
    /// Excluded entries encountered; excluded directory descendants are not traversed.
    pub skipped_entries: u32,
    /// Entries examined, including entries skipped without descending.
    pub scanned_entries: u32,
}
impl std::fmt::Debug for PreparedBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedBundle")
            .field("contract_version", &self.contract_version)
            .field("file_count", &self.file_count)
            .field("total_bytes", &self.total_bytes)
            .field("skipped_entries", &self.skipped_entries)
            .field("scanned_entries", &self.scanned_entries)
            .finish()
    }
}
