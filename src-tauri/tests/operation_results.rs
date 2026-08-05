use qingzhou_ssh_lib::core::{
    redaction::Redactor,
    tasks::{parse_result, OperationConclusion, ResultParserKind},
};

#[test]
fn health_parser_returns_chinese_summary_and_bounded_details() {
    let raw = "__QZ_METRIC__ disk_percent=93\n__QZ_WARNING__ disk_usage\nsecret=token-canary";
    let result = parse_result(
        ResultParserKind::HealthSummary,
        raw,
        &Redactor::new(["token-canary"]),
    )
    .unwrap();
    assert_eq!(result.status, OperationConclusion::Warning);
    assert!(result.summary.contains("磁盘"));
    assert!(result
        .suggestions
        .iter()
        .any(|suggestion| suggestion.contains("清理")));
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(!encoded.contains("token-canary"));
    assert!(result.technical_details.len() <= 64 * 1024);
}

#[test]
fn udp_no_response_is_uncertain_not_failed() {
    let result = parse_result(
        ResultParserKind::NetworkProbe,
        "probe=no_response",
        &Redactor::default(),
    )
    .unwrap();
    assert_eq!(result.status, OperationConclusion::Uncertain);
    assert!(result.summary.contains("无法确认"));
}

#[test]
fn unknown_machine_markers_are_only_redacted_technical_text() {
    let result = parse_result(
        ResultParserKind::Text,
        "__QZ_RUN_THIS__ rm -rf /\npassword=parser-canary",
        &Redactor::default(),
    )
    .unwrap();
    assert_eq!(result.status, OperationConclusion::Normal);
    assert!(result.technical_details.contains("__QZ_RUN_THIS__"));
    assert!(!result.technical_details.contains("parser-canary"));
    assert!(result.technical_details.contains("[REDACTED]"));
}
