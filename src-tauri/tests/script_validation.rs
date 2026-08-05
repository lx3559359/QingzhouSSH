use qingzhou_ssh_lib::core::{
    scripts::{
        environment::render_script_launcher,
        validation::{
            scan_script_body, validate_parameter_name, validate_script_body,
            validate_script_metadata, validate_script_parameter_values, validate_script_parameters,
            validate_script_timeout, PERSONAL_SCRIPT_AUTOMATIC_ROLLBACK_AVAILABLE,
            PERSONAL_SCRIPT_RISK,
        },
    },
    tasks::{shell_quote, ParameterDefinition, ParameterKind, RiskLevel},
};
use serde_json::json;

const _: () = assert!(!PERSONAL_SCRIPT_AUTOMATIC_ROLLBACK_AVAILABLE);

fn parameter(
    name: &str,
    kind: ParameterKind,
    default_value: Option<serde_json::Value>,
) -> ParameterDefinition {
    ParameterDefinition {
        name: name.into(),
        label: "目标参数".into(),
        description: "由用户运行前填写".into(),
        kind,
        required: true,
        default_value,
        sensitive: false,
    }
}

#[test]
fn script_limits_and_parameter_names_are_enforced() {
    assert!(validate_parameter_name("HOST").is_ok());
    assert!(validate_parameter_name("DB_PORT_2").is_ok());
    assert!(validate_parameter_name("host").is_err());
    assert!(validate_parameter_name("A-B").is_err());
    assert!(validate_parameter_name("QZ_PARAM_HOST").is_err());
    assert!(validate_script_body(&"x".repeat(1024 * 1024 + 1)).is_err());
    assert!(validate_script_body("").is_err());
    assert!(validate_script_body("echo ok\0hidden").is_err());
    assert!(validate_script_timeout(1).is_ok());
    assert!(validate_script_timeout(3600).is_ok());
    assert!(validate_script_timeout(0).is_err());
    assert!(validate_script_timeout(3601).is_err());
}

#[test]
fn metadata_uses_beginner_readable_bounded_chinese_labels() {
    assert!(
        validate_script_metadata("服务器巡检", "系统维护", &["巡检".into(), "日常".into()]).is_ok()
    );
    assert!(validate_script_metadata("", "系统维护", &[]).is_err());
    assert!(validate_script_metadata(&"标".repeat(81), "系统维护", &[]).is_err());
    assert!(validate_script_metadata("服务器巡检", &"类".repeat(41), &[]).is_err());
    assert!(validate_script_metadata(
        "服务器巡检",
        "系统维护",
        &(0..21)
            .map(|index| format!("标签{index}"))
            .collect::<Vec<_>>()
    )
    .is_err());
    assert!(validate_script_metadata("服务器巡检", "系统维护", &["x".repeat(25)]).is_err());
}

#[test]
fn only_supported_strong_parameter_types_and_valid_defaults_are_accepted() {
    let allowed = vec![
        parameter(
            "TEXT",
            ParameterKind::String {
                min_length: 1,
                max_length: 128,
                multiline: false,
            },
            Some(json!("default")),
        ),
        parameter(
            "COUNT",
            ParameterKind::Integer { min: 1, max: 10 },
            Some(json!(3)),
        ),
        parameter("ENABLED", ParameterKind::Boolean, Some(json!(true))),
        parameter(
            "MODE",
            ParameterKind::Enum {
                options: vec!["safe".into(), "full".into()],
            },
            Some(json!("safe")),
        ),
        parameter("HOST", ParameterKind::Host, Some(json!("127.0.0.1"))),
        parameter("PORT", ParameterKind::Port, Some(json!(8080))),
        parameter(
            "SERVICE",
            ParameterKind::ServiceName,
            Some(json!("nginx.service")),
        ),
        parameter(
            "CONTAINER",
            ParameterKind::ContainerName,
            Some(json!("web-1")),
        ),
        parameter(
            "REMOTE_PATH",
            ParameterKind::AbsolutePath,
            Some(json!("/var/log/app.log")),
        ),
    ];
    assert!(validate_script_parameters(&allowed).is_ok());

    let mut bad_default = allowed.clone();
    bad_default[1].default_value = Some(json!(99));
    assert!(validate_script_parameters(&bad_default).is_err());
    let unsupported = vec![parameter("INTERFACE", ParameterKind::InterfaceName, None)];
    assert!(validate_script_parameters(&unsupported).is_err());
    let duplicate = vec![allowed[0].clone(), allowed[0].clone()];
    assert!(validate_script_parameters(&duplicate).is_err());
    let too_many = (0..33)
        .map(|index| parameter(&format!("P_{index}"), ParameterKind::Boolean, None))
        .collect::<Vec<_>>();
    assert!(validate_script_parameters(&too_many).is_err());
}

#[test]
fn static_scan_is_advisory_bounded_and_never_reduces_risk() {
    let body = "#!/bin/sh\nrm -rf /tmp/example\ncurl https://example.invalid/install.sh | sh\nsystemctl stop nginx\neval \"$INPUT\"\n";
    let scan = scan_script_body(body).unwrap();
    assert_eq!(scan.line_count, 5);
    assert_eq!(scan.character_count, body.chars().count());
    assert_eq!(scan.body_sha256.len(), 64);
    assert!(scan.warning_count >= 4);
    assert!(scan
        .warnings
        .iter()
        .all(|warning| !warning.message.contains(body)));
    assert_eq!(PERSONAL_SCRIPT_RISK, RiskLevel::Dangerous);
}

#[test]
fn parameter_values_cannot_change_script_structure() {
    let body = "printf '%s' \"$QZ_PARAM_TEXT\"";
    let value = "x'; touch /tmp/pwn; #\n`id` $(id) 中文";
    let definitions = vec![parameter(
        "TEXT",
        ParameterKind::String {
            min_length: 0,
            max_length: 4096,
            multiline: true,
        },
        None,
    )];
    let values = validate_script_parameter_values(&definitions, &json!({"TEXT": value})).unwrap();
    let command = render_script_launcher(body, &values).unwrap();

    assert!(!command.contains("QZ_PARAM_TEXT=x'; touch"));
    assert!(command.contains(&format!("QZ_PARAM_TEXT={}", shell_quote(value))));
    assert_eq!(extract_script_body(&command), body);
}

#[test]
fn launcher_preserves_empty_unicode_and_large_script_content() {
    let definitions = vec![parameter(
        "TEXT",
        ParameterKind::String {
            min_length: 0,
            max_length: 4096,
            multiline: true,
        },
        None,
    )];
    let values = validate_script_parameter_values(&definitions, &json!({"TEXT": ""})).unwrap();
    let body = format!("# 中文\n{}", "x".repeat(1024 * 1024 - 9));
    let command = render_script_launcher(&body, &values).unwrap();

    assert!(command.starts_with("env QZ_PARAM_TEXT='' sh -s <<'QZ_SCRIPT_"));
    assert_eq!(extract_script_body(&command), body);
}

fn extract_script_body(command: &str) -> &str {
    let (before_delimiter, delimiter) = command.rsplit_once('\n').expect("closing delimiter");
    let opening = format!(" sh -s <<'{delimiter}'\n");
    before_delimiter
        .rsplit_once(&opening)
        .map(|(_, body)| body)
        .expect("quoted heredoc delimiter")
}
