#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::{Credentials, ProviderKind};

/// A real Vercel client that is never expected to reach the network.
fn offline_host() -> Arc<dyn Host> {
    Arc::from(crate::connect(ProviderKind::Vercel, Credentials::new("token").unwrap()).unwrap())
}

#[test]
fn a_launch_plan_never_bundles_credential_files() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let root = workspace.path();
    std::fs::write(root.join("package.json"), b"{}").expect("write");
    std::fs::write(root.join(".env.staging"), b"DB_PASSWORD=x").expect("write");
    std::fs::write(root.join(".npmrc"), b"//registry.npmjs.org/:_authToken=x").expect("write");
    std::fs::create_dir(root.join(".aws")).expect("mkdir");
    std::fs::write(root.join(".aws/credentials"), b"[default]").expect("write");
    let tool = LaunchSiteTool::new(offline_host(), root.to_path_buf());

    let plan = tool.plan(&json!({"site": "shop"})).expect("a plan");

    let paths: Vec<&str> = plan
        .bundle
        .files()
        .iter()
        .map(crate::SiteFile::path)
        .collect();
    assert_eq!(paths, ["package.json"], "credential files were bundled");
}
