use crate::core::tasks::model::{
    OutputKind, ResultParserKind, TaskCategory, TaskDefinition, TaskImplementation,
};

use super::helpers::{bounded_step, read_only_implementation, read_only_task, service_parameter};

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
    ]
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
