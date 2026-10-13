use super::*;
use serde_json::json;

fn deployment(status: DeploymentStatus) -> Deployment {
    Deployment {
        id: "dpl_1".into(),
        site: "demo".into(),
        url: Some("https://demo.example".into()),
        status,
        target: DeploymentTarget::Preview,
        created_at_ms: None,
        error_message: None,
    }
}

fn op(name: &str, args: Value) -> Result<Operation, ToolArgsError> {
    Operation::from_tool_args(name, &args, 1_000_000_000)
}

#[test]
fn rollback_refuses_everything_except_a_ready_deployment() {
    for status in [
        DeploymentStatus::Queued,
        DeploymentStatus::Building,
        DeploymentStatus::Failed,
        DeploymentStatus::Canceled,
        DeploymentStatus::Other("x".into()),
    ] {
        let d = deployment(status);
        assert!(!rollback_target_is_ready(&d));
        assert!(!rollback_refusal("dpl_1", &d).contains("is Ready"));
    }
    assert!(rollback_target_is_ready(&deployment(
        DeploymentStatus::Ready
    )));
}

#[test]
fn launch_summary_keeps_the_model_facing_status_and_wait_guidance() {
    let launch: Launch = serde_json::from_value(json!({
        "site": {"id": "s1", "name": "demo"},
        "created_site": true,
        "deployment": {"id": "dpl_1", "site": "demo", "status": "building", "url": "https://demo.example"},
        "database": null,
        "domains": []
    }))
    .unwrap();
    let summary = launch_summary(&launch);
    assert!(summary.contains("Site **demo** (created), deployment `dpl_1` is Building."));
    assert!(summary.contains("poll `hosting_deployment_status`"));
}

#[test]
fn launch_summary_reports_database_and_unverified_domains() {
    let launch: Launch = serde_json::from_value(json!({
        "site": {"id": "s1", "name": "demo"},
        "created_site": false,
        "database_env_keys": ["DATABASE_URL"],
        "database": {"id": "d1", "name": "db", "kind": "postgres", "status": "ready"},
        "domains": [{"name": "a.com", "site": "demo", "verified": false}],
        "deployment": {"id": "dpl_1", "site": "demo", "status": "ready"}
    }))
    .unwrap();
    let summary = launch_summary(&launch);
    assert!(summary.contains("already existed"));
    assert!(summary.contains("provider has not assigned a URL"));
    assert!(summary.contains("Database **db** (postgres) is ready; it injected DATABASE_URL"));
    assert!(summary.contains("pointed at the provider before they serve traffic: a.com."));
}

#[test]
fn single_step_tools_map_to_their_operations() {
    assert!(matches!(
        op("hosting_deployment_status", json!({"deployment_id": " d "})),
        Ok(Operation::Deployment { id }) if id == "d"
    ));
    assert!(matches!(
        op(
            "hosting_list_deployments",
            json!({"site": "s", "limit": 9999})
        ),
        Ok(Operation::ListDeployments { limit: 100, .. })
    ));
    assert!(matches!(
        op("hosting_list_sites", json!({})),
        Ok(Operation::ListSites { limit: 20 })
    ));
    assert!(matches!(
        op("hosting_domain_status", json!({"site": "s"})),
        Ok(Operation::ListDomains { .. })
    ));
    assert_eq!(
        op("hosting_deployment_status", json!({})).unwrap_err().0,
        "`deployment_id` is required"
    );
    assert_eq!(
        op("nope", json!({})).unwrap_err().0,
        "hosting tool declaration is unsupported"
    );
}

#[test]
fn set_env_shapes_targets_secret_and_value_types() {
    let Ok(Operation::SetEnv { vars, .. }) = op(
        "hosting_set_env",
        json!({"site": "s", "env": {"A": "x", "B": 2}, "production_only": true, "secret": true}),
    ) else {
        panic!("expected set_env");
    };
    assert_eq!(vars.len(), 2);
    assert!(vars.iter().all(|v| v.secret));
    assert!(
        vars.iter()
            .all(|v| v.targets == [DeploymentTarget::Production])
    );
    assert!(vars.iter().any(|v| v.key == "B" && v.value == "2"));
    assert!(op("hosting_set_env", json!({"site": "s"})).is_err());
    assert!(op("hosting_set_env", json!({"site": "s", "env": {"A": []}})).is_err());
}

#[test]
fn analytics_windows_and_rejects_unknown_breakdowns() {
    let Ok(Operation::Analytics { query }) = op(
        "hosting_analytics",
        json!({"site": "s", "days": 1, "breakdown": "country"}),
    ) else {
        panic!("expected analytics");
    };
    assert_eq!(query.until_ms, 1_000_000_000);
    assert_eq!(query.since_ms, 1_000_000_000 - 86_400_000);
    assert_eq!(query.limit, 10);
    assert_eq!(query.breakdown, Some(AnalyticsDimension::Country));
    let err = op("hosting_analytics", json!({"site": "s", "breakdown": "x"})).unwrap_err();
    assert!(err.0.contains("not `x`"));
}

#[test]
fn launch_args_become_a_plan() {
    let Ok(Operation::Launch { plan }) = Operation::launch_from_tool_args(
        &json!({"site": "s", "production": true, "database": " db ", "database_kind": "redis",
                "domains": ["a.com", " "], "env": {"K": true}}),
        Vec::new(),
    ) else {
        panic!("expected launch");
    };
    assert_eq!(plan.target, DeploymentTarget::Production);
    assert_eq!(plan.domains, ["a.com"]);
    assert_eq!(plan.database.unwrap().kind, DatabaseKind::Redis);
    assert_eq!(plan.env[0].value, "true");
    assert!(
        Operation::launch_from_tool_args(
            &json!({"site": "s", "database": "d", "database_kind": "zzz"}),
            vec![]
        )
        .is_err()
    );
}

#[test]
fn outcome_matches_the_operation_that_produces_it() {
    let list: Operation = Operation::ListSites { limit: 1 };
    assert!(Outcome::Sites(vec![]).matches(&list));
    assert!(!Outcome::Done.matches(&list));
    let promote =
        Operation::promote_from_tool_args(&json!({"site": "s", "deployment_id": "d"})).unwrap();
    assert!(Outcome::Done.matches(&promote));
    let find: Operation = Operation::FindSite { site: "s".into() };
    assert!(Outcome::NoSite.matches(&find));
}

#[test]
fn small_renderers_keep_their_wording() {
    let d = Domain {
        name: "a.com".into(),
        site: "s".into(),
        verified: true,
    };
    assert_eq!(
        add_domain_summary("s", &d),
        "a.com is attached to s and verified."
    );
    assert!(set_env_summary("s", &["A", "B"]).starts_with("Set A, B on s."));
    assert!(
        rollback_summary("s", "d", &deployment(DeploymentStatus::Ready))
            .contains("(https://demo.example)")
    );
    assert_eq!(bounded(&json!({"n": -5}), "n", 7, 1, 10), 1);
    assert_eq!(bounded(&json!({}), "n", 7, 1, 10), 7);
}
