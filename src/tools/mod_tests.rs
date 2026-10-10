//! Tests for the `hosting_*` tools and workspace containment.
//!
//! The provider itself is tested in its own modules against a mock of its REST
//! API. What is tested here is the tool layer: that a path an agent names
//! cannot escape the workspace, that each tool refuses a missing argument
//! before any outward call, the rollback guard against a mock provider, and
//! that every declaration an agent sees is unchanged (literal JSON below).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use serde_json::json;
use tinytools::Tool;

use super::*;
use crate::{Credentials, Host, ProviderKind};

/// A real Vercel client that is never expected to reach the network: every
/// test using it fails or returns before an outward call.
fn offline_host() -> Arc<dyn Host> {
    Arc::from(crate::connect(ProviderKind::Vercel, Credentials::new("token").unwrap()).unwrap())
}

#[test]
fn a_directory_inside_the_workspace_resolves() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(workspace.path().join("site")).expect("mkdir");

    let resolved = resolve_in_workspace(workspace.path(), "site").expect("resolves");

    assert_eq!(
        resolved,
        workspace
            .path()
            .canonicalize()
            .expect("canonical root")
            .join("site")
    );
}

#[test]
fn an_empty_path_is_the_workspace_root() {
    let workspace = tempfile::tempdir().expect("tempdir");

    let resolved = resolve_in_workspace(workspace.path(), "  ").expect("resolves");

    assert_eq!(
        resolved,
        workspace.path().canonicalize().expect("canonical root")
    );
}

#[test]
fn a_path_outside_the_workspace_is_refused() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let root = workspace.path();

    // A deployment uploads every byte under the directory to a third party, so
    // this is the check that decides what may leave the machine.
    assert!(resolve_in_workspace(root, "/etc").is_err());
    assert!(resolve_in_workspace(root, "../..").is_err());
    assert!(resolve_in_workspace(root, "does-not-exist").is_err());
}

#[test]
fn a_file_is_not_a_deployable_directory() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("page.tsx"), b"x").expect("write");

    let error =
        resolve_in_workspace(workspace.path(), "page.tsx").expect_err("a file is not a directory");

    assert!(error.to_string().contains("not a directory"), "{error}");
}

#[tokio::test]
async fn launching_reports_a_missing_directory_instead_of_deploying_nothing() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let host = offline_host();

    let tool = LaunchSiteTool::new(host, workspace.path().to_path_buf());
    let result = tool
        .execute(json!({"site": "shop", "path": "missing"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error);
}

#[tokio::test]
async fn launching_without_a_site_name_is_refused_before_any_upload() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("package.json"), b"{}").expect("write");
    let host = offline_host();

    let tool = LaunchSiteTool::new(host, workspace.path().to_path_buf());
    let result = tool
        .execute(json!({"path": "."}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error);
}

#[tokio::test]
async fn a_read_tool_reports_a_missing_argument_rather_than_calling_out() {
    let host = offline_host();

    let result = DeploymentStatusTool::new(Arc::clone(&host))
        .execute(json!({}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error);
}

// ── The deployment-history and rollback tools (issue opencompany#913) ────────

#[tokio::test]
async fn the_new_read_tools_report_a_missing_site_rather_than_calling_out() {
    let host = offline_host();

    let listed = ListDeploymentsTool::new(Arc::clone(&host))
        .execute(json!({}))
        .await
        .expect("the tool reports rather than panics");
    assert!(listed.is_error);

    let domains = DomainStatusTool::new(Arc::clone(&host))
        .execute(json!({}))
        .await
        .expect("the tool reports rather than panics");
    assert!(domains.is_error);
}

#[tokio::test]
async fn a_rollback_missing_either_argument_is_refused_before_any_call() {
    let host = offline_host();

    // A rollback names two things and neither can be guessed: repointing
    // production at an unnamed deployment is not a recoverable mistake.
    for args in [
        json!({}),
        json!({"site": "shop"}),
        json!({"deployment_id": "d1"}),
    ] {
        let result = RollbackTool::new(Arc::clone(&host))
            .execute(args.clone())
            .await
            .expect("the tool reports rather than panics");
        assert!(result.is_error, "{args} should have been refused");
    }
}

// ── The rollback guard, against a mock of the provider's API ─────────────────

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A real provider client pointed at a local mock rather than at Vercel.
///
/// `connect_to`'s own documentation names this case, and loopback `http://` is
/// accepted precisely so a test can carry a bearer token to a server that never
/// leaves the machine.
fn host_against(server: &MockServer) -> Arc<dyn crate::Host> {
    let base_url = server.uri();
    Arc::from(
        crate::connect_to(
            crate::ProviderKind::Vercel,
            crate::Credentials::new("token").expect("credentials"),
            Some(base_url.as_str()),
        )
        .expect("a client against the mock"),
    )
}

/// **The guard that makes this tool a recovery path rather than a second way to
/// break the site.**
///
/// `hosting_list_deployments` returns failed and still-building deployments too
/// — they are part of the history an agent reads — so the id it picks is not
/// necessarily one that can serve traffic. Promoting a failed build would take
/// the site down during an attempt to bring it back up.
///
/// The assertion that matters is `expect(0)` on the promote route: the refusal
/// has to happen *before* the outward call, not be an error message reported
/// after production already moved.
#[tokio::test]
async fn rolling_back_to_a_deployment_that_never_built_does_not_touch_production() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v13/deployments/dpl_broken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "dpl_broken",
            "name": "shop",
            "readyState": "ERROR",
            "errorMessage": "build failed"
        })))
        .mount(&server)
        .await;

    // Vercel's promote is POST /v10/projects/{project}/promote/{deployment},
    // reached only after GET /v9/projects/{site} resolves the id. Neither may
    // be called at all.
    Mock::given(method("GET"))
        .and(path("/v9/projects/shop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "prj_1",
            "name": "shop"
        })))
        .expect(0)
        .mount(&server)
        .await;

    let result = RollbackTool::new(host_against(&server))
        .execute(json!({"site": "shop", "deployment_id": "dpl_broken"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error, "a failed deployment must not be promoted");
    // The status is named, so a model can pick a different deployment rather
    // than retry the same one.
    let rendered = result.text();
    assert!(
        rendered.contains("Failed"),
        "the refusal should name the state it refused: {rendered}"
    );
}

/// The ordinary path: a deployment that built is promoted, and the site's URL
/// comes back so the agent can say where production now points.
#[tokio::test]
async fn rolling_back_to_a_ready_deployment_promotes_it() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v13/deployments/dpl_good"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "dpl_good",
            "name": "shop",
            "url": "shop-abc.vercel.app",
            "readyState": "READY",
            "target": "production"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v9/projects/shop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "prj_1",
            "name": "shop"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v10/projects/prj_1/promote/dpl_good"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let result = RollbackTool::new(host_against(&server))
        .execute(json!({"site": "shop", "deployment_id": "dpl_good"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(!result.is_error, "{result:?}");
    let rendered = result.text();
    assert!(
        rendered.contains("dpl_good") && rendered.contains("shop-abc.vercel.app"),
        "the result should say what is serving and where: {rendered}"
    );
}

/// The history read an agent uses to *find* the id above.
#[tokio::test]
async fn listing_deployments_reports_the_history_a_rollback_picks_from() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v7/deployments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "deployments": [
                {"id": "dpl_new", "name": "shop", "readyState": "ERROR"},
                {"id": "dpl_old", "name": "shop", "readyState": "READY"}
            ]
        })))
        .mount(&server)
        .await;

    let result = ListDeploymentsTool::new(host_against(&server))
        .execute(json!({"site": "shop"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(!result.is_error, "{result:?}");

    // Both are reported, and each status is bound to its id. The failed one is
    // why the agent is here and the ready one is where it is going, so a list
    // that returned the right ids against the wrong statuses would send the
    // rollback at the deployment that just broke the site.
    let deployments: serde_json::Value =
        serde_json::from_str(&result.text()).expect("the tool answers with JSON");
    let rows = deployments.as_array().expect("an array of deployments");

    assert_eq!(rows.len(), 2, "{deployments}");
    // Newest first, as the crate documents and as the provider returned them.
    assert_eq!(rows[0]["id"], "dpl_new", "{deployments}");
    assert_eq!(rows[0]["status"], "failed", "{deployments}");
    assert_eq!(rows[1]["id"], "dpl_old", "{deployments}");
    assert_eq!(rows[1]["status"], "ready", "{deployments}");
}

/// A domain that is attached but unverified is not serving, and the tool has to
/// say so — that difference is the entire reason to read domains.
#[tokio::test]
async fn domain_status_distinguishes_a_verified_domain_from_a_pending_one() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v9/projects/shop/domains"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "domains": [
                {"name": "shop.example.com", "verified": true},
                {"name": "www.example.com", "verified": false}
            ]
        })))
        .mount(&server)
        .await;

    let result = DomainStatusTool::new(host_against(&server))
        .execute(json!({"site": "shop"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(!result.is_error, "{result:?}");

    // Bound to the name rather than checked for presence: asserting only that a
    // `true` and a `false` both appear somewhere passes just as happily if the
    // two are swapped, which is the one thing this tool must not get wrong.
    let domains: serde_json::Value =
        serde_json::from_str(&result.text()).expect("the tool answers with JSON");
    let verified_for = |name: &str| -> bool {
        let entry = domains
            .as_array()
            .expect("an array of domains")
            .iter()
            .find(|domain| domain["name"] == name)
            .unwrap_or_else(|| panic!("{name} is missing from {domains}"));
        entry["verified"]
            .as_bool()
            .expect("`verified` is a boolean")
    };

    assert!(
        verified_for("shop.example.com"),
        "the verified domain must report verified: {domains}"
    );
    assert!(
        !verified_for("www.example.com"),
        "and the pending one must not — that difference is the entire reason to \
         read domains: {domains}"
    );
}

/// The read an agent reaches for once `hosting_deployment_status` says a build
/// failed. A status carries one error line; the reason is in the events.
#[tokio::test]
async fn deployment_logs_report_the_build_error_behind_a_failed_status() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/deployments/dpl_broken/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"created": 1, "type": "stdout", "payload": "installing dependencies"},
            {"created": 2, "type": "stderr", "payload": "error TS2304: cannot find name 'foo'"}
        ])))
        .mount(&server)
        .await;

    let result = DeploymentLogsTool::new(host_against(&server))
        .execute(json!({"deployment_id": "dpl_broken"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(!result.is_error, "{result:?}");

    let logs: serde_json::Value =
        serde_json::from_str(&result.text()).expect("the tool answers with JSON");
    let rows = logs.as_array().expect("an array of events");

    assert_eq!(rows.len(), 2, "{logs}");
    // Oldest first, as the crate documents, and each message bound to its kind:
    // a log that reported the right lines against the wrong streams would show
    // a build error as ordinary output and hide the one line worth reading.
    assert_eq!(rows[0]["kind"], "stdout", "{logs}");
    assert_eq!(rows[1]["kind"], "stderr", "{logs}");
    assert!(
        rows[1]["message"]
            .as_str()
            .expect("a message")
            .contains("TS2304"),
        "the build error must survive to the model: {logs}"
    );
}

/// Trimming keeps the tail. A build that fails after a thousand lines of setup
/// puts its error at the end, so a limit that kept the head would return a
/// thousand lines of noise and drop the only one an agent came for.
#[tokio::test]
async fn a_limited_log_read_keeps_the_end_where_the_failure_is() {
    let server = MockServer::start().await;

    let events: Vec<serde_json::Value> = (0..10)
        .map(|n| json!({"created": n, "type": "stdout", "payload": format!("line {n}")}))
        .collect();
    Mock::given(method("GET"))
        .and(path("/v3/deployments/dpl_long/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events))
        .mount(&server)
        .await;

    let result = DeploymentLogsTool::new(host_against(&server))
        .execute(json!({"deployment_id": "dpl_long", "limit": 3}))
        .await
        .expect("the tool reports rather than panics");

    assert!(!result.is_error, "{result:?}");

    let logs: serde_json::Value =
        serde_json::from_str(&result.text()).expect("the tool answers with JSON");
    let rows = logs.as_array().expect("an array of events");

    assert_eq!(rows.len(), 3, "{logs}");
    assert_eq!(rows[0]["message"], "line 7", "{logs}");
    assert_eq!(rows[2]["message"], "line 9", "{logs}");
}

#[tokio::test]
async fn a_negative_log_limit_is_clamped_to_one() {
    let server = MockServer::start().await;

    let events: Vec<serde_json::Value> = (0..3)
        .map(|n| json!({"created": n, "type": "stdout", "payload": format!("line {n}")}))
        .collect();
    Mock::given(method("GET"))
        .and(path("/v3/deployments/dpl_negative_limit/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(events))
        .mount(&server)
        .await;

    let result = DeploymentLogsTool::new(host_against(&server))
        .execute(json!({"deployment_id": "dpl_negative_limit", "limit": -1}))
        .await
        .expect("the tool reports rather than panics");

    assert!(!result.is_error, "{result:?}");
    let logs: serde_json::Value =
        serde_json::from_str(&result.text()).expect("the tool answers with JSON");
    let rows = logs.as_array().expect("an array of events");

    assert_eq!(rows.len(), 1, "{logs}");
    assert_eq!(rows[0]["message"], "line 2", "{logs}");
}

/// A read tool that calls out before checking its arguments turns a model's
/// omission into a provider request.
#[tokio::test]
async fn reading_logs_without_a_deployment_id_is_refused_before_any_call() {
    let server = MockServer::start().await;

    let result = DeploymentLogsTool::new(host_against(&server))
        .execute(json!({}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error, "{result:?}");
    assert!(
        result.text().contains("deployment_id"),
        "the error should name the argument: {}",
        result.text()
    );
    // Nothing was mounted, so any request would have been a miss; assert the
    // absence rather than trusting that.
    assert!(
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty(),
        "a missing argument must not reach the provider"
    );
}

// ── The declarations a model sees ────────────────────────────────────────────

/// Every tool's name, description, schema, permission and external-effect flag,
/// as the `OpenHuman` host declared them before the tools moved here. A change
/// to any of these changes a prompt, so it has to be a change to this file too.
const DECLARATIONS: &str = tinyhosts_bus::TOOL_DECLARATIONS_JSON;

fn declarations_of(tools: &[Box<dyn Tool>]) -> serde_json::Value {
    tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "parameters_schema": tool.parameters_schema(),
                "permission_level": format!("{:?}", tool.permission_level()),
                "external_effect": tool.external_effect(),
            })
        })
        .collect()
}

#[test]
fn every_declaration_is_byte_identical_to_the_fixture() {
    let workspace = tempfile::tempdir().unwrap();
    let tools = hosting_tools(&offline_host(), workspace.path());
    let actual = declarations_of(&tools);
    let expected: serde_json::Value = serde_json::from_str(DECLARATIONS).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(expected.as_array().unwrap().len(), 10);
}

#[test]
fn the_tools_come_in_a_stable_order() {
    let workspace = tempfile::tempdir().unwrap();
    let names: Vec<String> = hosting_tools(&offline_host(), workspace.path())
        .iter()
        .map(|tool| tool.name().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "hosting_launch_site",
            "hosting_deployment_status",
            "hosting_list_deployments",
            "hosting_deployment_logs",
            "hosting_rollback",
            "hosting_list_sites",
            "hosting_set_env",
            "hosting_add_domain",
            "hosting_domain_status",
            "hosting_analytics",
        ]
    );
}

// ── What `hosting_launch_site` asks for and what it tells the model ──────────

/// A launch as a provider would report it, from the JSON the crate serializes.
fn launch_from(value: serde_json::Value) -> crate::Launch {
    serde_json::from_value(value).expect("a launch")
}

#[test]
fn a_launch_without_a_url_says_to_poll_for_one() {
    let launch = launch_from(json!({
        "site": {"id": "prj_1", "name": "shop"},
        "created_site": true,
        "deployment": {"id": "dpl_1", "site": "shop", "status": "queued"}
    }));

    let text = launch::describe(&launch);

    assert!(text.contains("**shop** (created)"), "{text}");
    assert!(text.contains("has not assigned a URL yet"), "{text}");
    assert!(!text.contains("Database"), "{text}");
}

#[test]
fn a_launch_names_its_url_database_and_unverified_domains() {
    let launch = launch_from(json!({
        "site": {"id": "prj_1", "name": "shop"},
        "created_site": false,
        "database": {
            "id": "db_1", "name": "shop-db", "kind": "postgres", "status": "available"
        },
        "database_env_keys": ["DATABASE_URL", "PGHOST"],
        "domains": [
            {"name": "shop.example", "site": "shop", "verified": true},
            {"name": "www.shop.example", "site": "shop", "verified": false}
        ],
        "deployment": {
            "id": "dpl_1", "site": "shop", "url": "https://shop.example.app", "status": "building"
        }
    }));

    let text = launch::describe(&launch);

    assert!(text.contains("(already existed)"), "{text}");
    assert!(text.contains("https://shop.example.app"), "{text}");
    assert!(text.contains("**shop-db** (postgres)"), "{text}");
    assert!(text.contains("DATABASE_URL, PGHOST"), "{text}");
    assert!(text.contains(": www.shop.example."), "{text}");
}

#[test]
fn a_launch_with_a_bare_database_and_verified_domains_says_so() {
    let launch = launch_from(json!({
        "site": {"id": "prj_1", "name": "shop"},
        "created_site": true,
        "database": {"id": "db_1", "name": "cache", "kind": "redis", "status": "available"},
        "domains": [{"name": "shop.example", "site": "shop", "verified": true}],
        "deployment": {"id": "dpl_1", "site": "shop", "status": "ready"}
    }));

    let text = launch::describe(&launch);

    assert!(text.contains("injected no variables"), "{text}");
    assert!(text.contains("Every domain is verified."), "{text}");
}

#[test]
fn every_launch_argument_reaches_the_plan() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("package.json"), b"{}").expect("write");
    let tool = LaunchSiteTool::new(offline_host(), workspace.path().to_path_buf());

    let plan = tool
        .plan(&json!({
            "site": "shop",
            "database": " shop-db ",
            "database_kind": "redis",
            "env": {"API": "x", "PORT": 3000, "DEBUG": true},
            "domains": ["shop.example", "  ", 7],
            "production": true
        }))
        .expect("a plan");

    assert_eq!(plan.site.name, "shop");
    let database = plan.database.expect("a database");
    assert_eq!(database.name, "shop-db");
    assert_eq!(database.kind, crate::DatabaseKind::Redis);
    let env: Vec<(&str, &str)> = plan
        .env
        .iter()
        .map(|var| (var.key.as_str(), var.value.as_str()))
        .collect();
    assert_eq!(env, [("API", "x"), ("DEBUG", "true"), ("PORT", "3000")]);
    assert_eq!(plan.domains, ["shop.example"]);
    assert_eq!(plan.target, crate::DeploymentTarget::Production);
}

#[test]
fn the_database_kind_defaults_to_postgres_and_keeps_an_unknown_one() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("package.json"), b"{}").expect("write");
    let tool = LaunchSiteTool::new(offline_host(), workspace.path().to_path_buf());

    for (kind, expected) in [
        (None, crate::DatabaseKind::Postgres),
        (Some("postgres"), crate::DatabaseKind::Postgres),
        (Some("blob"), crate::DatabaseKind::Blob),
        (
            Some("mongo"),
            crate::DatabaseKind::Other("mongo".to_string()),
        ),
    ] {
        let mut args = json!({"site": "shop", "database": "db"});
        if let Some(kind) = kind {
            args["database_kind"] = json!(kind);
        }
        let plan = tool.plan(&args).expect("a plan");
        assert_eq!(plan.database.expect("a database").kind, expected);
    }

    let plan = tool
        .plan(&json!({"site": "shop", "database": "  "}))
        .expect("a plan");
    assert!(plan.database.is_none(), "a blank name provisions nothing");
    assert_eq!(plan.target, crate::DeploymentTarget::Preview);
}

#[tokio::test]
async fn a_launch_env_value_that_is_not_a_scalar_is_refused() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("package.json"), b"{}").expect("write");
    let tool = LaunchSiteTool::new(offline_host(), workspace.path().to_path_buf());

    for value in [json!(null), json!({"nested": 1}), json!([1])] {
        let result = tool
            .execute(json!({"site": "shop", "env": {"KEY": value}}))
            .await
            .expect("the tool reports rather than panics");
        assert!(result.is_error, "{value} should have been refused");
    }
}

#[tokio::test]
async fn a_launch_the_provider_rejects_is_reported_as_an_error() {
    let server = MockServer::start().await;
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("package.json"), b"{}").expect("write");

    let result = LaunchSiteTool::new(host_against(&server), workspace.path().to_path_buf())
        .execute(json!({"site": "shop"}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error);
}

// ── Analytics, sites and environment, against a mock of the provider ─────────

#[tokio::test]
async fn analytics_refuses_a_missing_site_and_an_unknown_breakdown() {
    let host = offline_host();

    let missing = AnalyticsTool::new(Arc::clone(&host))
        .execute(json!({}))
        .await
        .expect("the tool reports rather than panics");
    assert!(missing.is_error);

    let unknown = AnalyticsTool::new(host)
        .execute(json!({"site": "shop", "breakdown": "planet"}))
        .await
        .expect("the tool reports rather than panics");
    assert!(unknown.is_error);
}

#[tokio::test]
async fn analytics_reports_a_provider_failure_for_every_breakdown() {
    let server = MockServer::start().await;
    let host = host_against(&server);

    for breakdown in [
        None,
        Some("country"),
        Some("request_path"),
        Some("device_type"),
        Some("browser_name"),
        Some("os_name"),
        Some("referrer_hostname"),
        Some("route"),
    ] {
        let mut args = json!({"site": "shop", "days": 400});
        if let Some(breakdown) = breakdown {
            args["breakdown"] = json!(breakdown);
        }
        let result = AnalyticsTool::new(Arc::clone(&host))
            .execute(args)
            .await
            .expect("the tool reports rather than panics");
        assert!(result.is_error, "{breakdown:?}: the mock knows no route");
    }
}

#[tokio::test]
async fn listing_sites_reports_a_provider_failure() {
    let server = MockServer::start().await;

    let result = ListSitesTool::new(host_against(&server))
        .execute(json!({"limit": 500}))
        .await
        .expect("the tool reports rather than panics");

    assert!(result.is_error);
}

#[tokio::test]
async fn setting_env_refuses_bad_arguments_before_any_call() {
    let host = offline_host();

    for args in [
        json!({"env": {"A": "1"}}),
        json!({"site": "shop"}),
        json!({"site": "shop", "env": {"A": null}}),
    ] {
        let result = SetEnvTool::new(Arc::clone(&host))
            .execute(args.clone())
            .await
            .expect("the tool reports rather than panics");
        assert!(result.is_error, "{args} should have been refused");
    }
}

#[tokio::test]
async fn setting_env_reports_a_provider_failure() {
    let server = MockServer::start().await;

    for args in [
        json!({"site": "shop", "env": {"A": "1", "B": 2}}),
        json!({"site": "shop", "env": {"A": "1"}, "secret": true, "production_only": true}),
    ] {
        let result = SetEnvTool::new(host_against(&server))
            .execute(args)
            .await
            .expect("the tool reports rather than panics");
        assert!(result.is_error);
    }
}
