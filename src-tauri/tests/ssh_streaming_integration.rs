use qingzhou_ssh_lib::{
    core::{
        redaction::Redactor,
        ssh::executor::{OutputStream, StreamEventWriter, VecEventSink},
    },
    domain::events::ExecutionEventPayload,
    error::AppError,
};

#[tokio::test]
async fn streams_ordered_redacted_utf8_events_to_a_bounded_file() {
    let root = tempfile::tempdir().unwrap();
    let output_path = root.path().join("execution.log");
    let redactor = Redactor::new(["secret-canary"]);
    let mut sink = VecEventSink::default();
    {
        let mut writer = StreamEventWriter::open(&output_path, &redactor, &mut sink, 128, 8)
            .await
            .unwrap();
        let utf8 = "甲secret-canary乙".as_bytes();
        writer
            .write(OutputStream::Stdout, &utf8[..2])
            .await
            .unwrap();
        writer
            .write(OutputStream::Stdout, &utf8[2..])
            .await
            .unwrap();
        writer
            .write(OutputStream::Stderr, b"warning")
            .await
            .unwrap();
        writer.finish().await.unwrap();
    }

    assert!(sink
        .events
        .windows(2)
        .all(|pair| pair[1].sequence == pair[0].sequence + 1));
    assert!(sink.events.iter().all(|event| match &event.payload {
        ExecutionEventPayload::Stdout { text, .. } | ExecutionEventPayload::Stderr { text, .. } =>
            text.len() <= 8,
        _ => true,
    }));
    let combined = sink
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            ExecutionEventPayload::Stdout { text, .. }
            | ExecutionEventPayload::Stderr { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert!(!combined.contains("secret-canary"));
    assert!(combined.contains("[REDACTED]"));

    let file = tokio::fs::read_to_string(output_path).await.unwrap();
    assert!(!file.contains("secret-canary"));
    assert!(file.contains("[stdout]"));
    assert!(file.contains("[stderr]"));
}

#[tokio::test]
async fn rejects_output_beyond_the_configured_limit() {
    let root = tempfile::tempdir().unwrap();
    let redactor = Redactor::default();
    let mut sink = VecEventSink::default();
    let mut writer = StreamEventWriter::open(
        &root.path().join("execution.log"),
        &redactor,
        &mut sink,
        5,
        8,
    )
    .await
    .unwrap();
    writer.write(OutputStream::Stdout, b"12345").await.unwrap();
    let error = writer.write(OutputStream::Stderr, b"6").await.unwrap_err();
    assert!(matches!(error, AppError::OutputLimitExceeded { limit: 5 }));
}
