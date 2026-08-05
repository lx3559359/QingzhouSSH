use crate::{
    core::{
        logs::request::{LogSearchRequest, LogSearchTarget},
        system_probe::SystemCapabilities,
        tasks::shell_quote,
    },
    error::{AppError, AppResult},
};

pub fn build_search_command(
    request: &LogSearchRequest,
    capabilities: &SystemCapabilities,
) -> AppResult<String> {
    request.validate()?;
    let required_commands: &[&str] = match request.target {
        LogSearchTarget::Filename => &["find", "awk"],
        LogSearchTarget::Content if request.is_smart_search() => &["find", "grep", "awk"],
        LogSearchTarget::Content => &["grep", "awk"],
    };
    for command in required_commands {
        if !capabilities.has_command(command) {
            return Err(AppError::Compatibility(format!(
                "日志检索需要远端命令 {command}"
            )));
        }
    }
    if request.target == LogSearchTarget::Content
        && !request.is_smart_search()
        && request.is_gzip()
        && !capabilities.has_command("gzip")
    {
        return Err(AppError::Compatibility(
            "检索 .gz 日志需要远端命令 gzip".into(),
        ));
    }

    if request.target == LogSearchTarget::Filename {
        return Ok(build_filename_search_command(request));
    }

    if request.is_smart_search() {
        return Ok(build_smart_search_command(
            request,
            capabilities.has_command("gzip"),
        ));
    }

    let case_option = if request.case_sensitive { "" } else { " -i" };
    let grep = format!(
        "grep -n -F{case_option} -C {} -- {}",
        request.context_lines,
        shell_quote(&request.keyword)
    );
    let source = if request.is_gzip() {
        format!("gzip -cd -- {} | {grep}", shell_quote(&request.path))
    } else {
        format!("{grep} {}", shell_quote(&request.path))
    };
    let awk = r#"$0 == "--" { next } match($0, /^[0-9]+[:-]/) { mark=substr($0, RLENGTH, 1); line=substr($0, 1, RLENGTH-1); text=substr($0, RLENGTH+1); kind=(mark == ":" ? "match" : "context"); printf "__QZ_LOG__\037%s\037%s\037%s\n", line, kind, text }"#;
    Ok(format!("{source} | awk {}", shell_quote(awk)))
}

fn build_filename_search_command(request: &LogSearchRequest) -> String {
    let mut pattern = String::from("*");
    for character in request.keyword.chars() {
        if matches!(character, '*' | '?' | '[' | ']' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('*');

    let mut discovery = Vec::new();
    for root in ["/var/log", "/opt", "/srv", "/home"] {
        discovery.push(format!(
            "find {} -maxdepth 6 -type f -iname {} -print 2>/dev/null",
            shell_quote(root),
            shell_quote(&pattern)
        ));
    }
    let candidate_filter = r#"!seen[$0]++ && count < 200 { print; count++ }"#;
    let discovery = format!(
        "{{ {}; }} | awk {}",
        discovery.join("; "),
        shell_quote(candidate_filter)
    );
    format!(
        "{discovery} | while IFS= read -r qz_file; do qz_size=; qz_modified=; if command -v stat >/dev/null 2>&1; then qz_size=$(stat -c %s -- \"$qz_file\" 2>/dev/null || :); qz_modified=$(stat -c %Y -- \"$qz_file\" 2>/dev/null || :); fi; printf '__QZ_FILE__\\037%s\\037%s\\037%s\\n' \"$qz_file\" \"$qz_size\" \"$qz_modified\"; done; exit 0"
    )
}

fn build_smart_search_command(request: &LogSearchRequest, supports_gzip: bool) -> String {
    let names = r#"\( -name '*.log' -o -name '*.log.*' -o -name '*.out' -o -name '*.err' -o -name 'syslog' -o -name 'messages' -o -name 'secure' -o -name 'auth.log' -o -name 'kern.log' -o -name 'daemon.log' \)"#;
    let mut discovery = Vec::new();
    for root in ["/var/log", "/opt", "/srv", "/home"] {
        discovery.push(format!(
            "find {} -maxdepth 6 -type f {names} -mtime -30 -size -32M -print 2>/dev/null",
            shell_quote(root)
        ));
    }
    let candidate_filter = r#"!seen[$0]++ && count < 120 { print; count++ }"#;
    let discovery = format!(
        "{{ {}; }} | awk {}",
        discovery.join("; "),
        shell_quote(candidate_filter)
    );
    let case_option = if request.case_sensitive { "" } else { " -i" };
    let grep = format!(
        "grep -n -F{case_option} -C {} -- {}",
        request.context_lines,
        shell_quote(&request.keyword)
    );
    let record_formatter = r#"$0 == "--" { next } match($0, /^[0-9]+[:-]/) { mark=substr($0, RLENGTH, 1); line=substr($0, 1, RLENGTH-1); text=substr($0, RLENGTH+1); kind=(mark == ":" ? "match" : "context"); printf "__QZ_LOG__\037%s\037%s\037%s\037%s\n", qz_path, line, kind, text }"#;
    let plain = format!(
        "{grep} \"$qz_file\" 2>/dev/null | awk -v qz_path=\"$qz_file\" {}",
        shell_quote(record_formatter)
    );
    let gzip = if supports_gzip {
        format!(
            "gzip -cd -- \"$qz_file\" 2>/dev/null | {grep} | awk -v qz_path=\"$qz_file\" {}",
            shell_quote(record_formatter)
        )
    } else {
        ":".into()
    };
    format!(
        "{discovery} | while IFS= read -r qz_file; do case \"$qz_file\" in *.gz) {gzip} ;; *) {plain} ;; esac; done; exit 0"
    )
}
