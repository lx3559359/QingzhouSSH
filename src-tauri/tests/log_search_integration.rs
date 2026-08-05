use qingzhou_ssh_lib::core::{
    logs::{
        build_search_command, parse_search_output, LogLineKind, LogMatch, LogResultStore,
        LogSearchRequest, LogSearchTarget, RemoteFileMatch, SearchResultItem,
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
        target: LogSearchTarget::Content,
        path: path.into(),
        keyword: "Error $(id)".into(),
        case_sensitive: false,
        context_lines: 2,
        limit: 1_000,
        start_time: None,
        end_time: None,
    }
}

fn filename_request(keyword: &str) -> LogSearchRequest {
    LogSearchRequest {
        target: LogSearchTarget::Filename,
        path: String::new(),
        keyword: keyword.into(),
        case_sensitive: false,
        context_lines: 0,
        limit: 200,
        start_time: None,
        end_time: None,
    }
}

#[test]
fn accepts_standard_extensionless_linux_log_files() {
    assert!(request("/var/log/syslog").validate().is_ok());
    assert!(request("/var/log/messages").validate().is_ok());
}

#[test]
fn smart_search_is_pathless_bounded_and_never_scans_the_whole_server() {
    let smart = request("");
    smart.validate().unwrap();

    let command =
        build_search_command(&smart, &capabilities(&["find", "grep", "gzip", "awk"])).unwrap();
    for root in ["/var/log", "/opt", "/srv", "/home"] {
        assert!(command.contains(&format!("find '{root}'")));
    }
    assert!(!command.contains("find '/'"));
    assert!(command.contains("-maxdepth 6"));
    assert!(command.contains("-mtime -30"));
    assert!(command.contains("-size -32M"));
    assert!(command.contains("count < 120"));
    assert!(command.contains("'Error $(id)'"));
    assert!(command.contains("gzip -cd"));

    assert!(build_search_command(&smart, &capabilities(&["grep", "awk"])).is_err());
}

#[test]
fn filename_search_is_literal_bounded_and_never_scans_the_whole_server() {
    let request = filename_request("requi");
    request.validate().unwrap();

    let command = build_search_command(&request, &capabilities(&["find", "awk", "stat"])).unwrap();
    for root in ["/var/log", "/opt", "/srv", "/home"] {
        assert!(command.contains(&format!("find '{root}'")));
    }
    assert!(!command.contains("find '/'") && !command.contains("find / "));
    assert!(command.contains("-maxdepth 6"));
    assert!(command.contains("-iname '*requi*'"));
    assert!(command.contains("count < 200"));
}

#[test]
fn filename_search_validates_the_beginner_safe_contract() {
    for invalid in [
        filename_request(""),
        filename_request("bad\0name"),
        filename_request(&"a".repeat(257)),
        LogSearchRequest {
            path: "/var/log".into(),
            ..filename_request("requi")
        },
        LogSearchRequest {
            case_sensitive: true,
            ..filename_request("requi")
        },
        LogSearchRequest {
            context_lines: 1,
            ..filename_request("requi")
        },
        LogSearchRequest {
            limit: 201,
            ..filename_request("requi")
        },
        LogSearchRequest {
            start_time: Some("2026-08-04".into()),
            ..filename_request("requi")
        },
    ] {
        assert!(invalid.validate().is_err());
    }
}

#[test]
fn parses_filename_records_into_remote_file_results() {
    let output = concat!(
        "__QZ_FILE__\x1f/home/app/requirements.txt\x1f96\x1f1785801600\n",
        "__QZ_FILE__\x1f/opt/example/requirements-dev.txt\x1f\x1f\n",
    );
    let results = parse_search_output(
        &filename_request("requi"),
        output,
        &Redactor::new(std::iter::empty::<&str>()),
    )
    .unwrap();
    assert_eq!(
        results,
        vec![
            SearchResultItem::File(RemoteFileMatch {
                path: "/home/app/requirements.txt".into(),
                name: "requirements.txt".into(),
                size: Some(96),
                modified_at: Some(1_785_801_600),
            }),
            SearchResultItem::File(RemoteFileMatch {
                path: "/opt/example/requirements-dev.txt".into(),
                name: "requirements-dev.txt".into(),
                size: None,
                modified_at: None,
            }),
        ]
    );
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
        "__QZ_LOG__\x1f/var/log/nginx/error.log\x1f41\x1fcontext\x1f2026-08-03 before\n",
        "__QZ_LOG__\x1f/var/log/nginx/error.log\x1f42\x1fmatch\x1f2026-08-03 secret-canary error\n",
        "__QZ_LOG__\x1f/opt/example/logs/app.log\x1f43\x1fmatch\x1f2026-08-03 second\n",
        "__QZ_LOG__\x1f/opt/example/logs/app.log\x1f44\x1fmatch\x1f2026-08-03 ignored by limit\n",
    );
    let matches = parse_search_output(&request, output, &Redactor::new(["secret-canary"])).unwrap();
    assert_eq!(matches.len(), 3, "context plus two matches are retained");
    assert!(matches!(
        &matches[0],
        SearchResultItem::Content(item)
            if item.kind == LogLineKind::Context && item.path == "/var/log/nginx/error.log"
    ));
    assert!(matches!(
        &matches[1],
        SearchResultItem::Content(item)
            if item.line_number == 42
                && !item.text.contains("secret-canary")
                && item.text.contains("[REDACTED]")
    ));
    assert!(matches!(
        &matches[2],
        SearchResultItem::Content(item)
            if item.line_number == 43 && item.path == "/opt/example/logs/app.log"
    ));
}

#[tokio::test]
async fn stores_jsonl_and_text_then_pages_without_reexecuting_search() {
    let root = tempfile::tempdir().unwrap();
    let execution_id = uuid::Uuid::new_v4();
    let store = LogResultStore::new(root.path());
    let matches = (1..=125)
        .map(|line_number| {
            SearchResultItem::Content(LogMatch {
                path: "/var/log/app.log".into(),
                line_number,
                kind: LogLineKind::Match,
                timestamp: None,
                text: format!("line {line_number}"),
            })
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
    assert!(matches!(
        &first.items[49],
        SearchResultItem::Content(item) if item.line_number == 50
    ));
    assert!(matches!(
        &second.items[0],
        SearchResultItem::Content(item) if item.line_number == 51
    ));
    assert!(third.next_cursor.is_none());
}
