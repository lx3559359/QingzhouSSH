use crate::core::tasks::model::{
    BackupItemKind, OutputKind, ParameterKind, ResultParserKind, TaskCategory, TaskDefinition,
    TaskImplementation,
};

use super::helpers::{
    backup_item, bounded_step, bsd_read_only_implementation, dangerous_implementation,
    dangerous_task, enum_parameter, parameter, read_only_implementation, read_only_task,
    service_parameter, windows_read_only_implementation,
};
use serde_json::json;

const WINDOWS_SERVICE_INVENTORY_SCRIPT: &str = r#"$services=@(Get-CimInstance Win32_Service | Sort-Object Name | Select-Object -First 500 Name,DisplayName,State,StartMode,ProcessId,ExitCode)
[ordered]@{schemaVersion=1;services=$services} | ConvertTo-Json -Compress -Depth 4"#;

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![
        diagnostic(
            "service.inventory",
            "服务清单",
            "查看服务运行状态和开机启用状态",
            45,
            Vec::new(),
            vec![
                implementation(
                    "systemd",
                    "systemd",
                    &["systemctl", "head"],
                    45,
                    r#"printf '== running ==\n'; systemctl list-units --type=service --all --no-legend --no-pager | head -n 500; printf '== unit_files ==\n'; systemctl list-unit-files --type=service --no-legend --no-pager | head -n 500"#,
                    ResultParserKind::Table,
                ),
                implementation(
                    "service",
                    "service",
                    &["service", "head"],
                    45,
                    "service --status-all 2>&1 | head -n 500",
                    ResultParserKind::Table,
                ),
                bsd_read_only_implementation(
                    "bsd-service",
                    &["service"],
                    &["service", "head"],
                    45,
                    r#"printf '== configured ==\n'; service -l 2>/dev/null | head -n 500; printf '== enabled_or_running ==\n'; service -e 2>/dev/null | head -n 500"#,
                    ResultParserKind::Table,
                ),
                bsd_read_only_implementation(
                    "bsd-rcctl",
                    &["rcctl"],
                    &["rcctl", "head"],
                    45,
                    "rcctl ls all 2>/dev/null | head -n 500",
                    ResultParserKind::Table,
                ),
                windows_read_only_implementation(
                    "windows-powershell-services",
                    "powershell.exe",
                    &["get-ciminstance"],
                    45,
                    WINDOWS_SERVICE_INVENTORY_SCRIPT,
                    ResultParserKind::Table,
                ),
                windows_read_only_implementation(
                    "windows-pwsh-services",
                    "pwsh",
                    &["get-ciminstance"],
                    45,
                    WINDOWS_SERVICE_INVENTORY_SCRIPT,
                    ResultParserKind::Table,
                ),
            ],
            OutputKind::Table,
        ),
        diagnostic(
            "service.failed_logs",
            "失败服务与日志",
            "查看失败服务及每个服务最近的有限日志",
            90,
            Vec::new(),
            vec![
                implementation(
                    "systemd",
                    "systemd",
                    &["systemctl", "journalctl", "head"],
                    90,
                    r#"qz_units=$(systemctl list-units --state=failed --type=service --no-legend --no-pager | awk 'NR <= 20 { print $1 }'); if test -z "$qz_units"; then printf '%s\n' '__QZ_METRIC__ failed_services=0'; else printf '%s\n' "$qz_units" | while IFS= read -r qz_unit; do printf '== %s ==\n' "$qz_unit"; journalctl -u "$qz_unit" -n 100 --no-pager 2>/dev/null || true; done; fi"#,
                    ResultParserKind::HealthSummary,
                ),
                implementation(
                    "service",
                    "service",
                    &["service", "head"],
                    45,
                    r#"service --status-all 2>&1 | head -n 200; printf '%s\n' '__QZ_UNSUPPORTED__ per_service_journal'"#,
                    ResultParserKind::HealthSummary,
                ),
            ],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "service.status",
            "服务状态",
            "查看已发现服务的状态、主进程和退出信息",
            30,
            vec![service_parameter()],
            vec![
                implementation(
                    "systemd-status",
                    "systemd",
                    &["systemctl"],
                    30,
                    r#"systemctl show --no-pager -p Id -p LoadState -p ActiveState -p SubState -p MainPID -p ExecMainCode -p ExecMainStatus -- {{service}}; printf '== status ==\n'; systemctl status --no-pager --lines=100 -- {{service}} 2>&1 || true"#,
                    ResultParserKind::ServiceStatus,
                ),
                implementation(
                    "service-status",
                    "service",
                    &["service"],
                    30,
                    "service {{service}} status",
                    ResultParserKind::ServiceStatus,
                ),
            ],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "service.scheduled_tasks",
            "计划任务",
            "查看 systemd timers、当前用户 crontab 和固定 cron 目录",
            45,
            Vec::new(),
            vec![
                implementation(
                    "systemd",
                    "systemd",
                    &["systemctl", "find", "head"],
                    45,
                    &scheduled_command(true),
                    ResultParserKind::Table,
                ),
                implementation(
                    "service",
                    "service",
                    &["find", "head"],
                    45,
                    &scheduled_command(false),
                    ResultParserKind::Table,
                ),
            ],
            OutputKind::Table,
        ),
        service_action("service.start", "启动服务", "start"),
        service_action("service.stop", "停止服务", "stop"),
        service_action("service.restart", "重启服务", "restart"),
        service_boot_policy(),
        cron_manage(),
    ]
}

fn service_action(id: &str, title: &str, action: &str) -> TaskDefinition {
    dangerous_task(
        id,
        TaskCategory::Service,
        title,
        "保存服务运行状态后执行单个受控动作，失败时可恢复原状态",
        60,
        vec![service_parameter()],
        vec![
            service_action_implementation("systemd", "systemd", "systemctl", action),
            service_action_implementation("service", "service", "service", action),
        ],
        OutputKind::KeyValue,
    )
}

fn service_action_implementation(
    id: &str,
    manager: &str,
    executable: &str,
    action: &str,
) -> TaskImplementation {
    let preview = if executable == "systemctl" {
        "systemctl show --no-pager -p ActiveState -p SubState -p UnitFileState -- {{service}}; systemctl status --no-pager --lines=20 -- {{service}} 2>&1 || true".to_owned()
    } else {
        "service {{service}} status 2>&1 || true".to_owned()
    };
    let execute = if executable == "systemctl" {
        format!("systemctl {action} -- {{{{service}}}}")
    } else {
        format!("service {{{{service}}}} {action}")
    };
    let snapshot = service_snapshot_command(executable);
    let verify: String = match (executable, action) {
        ("systemctl", "stop") => {
            "test \"$(systemctl is-active -- {{service}} 2>/dev/null || true)\" = inactive".into()
        }
        ("systemctl", _) => {
            "test \"$(systemctl is-active -- {{service}} 2>/dev/null || true)\" = active".into()
        }
        (_, "stop") => "! service {{service}} status >/dev/null 2>&1".into(),
        _ => "service {{service}} status".into(),
    };
    dangerous_implementation(
        &format!("{id}-{action}"),
        &[manager],
        &[executable],
        &preview,
        vec![backup_item(
            "service-state-before",
            BackupItemKind::RuntimeState,
            snapshot,
        )],
        &execute,
        &verify,
        "{{restore:service:service-state-before}}",
        ResultParserKind::ServiceStatus,
    )
}

fn service_snapshot_command(executable: &str) -> &'static str {
    if executable == "systemctl" {
        r#"qz_active=$(systemctl is-active -- {{service}} 2>/dev/null || true); qz_enabled=$(systemctl is-enabled -- {{service}} 2>/dev/null || true); printf 'manager=systemd\nservice=%s\nactive=%s\nenabled=%s\n' {{service}} "$qz_active" "$qz_enabled""#
    } else {
        r#"if service {{service}} status >/dev/null 2>&1; then qz_active=active; else qz_active=inactive; fi; printf 'manager=service\nservice=%s\nactive=%s\nenabled=unsupported\n' {{service}} "$qz_active""#
    }
}

fn service_boot_policy() -> TaskDefinition {
    dangerous_task(
        "service.boot_policy",
        TaskCategory::Service,
        "设置服务开机策略",
        "保存 enabled/disabled/masked 状态后设置单个服务的开机策略",
        60,
        vec![
            service_parameter(),
            enum_parameter(
                "policy",
                "开机策略",
                "启用或禁用开机启动",
                &["enable", "disable"],
                None,
            ),
        ],
        vec![dangerous_implementation(
            "systemd-boot-policy",
            &["systemd"],
            &["systemctl"],
            "systemctl show --no-pager -p UnitFileState -p ActiveState -- {{service}}",
            vec![backup_item(
                "boot-policy-before",
                BackupItemKind::RuntimeState,
                service_snapshot_command("systemctl"),
            )],
            "systemctl {{policy}} -- {{service}}",
            r#"qz_policy={{policy}}; case "$qz_policy" in enable) qz_expected=enabled;; disable) qz_expected=disabled;; *) exit 64;; esac; test "$(systemctl is-enabled -- {{service}} 2>/dev/null || true)" = "$qz_expected""#,
            "{{restore:service-policy:boot-policy-before}}",
            ResultParserKind::ServiceStatus,
        )],
        OutputKind::KeyValue,
    )
}

fn cron_manage() -> TaskDefinition {
    dangerous_task(
        "service.cron_manage",
        TaskCategory::Service,
        "管理计划任务",
        "只管理带 # qingzhou:<uuid> 标识的工具条目，不修改其他 Cron 内容",
        75,
        vec![
            enum_parameter(
                "action",
                "操作",
                "新增、停用或移除工具条目",
                &["add", "disable", "remove"],
                None,
            ),
            parameter(
                "schedule",
                "执行周期",
                "标准五段 Cron 表达式",
                ParameterKind::CronExpression,
                false,
                Some(json!("0 2 * * *")),
            ),
            parameter(
                "entryId",
                "任务标识",
                "客户端自动生成，用于确保只修改本工具创建的条目",
                ParameterKind::ManagedId,
                true,
                None,
            ),
            enum_parameter(
                "task",
                "受控任务",
                "只允许引用内置安全快捷任务",
                &["system.overview", "system.disk_usage"],
                Some("system.overview"),
            ),
        ],
        vec![dangerous_implementation(
            "managed-cron-file",
            &[],
            &["awk", "sed", "mktemp", "wc", "chown", "chmod", "mv", "rm"],
            "if test -f /etc/cron.d/qingzhou-managed; then sed -n '1,300p' /etc/cron.d/qingzhou-managed; fi",
            vec![backup_item(
                "cron-managed-before",
                BackupItemKind::ManagedBlock,
                "/etc/cron.d/qingzhou-managed",
            )],
            cron_action_command(),
            cron_verify_command(),
            "{{restore:file:cron-managed-before}}",
            ResultParserKind::Text,
        )],
        OutputKind::Text,
    )
}

fn cron_action_command() -> &'static str {
    r##"qz_action={{action}}; qz_schedule={{schedule}}; qz_task={{task}}; qz_id={{entryId}}; qz_target=/etc/cron.d/qingzhou-managed; qz_marker="# qingzhou:$qz_id"; test ! -L "$qz_target"; if test -e "$qz_target"; then test -f "$qz_target"; fi; qz_count=$(if test -f "$qz_target"; then awk -v marker="$qz_marker" 'length($0) >= length(marker) && substr($0, length($0) - length(marker) + 1) == marker { count++ } END { print count + 0 }' "$qz_target"; else printf '0\n'; fi); test "$qz_count" -le 1; case "$qz_action" in add) ;; disable|remove) test "$qz_count" -eq 1;; *) exit 64;; esac; case "$qz_task" in system.overview) qz_command='uptime >> /var/log/qingzhou-system-overview.log 2>&1';; system.disk_usage) qz_command='df -hP >> /var/log/qingzhou-disk-usage.log 2>&1';; *) exit 64;; esac; qz_tmp=$(mktemp /etc/cron.d/.qingzhou-managed.XXXXXX) || exit; cleanup() { rm -f -- "$qz_tmp"; }; trap cleanup EXIT HUP INT TERM; if test -f "$qz_target"; then awk -v marker="$qz_marker" 'length($0) < length(marker) || substr($0, length($0) - length(marker) + 1) != marker { print }' "$qz_target" > "$qz_tmp" || exit; fi; case "$qz_action" in add) printf '%s root %s %s\n' "$qz_schedule" "$qz_command" "$qz_marker" >> "$qz_tmp";; disable) qz_line=$(awk -v marker="$qz_marker" 'length($0) >= length(marker) && substr($0, length($0) - length(marker) + 1) == marker { print; exit }' "$qz_target"); case "$qz_line" in '# disabled '*) printf '%s\n' "$qz_line" >> "$qz_tmp";; *) printf '# disabled %s\n' "$qz_line" >> "$qz_tmp";; esac;; remove) :;; esac; test "$(wc -c < "$qz_tmp")" -le 65536; awk 'BEGIN { ok=1 } /^[[:space:]]*($|#)/ { next } /^[A-Z_][A-Z0-9_]*=/ { next } NF < 7 { ok=0 } END { exit ok ? 0 : 1 }' "$qz_tmp"; chown root:root "$qz_tmp" && chmod 0644 "$qz_tmp" && mv -f -- "$qz_tmp" "$qz_target"; qz_tmp=''; trap - EXIT HUP INT TERM"##
}

fn cron_verify_command() -> &'static str {
    r##"qz_action={{action}}; qz_id={{entryId}}; qz_target=/etc/cron.d/qingzhou-managed; qz_marker="# qingzhou:$qz_id"; test ! -L "$qz_target" && test -f "$qz_target"; qz_line=$(awk -v marker="$qz_marker" 'length($0) >= length(marker) && substr($0, length($0) - length(marker) + 1) == marker { print }' "$qz_target"); qz_count=$(printf '%s\n' "$qz_line" | awk 'NF { count++ } END { print count + 0 }'); case "$qz_action" in add) test "$qz_count" -eq 1; case "$qz_line" in '#'* ) exit 1;; *) exit 0;; esac;; disable) test "$qz_count" -eq 1; case "$qz_line" in '# disabled '*) exit 0;; *) exit 1;; esac;; remove) test "$qz_count" -eq 0;; *) exit 64;; esac"##
}

fn scheduled_command(include_timers: bool) -> String {
    let timers = if include_timers {
        "printf '== timers ==\\n'; systemctl list-timers --all --no-legend --no-pager | head -n 300; "
    } else {
        ""
    };
    format!(
        "{timers}printf '== user_crontab ==\\n'; if command -v crontab >/dev/null 2>&1; then crontab -l 2>/dev/null | head -n 300 || true; fi; printf '== cron_files ==\\n'; for qz_dir in /etc/cron.d /etc/cron.daily /etc/cron.hourly /etc/cron.weekly /etc/cron.monthly; do if test -d \"$qz_dir\"; then find \"$qz_dir\" -maxdepth 1 -type f -printf '%p\\n' 2>/dev/null | head -n 300; fi; done"
    )
}

#[allow(clippy::too_many_arguments)]
fn diagnostic(
    id: &str,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<crate::core::tasks::model::ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
    output_kind: OutputKind,
) -> TaskDefinition {
    let mut task = read_only_task(
        id,
        TaskCategory::Service,
        title,
        description,
        estimated_seconds,
        parameters,
        implementations,
    );
    task.output_kind = output_kind;
    task
}

fn implementation(
    id: &str,
    service_manager: &str,
    required_commands: &[&str],
    timeout_seconds: u64,
    command: &str,
    parser: ResultParserKind,
) -> TaskImplementation {
    let mut implementation = read_only_implementation(
        id,
        required_commands,
        vec![bounded_step(
            "collect",
            "采集服务诊断",
            timeout_seconds,
            command,
        )],
        parser,
    );
    implementation.compatibility.service_managers = vec![service_manager.into()];
    implementation
}
