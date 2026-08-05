use std::collections::BTreeMap;

use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    services::server_connector::ServerConnector,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRecoveryLayout {
    pub relative_dir: String,
    pub prepare_command: String,
    pub verify_command: String,
}

#[derive(Clone)]
pub struct RemoteRecoveryService {
    connector: ServerConnector,
}

impl RemoteRecoveryService {
    pub fn new(connector: ServerConnector) -> Self {
        Self { connector }
    }

    pub fn connector(&self) -> &ServerConnector {
        &self.connector
    }
}

pub fn build_remote_recovery_layout(
    run_id: Uuid,
    expected_sha256: &str,
) -> AppResult<RemoteRecoveryLayout> {
    validate_sha256(expected_sha256)?;
    let relative_dir = format!("qingzhou-recovery/{run_id}");
    let prepare_command = format!(
        "umask 077; qz_base=${{XDG_RUNTIME_DIR:-/tmp}}; case \"$qz_base\" in /*) :;; *) exit 64;; esac; test ! -L \"$qz_base\"; if test ! -d \"$qz_base\"; then mkdir -m 700 -- \"$qz_base\"; fi; test \"$(stat -Lc %u -- \"$qz_base\")\" = \"$(id -u)\"; qz_parent=\"$qz_base/qingzhou-recovery\"; test ! -L \"$qz_parent\"; if test ! -d \"$qz_parent\"; then mkdir -m 700 -- \"$qz_parent\"; fi; chmod 700 -- \"$qz_parent\"; test \"$(stat -Lc %u -- \"$qz_parent\")\" = \"$(id -u)\"; qz_dir=\"$qz_parent/{run_id}\"; test ! -e \"$qz_dir\"; mkdir -m 700 -- \"$qz_dir\"; printf '%s\\n' '{expected_sha256}' > \"$qz_dir/expected.sha256\"; chmod 600 -- \"$qz_dir/expected.sha256\"; : > \"$qz_dir/rollback.sh\"; chmod 700 -- \"$qz_dir/rollback.sh\""
    );
    let verify_command = format!(
        "qz_base=${{XDG_RUNTIME_DIR:-/tmp}}; qz_dir=\"$qz_base/qingzhou-recovery/{run_id}\"; test ! -L \"$qz_dir\"; test \"$(stat -Lc %u -- \"$qz_dir\")\" = \"$(id -u)\"; test \"$(stat -Lc %a -- \"$qz_dir\")\" = 700; test \"$(stat -Lc %a -- \"$qz_dir/expected.sha256\")\" = 600; test \"$(stat -Lc %a -- \"$qz_dir/rollback.sh\")\" = 700; test \"$(sed -n '1p' \"$qz_dir/expected.sha256\")\" = '{expected_sha256}'"
    );
    Ok(RemoteRecoveryLayout {
        relative_dir,
        prepare_command,
        verify_command,
    })
}

pub fn validate_remote_recovery_observation(
    run_id: Uuid,
    current_uid: u32,
    expected_sha256: &str,
    observation: &str,
) -> AppResult<()> {
    validate_sha256(expected_sha256)?;
    let values = parse_observation(observation)?;
    let path = required(&values, "path")?;
    if !is_confined_recovery_path(path, run_id) {
        return Err(AppError::Security(
            "远程恢复目录超出当前运行的私有临时目录".into(),
        ));
    }
    if parse_u32(&values, "uid")? != current_uid || parse_u32(&values, "diruid")? != current_uid {
        return Err(AppError::Security("远程恢复目录所有者不匹配".into()));
    }
    for (key, expected) in [
        ("dirmode", "700"),
        ("filemode", "600"),
        ("scriptmode", "700"),
    ] {
        if required(&values, key)? != expected {
            return Err(AppError::Security(format!("远程恢复目录权限不安全：{key}")));
        }
    }
    if required(&values, "sha256")? != expected_sha256 {
        return Err(AppError::Integrity("远程恢复脚本 SHA-256 校验失败".into()));
    }
    Ok(())
}

fn parse_observation(observation: &str) -> AppResult<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for line in observation.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| AppError::Integrity("远程恢复目录观测格式无效".into()))?;
        if !matches!(
            key,
            "path" | "uid" | "diruid" | "dirmode" | "filemode" | "scriptmode" | "sha256"
        ) || values.insert(key.into(), value.into()).is_some()
        {
            return Err(AppError::Integrity(
                "远程恢复目录观测包含未知或重复字段".into(),
            ));
        }
    }
    if values.len() != 7 {
        return Err(AppError::Integrity("远程恢复目录观测字段不完整".into()));
    }
    Ok(values)
}

fn required<'a>(values: &'a BTreeMap<String, String>, key: &str) -> AppResult<&'a str> {
    values
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| AppError::Integrity(format!("远程恢复目录观测缺少字段：{key}")))
}

fn parse_u32(values: &BTreeMap<String, String>, key: &str) -> AppResult<u32> {
    required(values, key)?
        .parse::<u32>()
        .map_err(|_| AppError::Integrity(format!("远程恢复目录观测字段无效：{key}")))
}

fn validate_sha256(value: &str) -> AppResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(AppError::Integrity("远程恢复脚本 SHA-256 无效".into()));
    }
    Ok(())
}

fn is_confined_recovery_path(path: &str, run_id: Uuid) -> bool {
    path.starts_with('/')
        && !path.contains('\\')
        && !path.contains("//")
        && path
            .split('/')
            .skip(1)
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
        && path.ends_with(&format!("/qingzhou-recovery/{run_id}"))
}
