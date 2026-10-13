//! Pure, I/O-free mapping from declared hosting tools to bus operations, and
//! the model-facing text rendered from their outcomes.
//!
//! Hosts keep workspace policy, approvals and the call itself; everything that
//! only depends on the declared tool name, its JSON arguments and the typed
//! outcome lives here so every host shapes and renders them identically.
use crate::inputs::{BundleFile, LaunchInput};
use crate::model::{
    AnalyticsDimension, AnalyticsQuery, DatabaseKind, DatabaseSpec, Deployment, DeploymentLog,
    DeploymentStatus, DeploymentTarget, Domain, EnvVar, SiteSpec,
};
use crate::rpc::{Operation, Outcome};
use crate::Launch;
use serde_json::Value;
use std::fmt;

/// Why a tool call's arguments could not become an operation.
///
/// The text is model-facing and safe to return as a tool error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolArgsError(pub String);

impl fmt::Display for ToolArgsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ToolArgsError {}

/// Declared tool name that launches a site; it needs a prepared bundle first.
pub const TOOL_LAUNCH_SITE: &str = "hosting_launch_site";
/// Declared tool name that promotes an earlier deployment.
pub const TOOL_ROLLBACK: &str = "hosting_rollback";
/// Declared tool name that reads deployment logs.
pub const TOOL_DEPLOYMENT_LOGS: &str = "hosting_deployment_logs";

/// A non-blank string argument, trimmed.
///
/// # Errors
///
/// Returns a [`ToolArgsError`] naming `key` when it is absent or blank.
pub fn required_str(args: &Value, key: &str) -> Result<String, ToolArgsError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ToolArgsError(format!("`{key}` is required")))
}

/// An environment value as the string the provider stores.
///
/// # Errors
///
/// Returns a [`ToolArgsError`] unless `value` is a string, number or boolean.
pub fn env_string(key: &str, value: &Value) -> Result<String, ToolArgsError> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        _ => Err(ToolArgsError(format!(
            "`env.{key}` must be a string, number, or boolean"
        ))),
    }
}

/// An integer argument clamped to `min..=max`, or `default` when absent.
#[must_use]
pub fn bounded(args: &Value, key: &str, default: u64, min: u64, max: u64) -> u32 {
    let value = args.get(key);
    value
        .and_then(Value::as_i64)
        .map(|value| value.clamp(min as i64, max as i64) as u64)
        .or_else(|| value.and_then(Value::as_u64))
        .unwrap_or(default)
        .clamp(min, max) as u32
}

fn env_vars(
    env: &serde_json::Map<String, Value>,
    targets: &[DeploymentTarget],
    secret: bool,
) -> Result<Vec<EnvVar>, ToolArgsError> {
    env.iter()
        .map(|(key, value)| {
            Ok(EnvVar {
                key: key.clone(),
                value: env_string(key, value)?,
                targets: targets.to_vec(),
                secret,
            })
        })
        .collect()
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

impl Operation {
    /// The operation a declared single-step tool asks for.
    ///
    /// `now_ms` is the current time in milliseconds since the Unix epoch; the
    /// caller supplies it so this stays free of clocks. Launch needs a prepared
    /// bundle ([`Operation::launch_from_tool_args`]) and rollback is a read
    /// followed by [`Operation::promote_from_tool_args`]; asking for either
    /// here yields the first, read-only step (`hosting_rollback`) or an error.
    ///
    /// # Errors
    ///
    /// Returns a [`ToolArgsError`] for a missing or malformed argument or an
    /// unsupported tool name.
    pub fn from_tool_args(name: &str, args: &Value, now_ms: u64) -> Result<Self, ToolArgsError> {
        Ok(match name {
            "hosting_deployment_status" => Self::Deployment {
                id: required_str(args, "deployment_id")?,
            },
            "hosting_list_deployments" => Self::ListDeployments {
                site: required_str(args, "site")?,
                limit: bounded(args, "limit", 20, 1, 100),
            },
            TOOL_DEPLOYMENT_LOGS => Self::DeploymentLogs {
                id: required_str(args, "deployment_id")?,
            },
            TOOL_ROLLBACK => Self::Deployment {
                id: required_str(args, "deployment_id")?,
            },
            "hosting_list_sites" => Self::ListSites {
                limit: bounded(args, "limit", 20, 1, 100),
            },
            "hosting_set_env" => {
                let site = required_str(args, "site")?;
                let Some(env) = args.get("env").and_then(Value::as_object) else {
                    return Err(ToolArgsError(
                        "`env` must be an object of variables".to_owned(),
                    ));
                };
                let targets: &[DeploymentTarget] = if flag(args, "production_only") {
                    &[DeploymentTarget::Production]
                } else {
                    &[]
                };
                Self::SetEnv {
                    site,
                    vars: env_vars(env, targets, flag(args, "secret"))?,
                }
            }
            "hosting_add_domain" => Self::AddDomain {
                site: required_str(args, "site")?,
                domain: required_str(args, "domain")?,
            },
            "hosting_domain_status" => Self::ListDomains {
                site: required_str(args, "site")?,
            },
            "hosting_analytics" => {
                let site = required_str(args, "site")?;
                let days = u64::from(bounded(args, "days", 7, 1, 365));
                let breakdown = match args.get("breakdown").and_then(Value::as_str) {
                    None => None,
                    Some(slug) => Some(
                        serde_json::from_value::<AnalyticsDimension>(Value::String(
                            slug.to_owned(),
                        ))
                        .map_err(|_| {
                            ToolArgsError(format!(
                                "`breakdown` must be one of country, request_path, device_type, \
                                 browser_name, os_name, referrer_hostname, route — not `{slug}`"
                            ))
                        })?,
                    ),
                };
                let mut query = AnalyticsQuery::new(
                    site,
                    now_ms.saturating_sub(days * 24 * 60 * 60 * 1000),
                    now_ms,
                );
                query.breakdown = breakdown;
                query.limit = 10;
                Self::Analytics { query }
            }
            TOOL_LAUNCH_SITE => {
                return Err(ToolArgsError(
                    "hosting_launch_site needs a prepared bundle".to_owned(),
                ));
            }
            _ => {
                return Err(ToolArgsError(
                    "hosting tool declaration is unsupported".to_owned(),
                ));
            }
        })
    }

    /// The launch a `hosting_launch_site` call asks for, over a bundle the host
    /// already prepared from its authorized directory.
    ///
    /// # Errors
    ///
    /// Returns a [`ToolArgsError`] for a missing or malformed argument.
    pub fn launch_from_tool_args(
        args: &Value,
        bundle: Vec<BundleFile>,
    ) -> Result<Self, ToolArgsError> {
        let site = required_str(args, "site")?;
        let env = match args.get("env").and_then(Value::as_object) {
            Some(values) => env_vars(values, &[], false)?,
            None => Vec::new(),
        };
        let database = args
            .get("database")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| {
                let kind = args
                    .get("database_kind")
                    .and_then(Value::as_str)
                    .and_then(|kind| {
                        serde_json::from_value::<DatabaseKind>(Value::String(kind.to_owned())).ok()
                    })
                    .unwrap_or_default();
                let mut spec = DatabaseSpec {
                    name: name.to_owned(),
                    kind,
                    product: None,
                };
                spec.kind = kind;
                spec
            });
        let domains = args
            .get("domains")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        let target = if flag(args, "production") {
            DeploymentTarget::Production
        } else {
            DeploymentTarget::Preview
        };
        Ok(Self::Launch {
            plan: Box::new(LaunchInput {
                site: SiteSpec {
                    name: site,
                    framework: Default::default(),
                },
                bundle,
                database,
                env,
                domains,
                target,
            }),
        })
    }

    /// The promotion a `hosting_rollback` call asks for, once the target
    /// deployment has been read and found ready.
    ///
    /// # Errors
    ///
    /// Returns a [`ToolArgsError`] for a missing argument.
    pub fn promote_from_tool_args(args: &Value) -> Result<Self, ToolArgsError> {
        Ok(Self::Promote {
            site: required_str(args, "site")?,
            deployment: required_str(args, "deployment_id")?,
        })
    }
}

impl Outcome {
    /// Whether this outcome is the one `operation` produces.
    #[must_use]
    pub fn matches<P, D>(&self, operation: &Operation<P, D>) -> bool {
        match operation {
            Operation::PrepareBundle { .. } => matches!(self, Self::PreparedBundle(_)),
            Operation::Launch { .. } => matches!(self, Self::Launch(_)),
            Operation::CreateSite { .. } => matches!(self, Self::Site(_)),
            Operation::FindSite { .. } => matches!(self, Self::Site(_) | Self::NoSite),
            Operation::ListSites { .. } => matches!(self, Self::Sites(_)),
            Operation::SetEnv { .. }
            | Operation::AttachDatabase { .. }
            | Operation::Promote { .. } => matches!(self, Self::Done),
            Operation::ListEnv { .. } => matches!(self, Self::Env(_)),
            Operation::ProvisionDatabase { .. } => matches!(self, Self::Database(_)),
            Operation::Deploy { .. } | Operation::Deployment { .. } => {
                matches!(self, Self::Deployment(_))
            }
            Operation::ListDeployments { .. } => matches!(self, Self::Deployments(_)),
            Operation::DeploymentLogs { .. } => matches!(self, Self::DeploymentLogs(_)),
            Operation::AddDomain { .. } => matches!(self, Self::Domain(_)),
            Operation::ListDomains { .. } => matches!(self, Self::Domains(_)),
            Operation::Analytics { .. } => matches!(self, Self::Analytics(_)),
        }
    }
}

/// The capitalised status word shown to the model.
#[must_use]
pub fn status_debug(status: &DeploymentStatus) -> &'static str {
    match status {
        DeploymentStatus::Queued => "Queued",
        DeploymentStatus::Building => "Building",
        DeploymentStatus::Ready => "Ready",
        DeploymentStatus::Failed => "Failed",
        DeploymentStatus::Canceled => "Canceled",
        _ => "Other",
    }
}

/// Only a deployment that finished building can serve traffic.
#[must_use]
pub fn rollback_target_is_ready(deployment: &Deployment) -> bool {
    deployment.status == DeploymentStatus::Ready
}

/// The refusal text for a rollback onto a deployment that is not ready.
#[must_use]
pub fn rollback_refusal(id: &str, deployment: &Deployment) -> String {
    format!(
        "Deployment `{id}` is {}, so it cannot be promoted — only a deployment that finished building can serve traffic. Use hosting_list_deployments to find one that is ready.",
        status_debug(&deployment.status)
    )
}

/// The confirmation text after a successful rollback.
#[must_use]
pub fn rollback_summary(site: &str, id: &str, deployment: &Deployment) -> String {
    match deployment.url.as_deref() {
        Some(url) => format!(
            "{site} is now serving deployment `{id}` in production ({url}). The change is at the provider's edge; nothing was rebuilt."
        ),
        None => format!(
            "{site} is now serving deployment `{id}` in production. The change is at the provider's edge; nothing was rebuilt."
        ),
    }
}

/// The confirmation text after setting environment variables.
#[must_use]
pub fn set_env_summary(site: &str, keys: &[&str]) -> String {
    format!(
        "Set {} on {site}. Redeploy the site for a build-time variable to take effect.",
        keys.join(", ")
    )
}

/// The confirmation text after attaching a domain.
#[must_use]
pub fn add_domain_summary(site: &str, domain: &Domain) -> String {
    if domain.verified {
        format!("{} is attached to {site} and verified.", domain.name)
    } else {
        format!(
            "{} is attached to {site} but not verified yet: its DNS records still have to point at the provider.",
            domain.name
        )
    }
}

/// The last `limit` log entries, oldest first.
#[must_use]
pub fn tail_logs(mut logs: Vec<DeploymentLog>, limit: usize) -> Vec<DeploymentLog> {
    if logs.len() > limit {
        logs = logs.split_off(logs.len() - limit);
    }
    logs
}

/// The model-facing summary of a completed launch.
#[must_use]
pub fn launch_summary(launch: &Launch) -> String {
    let site = &launch.site.name;
    let deployment = &launch.deployment;
    let mut lines = vec![format!(
        "Site **{site}** ({}), deployment `{}` is {}.",
        if launch.created_site {
            "created"
        } else {
            "already existed"
        },
        deployment.id,
        status_debug(&deployment.status)
    )];
    match deployment.url.as_deref() {
        Some(url) => lines.push(format!("It will serve from {url} once the build finishes — poll `hosting_deployment_status` with the deployment id.")),
        None => lines.push("The provider has not assigned a URL yet; poll `hosting_deployment_status` with the deployment id.".to_owned()),
    }
    if let Some(database) = &launch.database {
        let kind = serde_json::to_value(&database.kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "database".to_owned());
        let keys = if launch.database_env_keys.is_empty() {
            "no variables".to_owned()
        } else {
            launch.database_env_keys.join(", ")
        };
        lines.push(format!("Database **{}** ({kind}) is {}; it injected {keys} into the site's environment. The values are the provider's — nothing here can read them.", database.name, database.status));
    }
    if !launch.domains.is_empty() {
        let unverified: Vec<&str> = launch
            .domains
            .iter()
            .filter(|domain| !domain.verified)
            .map(|domain| domain.name.as_str())
            .collect();
        if unverified.is_empty() {
            lines.push("Every domain is verified.".to_owned());
        } else {
            lines.push(format!("These domains still need their DNS records pointed at the provider before they serve traffic: {}.", unverified.join(", ")));
        }
    }
    lines.join("\n\n")
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
