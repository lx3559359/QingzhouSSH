use std::{collections::BTreeMap, net::IpAddr};

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    core::{
        ssh::transport::{execute_authenticated, CommandOutput},
        tasks::{elevate_fixed_command, PrivilegeMode},
    },
    error::{AppError, AppResult},
    services::server_connector::ServerConnector,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRecoveryLayout {
    pub relative_dir: String,
    pub prepare_command: String,
    pub verify_command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpChangeRecoveryPlan {
    pub target_host: String,
    pub arm_command: String,
    pub apply_command: String,
    pub finalize_command: String,
    pub inspect_command: String,
    pub rollback_script_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpRecoveryState {
    Armed,
    Staged,
    Committed,
    RolledBack,
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

    pub async fn finalize_ip_change(
        &self,
        server_id: &str,
        target_host: &str,
        command: &str,
        privilege_mode: PrivilegeMode,
    ) -> AppResult<CommandOutput> {
        let connected = self
            .connector
            .connect_at_verified_ip(server_id, target_host)
            .await?;
        execute_connected(connected, command, privilege_mode).await
    }

    pub async fn inspect_ip_change_current(
        &self,
        server_id: &str,
        command: &str,
        privilege_mode: PrivilegeMode,
    ) -> AppResult<CommandOutput> {
        let connected = self.connector.connect(server_id).await?;
        execute_connected(connected, command, privilege_mode).await
    }

    pub async fn inspect_ip_change(
        &self,
        server_id: &str,
        host: &str,
        command: &str,
        privilege_mode: PrivilegeMode,
    ) -> AppResult<CommandOutput> {
        self.finalize_ip_change(server_id, host, command, privilege_mode)
            .await
    }

    pub async fn cleanup_operation_assets(&self, server_id: &str, run_id: Uuid) -> AppResult<()> {
        let connected = self.connector.connect(server_id).await?;
        let privilege_mode = match crate::core::tasks::probe_privilege(&connected.session).await {
            Ok(mode) => mode,
            Err(error) => {
                connected.session.disconnect().await;
                return Err(error);
            }
        };
        execute_connected(
            connected,
            &build_remote_recovery_cleanup_command(run_id),
            privilege_mode,
        )
        .await?;
        Ok(())
    }
}

pub fn build_remote_recovery_cleanup_command(run_id: Uuid) -> String {
    format!(
        "qz_base=${{XDG_RUNTIME_DIR:-/tmp}}; case \"$qz_base\" in /*) :;; *) exit 64;; esac; qz_parent=\"$qz_base/qingzhou-recovery\"; qz_dir=\"$qz_parent/{run_id}\"; if test ! -e \"$qz_dir\"; then exit 0; fi; test -d \"$qz_parent\" && test ! -L \"$qz_parent\" && test \"$(stat -Lc %u -- \"$qz_parent\")\" = \"$(id -u)\"; test -d \"$qz_dir\" && test ! -L \"$qz_dir\" && test \"$(stat -Lc %u -- \"$qz_dir\")\" = \"$(id -u)\"; find \"$qz_dir\" -xdev -depth -mindepth 1 -delete; rmdir -- \"$qz_dir\"; test ! -e \"$qz_dir\""
    )
}

async fn execute_connected(
    connected: crate::services::server_connector::ConnectedServer,
    command: &str,
    privilege_mode: PrivilegeMode,
) -> AppResult<CommandOutput> {
    let command = elevate_fixed_command(command, privilege_mode)?;
    let output = execute_authenticated(&connected.session, &command).await;
    let redactor = connected.redactor.clone();
    connected.session.disconnect().await;
    let output = output?;
    if output.exit_status != 0 {
        return Err(AppError::ssh_command(
            output.exit_status,
            redactor.redact(&output.stderr),
        ));
    }
    Ok(output)
}

pub fn parse_ip_recovery_state(output: &str, expected_sha256: &str) -> AppResult<IpRecoveryState> {
    validate_sha256(expected_sha256)?;
    let mut values = BTreeMap::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| AppError::Integrity("IP 自动恢复状态格式无效".into()))?;
        if !matches!(key, "state" | "scriptsha" | "committed")
            || values.insert(key.into(), value.into()).is_some()
        {
            return Err(AppError::Integrity(
                "IP 自动恢复状态包含未知或重复字段".into(),
            ));
        }
    }
    if values.len() != 3 {
        return Err(AppError::Integrity("IP 自动恢复状态字段不完整".into()));
    }
    if required(&values, "scriptsha")? != expected_sha256 {
        return Err(AppError::Integrity(
            "IP 自动恢复脚本 SHA-256 校验失败".into(),
        ));
    }
    let committed = match required(&values, "committed")? {
        "true" => true,
        "false" => false,
        _ => return Err(AppError::Integrity("IP 自动恢复提交标志无效".into())),
    };
    match (required(&values, "state")?, committed) {
        ("armed", false) => Ok(IpRecoveryState::Armed),
        ("staged", false) => Ok(IpRecoveryState::Staged),
        ("committed", true) => Ok(IpRecoveryState::Committed),
        ("rolled_back", false) => Ok(IpRecoveryState::RolledBack),
        _ => Err(AppError::Integrity(
            "IP 自动恢复状态与提交标志不一致".into(),
        )),
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

pub fn build_ip_change_recovery_plan(
    run_id: Uuid,
    implementation_id: &str,
    interface: &str,
    cidr: &str,
    gateway: &str,
    rollback_seconds: u64,
) -> AppResult<IpChangeRecoveryPlan> {
    if !is_safe_interface(interface) || !(60..=300).contains(&rollback_seconds) {
        return Err(AppError::Validation(
            "IP 修改的网卡或自动恢复等待时间无效".into(),
        ));
    }
    let (target, prefix) = parse_cidr(cidr)?;
    let gateway = gateway
        .parse::<IpAddr>()
        .map_err(|_| AppError::Validation("默认网关必须是有效 IP 地址".into()))?;
    if target.is_ipv4() != gateway.is_ipv4() {
        return Err(AppError::Validation(
            "新地址与默认网关必须使用相同的 IP 协议版本".into(),
        ));
    }
    let (backend, scheduler) = parse_ip_implementation(implementation_id)?;
    if backend == "legacy-ifcfg" && target.is_ipv6() {
        return Err(AppError::Compatibility(
            "legacy ifcfg 的受控修改目前只支持 IPv4".into(),
        ));
    }

    let family = if target.is_ipv4() { "-4" } else { "-6" };
    let target_host = target.to_string();
    let canonical_cidr = format!("{target}/{prefix}");
    let rollback_script = build_ip_rollback_script(backend, interface, family, run_id);
    let rollback_script_sha256 = format!("{:x}", Sha256::digest(rollback_script.as_bytes()));
    let encoded_script = STANDARD.encode(rollback_script.as_bytes());
    let prepare = recovery_prepare_prefix(run_id);
    let capture = build_ip_capture_command(backend, interface, family, run_id);
    let schedule = build_schedule_command(scheduler, run_id, rollback_seconds);
    let arm_command = format!(
        "{prepare}; qz_dir=\"$qz_parent/{run_id}\"; printf '%s' '{encoded_script}' | base64 -d > \"$qz_dir/rollback.sh\"; chmod 700 -- \"$qz_dir/rollback.sh\"; printf '%s\\n' '{rollback_script_sha256}' > \"$qz_dir/expected.sha256\"; chmod 600 -- \"$qz_dir/expected.sha256\"; test \"$(sha256sum -- \"$qz_dir/rollback.sh\" | awk '{{print $1}}')\" = '{rollback_script_sha256}'; {capture}; printf '%s\\n' armed > \"$qz_dir/state\"; chmod 600 -- \"$qz_dir/state\"; {schedule}; printf '%s\\n' rollback_armed"
    );

    let runtime_base = runtime_base_command();
    let apply_command = format!(
        "{runtime_base}; qz_dir=\"$qz_base/qingzhou-recovery/{run_id}\"; test ! -L \"$qz_dir\" && test \"$(stat -Lc %u -- \"$qz_dir\")\" = \"$(id -u)\"; test \"$(sed -n '1p' \"$qz_dir/state\")\" = armed; test \"$(sha256sum -- \"$qz_dir/rollback.sh\" | awk '{{print $1}}')\" = '{rollback_script_sha256}'; ip {family} -o address show dev '{interface}' scope global | awk -v cidr='{canonical_cidr}' '$4 == cidr {{ found=1 }} END {{ exit found ? 0 : 1 }}' || ip {family} address add '{canonical_cidr}' dev '{interface}'; printf '%s\\n' staged > \"$qz_dir/state\"; printf '%s\\n' network_applied"
    );

    let persist = build_ip_persist_command(
        backend,
        interface,
        &canonical_cidr,
        &gateway.to_string(),
        target.is_ipv4(),
        run_id,
    );
    let cancel = build_cancel_command(scheduler, run_id);
    let finalize_command = format!(
        "{runtime_base}; qz_dir=\"$qz_base/qingzhou-recovery/{run_id}\"; test ! -L \"$qz_dir\" && test \"$(stat -Lc %u -- \"$qz_dir\")\" = \"$(id -u)\"; test \"$(sed -n '1p' \"$qz_dir/state\")\" = staged; test \"$(sha256sum -- \"$qz_dir/rollback.sh\" | awk '{{print $1}}')\" = '{rollback_script_sha256}'; ip {family} -o address show dev '{interface}' scope global | awk -v cidr='{canonical_cidr}' '$4 == cidr {{ found=1 }} END {{ exit found ? 0 : 1 }}'; printf '%s\\n' target_connection_verified; {persist}; ip {family} address flush dev '{interface}' scope global; ip {family} address add '{canonical_cidr}' dev '{interface}'; ip {family} route replace default via '{gateway}' dev '{interface}'; ip {family} -o address show dev '{interface}' scope global | awk -v cidr='{canonical_cidr}' '$4 == cidr {{ found=1 }} END {{ exit found ? 0 : 1 }}'; ip {family} route show default dev '{interface}' | awk -v gateway='{gateway}' '$1 == \"default\" {{ for (i=1; i<=NF; i++) if ($i == \"via\" && $(i+1) == gateway) found=1 }} END {{ exit found ? 0 : 1 }}'; : > \"$qz_dir/committed\"; chmod 600 -- \"$qz_dir/committed\"; printf '%s\\n' committed > \"$qz_dir/state\"; if {cancel}; then printf '%s\\n' rollback_cancelled; else : > \"$qz_dir/cleanup_pending\"; chmod 600 -- \"$qz_dir/cleanup_pending\"; printf '%s\\n' '__QZ_WARNING__ rollback_cleanup_pending'; fi"
    );
    let inspect_command = format!(
        "{runtime_base}; qz_dir=\"$qz_base/qingzhou-recovery/{run_id}\"; test ! -L \"$qz_dir\"; printf 'state='; sed -n '1p' \"$qz_dir/state\"; printf 'scriptsha='; sha256sum -- \"$qz_dir/rollback.sh\" | awk '{{print $1}}'; printf 'committed='; if test -f \"$qz_dir/committed\"; then printf 'true\\n'; else printf 'false\\n'; fi"
    );

    Ok(IpChangeRecoveryPlan {
        target_host,
        arm_command,
        apply_command,
        finalize_command,
        inspect_command,
        rollback_script_sha256,
    })
}

fn parse_cidr(value: &str) -> AppResult<(IpAddr, u8)> {
    let (address, prefix) = value
        .split_once('/')
        .ok_or_else(|| AppError::Validation("新地址必须包含网络前缀长度".into()))?;
    let address = address
        .parse::<IpAddr>()
        .map_err(|_| AppError::Validation("新地址不是有效 IP 地址".into()))?;
    let prefix = prefix
        .parse::<u8>()
        .map_err(|_| AppError::Validation("新地址前缀长度无效".into()))?;
    let max = if address.is_ipv4() { 32 } else { 128 };
    if prefix > max {
        return Err(AppError::Validation("新地址前缀长度无效".into()));
    }
    Ok((address, prefix))
}

fn parse_ip_implementation(value: &str) -> AppResult<(&'static str, &'static str)> {
    for backend in ["network-manager", "netplan", "legacy-ifcfg"] {
        if value == format!("{backend}-systemd-run") {
            return Ok((backend, "systemd-run"));
        }
        if value == format!("{backend}-at") {
            return Ok((backend, "at"));
        }
    }
    Err(AppError::Compatibility(
        "IP 修改缺少受支持的网络后端或自动恢复调度器".into(),
    ))
}

fn is_safe_interface(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}

fn recovery_prepare_prefix(run_id: Uuid) -> String {
    format!(
        "umask 077; {}; qz_parent=\"$qz_base/qingzhou-recovery\"; test ! -L \"$qz_parent\"; if test ! -d \"$qz_parent\"; then mkdir -m 700 -- \"$qz_parent\"; fi; test \"$(stat -Lc %u -- \"$qz_parent\")\" = \"$(id -u)\"; chmod 700 -- \"$qz_parent\"; test ! -e \"$qz_parent/{run_id}\"; mkdir -m 700 -- \"$qz_parent/{run_id}\"",
        runtime_base_command()
    )
}

fn runtime_base_command() -> &'static str {
    "qz_base=${XDG_RUNTIME_DIR:-/tmp}; case \"$qz_base\" in /*) :;; *) qz_base=/tmp;; esac; if test ! -d \"$qz_base\" || test -L \"$qz_base\" || test \"$(stat -Lc %u -- \"$qz_base\" 2>/dev/null || printf invalid)\" != \"$(id -u)\"; then qz_base=/tmp; fi; test -d \"$qz_base\" && test ! -L \"$qz_base\" && test \"$(stat -Lc %u -- \"$qz_base\")\" = \"$(id -u)\""
}

fn build_ip_capture_command(backend: &str, interface: &str, family: &str, run_id: Uuid) -> String {
    let common = format!(
        "ip {family} -o address show dev '{interface}' scope global | awk '{{print $4}}' > \"$qz_dir/original-addresses\"; ip {family} route show default dev '{interface}' > \"$qz_dir/original-routes\"; chmod 600 -- \"$qz_dir/original-addresses\" \"$qz_dir/original-routes\""
    );
    let backend_capture = match backend {
        "network-manager" => format!(
            "qz_connection=$(nmcli -t -g GENERAL.CONNECTION device show '{interface}' | sed -n '1p'); test -n \"$qz_connection\" && test \"$qz_connection\" != --; printf '%s\\n' \"$qz_connection\" > \"$qz_dir/connection\"; nmcli -g ipv4.method,ipv4.addresses,ipv4.gateway,ipv6.method,ipv6.addresses,ipv6.gateway connection show \"$qz_connection\" > \"$qz_dir/backend-state\""
        ),
        "netplan" => capture_config_file("/etc/netplan/99-qingzhou.yaml", run_id),
        "legacy-ifcfg" => capture_config_file(
            &format!("/etc/sysconfig/network-scripts/ifcfg-{interface}"),
            run_id,
        ),
        _ => unreachable!(),
    };
    format!("{common}; {backend_capture}")
}

fn capture_config_file(path: &str, _run_id: Uuid) -> String {
    format!(
        "qz_config='{path}'; test ! -L \"$qz_config\"; if test -f \"$qz_config\"; then printf '%s\\n' true > \"$qz_dir/config-present\"; base64 < \"$qz_config\" > \"$qz_dir/config.b64\"; sha256sum -- \"$qz_config\" | awk '{{print $1}}' > \"$qz_dir/config.sha256\"; else printf '%s\\n' false > \"$qz_dir/config-present\"; : > \"$qz_dir/config.b64\"; printf '%s\\n' none > \"$qz_dir/config.sha256\"; fi"
    )
}

fn build_schedule_command(scheduler: &str, run_id: Uuid, seconds: u64) -> String {
    match scheduler {
        "systemd-run" => format!(
            "systemd-run --unit='qingzhou-recovery-{run_id}' --on-active={seconds}s /bin/sh \"$qz_dir/rollback.sh\" >/dev/null; systemctl list-timers --all 'qingzhou-recovery-{run_id}.timer' --no-legend | awk 'NF {{ found=1 }} END {{ exit found ? 0 : 1 }}'"
        ),
        "at" => {
            let minutes = seconds.div_ceil(60);
            format!(
                "printf '%s\\n' \"/bin/sh '$qz_dir/rollback.sh'\" > \"$qz_dir/at-job.sh\"; chmod 700 -- \"$qz_dir/at-job.sh\"; qz_at_output=$(at -f \"$qz_dir/at-job.sh\" now + {minutes} minutes 2>&1); qz_job=$(printf '%s\\n' \"$qz_at_output\" | awk '/job [0-9]+/ {{ for (i=1; i<=NF; i++) if ($i == \"job\") {{ print $(i+1); exit }} }}'); test -n \"$qz_job\"; printf '%s\\n' \"$qz_job\" > \"$qz_dir/at-job-id\"; atq | awk -v job=\"$qz_job\" '$1 == job {{ found=1 }} END {{ exit found ? 0 : 1 }}'"
            )
        }
        _ => unreachable!(),
    }
}

fn build_cancel_command(scheduler: &str, run_id: Uuid) -> String {
    match scheduler {
        "systemd-run" => format!(
            "systemctl stop 'qingzhou-recovery-{run_id}.timer' >/dev/null 2>&1 && {{ systemctl reset-failed 'qingzhou-recovery-{run_id}.service' >/dev/null 2>&1 || true; }}"
        ),
        "at" => "qz_job=$(sed -n '1p' \"$qz_dir/at-job-id\"); test -n \"$qz_job\" && atrm \"$qz_job\"".into(),
        _ => unreachable!(),
    }
}

fn build_ip_persist_command(
    backend: &str,
    interface: &str,
    cidr: &str,
    gateway: &str,
    ipv4: bool,
    run_id: Uuid,
) -> String {
    match backend {
        "network-manager" => {
            let family = if ipv4 { "ipv4" } else { "ipv6" };
            format!(
                "qz_connection=$(sed -n '1p' \"$qz_dir/connection\"); test -n \"$qz_connection\"; nmcli connection modify \"$qz_connection\" {family}.method manual {family}.addresses '{cidr}' {family}.gateway '{gateway}'; nmcli device reapply '{interface}'"
            )
        }
        "netplan" => {
            let yaml = format!(
                "network:\n  version: 2\n  ethernets:\n    {interface}:\n      addresses: [{cidr}]\n      routes:\n        - to: default\n          via: {gateway}\n"
            );
            let encoded = STANDARD.encode(yaml.as_bytes());
            format!(
                "qz_tmp='/etc/netplan/.qingzhou-{run_id}.tmp'; test ! -L /etc/netplan/99-qingzhou.yaml; printf '%s' '{encoded}' | base64 -d > \"$qz_tmp\"; chmod 600 -- \"$qz_tmp\"; mv -f -- \"$qz_tmp\" /etc/netplan/99-qingzhou.yaml; netplan generate; netplan apply"
            )
        }
        "legacy-ifcfg" => {
            let prefix = cidr.split_once('/').map(|(_, value)| value).unwrap_or("24");
            let address = cidr.split_once('/').map(|(value, _)| value).unwrap_or(cidr);
            format!(
                "qz_config='/etc/sysconfig/network-scripts/ifcfg-{interface}'; test ! -L \"$qz_config\" && test -f \"$qz_config\"; qz_tmp='/etc/sysconfig/network-scripts/.qingzhou-{run_id}.tmp'; awk '!/^(BOOTPROTO|IPADDR|PREFIX|GATEWAY)=/' \"$qz_config\" > \"$qz_tmp\"; printf 'BOOTPROTO=none\\nIPADDR=%s\\nPREFIX=%s\\nGATEWAY=%s\\n' '{address}' '{prefix}' '{gateway}' >> \"$qz_tmp\"; chmod \"$(stat -Lc %a -- \"$qz_config\")\" \"$qz_tmp\"; chown \"$(stat -Lc %u -- \"$qz_config\"):$(stat -Lc %g -- \"$qz_config\")\" \"$qz_tmp\"; mv -f -- \"$qz_tmp\" \"$qz_config\""
            )
        }
        _ => unreachable!(),
    }
}

fn build_ip_rollback_script(backend: &str, interface: &str, family: &str, run_id: Uuid) -> String {
    let restore_backend = match backend {
        "network-manager" => format!(
            "qz_connection=$(sed -n '1p' \"$qz_dir/connection\")\nqz_ipv4_method=$(sed -n '1p' \"$qz_dir/backend-state\")\nqz_ipv4_addresses=$(sed -n '2p' \"$qz_dir/backend-state\")\nqz_ipv4_gateway=$(sed -n '3p' \"$qz_dir/backend-state\")\nqz_ipv6_method=$(sed -n '4p' \"$qz_dir/backend-state\")\nqz_ipv6_addresses=$(sed -n '5p' \"$qz_dir/backend-state\")\nqz_ipv6_gateway=$(sed -n '6p' \"$qz_dir/backend-state\")\nnmcli connection modify \"$qz_connection\" ipv4.method \"$qz_ipv4_method\" ipv4.addresses \"$qz_ipv4_addresses\" ipv4.gateway \"$qz_ipv4_gateway\" ipv6.method \"$qz_ipv6_method\" ipv6.addresses \"$qz_ipv6_addresses\" ipv6.gateway \"$qz_ipv6_gateway\"\nnmcli device reapply '{interface}' || true"
        ),
        "netplan" => restore_config_script("/etc/netplan/99-qingzhou.yaml", "netplan apply"),
        "legacy-ifcfg" => restore_config_script(
            &format!("/etc/sysconfig/network-scripts/ifcfg-{interface}"),
            ":",
        ),
        _ => unreachable!(),
    };
    format!(
        "#!/bin/sh\nset -eu\nqz_dir=${{0%/*}}\ntest ! -L \"$qz_dir\"\ntest \"$(sha256sum -- \"$0\" | awk '{{print $1}}')\" = \"$(sed -n '1p' \"$qz_dir/expected.sha256\")\"\nif test -f \"$qz_dir/committed\"; then exit 0; fi\nif ! mkdir \"$qz_dir/consumed\" 2>/dev/null; then exit 0; fi\n{restore_backend}\nip {family} address flush dev '{interface}' scope global\nwhile IFS= read -r qz_address; do test -z \"$qz_address\" || ip {family} address add \"$qz_address\" dev '{interface}'; done < \"$qz_dir/original-addresses\"\nwhile ip {family} route del default dev '{interface}' >/dev/null 2>&1; do :; done\nwhile IFS= read -r qz_route; do if test -n \"$qz_route\"; then set -- $qz_route; ip {family} route add \"$@\"; fi; done < \"$qz_dir/original-routes\"\nprintf '%s\\n' rolled_back > \"$qz_dir/state\"\nchmod 600 -- \"$qz_dir/state\"\nprintf '%s\\n' 'qingzhou recovery {run_id} completed'\n"
    )
}

fn restore_config_script(path: &str, apply: &str) -> String {
    format!(
        "qz_config='{path}'\ntest ! -L \"$qz_config\"\nqz_present=$(sed -n '1p' \"$qz_dir/config-present\")\nif test \"$qz_present\" = true; then qz_tmp=\"${{qz_config}}.qingzhou-restore-$$\"; base64 -d < \"$qz_dir/config.b64\" > \"$qz_tmp\"; test \"$(sha256sum -- \"$qz_tmp\" | awk '{{print $1}}')\" = \"$(sed -n '1p' \"$qz_dir/config.sha256\")\"; chmod 600 -- \"$qz_tmp\"; mv -f -- \"$qz_tmp\" \"$qz_config\"; else rm -f -- \"$qz_config\"; fi\n{apply}"
    )
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
