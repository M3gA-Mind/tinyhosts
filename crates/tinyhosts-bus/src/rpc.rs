//! Existing hosting JSON envelopes, independent of providers and transport.
use crate::Launch;
use crate::ProviderId;
use crate::model::*;
use serde::{Deserialize, Serialize};
/// A request to act on one hosting account.
#[derive(Deserialize)]
#[serde(bound(
    deserialize = "C: Deserialize<'de>, P: Deserialize<'de>, D: Deserialize<'de>, K: Deserialize<'de> + Default"
))]
pub struct Request<
    C = Credentials,
    P = crate::inputs::LaunchInput,
    D = crate::inputs::DeploymentInput,
    K = ProviderId,
> {
    /// Which provider to act on. Defaults to the implementation default provider.
    #[serde(default)]
    pub provider: K,
    /// The account's credential. Omitted, it is read from the environment.
    #[serde(default)]
    pub credentials: Option<C>,
    /// An alternate API root for the provider.
    ///
    /// Set it when the provider is reached through an egress proxy. Omitted, the
    /// provider's own root is used.
    #[serde(default)]
    pub base_url: Option<String>,
    /// What to do.
    #[serde(flatten)]
    pub operation: Operation<P, D>,
}

impl<C: std::fmt::Debug, P: std::fmt::Debug, D: std::fmt::Debug, K: std::fmt::Debug> std::fmt::Debug
    for Request<C, P, D, K>
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Request")
            .field("provider", &self.provider)
            .field("credentials", &"<redacted>")
            .field("base_url", &"<redacted>")
            .field("operation", &self.operation)
            .finish()
    }
}

/// One thing a request can ask for.
///
/// The variants are exactly the implementation hosting interface surface plus
/// launch operation, so the bus exposes no more authority than the
/// library does.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Operation<P = crate::inputs::LaunchInput, D = crate::inputs::DeploymentInput> {
    /// Collect an already-authorized directory without provider effects or credential lookup.
    PrepareBundle {
        /// The host-approved workspace scope and relative directory.
        directory: crate::preparation::AuthorizedDirectory,
    },
    /// Run a whole launch: site, database, environment, domains, deployment.
    Launch {
        /// The plan to run.
        ///
        /// Boxed because it carries a whole bundle: unboxed, every other
        /// variant of this enum would be as large as an application.
        plan: Box<P>,
    },
    /// Create a site.
    CreateSite {
        /// What the site should be.
        spec: SiteSpec,
    },
    /// Find a site by name, or report that there is none.
    FindSite {
        /// The site's name or identifier.
        site: String,
    },
    /// List sites, newest first.
    ListSites {
        /// How many to return.
        #[serde(default = "default_limit")]
        limit: u32,
    },
    /// Set environment variables on a site.
    SetEnv {
        /// The site's name or identifier.
        site: String,
        /// The variables to set.
        vars: Vec<EnvVar>,
    },
    /// List a site's environment variables, without their values.
    ListEnv {
        /// The site's name or identifier.
        site: String,
    },
    /// Provision a managed database.
    ProvisionDatabase {
        /// What the database should be.
        spec: DatabaseSpec,
    },
    /// Connect a database to a site.
    AttachDatabase {
        /// The database, as [`Operation::ProvisionDatabase`] returned it.
        database: Database,
        /// The site's name or identifier.
        site: String,
    },
    /// Upload a bundle and start a deployment.
    Deploy {
        /// The deployment to start. Boxed for the same reason as
        /// [`Operation::Launch`]'s plan.
        request: Box<D>,
    },
    /// Read a deployment's current state.
    Deployment {
        /// The deployment's identifier.
        id: String,
    },
    /// List a site's deployments, newest first.
    ListDeployments {
        /// The site's name or identifier.
        site: String,
        /// How many to return.
        #[serde(default = "default_limit")]
        limit: u32,
    },
    /// List a deployment's build and deployment events, oldest first.
    DeploymentLogs {
        /// The deployment's identifier.
        id: String,
    },
    /// Point production traffic at an existing deployment.
    Promote {
        /// The site's name or identifier.
        site: String,
        /// The deployment's identifier.
        deployment: String,
    },
    /// Add a custom domain to a site.
    AddDomain {
        /// The site's name or identifier.
        site: String,
        /// The domain to add.
        domain: String,
    },
    /// List a site's domains.
    ListDomains {
        /// The site's name or identifier.
        site: String,
    },
    /// Report the traffic a site served.
    Analytics {
        /// The window to report on.
        query: AnalyticsQuery,
    },
}

const fn default_limit() -> u32 {
    20
}

/// What an operation produced.
///
/// The envelope is adjacently tagged — `{"result": "...", "value": ...}` — so a
/// list result and a record result have the same shape on the wire, and a reader
/// can dispatch on one field.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Outcome {
    /// Bounded source files and preparation facts, ready for Launch or Deploy.
    PreparedBundle(crate::preparation::PreparedBundle),
    /// A completed launch.
    ///
    /// Boxed because it carries a whole site, database and deployment: unboxed,
    /// every other variant would be as large as the largest one.
    Launch(Box<Launch>),
    /// One site.
    Site(Site),
    /// A site that does not exist.
    NoSite,
    /// Several sites.
    Sites(Vec<Site>),
    /// One deployment.
    Deployment(Deployment),
    /// Several deployments.
    Deployments(Vec<Deployment>),
    /// A deployment's build and deployment events.
    DeploymentLogs(Vec<DeploymentLog>),
    /// A site's environment variables, without their values.
    Env(Vec<EnvVarRecord>),
    /// One database.
    Database(Database),
    /// The environment variable names a database injected.
    EnvKeys(Vec<String>),
    /// One domain.
    Domain(Domain),
    /// Several domains.
    Domains(Vec<Domain>),
    /// A traffic report.
    Analytics(AnalyticsSummary),
    /// An operation that produced nothing but succeeded.
    Done,
}

/// Deserialize-only credential input; never serialized into results.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    /// Provider API key, consumed only by module-side credential validation.
    #[serde(rename = "api_key")]
    _api_key: String,
    /// Optional team identifier.
    #[serde(default)]
    team: Option<String>,
}
impl std::fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("api_key", &"<redacted>")
            .field("team", &self.team)
            .finish()
    }
}
