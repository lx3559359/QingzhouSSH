use std::collections::HashMap;

use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const PROBE_COMMAND: &str = r#"printf '__QZ_OS_BEGIN__\n'; cat /etc/os-release; printf '__QZ_OS_END__\n'; if command -v apt >/dev/null 2>&1; then echo PKG=apt; elif command -v dnf >/dev/null 2>&1; then echo PKG=dnf; elif command -v yum >/dev/null 2>&1; then echo PKG=yum; else echo PKG=; fi; if command -v systemctl >/dev/null 2>&1; then echo SERVICE=systemd; else echo SERVICE=service; fi; printf 'ARCH='; uname -m; printf 'SHELL='; printf '%s\n' "${SHELL:-unknown}""#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCapabilities {
    pub os_id: String,
    pub os_family: String,
    pub version_id: Option<String>,
    pub package_manager: Option<String>,
    pub service_manager: String,
    pub architecture: String,
    pub shell: String,
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}

fn key_values(block: &str) -> HashMap<String, String> {
    block
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), unquote(value)))
        .collect()
}

pub fn parse_probe(output: &str) -> AppResult<SystemCapabilities> {
    const BEGIN: &str = "__QZ_OS_BEGIN__";
    const END: &str = "__QZ_OS_END__";

    let os_start = output
        .find(BEGIN)
        .map(|index| index + BEGIN.len())
        .ok_or_else(|| AppError::Validation("探测输出缺少开始标记".into()))?;
    let after_start = &output[os_start..];
    let relative_end = after_start
        .find(END)
        .ok_or_else(|| AppError::Validation("探测输出缺少结束标记".into()))?;
    let os_block = &after_start[..relative_end];
    let capability_block = &after_start[relative_end + END.len()..];

    let mut os_values = key_values(os_block);
    let mut capabilities = key_values(capability_block);
    let os_id = os_values
        .remove("ID")
        .ok_or_else(|| AppError::Validation("无法识别系统 ID".into()))?
        .to_ascii_lowercase();
    let id_like = os_values
        .remove("ID_LIKE")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id_like = id_like.split_whitespace().collect::<Vec<_>>();

    let os_family = if os_id == "openeuler" {
        "openeuler"
    } else if ["debian", "ubuntu", "kylin", "uos"].contains(&os_id.as_str())
        || id_like.contains(&"debian")
    {
        "debian"
    } else if ["rhel", "centos", "rocky", "almalinux", "anolis"].contains(&os_id.as_str())
        || id_like.contains(&"rhel")
        || id_like.contains(&"fedora")
    {
        "rhel"
    } else {
        "unknown"
    }
    .to_string();

    Ok(SystemCapabilities {
        os_id,
        os_family,
        version_id: os_values.remove("VERSION_ID"),
        package_manager: capabilities.remove("PKG").filter(|value| !value.is_empty()),
        service_manager: capabilities
            .remove("SERVICE")
            .unwrap_or_else(|| "unknown".into()),
        architecture: capabilities
            .remove("ARCH")
            .unwrap_or_else(|| "unknown".into()),
        shell: capabilities
            .remove("SHELL")
            .unwrap_or_else(|| "unknown".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ubuntu_capabilities() {
        let result = parse_probe(include_str!("../../tests/fixtures/ubuntu_probe.txt")).unwrap();
        assert_eq!(result.os_id, "ubuntu");
        assert_eq!(result.os_family, "debian");
        assert_eq!(result.package_manager.as_deref(), Some("apt"));
        assert_eq!(result.service_manager, "systemd");
        assert_eq!(result.architecture, "x86_64");
    }

    #[test]
    fn maps_kylin_by_id_like_and_detected_tools() {
        let result = parse_probe(include_str!("../../tests/fixtures/kylin_probe.txt")).unwrap();
        assert_eq!(result.os_id, "kylin");
        assert_eq!(result.os_family, "debian");
        assert_eq!(result.package_manager.as_deref(), Some("apt"));
    }

    #[test]
    fn maps_rhel_openeuler_and_domestic_variants() {
        let cases = [
            (
                include_str!("../../tests/fixtures/rocky_probe.txt"),
                "rocky",
                "rhel",
                "dnf",
            ),
            (
                include_str!("../../tests/fixtures/openeuler_probe.txt"),
                "openeuler",
                "openeuler",
                "dnf",
            ),
            (
                include_str!("../../tests/fixtures/anolis_probe.txt"),
                "anolis",
                "rhel",
                "dnf",
            ),
            (
                include_str!("../../tests/fixtures/uos_probe.txt"),
                "uos",
                "debian",
                "apt",
            ),
        ];
        for (fixture, id, family, package_manager) in cases {
            let result = parse_probe(fixture).unwrap();
            assert_eq!(result.os_id, id);
            assert_eq!(result.os_family, family);
            assert_eq!(result.package_manager.as_deref(), Some(package_manager));
        }
    }

    #[test]
    fn ignores_identity_fields_outside_the_os_sentinels() {
        let output =
            "__QZ_OS_BEGIN__\nID=ubuntu\nID_LIKE=debian\n__QZ_OS_END__\nID=attacker\nPKG=apt\n";
        let result = parse_probe(output).unwrap();
        assert_eq!(result.os_id, "ubuntu");
        assert_eq!(result.os_family, "debian");
    }

    #[test]
    fn rejects_reversed_or_missing_sentinels_without_panicking() {
        assert!(parse_probe("__QZ_OS_END__\n__QZ_OS_BEGIN__\nID=ubuntu").is_err());
        assert!(parse_probe("ID=ubuntu").is_err());
    }
}
