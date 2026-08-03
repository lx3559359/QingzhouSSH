use qingzhou_ssh_lib::{
    core::{
        system_probe::SystemCapabilities,
        tasks::{
            built_in_catalog, render_command, select_implementation, shell_quote,
            validate_parameters, ParameterKind, TaskCategory,
        },
    },
    error::AppError,
};

fn capabilities(os_id: &str, family: &str, service: &str, commands: &[&str]) -> SystemCapabilities {
    SystemCapabilities {
        os_id: os_id.into(),
        os_family: family.into(),
        version_id: Some("1".into()),
        package_manager: None,
        service_manager: service.into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: commands.iter().map(|value| (*value).into()).collect(),
    }
}

#[test]
fn shell_quote_handles_empty_quotes_newlines_and_substitutions() {
    assert_eq!(shell_quote(""), "''");
    assert_eq!(shell_quote("a b"), "'a b'");
    assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
    assert_eq!(shell_quote("$(id)\nnext"), "'$(id)\nnext'");
}

#[test]
fn catalog_contains_stable_required_tasks_and_dangerous_service_actions() {
    let catalog = built_in_catalog();
    let ids = catalog
        .iter()
        .map(|definition| definition.id.as_str())
        .collect::<Vec<_>>();
    for required in [
        "system.overview",
        "system.disk_usage",
        "system.process_query",
        "service.status",
        "service.start",
        "service.stop",
        "service.restart",
        "logs.search",
    ] {
        assert!(ids.contains(&required), "missing {required}");
    }
    assert!(catalog.iter().all(|definition| definition.version > 0));
    assert!(catalog
        .iter()
        .filter(|definition| {
            matches!(
                definition.id.as_str(),
                "service.start" | "service.stop" | "service.restart"
            )
        })
        .all(|definition| definition.risk_level.as_str() == "dangerous"));
}

#[test]
fn validates_typed_parameters_and_rejects_unknown_or_unsafe_values() {
    let definition = built_in_catalog()
        .into_iter()
        .find(|definition| definition.id == "service.restart")
        .unwrap();
    assert!(matches!(definition.category, TaskCategory::Service));
    assert!(matches!(
        definition.parameters[0].kind,
        ParameterKind::ServiceName
    ));

    let validated =
        validate_parameters(&definition, &serde_json::json!({"service": "nginx@blue"})).unwrap();
    let implementation = select_implementation(
        &definition,
        &capabilities("kylin", "debian", "systemd", &["systemctl"]),
    )
    .unwrap();
    let rendered = render_command(implementation, &validated).unwrap();
    assert_eq!(rendered, "systemctl restart -- 'nginx@blue'");

    for invalid in [
        serde_json::json!({"service": "nginx; id"}),
        serde_json::json!({"service": "../nginx"}),
        serde_json::json!({"service": "nginx", "raw": "id"}),
    ] {
        assert!(validate_parameters(&definition, &invalid).is_err());
    }
}

#[test]
fn automatically_matches_mainstream_and_domestic_linux_families() {
    let definition = built_in_catalog()
        .into_iter()
        .find(|definition| definition.id == "service.status")
        .unwrap();
    for capabilities in [
        capabilities("ubuntu", "debian", "systemd", &["systemctl"]),
        capabilities("rocky", "rhel", "systemd", &["systemctl"]),
        capabilities("anolis", "rhel", "systemd", &["systemctl"]),
        capabilities("openeuler", "openeuler", "systemd", &["systemctl"]),
        capabilities("kylin", "debian", "systemd", &["systemctl"]),
        capabilities("uos", "debian", "service", &["service"]),
    ] {
        assert!(select_implementation(&definition, &capabilities).is_ok());
    }

    let unsupported = capabilities("unknown", "unknown", "unknown", &[]);
    assert!(matches!(
        select_implementation(&definition, &unsupported),
        Err(AppError::Compatibility(_))
    ));
}

#[test]
fn process_query_is_quoted_and_limits_are_bounded() {
    let definition = built_in_catalog()
        .into_iter()
        .find(|definition| definition.id == "system.process_query")
        .unwrap();
    let validated = validate_parameters(
        &definition,
        &serde_json::json!({"query": "worker$(touch /tmp/pwned)", "limit": 25}),
    )
    .unwrap();
    let implementation = select_implementation(
        &definition,
        &capabilities("uos", "debian", "systemd", &["ps", "grep", "head"]),
    )
    .unwrap();
    let rendered = render_command(implementation, &validated).unwrap();
    assert!(rendered.contains("'worker$(touch /tmp/pwned)'"));
    assert!(rendered.ends_with("head -n 25"));

    let error = validate_parameters(
        &definition,
        &serde_json::json!({"query": "worker", "limit": 5000}),
    )
    .unwrap_err();
    assert!(matches!(error, AppError::Validation(_)));
}
