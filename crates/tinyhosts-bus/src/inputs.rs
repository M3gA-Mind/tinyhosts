//! Serialized deployment input. Byte preparation and validation remain in the implementation.
use crate::model::{DatabaseSpec, DeploymentTarget, EnvVar, Framework, SiteSpec};
use serde::{Deserialize, Serialize};
/// A deployment file on the existing base64 JSON wire.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BundleFile {
    /// Relative, slash-separated destination path, validated by the module.
    pub path: String,
    /// Standard-base64 encoded content; encoding and decoding are implementation work.
    pub contents: String,
}
/// Deployment request vocabulary, without filesystem or encoding behavior.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeploymentInput {
    /// Site name or identifier.
    pub site: String,
    /// Build framework.
    #[serde(default)]
    pub framework: Framework,
    /// Deployment environment.
    #[serde(default)]
    pub target: DeploymentTarget,
    /// Authorized file contents, on the existing wire representation.
    pub bundle: Vec<BundleFile>,
}
/// Full launch request vocabulary; the compiled module owns deployment sequencing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LaunchInput {
    /// Site configuration.
    pub site: SiteSpec,
    /// Authorized deployment files.
    pub bundle: Vec<BundleFile>,
    /// Optional managed database.
    #[serde(default)]
    pub database: Option<DatabaseSpec>,
    /// Environment values to install before deployment.
    #[serde(default)]
    pub env: Vec<EnvVar>,
    /// Custom domains to attach.
    #[serde(default)]
    pub domains: Vec<String>,
    /// Deployment environment.
    #[serde(default)]
    pub target: DeploymentTarget,
}
