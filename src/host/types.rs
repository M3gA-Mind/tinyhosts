//! Hosting vocabulary shared with the pure bus contract.
use crate::bundle::Bundle;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
pub use tinyhosts_bus::model::*;

/// A request to deploy a bundle of files as a site.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployRequest {
    /// The site to deploy to, by name.
    pub site: String,
    /// The framework to build with.
    #[serde(default)]
    pub framework: Framework,
    /// The environment this deployment serves.
    #[serde(default)]
    pub target: DeploymentTarget,
    /// The files to deploy.
    pub bundle: Bundle,
}

impl DeployRequest {
    /// A preview deployment of `bundle` to the Next.js site `site`.
    #[must_use]
    pub fn new(site: impl Into<String>, bundle: Bundle) -> Self {
        Self {
            site: site.into(),
            framework: Framework::NextJs,
            target: DeploymentTarget::Preview,
            bundle,
        }
    }

    /// Sends the deployment to `target` instead of a preview URL.
    #[must_use]
    pub fn with_target(mut self, target: DeploymentTarget) -> Self {
        self.target = target;
        self
    }

    /// Builds with a different framework.
    #[must_use]
    pub fn with_framework(mut self, framework: Framework) -> Self {
        self.framework = framework;
        self
    }

    /// Checks the request before an adapter starts uploading files.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptySiteName`] when the site name is blank, or
    /// [`Error::EmptyBundle`] when there is nothing to deploy.
    pub fn validate(&self) -> Result<()> {
        if self.site.trim().is_empty() {
            return Err(Error::EmptySiteName);
        }
        if self.bundle.is_empty() {
            return Err(Error::EmptyBundle);
        }
        Ok(())
    }
}
