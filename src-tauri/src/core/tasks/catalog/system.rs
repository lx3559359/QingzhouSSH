use crate::core::tasks::model::{
    BackupItemKind, OutputKind, ParameterDefinition, ParameterKind, ResultParserKind, TaskCategory,
    TaskDefinition,
};

use super::helpers::{
    backup_item, bounded_step, dangerous_implementation, dangerous_task, host_parameter,
    integer_parameter, parameter, read_only_implementation, read_only_task, string_parameter,
};

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![
        diagnostic(
            "system.overview",
            "系统概览",
            "查看系统、负载、内存、磁盘、登录用户和网络摘要",
            45,
            Vec::new(),
            &["uname", "uptime", "df", "ps"],
            45,
            r#"printf '== system ==\n'; uname -a; if test -r /etc/os-release; then sed -n '1,20p' /etc/os-release; fi; printf '== uptime ==\n'; uptime; printf '== users ==\n'; if command -v who >/dev/null 2>&1; then who; fi; printf '== memory ==\n'; if command -v free >/dev/null 2>&1; then free -b; fi; printf '== disk ==\n'; df -P -B1; printf '== network ==\n'; if command -v ip >/dev/null 2>&1; then ip -brief address; else hostname -I 2>/dev/null || true; fi"#,
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "system.cpu_pressure",
            "CPU 压力",
            "查看负载、CPU 数量、热点进程和压力指标",
            45,
            Vec::new(),
            &["uptime", "ps"],
            45,
            r#"printf '== load ==\n'; uptime; printf '== cpu_count ==\n'; if command -v getconf >/dev/null 2>&1; then getconf _NPROCESSORS_ONLN; elif command -v nproc >/dev/null 2>&1; then nproc; fi; printf '== cpu_processes ==\n'; ps -eo pid,user,%cpu,%mem,stat,etime,args --sort=-%cpu | head -n 31; printf '== cpu_pressure ==\n'; if test -r /proc/pressure/cpu; then cat /proc/pressure/cpu; else printf '%s\n' '__QZ_UNSUPPORTED__ cpu_pressure'; fi"#,
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "system.memory_oom",
            "内存与 OOM",
            "查看内存、换页、热点进程和近期 OOM 记录",
            60,
            Vec::new(),
            &["ps"],
            60,
            r#"printf '== memory ==\n'; if command -v free >/dev/null 2>&1; then free -b; fi; printf '== vmstat ==\n'; if command -v vmstat >/dev/null 2>&1; then vmstat 1 3; fi; printf '== memory_processes ==\n'; ps -eo pid,user,%cpu,%mem,rss,vsz,etime,args --sort=-%mem | head -n 31; printf '== oom_48h ==\n'; if command -v journalctl >/dev/null 2>&1; then journalctl -k --since '48 hours ago' --no-pager -n 1000 2>/dev/null | grep -Ei 'out of memory|oom-killer|killed process' | tail -n 200 || true; elif command -v dmesg >/dev/null 2>&1; then dmesg 2>/dev/null | tail -n 1000 | grep -Ei 'out of memory|oom-killer|killed process' | tail -n 200 || true; else printf '%s\n' '__QZ_UNSUPPORTED__ kernel_log'; fi"#,
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "system.process_top",
            "进程排行",
            "按 CPU 和内存查看资源占用最高的进程",
            30,
            vec![integer_parameter(
                "limit",
                "结果上限",
                "每个排行最多显示的进程数",
                10,
                200,
                30,
            )],
            &["ps", "head"],
            30,
            r#"printf '== cpu_top ==\n'; ps -eo pid,user,%cpu,%mem,rss,etime,args --sort=-%cpu | head -n {{limit}}; printf '== memory_top ==\n'; ps -eo pid,user,%cpu,%mem,rss,etime,args --sort=-%mem | head -n {{limit}}"#,
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        diagnostic(
            "system.process_query",
            "进程查询",
            "按固定文本查找进程，不使用正则表达式",
            30,
            vec![
                string_parameter("query", "进程关键词", "要查找的固定文本", 1, 128),
                integer_parameter("limit", "结果上限", "最多返回的进程数", 1, 200, 50),
            ],
            &["ps", "grep", "head"],
            30,
            r#"ps -eo pid,user,%cpu,%mem,stat,etime,args | grep -F -- {{query}} | grep -v '[g]rep -F' | head -n {{limit}}"#,
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        diagnostic(
            "system.process_detail",
            "进程详情",
            "查看指定 PID 的状态、命令行、限制和资源占用",
            30,
            vec![parameter(
                "pid",
                "进程 PID",
                "目标进程的数字 PID",
                ParameterKind::Integer {
                    min: 1,
                    max: 4_194_304,
                },
                true,
                None,
            )],
            &["ps"],
            30,
            r#"if test ! -d /proc/{{pid}}; then printf '%s\n' '__QZ_ERROR__ process_not_found'; exit 4; fi; printf '== ps ==\n'; ps -p {{pid}} -o pid,ppid,user,group,%cpu,%mem,rss,vsz,stat,lstart,etime,args; printf '== status ==\n'; sed -n '1,160p' /proc/{{pid}}/status; printf '== cmdline ==\n'; tr '\000' ' ' < /proc/{{pid}}/cmdline; printf '\n== limits ==\n'; sed -n '1,160p' /proc/{{pid}}/limits"#,
            ResultParserKind::KeyValue,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "system.kernel_events",
            "内核事件",
            "查看最近一段时间的有界内核日志",
            60,
            vec![integer_parameter(
                "hours",
                "最近小时数",
                "查看最近多少小时",
                1,
                168,
                24,
            )],
            &[],
            60,
            r#"if command -v journalctl >/dev/null 2>&1; then journalctl -k --since {{hours}}' hours ago' --no-pager -n 1000; elif command -v dmesg >/dev/null 2>&1; then dmesg | tail -n 1000; else printf '%s\n' '__QZ_UNSUPPORTED__ kernel_log'; fi"#,
            ResultParserKind::Text,
            OutputKind::Text,
        ),
        diagnostic(
            "system.boot_history",
            "开关机历史",
            "查看最近启动、关机和重启记录",
            30,
            vec![integer_parameter(
                "limit",
                "结果上限",
                "最多返回的历史条数",
                10,
                100,
                30,
            )],
            &[],
            30,
            r#"printf '== current_boot ==\n'; if command -v who >/dev/null 2>&1; then who -b; fi; printf '== history ==\n'; if command -v last >/dev/null 2>&1; then last -x reboot shutdown | head -n {{limit}}; else printf '%s\n' '__QZ_UNSUPPORTED__ boot_history'; fi"#,
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        diagnostic(
            "system.time",
            "系统时间",
            "查看时区、时钟同步和时间源状态",
            30,
            Vec::new(),
            &["date"],
            30,
            r#"printf '== date ==\n'; date -Is; printf '== timedatectl ==\n'; if command -v timedatectl >/dev/null 2>&1; then timedatectl status 2>/dev/null || true; fi; printf '== source ==\n'; if command -v chronyc >/dev/null 2>&1; then chronyc tracking; elif command -v ntpq >/dev/null 2>&1; then ntpq -pn; else printf '%s\n' '__QZ_UNSUPPORTED__ time_source'; fi"#,
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "system.disk_usage",
            "磁盘使用",
            "按字节查看挂载点容量和使用率",
            30,
            Vec::new(),
            &["df"],
            30,
            "df -P -B1",
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        hostname_change(),
        timezone_change(),
    ]
}

fn hostname_change() -> TaskDefinition {
    dangerous_task(
        "system.hostname_change",
        TaskCategory::System,
        "修改主机名",
        "预演并保存原主机名，修改后精确核验，可恢复到原值",
        45,
        vec![host_parameter(
            "hostname",
            "新主机名",
            "符合主机名规则的新名称",
            true,
        )],
        vec![dangerous_implementation(
            "hostnamectl",
            &[],
            &["hostnamectl", "hostname"],
            "hostnamectl status --static; hostname",
            vec![backup_item(
                "hostname-before",
                BackupItemKind::RuntimeState,
                "hostname",
            )],
            "hostnamectl set-hostname -- {{hostname}}",
            "test \"$(hostname)\" = {{hostname}}; hostnamectl status --static",
            "hostnamectl set-hostname -- {{restore:hostname-before}}",
            ResultParserKind::KeyValue,
        )],
        OutputKind::KeyValue,
    )
}

fn timezone_change() -> TaskDefinition {
    dangerous_task(
        "system.timezone_change",
        TaskCategory::System,
        "修改时区",
        "保存原时区后修改，并核验系统报告的时区值",
        45,
        vec![string_parameter(
            "timezone",
            "新时区",
            "IANA 时区名称，例如 Asia/Shanghai",
            1,
            128,
        )],
        vec![dangerous_implementation(
            "timedatectl",
            &[],
            &["timedatectl"],
            "timedatectl show -p Timezone --value; timedatectl status",
            vec![backup_item(
                "timezone-before",
                BackupItemKind::RuntimeState,
                "timedatectl show -p Timezone --value",
            )],
            "timedatectl set-timezone -- {{timezone}}",
            "test \"$(timedatectl show -p Timezone --value)\" = {{timezone}}; timedatectl status",
            "timedatectl set-timezone -- {{restore:timezone-before}}",
            ResultParserKind::KeyValue,
        )],
        OutputKind::KeyValue,
    )
}

#[allow(clippy::too_many_arguments)]
fn diagnostic(
    id: &str,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<ParameterDefinition>,
    required_commands: &[&str],
    timeout_seconds: u64,
    command: &str,
    parser: ResultParserKind,
    output_kind: OutputKind,
) -> TaskDefinition {
    let implementation = read_only_implementation(
        "posix",
        required_commands,
        vec![bounded_step(
            "collect",
            "采集诊断信息",
            timeout_seconds,
            command,
        )],
        parser,
    );
    let mut task = read_only_task(
        id,
        TaskCategory::System,
        title,
        description,
        estimated_seconds,
        parameters,
        vec![implementation],
    );
    task.output_kind = output_kind;
    task
}
