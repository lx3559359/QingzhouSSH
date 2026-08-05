use crate::core::tasks::model::{
    BackupItemKind, OutputKind, ParameterKind, ResultParserKind, TaskCategory, TaskDefinition,
    TaskImplementation,
};

use super::helpers::{
    absolute_path_parameter, backup_item, bounded_step, dangerous_implementation, dangerous_task,
    enum_parameter, parameter, port_parameter, read_only_implementation, read_only_task,
};
use serde_json::json;

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![
        diagnostic(
            "security.ssh_events",
            "SSH 安全事件",
            "查看 SSH 配置摘要、近期登录事件和 UID 0 账号",
            60,
            60,
            r#"printf '== sshd_config ==\n'; if command -v sshd >/dev/null 2>&1; then sshd -T 2>/dev/null | grep -E '^(port|listenaddress|permitrootlogin|passwordauthentication|pubkeyauthentication|maxauthtries|allowusers|allowgroups|denyusers|denygroups) ' | head -n 100 || true; fi; printf '== ssh_events ==\n'; if command -v journalctl >/dev/null 2>&1; then journalctl --since '24 hours ago' --no-pager -n 300 -u ssh -u sshd 2>/dev/null || true; elif test -r /var/log/auth.log; then tail -n 300 /var/log/auth.log; elif test -r /var/log/secure; then tail -n 300 /var/log/secure; else printf '%s\n' '__QZ_UNSUPPORTED__ ssh_log'; fi; printf '== logins ==\n'; if command -v last >/dev/null 2>&1; then last -n 100; fi; if command -v lastb >/dev/null 2>&1; then lastb -n 100 2>/dev/null || true; fi; printf '== uid0 ==\n'; awk -F: '$3 == 0 { print $1 ":" $6 ":" $7 }' /etc/passwd | head -n 50"#,
            &["grep", "head", "awk"],
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "security.firewall_exposure",
            "防火墙与暴露面",
            "查看监听端口和现有防火墙只读规则摘要",
            60,
            60,
            r#"printf '== listeners ==\n'; if command -v ss >/dev/null 2>&1; then ss -lntup 2>/dev/null | head -n 300; elif command -v netstat >/dev/null 2>&1; then netstat -lntup 2>/dev/null | head -n 300; fi; printf '== firewalld ==\n'; if command -v firewall-cmd >/dev/null 2>&1; then firewall-cmd --list-all-zones 2>/dev/null | head -n 300 || true; fi; printf '== ufw ==\n'; if command -v ufw >/dev/null 2>&1; then ufw status verbose 2>/dev/null | head -n 300 || true; fi; printf '== nftables ==\n'; if command -v nft >/dev/null 2>&1; then nft list ruleset 2>/dev/null | head -n 300 || true; fi; printf '== iptables ==\n'; if command -v iptables >/dev/null 2>&1; then iptables -S 2>/dev/null | head -n 300 || true; fi"#,
            &["head"],
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        file_permissions(),
        firewall_open_port(),
    ]
}

fn file_permissions() -> TaskDefinition {
    dangerous_task(
        "security.file_permissions",
        TaskCategory::Security,
        "修改文件权限",
        "只修改单一路径的所有者和权限，不递归，并保存原始元数据",
        45,
        vec![
            absolute_path_parameter("path", "目标路径", "要修改的单个普通文件或目录"),
            parameter(
                "mode",
                "权限模式",
                "三位或四位八进制权限",
                ParameterKind::FileMode,
                true,
                None,
            ),
            parameter(
                "uid",
                "用户 UID",
                "目标数字 UID",
                ParameterKind::Integer { min: 0, max: 60000 },
                false,
                Some(json!(0)),
            ),
            parameter(
                "gid",
                "用户组 GID",
                "目标数字 GID",
                ParameterKind::Integer { min: 0, max: 60000 },
                false,
                Some(json!(0)),
            ),
        ],
        vec![dangerous_implementation(
            "posix-stat",
            &[],
            &["stat", "chmod", "chown"],
            "test ! -L {{path}}; stat -Lc '%n %u %g %a %F' -- {{path}}",
            vec![backup_item(
                "metadata-before",
                BackupItemKind::RuntimeState,
                r#"test ! -L {{path}} && stat -Lc 'path=%n\nuid=%u\ngid=%g\nmode=%a' -- {{path}}"#,
            )],
            "test ! -L {{path}} && chown -- {{uid}}:{{gid}} {{path}} && chmod -- {{mode}} {{path}}",
            "test ! -L {{path}}; test \"$(stat -Lc '%u:%g:%a' -- {{path}})\" = {{uid}}:{{gid}}:{{mode}}",
            "{{restore:metadata:metadata-before}}",
            ResultParserKind::KeyValue,
        )],
        OutputKind::KeyValue,
    )
}

fn firewall_open_port() -> TaskDefinition {
    dangerous_task(
        "security.firewall_open_port",
        TaskCategory::Security,
        "管理防火墙端口",
        "只添加或移除一条受控 TCP/UDP 端口规则，不关闭防火墙或修改默认策略",
        90,
        vec![
            enum_parameter("action", "操作", "添加或移除规则", &["add", "remove"], None),
            parameter(
                "entryId",
                "规则标识",
                "客户端自动生成，只允许修改同一标识的工具规则",
                ParameterKind::ManagedId,
                true,
                None,
            ),
            port_parameter("port", "端口", "要管理的单个端口", true, None),
            enum_parameter(
                "protocol",
                "协议",
                "TCP 或 UDP",
                &["tcp", "udp"],
                Some("tcp"),
            ),
        ],
        vec![
            firewall_implementation(
                "firewalld",
                &["firewall-cmd", "grep"],
                firewalld_snapshot(),
                firewalld_snapshot(),
                firewalld_action(),
                firewalld_verify(),
            ),
            firewall_implementation(
                "ufw",
                &["ufw", "awk", "sed"],
                ufw_snapshot(),
                ufw_snapshot(),
                ufw_action(),
                ufw_verify(),
            ),
            firewall_implementation(
                "nftables",
                &["nft", "awk"],
                nft_snapshot(),
                nft_snapshot(),
                nft_action(),
                nft_verify(),
            ),
            firewall_implementation(
                "iptables",
                &["iptables"],
                iptables_snapshot(),
                iptables_snapshot(),
                iptables_action(),
                iptables_verify(),
            ),
        ],
        OutputKind::KeyValue,
    )
}

fn firewalld_snapshot() -> &'static str {
    r#"qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rule="0 -p $qz_protocol --dport $qz_port -m comment --comment $qz_marker -j ACCEPT"; qz_present=false; qz_owned=$(firewall-cmd --permanent --direct --get-rules ipv4 filter INPUT | grep --fixed-strings -- "$qz_marker" || true); if test -n "$qz_owned"; then test "$qz_owned" = "$qz_rule" || exit 65; qz_present=true; fi; printf 'backend=firewalld\nentryid=%s\nport=%s\nprotocol=%s\npresent=%s\n' "$qz_id" "$qz_port" "$qz_protocol" "$qz_present""#
}

fn firewalld_action() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rule="0 -p $qz_protocol --dport $qz_port -m comment --comment $qz_marker -j ACCEPT"; qz_owned=$(firewall-cmd --permanent --direct --get-rules ipv4 filter INPUT | grep --fixed-strings -- "$qz_marker" || true); if test -n "$qz_owned"; then test "$qz_owned" = "$qz_rule" || exit 65; fi; case "$qz_action" in add) if test -z "$qz_owned"; then firewall-cmd --permanent --direct --add-rule ipv4 filter INPUT 0 -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT || exit; fi;; remove) if test -n "$qz_owned"; then firewall-cmd --permanent --direct --remove-rule ipv4 filter INPUT 0 -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT || exit; fi;; *) exit 64;; esac; firewall-cmd --reload"#
}

fn firewalld_verify() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rule="0 -p $qz_protocol --dport $qz_port -m comment --comment $qz_marker -j ACCEPT"; qz_owned=$(firewall-cmd --permanent --direct --get-rules ipv4 filter INPUT | grep --fixed-strings -- "$qz_marker" || true); case "$qz_action" in add) test "$qz_owned" = "$qz_rule";; remove) test -z "$qz_owned";; *) exit 64;; esac"#
}

fn ufw_snapshot() -> &'static str {
    r#"qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_present=false; qz_rows=$(ufw status numbered | awk -v marker="$qz_marker" 'index($0, marker) { print }'); if test -n "$qz_rows"; then test "$(printf '%s\n' "$qz_rows" | awk 'NF { count++ } END { print count + 0 }')" -eq 1; printf '%s\n' "$qz_rows" | awk -v port="$qz_port/$qz_protocol" 'index($0, port) { found=1 } END { exit !found }'; qz_present=true; fi; printf 'backend=ufw\nentryid=%s\nport=%s\nprotocol=%s\npresent=%s\n' "$qz_id" "$qz_port" "$qz_protocol" "$qz_present""#
}

fn ufw_action() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rows=$(ufw status numbered | awk -v marker="$qz_marker" 'index($0, marker) { print }'); if test -n "$qz_rows"; then test "$(printf '%s\n' "$qz_rows" | awk 'NF { count++ } END { print count + 0 }')" -eq 1; printf '%s\n' "$qz_rows" | awk -v port="$qz_port/$qz_protocol" 'index($0, port) { found=1 } END { exit !found }'; fi; case "$qz_action" in add) test -n "$qz_rows" || ufw allow "$qz_port/$qz_protocol" comment "$qz_marker";; remove) if test -n "$qz_rows"; then qz_number=$(printf '%s\n' "$qz_rows" | sed -n 's/^\[[[:space:]]*\([0-9][0-9]*\)\].*/\1/p'); test -n "$qz_number" && ufw --force delete "$qz_number"; fi;; *) exit 64;; esac"#
}

fn ufw_verify() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rows=$(ufw status numbered | awk -v marker="$qz_marker" 'index($0, marker) { print }'); case "$qz_action" in add) test "$(printf '%s\n' "$qz_rows" | awk 'NF { count++ } END { print count + 0 }')" -eq 1 && printf '%s\n' "$qz_rows" | awk -v port="$qz_port/$qz_protocol" 'index($0, port) { found=1 } END { exit !found }';; remove) test -z "$qz_rows";; *) exit 64;; esac"#
}

fn nft_snapshot() -> &'static str {
    r#"qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_present=false; qz_rows=$(nft -a list chain inet qingzhou input 2>/dev/null | awk -v marker="$qz_marker" 'index($0, marker) { print }'); if test -n "$qz_rows"; then test "$(printf '%s\n' "$qz_rows" | awk 'NF { count++ } END { print count + 0 }')" -eq 1; printf '%s\n' "$qz_rows" | awk -v protocol="$qz_protocol" -v port="$qz_port" 'index($0, protocol " dport " port) { found=1 } END { exit !found }'; qz_present=true; fi; printf 'backend=nftables\nentryid=%s\nport=%s\nprotocol=%s\npresent=%s\n' "$qz_id" "$qz_port" "$qz_protocol" "$qz_present""#
}

fn nft_action() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rows=$(nft -a list chain inet qingzhou input 2>/dev/null | awk -v marker="$qz_marker" 'index($0, marker) { print }'); if test -n "$qz_rows"; then test "$(printf '%s\n' "$qz_rows" | awk 'NF { count++ } END { print count + 0 }')" -eq 1; printf '%s\n' "$qz_rows" | awk -v protocol="$qz_protocol" -v port="$qz_port" 'index($0, protocol " dport " port) { found=1 } END { exit !found }'; fi; case "$qz_action" in add) if test -z "$qz_rows"; then nft list table inet qingzhou >/dev/null 2>&1 || nft add table inet qingzhou; nft list chain inet qingzhou input >/dev/null 2>&1 || nft 'add chain inet qingzhou input { type filter hook input priority 0; }'; nft add rule inet qingzhou input "$qz_protocol" dport "$qz_port" counter accept comment "$qz_marker"; fi;; remove) if test -n "$qz_rows"; then qz_handle=$(printf '%s\n' "$qz_rows" | awk '{ for (i=1; i<=NF; i++) if ($i == "handle") { print $(i+1); exit } }'); test -n "$qz_handle" && nft delete rule inet qingzhou input handle "$qz_handle"; fi;; *) exit 64;; esac"#
}

fn nft_verify() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_rows=$(nft -a list chain inet qingzhou input 2>/dev/null | awk -v marker="$qz_marker" 'index($0, marker) { print }'); case "$qz_action" in add) test "$(printf '%s\n' "$qz_rows" | awk 'NF { count++ } END { print count + 0 }')" -eq 1 && printf '%s\n' "$qz_rows" | awk -v protocol="$qz_protocol" -v port="$qz_port" 'index($0, protocol " dport " port) { found=1 } END { exit !found }';; remove) test -z "$qz_rows";; *) exit 64;; esac"#
}

fn iptables_snapshot() -> &'static str {
    r#"qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; qz_present=false; if iptables -C INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT >/dev/null 2>&1; then qz_present=true; fi; printf 'backend=iptables\nentryid=%s\nport=%s\nprotocol=%s\npresent=%s\n' "$qz_id" "$qz_port" "$qz_protocol" "$qz_present""#
}

fn iptables_action() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; case "$qz_action" in add) iptables -C INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT >/dev/null 2>&1 || iptables -I INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT;; remove) if iptables -C INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT >/dev/null 2>&1; then iptables -D INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT; fi;; *) exit 64;; esac"#
}

fn iptables_verify() -> &'static str {
    r#"qz_action={{action}}; qz_id={{entryId}}; qz_port={{port}}; qz_protocol={{protocol}}; qz_marker="qingzhou:$qz_id"; case "$qz_action" in add) iptables -C INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT;; remove) ! iptables -C INPUT -p "$qz_protocol" --dport "$qz_port" -m comment --comment "$qz_marker" -j ACCEPT >/dev/null 2>&1;; *) exit 64;; esac"#
}

fn firewall_implementation(
    id: &str,
    required_commands: &[&str],
    preview: &str,
    snapshot: &str,
    execute: &str,
    verify: &str,
) -> TaskImplementation {
    dangerous_implementation(
        id,
        &[],
        required_commands,
        preview,
        vec![backup_item(
            "firewall-before",
            BackupItemKind::CommandSnapshot,
            snapshot,
        )],
        execute,
        verify,
        "{{restore:firewall:firewall-before}}",
        ResultParserKind::KeyValue,
    )
}

#[allow(clippy::too_many_arguments)]
fn diagnostic(
    id: &str,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    timeout_seconds: u64,
    command: &str,
    required_commands: &[&str],
    parser: ResultParserKind,
    output_kind: OutputKind,
) -> TaskDefinition {
    let implementation: TaskImplementation = read_only_implementation(
        "posix",
        required_commands,
        vec![bounded_step(
            "collect",
            "采集安全诊断",
            timeout_seconds,
            command,
        )],
        parser,
    );
    let mut task = read_only_task(
        id,
        TaskCategory::Security,
        title,
        description,
        estimated_seconds,
        Vec::new(),
        vec![implementation],
    );
    task.output_kind = output_kind;
    task
}
