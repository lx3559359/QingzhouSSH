use crate::{
    core::{logs::request::LogSearchRequest, system_probe::SystemCapabilities, tasks::shell_quote},
    error::{AppError, AppResult},
};

pub fn build_search_command(
    request: &LogSearchRequest,
    capabilities: &SystemCapabilities,
) -> AppResult<String> {
    request.validate()?;
    for command in ["grep", "awk"] {
        if !capabilities.has_command(command) {
            return Err(AppError::Compatibility(format!(
                "日志检索需要远端命令 {command}"
            )));
        }
    }
    if request.is_gzip() && !capabilities.has_command("gzip") {
        return Err(AppError::Compatibility(
            "检索 .gz 日志需要远端命令 gzip".into(),
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
