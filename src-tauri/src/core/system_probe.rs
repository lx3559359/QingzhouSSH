use std::collections::HashMap;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const PROBE_COMMAND: &str = r#"printf '__QZ_OS_BEGIN__\n'; if test -r /etc/os-release; then cat /etc/os-release; fi; printf '__QZ_OS_END__\n';
printf 'UNAME_SYSTEM='; uname -s 2>/dev/null || printf 'unknown\n';
if command -v apt >/dev/null 2>&1; then echo PKG=apt; elif command -v dnf >/dev/null 2>&1; then echo PKG=dnf; elif command -v yum >/dev/null 2>&1; then echo PKG=yum; elif command -v apk >/dev/null 2>&1; then echo PKG=apk; elif command -v pkg >/dev/null 2>&1; then echo PKG=pkg; elif command -v pkg_add >/dev/null 2>&1; then echo PKG=pkg_add; else echo PKG=; fi;
if command -v systemctl >/dev/null 2>&1; then echo SERVICE=systemd; elif command -v rcctl >/dev/null 2>&1; then echo SERVICE=rcctl; elif command -v service >/dev/null 2>&1; then echo SERVICE=service; else echo SERVICE=unknown; fi;
printf 'ARCH='; uname -m; printf 'SHELL='; printf '%s\n' "${SHELL:-unknown}";
printf 'COMMANDS='; first=1; for cmd in find grep gzip awk systemctl hostnamectl service rcctl pkg pkg_info pkg_add ps head df uptime uname free ip hostname sh bash powershell pwsh sed cat tr tail sort du getent getconf nproc who ping curl openssl timeout nc ncat tcpdump wc ss netstat date last lsof iostat vmstat sysctl findmnt firewall-cmd ufw nft iptables sshd journalctl dmesg timedatectl chronyc ntpq tracepath dig docker podman nginx apachectl crontab mktemp mkdir chown chmod mv rm stat id base64 sha256sum sha256 shasum swapon swapoff mkswap nmcli netplan systemd-run at atq atrm; do if command -v "$cmd" >/dev/null 2>&1; then if [ "$first" -eq 0 ]; then printf ','; fi; printf '%s' "$cmd"; first=0; fi; done; printf '\n';
printf 'SERVICES='; if command -v systemctl >/dev/null 2>&1; then systemctl list-unit-files --type=service --no-legend --no-pager 2>/dev/null | awk 'NR <= 500 { print $1 }' | while IFS= read -r qz_name; do if [ -n "$qz_name" ]; then printf '%s,' "$qz_name"; fi; done; elif test -d /etc/init.d; then for qz_path in /etc/init.d/*; do test -f "$qz_path" || continue; qz_name=${qz_path##*/}; printf '%s,' "$qz_name"; done; elif test -d /etc/rc.d; then for qz_path in /etc/rc.d/*; do test -f "$qz_path" || continue; qz_name=${qz_path##*/}; printf '%s,' "$qz_name"; done; fi; printf '\n';
printf 'CONTAINERS='; if command -v docker >/dev/null 2>&1; then docker ps -a --format '{{.Names}}' 2>/dev/null | head -n 500 | while IFS= read -r qz_name; do if [ -n "$qz_name" ]; then printf '%s,' "$qz_name"; fi; done; elif command -v podman >/dev/null 2>&1; then podman ps -a --format '{{.Names}}' 2>/dev/null | head -n 500 | while IFS= read -r qz_name; do if [ -n "$qz_name" ]; then printf '%s,' "$qz_name"; fi; done; fi; printf '\n';
printf 'INTERFACES='; if command -v ip >/dev/null 2>&1; then ip -o link show 2>/dev/null | awk -F': ' 'NR <= 128 { name=$2; sub(/@.*/, "", name); if (name ~ /^[[:alnum:]_.:-]+$/) printf "%s,", name }'; fi; printf '\n';
printf 'ACTIVE_INTERFACES='; if command -v ip >/dev/null 2>&1; then ip -o link show up 2>/dev/null | awk -F': ' 'NR <= 128 { name=$2; sub(/@.*/, "", name); if (name ~ /^[[:alnum:]_.:-]+$/) printf "%s,", name }'; fi; printf '\n';
printf 'DEFAULT_INTERFACE='; if command -v ip >/dev/null 2>&1; then ip -4 route show default 2>/dev/null | awk 'NR == 1 { for (i=1; i<=NF; i++) if ($i == "dev" && i < NF) { print $(i+1); exit } }'; fi;
printf 'ADDRESSES='; if command -v ip >/dev/null 2>&1; then ip -o address show scope global 2>/dev/null | awk 'NR <= 512 { name=$2; address=$4; sub(/@.*/, "", name); if (name ~ /^[[:alnum:]_.:-]+$/ && address ~ /^[0-9A-Fa-f:.]+\/[0-9]+$/) printf "%s|%s;", name, address }'; fi; printf '\n';
printf 'GATEWAYS4='; if command -v ip >/dev/null 2>&1; then ip -4 route show default 2>/dev/null | awk 'NR <= 8 { dev=""; gw=""; for (i=1; i<=NF; i++) { if ($i == "dev" && i < NF) dev=$(i+1); if ($i == "via" && i < NF) gw=$(i+1) } if (dev ~ /^[[:alnum:]_.:-]+$/ && gw ~ /^[0-9.]+$/) printf "%s|%s;", dev, gw }'; fi; printf '\n';
printf 'GATEWAYS6='; if command -v ip >/dev/null 2>&1; then ip -6 route show default 2>/dev/null | awk 'NR <= 8 { dev=""; gw=""; for (i=1; i<=NF; i++) { if ($i == "dev" && i < NF) dev=$(i+1); if ($i == "via" && i < NF) gw=$(i+1) } if (dev ~ /^[[:alnum:]_.:-]+$/ && gw ~ /^[0-9A-Fa-f:]+$/) printf "%s|%s;", dev, gw }'; fi; printf '\n';
printf 'DNS_SERVERS='; if test -r /etc/resolv.conf; then awk 'NR <= 256 && $1 == "nameserver" && $2 ~ /^[0-9A-Fa-f:.]+$/ { printf "%s,", $2 }' /etc/resolv.conf; fi; printf '\n';
printf 'CURRENT_TIMEZONE='; if command -v timedatectl >/dev/null 2>&1; then timedatectl show -p Timezone --value 2>/dev/null | head -n 1; elif test -r /etc/timezone; then sed -n '1p' /etc/timezone; else printf '\n'; fi;
printf 'CURRENT_TIME='; date -Is 2>/dev/null | head -n 1 || date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null | head -n 1;
printf 'NTP_ENABLED='; if command -v timedatectl >/dev/null 2>&1; then timedatectl show -p NTP --value 2>/dev/null | head -n 1; else printf '\n'; fi;
printf 'NTP_SYNCHRONIZED='; if command -v timedatectl >/dev/null 2>&1; then timedatectl show -p NTPSynchronized --value 2>/dev/null | head -n 1; else printf '\n'; fi;
printf 'TIMEZONES='; if command -v timedatectl >/dev/null 2>&1; then timedatectl list-timezones 2>/dev/null | awk 'NR <= 600 { printf "%s,", $1 }'; fi; printf '\n'"#;

const POWERSHELL_PROBE_SCRIPT: &str = r#"$ErrorActionPreference='Stop'
$commandNames=@('powershell','pwsh','Get-Date','Get-FileHash','Get-Service','Get-Process','Get-CimInstance','Get-NetAdapter','Get-NetIPAddress')
$commands=@($commandNames | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue } | ForEach-Object { $_.ToLowerInvariant() })
$services=@(Get-Service -ErrorAction SilentlyContinue | Select-Object -First 500 -ExpandProperty Name)
$payload=[ordered]@{schemaVersion=1;osId='windows';version=[Environment]::OSVersion.Version.ToString();architecture=$env:PROCESSOR_ARCHITECTURE;shell='powershell';commands=$commands;services=$services}
Write-Output '__QZ_WINDOWS_JSON_BEGIN__'
Write-Output ($payload | ConvertTo-Json -Compress -Depth 3)
Write-Output '__QZ_WINDOWS_JSON_END__'"#;

pub fn powershell_probe_command() -> String {
    encoded_powershell_probe_command("powershell.exe")
}

pub fn pwsh_probe_command() -> String {
    encoded_powershell_probe_command("pwsh")
}

fn encoded_powershell_probe_command(executable: &str) -> String {
    let mut utf16le = Vec::with_capacity(POWERSHELL_PROBE_SCRIPT.len() * 2);
    for unit in POWERSHELL_PROBE_SCRIPT.encode_utf16() {
        utf16le.extend_from_slice(&unit.to_le_bytes());
    }
    format!(
        "{executable} -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        STANDARD.encode(utf16le)
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteOsFamily {
    Linux,
    Bsd,
    Windows,
    #[default]
    Unknown,
}

impl RemoteOsFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Bsd => "bsd",
            Self::Windows => "windows",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteShell {
    PosixSh,
    Bash,
    #[serde(rename = "powershell")]
    PowerShell,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemotePathStyle {
    #[default]
    Posix,
    WindowsSftp,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCapabilities {
    pub probe_schema_version: u32,
    pub platform_family: RemoteOsFamily,
    pub remote_shell: RemoteShell,
    pub path_style: RemotePathStyle,
    pub os_id: String,
    pub os_family: String,
    pub version_id: Option<String>,
    pub package_manager: Option<String>,
    pub service_manager: String,
    pub architecture: String,
    pub shell: String,
    pub commands: Vec<String>,
    pub services: Vec<String>,
    pub containers: Vec<String>,
    pub interfaces: Vec<NetworkInterfaceCapability>,
    pub dns_servers: Vec<String>,
    pub current_timezone: Option<String>,
    pub current_time: Option<String>,
    pub ntp_enabled: Option<bool>,
    pub ntp_synchronized: Option<bool>,
    pub timezones: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterfaceCapability {
    pub name: String,
    pub is_up: bool,
    pub is_default: bool,
    pub addresses: Vec<String>,
    pub gateway4: Option<String>,
    pub gateway6: Option<String>,
}

impl SystemCapabilities {
    pub fn has_command(&self, command: &str) -> bool {
        self.commands
            .iter()
            .any(|available| available.eq_ignore_ascii_case(command))
            || (command == "systemctl" && self.service_manager == "systemd")
            || (command == "service" && self.service_manager == "service")
    }

    pub fn has_service(&self, service: &str) -> bool {
        self.services
            .iter()
            .any(|available| available.eq_ignore_ascii_case(service))
    }

    pub fn has_container(&self, container: &str) -> bool {
        self.containers
            .iter()
            .any(|available| available == container)
    }

    pub fn interface(&self, name: &str) -> Option<&NetworkInterfaceCapability> {
        self.interfaces.iter().find(|item| item.name == name)
    }

    pub fn has_interface(&self, name: &str) -> bool {
        self.interface(name).is_some()
    }

    pub fn default_interface(&self) -> Option<&NetworkInterfaceCapability> {
        self.interfaces
            .iter()
            .find(|item| item.is_default)
            .or_else(|| {
                self.interfaces
                    .iter()
                    .find(|item| item.is_up && item.name != "lo")
            })
            .or_else(|| self.interfaces.iter().find(|item| item.name != "lo"))
    }
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
    let uname_system = capabilities
        .remove("UNAME_SYSTEM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let os_id = os_values
        .remove("ID")
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| bsd_id_from_uname(&uname_system).map(str::to_string))
        .ok_or_else(|| AppError::Validation("无法识别系统 ID".into()))?;
    let id_like = os_values
        .remove("ID_LIKE")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id_like = id_like.split_whitespace().collect::<Vec<_>>();

    let os_family = if ["freebsd", "openbsd", "netbsd", "dragonfly"].contains(&os_id.as_str()) {
        "bsd"
    } else if os_id == "openeuler" {
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

    let commands = take_csv(&mut capabilities, "COMMANDS");
    let services = take_csv(&mut capabilities, "SERVICES");
    let containers = take_csv(&mut capabilities, "CONTAINERS");
    let interface_names = take_csv(&mut capabilities, "INTERFACES");
    let active_interfaces = take_csv(&mut capabilities, "ACTIVE_INTERFACES");
    let default_interface = capabilities
        .remove("DEFAULT_INTERFACE")
        .filter(|value| is_interface_name(value));
    let addresses = take_interface_records(&mut capabilities, "ADDRESSES");
    let gateways4 = take_interface_records(&mut capabilities, "GATEWAYS4");
    let gateways6 = take_interface_records(&mut capabilities, "GATEWAYS6");
    let dns_servers = take_csv(&mut capabilities, "DNS_SERVERS");
    let current_timezone = capabilities
        .remove("CURRENT_TIMEZONE")
        .filter(|value| is_timezone_name(value));
    let current_time = capabilities
        .remove("CURRENT_TIME")
        .filter(|value| !value.is_empty() && value.len() <= 64);
    let ntp_enabled = take_bool(&mut capabilities, "NTP_ENABLED");
    let ntp_synchronized = take_bool(&mut capabilities, "NTP_SYNCHRONIZED");
    let mut timezones = take_csv(&mut capabilities, "TIMEZONES")
        .into_iter()
        .filter(|value| is_timezone_name(value))
        .collect::<Vec<_>>();
    if let Some(current) = &current_timezone {
        if !timezones.contains(current) {
            timezones.push(current.clone());
            timezones.sort();
        }
    }
    let interfaces = interface_names
        .into_iter()
        .filter(|name| is_interface_name(name))
        .map(|name| NetworkInterfaceCapability {
            is_up: active_interfaces.contains(&name),
            is_default: default_interface.as_deref() == Some(name.as_str()),
            addresses: interface_values(&addresses, &name),
            gateway4: interface_value(&gateways4, &name),
            gateway6: interface_value(&gateways6, &name),
            name,
        })
        .collect();

    let platform_family = if os_family == "bsd" {
        RemoteOsFamily::Bsd
    } else if uname_system == "linux"
        || ["debian", "rhel", "openeuler"].contains(&os_family.as_str())
    {
        RemoteOsFamily::Linux
    } else {
        RemoteOsFamily::Unknown
    };
    let shell = capabilities
        .remove("SHELL")
        .unwrap_or_else(|| "unknown".into());
    let remote_shell = if shell.to_ascii_lowercase().contains("bash") {
        RemoteShell::Bash
    } else if commands.iter().any(|command| command == "sh") {
        RemoteShell::PosixSh
    } else {
        RemoteShell::Unknown
    };

    Ok(SystemCapabilities {
        probe_schema_version: 1,
        platform_family,
        remote_shell,
        path_style: RemotePathStyle::Posix,
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
        shell,
        commands,
        services,
        containers,
        interfaces,
        dns_servers,
        current_timezone,
        current_time,
        ntp_enabled,
        ntp_synchronized,
        timezones,
    })
}

fn bsd_id_from_uname(uname_system: &str) -> Option<&'static str> {
    match uname_system {
        "freebsd" => Some("freebsd"),
        "openbsd" => Some("openbsd"),
        "netbsd" => Some("netbsd"),
        "dragonfly" => Some("dragonfly"),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowsProbePayload {
    schema_version: u32,
    os_id: String,
    version: String,
    architecture: String,
    shell: String,
    commands: Vec<String>,
    services: Vec<String>,
}

pub fn parse_powershell_probe(output: &str) -> AppResult<SystemCapabilities> {
    const BEGIN: &str = "__QZ_WINDOWS_JSON_BEGIN__";
    const END: &str = "__QZ_WINDOWS_JSON_END__";
    let start = output
        .find(BEGIN)
        .map(|index| index + BEGIN.len())
        .ok_or_else(|| AppError::Validation("Windows 探测输出缺少开始标记".into()))?;
    let after_start = &output[start..];
    let end = after_start
        .find(END)
        .ok_or_else(|| AppError::Validation("Windows 探测输出缺少结束标记".into()))?;
    if after_start[end + END.len()..].contains(END) {
        return Err(AppError::Validation(
            "Windows 探测输出包含重复结束标记".into(),
        ));
    }
    let payload: WindowsProbePayload = serde_json::from_str(after_start[..end].trim())
        .map_err(|error| AppError::Validation(format!("Windows 探测 JSON 无效：{error}")))?;
    if payload.schema_version != 1
        || !payload.os_id.eq_ignore_ascii_case("windows")
        || payload.version.is_empty()
        || payload.version.len() > 64
        || payload.architecture.is_empty()
        || payload.architecture.len() > 32
        || !payload.shell.eq_ignore_ascii_case("powershell")
        || payload.commands.len() > 256
        || payload.services.len() > 500
    {
        return Err(AppError::Validation(
            "Windows 探测结果超出受支持边界".into(),
        ));
    }
    let mut commands = bounded_names(payload.commands, 64)?;
    commands
        .iter_mut()
        .for_each(|value| value.make_ascii_lowercase());
    commands.sort();
    commands.dedup();
    let mut services = bounded_names(payload.services, 256)?;
    services.sort_by_key(|value| value.to_ascii_lowercase());
    services.dedup_by(|left, right| left.eq_ignore_ascii_case(right));

    Ok(SystemCapabilities {
        probe_schema_version: payload.schema_version,
        platform_family: RemoteOsFamily::Windows,
        remote_shell: RemoteShell::PowerShell,
        path_style: RemotePathStyle::WindowsSftp,
        os_id: "windows".into(),
        os_family: "windows".into(),
        version_id: Some(payload.version),
        package_manager: None,
        service_manager: "windows_service_control_manager".into(),
        architecture: payload.architecture.to_ascii_lowercase(),
        shell: "powershell".into(),
        commands,
        services,
        containers: Vec::new(),
        interfaces: Vec::new(),
        dns_servers: Vec::new(),
        current_timezone: None,
        current_time: None,
        ntp_enabled: None,
        ntp_synchronized: None,
        timezones: Vec::new(),
    })
}

fn bounded_names(values: Vec<String>, max_len: usize) -> AppResult<Vec<String>> {
    if values.iter().any(|value| {
        value.is_empty()
            || value.len() > max_len
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'$')
            })
    }) {
        return Err(AppError::Validation("Windows 探测名称字段无效".into()));
    }
    Ok(values)
}

fn take_interface_records(
    values: &mut HashMap<String, String>,
    key: &str,
) -> Vec<(String, String)> {
    values
        .remove(key)
        .unwrap_or_default()
        .split(';')
        .filter_map(|record| record.split_once('|'))
        .filter(|(name, value)| {
            is_interface_name(name)
                && !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b'.' | b':' | b'/'))
        })
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .take(512)
        .collect()
}

fn interface_values(records: &[(String, String)], name: &str) -> Vec<String> {
    let mut values = records
        .iter()
        .filter(|(record_name, _)| record_name == name)
        .map(|(_, value)| value.clone())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn interface_value(records: &[(String, String)], name: &str) -> Option<String> {
    records
        .iter()
        .find(|(record_name, _)| record_name == name)
        .map(|(_, value)| value.clone())
}

fn is_interface_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}

fn is_timezone_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'+'))
}

fn take_csv(values: &mut HashMap<String, String>, key: &str) -> Vec<String> {
    let mut parsed = values
        .remove(key)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    parsed.sort();
    parsed.dedup();
    parsed
}

fn take_bool(values: &mut HashMap<String, String>, key: &str) -> Option<bool> {
    match values.remove(key)?.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" | "enabled" | "active" => Some(true),
        "no" | "false" | "0" | "disabled" | "inactive" => Some(false),
        _ => None,
    }
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
    fn parses_bounded_service_and_container_discovery() {
        let output = "__QZ_OS_BEGIN__\nID=openeuler\nVERSION_ID=24.03\n__QZ_OS_END__\nPKG=dnf\nSERVICE=systemd\nARCH=x86_64\nSHELL=/bin/sh\nCOMMANDS=systemctl,docker\nSERVICES=sshd.service,nginx.service,sshd.service,\nCONTAINERS=web,database,web,\n";
        let result = parse_probe(output).unwrap();
        assert_eq!(result.services, ["nginx.service", "sshd.service"]);
        assert_eq!(result.containers, ["database", "web"]);
    }

    #[test]
    fn parses_network_time_and_dns_choices_for_dynamic_task_parameters() {
        let output = "__QZ_OS_BEGIN__\nID=openeuler\nVERSION_ID=24.03\n__QZ_OS_END__\nPKG=dnf\nSERVICE=systemd\nARCH=x86_64\nSHELL=/bin/sh\nCOMMANDS=ip,timedatectl\nSERVICES=sshd.service,nginx.service,\nCONTAINERS=web,\nINTERFACES=eth0,lo,\nDEFAULT_INTERFACE=eth0\nADDRESSES=eth0|192.0.2.10/24;eth0|2001:db8::10/64;lo|127.0.0.1/8;\nGATEWAYS4=eth0|192.0.2.1;\nGATEWAYS6=eth0|2001:db8::1;\nDNS_SERVERS=223.5.5.5,1.1.1.1,\nCURRENT_TIMEZONE=Asia/Shanghai\nCURRENT_TIME=2026-08-07T00:20:00+08:00\nNTP_ENABLED=yes\nNTP_SYNCHRONIZED=no\nTIMEZONES=Asia/Shanghai,Asia/Hong_Kong,UTC,\n";

        let result = parse_probe(output).unwrap();

        assert_eq!(
            result.default_interface().map(|item| item.name.as_str()),
            Some("eth0")
        );
        let eth0 = result.interface("eth0").expect("eth0 discovery");
        assert_eq!(eth0.addresses, ["192.0.2.10/24", "2001:db8::10/64"]);
        assert_eq!(eth0.gateway4.as_deref(), Some("192.0.2.1"));
        assert_eq!(eth0.gateway6.as_deref(), Some("2001:db8::1"));
        assert_eq!(result.dns_servers, ["1.1.1.1", "223.5.5.5"]);
        assert_eq!(result.current_timezone.as_deref(), Some("Asia/Shanghai"));
        assert_eq!(result.ntp_enabled, Some(true));
        assert_eq!(result.ntp_synchronized, Some(false));
        assert!(result.timezones.contains(&"UTC".to_string()));
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

    #[test]
    fn classifies_bsd_from_uname_without_linux_release_files() {
        let output = "__QZ_OS_BEGIN__\n__QZ_OS_END__\nUNAME_SYSTEM=FreeBSD\nPKG=pkg\nSERVICE=service\nARCH=amd64\nSHELL=/bin/sh\nCOMMANDS=sh,service,sha256,sysctl\nSERVICES=sshd\n";

        let result = parse_probe(output).unwrap();

        assert_eq!(result.os_id, "freebsd");
        assert_eq!(result.os_family, "bsd");
        assert_eq!(result.platform_family, RemoteOsFamily::Bsd);
        assert_eq!(result.remote_shell, RemoteShell::PosixSh);
        assert_eq!(result.path_style, RemotePathStyle::Posix);
        assert_eq!(result.package_manager.as_deref(), Some("pkg"));
    }

    #[test]
    fn parses_versioned_windows_probe_json_without_localized_text() {
        let output = r#"noise
__QZ_WINDOWS_JSON_BEGIN__
{"schemaVersion":1,"osId":"windows","version":"10.0.20348.0","architecture":"X64","shell":"powershell","commands":["Get-FileHash","powershell"],"services":["sshd","WinRM"]}
__QZ_WINDOWS_JSON_END__
"#;

        let result = parse_powershell_probe(output).unwrap();

        assert_eq!(result.os_family, "windows");
        assert_eq!(result.platform_family, RemoteOsFamily::Windows);
        assert_eq!(result.remote_shell, RemoteShell::PowerShell);
        assert_eq!(result.path_style, RemotePathStyle::WindowsSftp);
        assert_eq!(result.probe_schema_version, 1);
        assert!(result.has_command("get-filehash"));
        assert!(result.has_service("sshd"));
    }

    #[test]
    fn windows_probe_launcher_is_fixed_and_non_interactive() {
        let command = powershell_probe_command();
        assert!(command.starts_with("powershell.exe -NoLogo -NoProfile -NonInteractive"));
        assert!(command.contains("-EncodedCommand"));
        assert!(!command.contains("__QZ_WINDOWS_JSON_BEGIN__"));
        assert!(pwsh_probe_command().starts_with("pwsh -NoLogo -NoProfile -NonInteractive"));
    }
}
