use qingzhou_ssh_lib::core::{
    logs::{
        build_search_command, parse_search_output, LogLineKind, LogMatch, LogResultStore,
        LogSearchRequest,
    },
    redaction::Redactor,
    system_probe::SystemCapabilities,
};

fn capabilities(commands: &[&str]) -> SystemCapabilities {
    SystemCapabilities {
        os_id: "kylin".into(),
        os_family: "debian".into(),
        version_id: Some("V10".into()),
        package_manager: Some("apt".into()),
        service_manager: "systemd".into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: commands.iter().map(|value| (*value).into()).collect(),
    }
}

fn request(path: &str) -> LogSearchRequest {
    LogSearchRequest {
        path: path.into(),
        keyword: "Error $(id)".into(),
        case_sensitive: false,
        context_lines: 2,
        limit: 1_000,
        start_time: None,
        end_time: None,
    }
}

#[test]
fn validates_log_request_bounds_and_selects_plain_or_gzip_command() {
    let plain = request("/var/log/app's.log");
    plain.validate().unwrap();
    let command = build_search_command(&plain, &capabilities(&["grep", "awk"])).unwrap();
    assert!(command.contains("grep -n -F -i -C 2"));
    assert!(command.contains("'/var/log/app'\"'\"'s.log'"));
    assert!(command.contains("'Error $(id)'"));
    assert!(!command.contains("gzip -cd"));

    let gzip = request("/var/log/app.log.gz");
    let command = build_search_command(&gzip, &capabilities(&["grep", "gzip", "awk"])).unwrap();
    assert!(command.contains("gzip -cd -- '/var/log/app.log.gz'"));

    for invalid in [
        LogSearchRequest {
            path: "relative.log".into(),
            ..plain.clone()
        },
        LogSearchRequest {
            context_lines: 21,
            ..plain.clone()
        },
        LogSearchRequest {
            limit: 10_001,
            ..plain.clone()
        },
        LogSearchRequest {
            keyword: String::new(),
            ..plain.clone()
        },
    ] {
        assert!(invalid.validate().is_err());
    }
    assert!(build_search_command(&gzip, &capabilities(&["grep", "awk"])).is_err());
}

#[test]
fn parses_machine_records_applies_limit_and_redacts_preview() {
    let request = LogSearchRequest {
        limit: 2,
        ..request("/var/log/app.log")
    };
    let output = concat!(
        "__QZ_LOG__\x1f41\x1fcontext\x1f2026-08-03 before\n",
        "__QZ_LOG__\x1f42\x1fmatch\x1f2026-08-03 secret-canary error\n",
        "__QZ_LOG__\x1f43\x1fmatch\x1f2026-08-03 second\n",
        "__QZ_LOG__\x1f44\x1fmatch\x1f2026-08-03 ignored by limit\n",
    );
    let matches = parse_search_output(&request, output, &Redactor::new(["secret-canary"])).unwrap();
    assert_eq!(matches.len(), 3, "context plus two matches are retained");
    assert_eq!(matches[0].kind, LogLineKind::Context);
    assert_eq!(matches[1].line_number, 42);
    assert!(!matches[1].text.contains("secret-canary"));
    assert!(matches[1].text.contains("[REDACTED]"));
    assert_eq!(matches[2].line_number, 43);
}

#[tokio::test]
async fn stores_jsonl_and_text_then_pages_without_reexecuting_search() {
    let root = tempfile::tempdir().unwrap();
    let execution_id = uuid::Uuid::new_v4();
    let store = LogResultStore::new(root.path());
    let matches = (1..=125)
        .map(|line_number| LogMatch {
            path: "/var/log/app.log".into(),
            line_number,
            kind: LogLineKind::Match,
            timestamp: None,
            text: format!("line {line_number}"),
        })
        .collect::<Vec<_>>();
    let stored = store.write(execution_id, &matches).await.unwrap();
    assert_eq!(stored.count, 125);
    assert!(root.path().join(&stored.jsonl_relative_path).is_file());
    assert!(root.path().join(&stored.text_relative_path).is_file());

    let first = store.read_page(execution_id, None, 50).await.unwrap();
    let second = store
        .read_page(execution_id, first.next_cursor.as_deref(), 50)
        .await
        .unwrap();
    let third = store
        .read_page(execution_id, second.next_cursor.as_deref(), 50)
        .await
        .unwrap();
    assert_eq!(
        (first.items.len(), second.items.len(), third.items.len()),
        (50, 50, 25)
    );
    assert_eq!(first.items[49].line_number, 50);
    assert_eq!(second.items[0].line_number, 51);
    assert!(third.next_cursor.is_none());
}
