//! A completed hosting launch.
use crate::model::{Database, Deployment, Domain, Site};
use serde::{Deserialize, Serialize};
/// What a launch produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    /// The site the application lives on.
    pub site: Site,
    /// Whether this launch created the site, rather than finding it.
    pub created_site: bool,
    /// The database that was provisioned, when the plan asked for one.
    #[serde(default)]
    pub database: Option<Database>,
    /// The environment variable names the database injected into the site.
    #[serde(default)]
    pub database_env_keys: Vec<String>,
    /// The domains that were attached.
    #[serde(default)]
    pub domains: Vec<Domain>,
    /// The deployment, which is usually still building.
    pub deployment: Deployment,
}

impl Launch {
    /// The URL the application will serve from, once the deployment is ready.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        self.deployment.url.as_deref()
    }
}
