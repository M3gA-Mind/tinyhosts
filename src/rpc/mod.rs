//! One JSON request in, one JSON result out.
//!
//! Everything above this crate is in another process: `OpenCompany`'s front end
//! takes an API key from a form, `OpenHuman`'s agent decides to ship a site, and
//! neither one links against this library. [`execute_json`] is the boundary they
//! meet at, and the `TinyBus` adapter is a thin wrapper over it.
//!
//! # The credential travels with the request
//!
//! A hosting account belongs to a user, not to the process, so [`Request`]
//! carries the credential rather than reading it from a global. A request that
//! omits it falls back to the environment, which is what a self-hosted single
//! tenant wants. The credential is read out of the envelope and never written
//! back into a result — see [`Credentials`].

use crate::host::types::DeployRequest;
use crate::launch::types::LaunchPlan;
use crate::providers::{ProviderKind, connect_to};
use crate::{Credentials, Error, Result};

/// Existing library request, using implementation-owned validated input values.
pub type Request =
    tinyhosts_bus::rpc::Request<Credentials, LaunchPlan, DeployRequest, ProviderKind>;
/// Existing operations, with implementation-owned deployment inputs.
pub type Operation = tinyhosts_bus::rpc::Operation<LaunchPlan, DeployRequest>;
/// Shared result envelope.
pub use tinyhosts_bus::rpc::Outcome;

/// Runs one request.
///
/// # Errors
///
/// Returns [`Error::MissingApiKey`] when the request carries no credential and
/// the environment holds none, [`Error::UnknownProvider`] when this build has no
/// adapter for the named provider, or whatever the operation itself returns.
pub async fn execute(request: Request) -> Result<Outcome> {
    if let Operation::PrepareBundle { directory } = &request.operation {
        return crate::preparation::prepare_bundle(directory).map(Outcome::PreparedBundle);
    }
    match &request.operation {
        Operation::Launch { plan } => crate::preparation::validate_deployment_bundle(&plan.bundle)?,
        Operation::Deploy { request } => {
            crate::preparation::validate_deployment_bundle(&request.bundle)?;
        }
        _ => {}
    }
    let credentials = match request.credentials {
        Some(credentials) => credentials,
        None => request.provider.credentials_from_env()?,
    };
    let host = connect_to(request.provider, credentials, request.base_url.as_deref())?;

    match request.operation {
        Operation::Launch { plan } => crate::launch::launch(host.as_ref(), &plan)
            .await
            .map(|launched| Outcome::Launch(Box::new(launched))),
        Operation::CreateSite { spec } => host.create_site(&spec).await.map(Outcome::Site),
        Operation::FindSite { site } => Ok(match host.find_site(&site).await? {
            Some(site) => Outcome::Site(site),
            None => Outcome::NoSite,
        }),
        Operation::ListSites { limit } => host.list_sites(limit).await.map(Outcome::Sites),
        Operation::SetEnv { site, vars } => {
            host.set_env(&site, &vars).await.map(|()| Outcome::Done)
        }
        Operation::ListEnv { site } => host.list_env(&site).await.map(Outcome::Env),
        Operation::ProvisionDatabase { spec } => {
            host.provision_database(&spec).await.map(Outcome::Database)
        }
        Operation::AttachDatabase { database, site } => host
            .attach_database(&database, &site)
            .await
            .map(Outcome::EnvKeys),
        Operation::Deploy { request } => host.deploy(&request).await.map(Outcome::Deployment),
        Operation::Deployment { id } => host.deployment(&id).await.map(Outcome::Deployment),
        Operation::ListDeployments { site, limit } => host
            .list_deployments(&site, limit)
            .await
            .map(Outcome::Deployments),
        Operation::DeploymentLogs { id } => {
            host.deployment_logs(&id).await.map(Outcome::DeploymentLogs)
        }
        Operation::Promote { site, deployment } => host
            .promote(&site, &deployment)
            .await
            .map(|()| Outcome::Done),
        Operation::AddDomain { site, domain } => {
            host.add_domain(&site, &domain).await.map(Outcome::Domain)
        }
        Operation::ListDomains { site } => host.list_domains(&site).await.map(Outcome::Domains),
        Operation::Analytics { query } => host.analytics(&query).await.map(Outcome::Analytics),
        _ => Err(Error::Unsupported {
            provider: request.provider.as_str().into(),
            capability: "execute this operation".into(),
        }),
    }
}

/// Runs one request given as JSON, returning its result as JSON.
///
/// # Errors
///
/// Returns [`Error::Envelope`] when the request is not a [`Request`] or the
/// result cannot be serialized, and otherwise whatever [`execute`] returns.
pub async fn execute_json(request: &str) -> Result<String> {
    let request: Request = serde_json::from_str(request).map_err(|error| Error::Envelope {
        reason: error.to_string(),
    })?;

    let outcome = execute(request).await?;
    serde_json::to_string(&outcome).map_err(|error| Error::Envelope {
        reason: error.to_string(),
    })
}

/// The providers this build can connect to, as their slugs.
///
/// A caller uses this to populate a provider picker without hard-coding what a
/// given build was compiled with.
#[must_use]
pub fn providers() -> Vec<&'static str> {
    let mut available = Vec::new();
    if cfg!(feature = "vercel") {
        available.push(ProviderKind::Vercel.as_str());
    }
    available
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod test;
