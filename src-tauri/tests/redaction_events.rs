use qingzhou_ssh_lib::{
    core::redaction::{Redactor, REDACTED},
    domain::events::{
        truncate_utf8, EventSequence, ExecutionEventPayload, OutputBudget, Utf8Chunker,
    },
    error::AppError,
};

#[test]
fn redacts_runtime_secrets_private_keys_and_named_credentials() {
    let redactor = Redactor::new(["canary-password", "token-canary"]);
    let input = "password=canary-password token: token-canary\n-----BEGIN OPENSSH PRIVATE KEY-----\nprivate-canary\n-----END OPENSSH PRIVATE KEY-----";
    let output = redactor.redact(input);

    assert!(!output.contains("canary-password"));
    assert!(!output.contains("token-canary"));
    assert!(!output.contains("private-canary"));
    assert!(output.contains(REDACTED));
}

#[test]
fn redacts_nested_json_before_it_reaches_ipc() {
    let redactor = Redactor::new(["nested-canary"]);
    let value = serde_json::json!({
        "password": "visible-value",
        "nested": { "message": "prefix nested-canary suffix" },
        "safe": "kept"
    });
    let redacted = redactor.redact_json(&value);

    assert_eq!(redacted["password"], REDACTED);
    assert_eq!(
        redacted["nested"]["message"],
        format!("prefix {REDACTED} suffix")
    );
    assert_eq!(redacted["safe"], "kept");
}

#[test]
fn utf8_chunker_preserves_characters_across_network_boundaries() {
    let bytes = "甲乙丙丁".as_bytes();
    let mut chunker = Utf8Chunker::new(7).unwrap();
    let mut chunks = chunker.push(&bytes[..2]);
    assert!(chunks.is_empty());
    chunks.extend(chunker.push(&bytes[2..8]));
    chunks.extend(chunker.push(&bytes[8..]));
    chunks.extend(chunker.finish());

    assert_eq!(chunks.concat(), "甲乙丙丁");
    assert!(chunks.iter().all(|chunk| chunk.len() <= 7));
}

#[test]
fn sequence_is_monotonic_and_output_budget_is_bounded() {
    let mut sequence = EventSequence::default();
    let first = sequence.next(ExecutionEventPayload::Stdout {
        text: "one".into(),
        total_bytes: 3,
    });
    let second = sequence.next(ExecutionEventPayload::Stderr {
        text: "two".into(),
        total_bytes: 6,
    });
    assert_eq!((first.sequence, second.sequence), (1, 2));

    let mut budget = OutputBudget::new(8);
    budget.consume(5).unwrap();
    let error = budget.consume(4).unwrap_err();
    assert!(matches!(error, AppError::OutputLimitExceeded { limit: 8 }));
}

#[test]
fn summaries_are_capped_on_a_utf8_boundary() {
    let summary = truncate_utf8("错".repeat(4_000), 8 * 1024);
    assert!(summary.len() <= 8 * 1024);
    assert!(summary.is_char_boundary(summary.len()));
}
