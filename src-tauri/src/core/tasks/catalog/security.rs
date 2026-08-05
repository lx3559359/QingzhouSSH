use crate::core::tasks::model::{
    OutputKind, ResultParserKind, TaskCategory, TaskDefinition, TaskImplementation,
};

use super::helpers::{bounded_step, read_only_implementation, read_only_task};

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
    ]
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
