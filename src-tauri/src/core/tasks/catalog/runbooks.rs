use crate::core::tasks::model::{
    ParameterDefinition, ResultParserKind, TaskCategory, TaskDefinition, TaskImplementation,
    TaskStep,
};

use super::helpers::{
    bounded_step, host_parameter, port_parameter, read_only_implementation, read_only_task,
    service_multi_parameter,
};

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![
        runbook(
            "runbook.health.baseline",
            "综合健康巡检",
            "依次检查系统、资源、存储、网络和服务基础状态",
            120,
            Vec::new(),
            vec![implementation(
                "posix",
                &["uname", "uptime", "df", "ps", "head"],
                vec![
                    step(
                        "system",
                        "系统与运行时间",
                        30,
                        r#"uname -a; uptime; if test -r /etc/os-release; then sed -n '1,20p' /etc/os-release; fi; if command -v free >/dev/null 2>&1; then free -b; fi"#,
                    ),
                    step(
                        "resources",
                        "CPU 与内存热点",
                        30,
                        r#"ps -eo pid,user,%cpu,%mem,rss,etime,args --sort=-%cpu | head -n 31; if test -r /proc/pressure/cpu; then cat /proc/pressure/cpu; fi; if test -r /proc/pressure/memory; then cat /proc/pressure/memory; fi"#,
                    ),
                    step(
                        "storage",
                        "容量与挂载",
                        30,
                        r#"df -P -B1; df -Pi; if command -v findmnt >/dev/null 2>&1; then findmnt -rn -o TARGET,SOURCE,FSTYPE,OPTIONS | head -n 300; fi"#,
                    ),
                    step(
                        "network",
                        "地址、路由与监听",
                        30,
                        r#"if command -v ip >/dev/null 2>&1; then ip -brief address; ip route show table main; fi; if command -v ss >/dev/null 2>&1; then ss -s; ss -H -lntup | head -n 200; fi"#,
                    ),
                    step(
                        "services",
                        "失败服务",
                        30,
                        r#"if command -v systemctl >/dev/null 2>&1; then systemctl list-units --state=failed --type=service --no-legend --no-pager | head -n 100; elif command -v service >/dev/null 2>&1; then service --status-all 2>&1 | head -n 200; fi"#,
                    ),
                ],
                ResultParserKind::HealthSummary,
            )],
        ),
        runbook(
            "runbook.cpu.incident",
            "CPU 异常排查",
            "按负载、短时采样、热点进程和调度压力逐步排查",
            120,
            Vec::new(),
            vec![implementation(
                "posix",
                &["uptime", "ps", "head"],
                vec![
                    step(
                        "load",
                        "负载与 CPU 数量",
                        30,
                        r#"uptime; if command -v getconf >/dev/null 2>&1; then getconf _NPROCESSORS_ONLN; elif command -v nproc >/dev/null 2>&1; then nproc; fi"#,
                    ),
                    step(
                        "sampling",
                        "短时 CPU 采样",
                        45,
                        r#"if command -v vmstat >/dev/null 2>&1; then vmstat 1 3; else sed -n '1,12p' /proc/stat; fi"#,
                    ),
                    step(
                        "processes",
                        "CPU 热点进程",
                        30,
                        "ps -eo pid,ppid,user,%cpu,%mem,stat,etime,args --sort=-%cpu | head -n 51",
                    ),
                    step(
                        "scheduler",
                        "调度与压力",
                        30,
                        r#"if test -r /proc/pressure/cpu; then cat /proc/pressure/cpu; else printf '%s\n' '__QZ_UNSUPPORTED__ cpu_pressure'; fi; ps -eo stat | awk 'NR > 1 { count[$1]++ } END { for (state in count) print state,count[state] }' | head -n 100"#,
                    ),
                ],
                ResultParserKind::HealthSummary,
            )],
        ),
        runbook(
            "runbook.memory.oom",
            "内存与 OOM 排查",
            "查看内存概况、热点进程、OOM 事件和内核压力",
            120,
            Vec::new(),
            vec![implementation(
                "posix",
                &["ps", "head"],
                vec![
                    step(
                        "overview",
                        "内存概况",
                        30,
                        r#"if command -v free >/dev/null 2>&1; then free -b; fi; sed -n '1,80p' /proc/meminfo"#,
                    ),
                    step(
                        "processes",
                        "内存热点进程",
                        30,
                        "ps -eo pid,ppid,user,%cpu,%mem,rss,vsz,etime,args --sort=-%mem | head -n 51",
                    ),
                    step(
                        "oom",
                        "近期 OOM 事件",
                        45,
                        r#"if command -v journalctl >/dev/null 2>&1; then journalctl -k --since '48 hours ago' --no-pager -n 1000 2>/dev/null | grep -Ei 'out of memory|oom-killer|killed process' | tail -n 200 || true; elif command -v dmesg >/dev/null 2>&1; then dmesg 2>/dev/null | tail -n 1000 | grep -Ei 'out of memory|oom-killer|killed process' | tail -n 200 || true; else printf '%s\n' '__QZ_UNSUPPORTED__ kernel_log'; fi"#,
                    ),
                    step(
                        "kernel",
                        "内存压力与换页",
                        30,
                        r#"if test -r /proc/pressure/memory; then cat /proc/pressure/memory; fi; if command -v vmstat >/dev/null 2>&1; then vmstat 1 3; fi"#,
                    ),
                ],
                ResultParserKind::HealthSummary,
            )],
        ),
        runbook(
            "runbook.storage.capacity_io",
            "存储容量与 I/O 排查",
            "查看容量、常见目录增长、I/O 延迟和未释放文件",
            240,
            Vec::new(),
            vec![implementation(
                "posix",
                &["df"],
                vec![
                    step("capacity", "文件系统容量", 30, "df -P -B1; df -Pi"),
                    step(
                        "growth",
                        "常见目录增长",
                        120,
                        r#"for qz_root in /var /opt /home; do if test -d "$qz_root"; then du -x -B1 --max-depth=2 -- "$qz_root" 2>/dev/null | sort -nr -k1,1 | head -n 50; fi; done"#,
                    ),
                    step(
                        "latency",
                        "I/O 延迟采样",
                        45,
                        r#"if command -v iostat >/dev/null 2>&1; then iostat -xz 1 3; elif command -v vmstat >/dev/null 2>&1; then vmstat 1 3; else printf '%s\n' '__QZ_UNSUPPORTED__ io_sampler'; fi"#,
                    ),
                    step(
                        "open_deleted",
                        "已删除未释放文件",
                        45,
                        r#"if command -v lsof >/dev/null 2>&1; then lsof -nP +L1 2>/dev/null | head -n 200; else printf '%s\n' '__QZ_UNSUPPORTED__ lsof'; fi"#,
                    ),
                ],
                ResultParserKind::HealthSummary,
            )],
        ),
        runbook(
            "runbook.network.intermittent",
            "网络间歇故障排查",
            "围绕指定目标检查接口、路由、DNS、时延和路径",
            150,
            vec![host_parameter("host", "目标主机", "故障目标主机名或 IP", true)],
            vec![implementation(
                "iproute2",
                &["ip", "getent", "ping"],
                vec![
                    step(
                        "interfaces",
                        "接口状态",
                        30,
                        "ip -brief link; ip -brief address; ip -s link",
                    ),
                    step(
                        "route",
                        "路由与邻居",
                        30,
                        "ip route show table main; ip rule show; ip neigh show",
                    ),
                    step(
                        "dns",
                        "DNS 解析",
                        30,
                        "getent ahosts {{host}} | head -n 100; sed -n '1,120p' /etc/resolv.conf",
                    ),
                    step(
                        "latency",
                        "连通与时延",
                        30,
                        r#"ping -n -c 6 -W 3 -- {{host}} || printf '%s\n' '__QZ_WARNING__ connectivity_failed'"#,
                    ),
                    step(
                        "path",
                        "网络路径",
                        30,
                        r#"if command -v tracepath >/dev/null 2>&1; then timeout 20 tracepath -n {{host}} 2>/dev/null | head -n 40 || true; else printf '%s\n' '__QZ_UNSUPPORTED__ tracepath'; fi"#,
                    ),
                ],
                ResultParserKind::NetworkProbe,
            )],
        ),
        runbook(
            "runbook.security.ssh_audit",
            "SSH 安全审计",
            "查看 SSH 配置、监听、近期登录和高权限账号",
            120,
            Vec::new(),
            vec![implementation(
                "posix",
                &["awk", "head"],
                vec![
                    step(
                        "configuration",
                        "SSH 配置摘要",
                        30,
                        r#"if command -v sshd >/dev/null 2>&1; then sshd -T 2>/dev/null | grep -E '^(port|listenaddress|permitrootlogin|passwordauthentication|pubkeyauthentication|maxauthtries|allowusers|allowgroups|denyusers|denygroups) ' | head -n 100 || true; fi"#,
                    ),
                    step(
                        "listeners",
                        "SSH 监听端口",
                        30,
                        r#"if command -v ss >/dev/null 2>&1; then ss -H -lntp | grep -E 'sshd|:22([[:space:]]|$)' | head -n 100 || true; elif command -v netstat >/dev/null 2>&1; then netstat -lntp | grep -E 'sshd|:22([[:space:]]|$)' | head -n 100 || true; fi"#,
                    ),
                    step(
                        "logins",
                        "近期登录事件",
                        45,
                        r#"if command -v journalctl >/dev/null 2>&1; then journalctl --since '24 hours ago' --no-pager -n 300 -u ssh -u sshd 2>/dev/null || true; elif test -r /var/log/auth.log; then tail -n 300 /var/log/auth.log; elif test -r /var/log/secure; then tail -n 300 /var/log/secure; fi"#,
                    ),
                    step(
                        "accounts",
                        "高权限账号",
                        30,
                        r#"awk -F: '$3 == 0 { print $1 ":" $6 ":" $7 }' /etc/passwd | head -n 50; if command -v last >/dev/null 2>&1; then last -n 100; fi"#,
                    ),
                ],
                ResultParserKind::HealthSummary,
            )],
        ),
        runbook(
            "runbook.web.gateway",
            "Web 502/504 排查",
            "检查 Web 服务、监听、配置、日志和上游探测",
            150,
            vec![
                host_parameter("host", "目标主机", "网关或上游主机名/IP", true),
                port_parameter("port", "目标端口", "要探测的 Web 端口", false, Some(80)),
            ],
            vec![implementation(
                "posix",
                &["curl", "head"],
                vec![
                    step(
                        "services",
                        "Web 服务状态",
                        30,
                        r#"if command -v systemctl >/dev/null 2>&1; then for qz_unit in nginx apache2 httpd; do systemctl show --no-pager -p Id -p ActiveState -p SubState -- "$qz_unit" 2>/dev/null || true; done; fi"#,
                    ),
                    step(
                        "listeners",
                        "80/443 监听",
                        30,
                        r#"if command -v ss >/dev/null 2>&1; then ss -H -lntp '( sport = :80 or sport = :443 )' | head -n 200; fi"#,
                    ),
                    step(
                        "configuration",
                        "配置语法",
                        30,
                        r#"if command -v nginx >/dev/null 2>&1; then nginx -t 2>&1 || printf '%s\n' '__QZ_WARNING__ service_failed'; elif command -v apachectl >/dev/null 2>&1; then apachectl configtest 2>&1 || printf '%s\n' '__QZ_WARNING__ service_failed'; else printf '%s\n' '__QZ_UNSUPPORTED__ web_server'; fi"#,
                    ),
                    step(
                        "logs",
                        "近期网关错误",
                        30,
                        r#"for qz_log in /var/log/nginx/error.log /var/log/apache2/error.log /var/log/httpd/error_log; do if test -r "$qz_log"; then printf '== %s ==\n' "$qz_log"; tail -n 200 "$qz_log"; fi; done"#,
                    ),
                    step(
                        "probe",
                        "HTTP 根路径探测",
                        30,
                        r#"qz_host={{host}}; qz_port={{port}}; case "$qz_host" in *:*) qz_authority="[$qz_host]";; *) qz_authority="$qz_host";; esac; curl --head --silent --show-error --connect-timeout 5 --max-time 8 "http://$qz_authority:$qz_port/""#,
                    ),
                ],
                ResultParserKind::HealthSummary,
            )],
        ),
        container_runbook(),
        service_runbook(),
    ]
}

fn container_runbook() -> TaskDefinition {
    let implementations = ["docker", "podman"]
        .into_iter()
        .map(|runtime| {
            implementation(
                runtime,
                &[runtime, "head"],
                vec![
                    step(
                        "runtime",
                        "运行时版本",
                        30,
                        &format!("{runtime} version 2>&1 | head -n 100"),
                    ),
                    step(
                        "inventory",
                        "容器清单",
                        30,
                        &format!("{runtime} ps -a --no-trunc 2>&1 | head -n 101"),
                    ),
                    step(
                        "resources",
                        "资源快照",
                        45,
                        &format!("{runtime} stats --no-stream 2>&1 | head -n 101"),
                    ),
                    step(
                        "events",
                        "近期事件",
                        45,
                        &format!(
                            "{runtime} events --since 1h --until 0s 2>&1 | tail -n 200 || true"
                        ),
                    ),
                ],
                ResultParserKind::ContainerStatus,
            )
        })
        .collect();
    runbook(
        "runbook.container.runtime",
        "容器运行时排查",
        "查看运行时版本、容器清单、资源和近期事件",
        150,
        Vec::new(),
        implementations,
    )
}

fn service_runbook() -> TaskDefinition {
    let parameters = vec![service_multi_parameter(10)];
    let systemd_steps = vec![
        step(
            "status",
            "服务状态",
            45,
            r#"for qz_service in {{services}}; do printf '== %s ==\n' "$qz_service"; systemctl show --no-pager -p Id -p LoadState -p ActiveState -p SubState -p MainPID -p ExecMainStatus -- "$qz_service"; done"#,
        ),
        step(
            "process",
            "服务主进程",
            30,
            r#"for qz_service in {{services}}; do qz_pid=$(systemctl show -p MainPID --value -- "$qz_service"); if test "$qz_pid" -gt 0 2>/dev/null; then ps -p "$qz_pid" -o pid,ppid,user,%cpu,%mem,rss,stat,etime,args; fi; done"#,
        ),
        step(
            "logs",
            "服务近期日志",
            60,
            r#"for qz_service in {{services}}; do printf '== %s ==\n' "$qz_service"; journalctl -u "$qz_service" -n 100 --no-pager 2>/dev/null || true; done"#,
        ),
        step(
            "ports",
            "服务监听端口",
            30,
            r#"if command -v ss >/dev/null 2>&1; then for qz_service in {{services}}; do qz_pid=$(systemctl show -p MainPID --value -- "$qz_service"); if test "$qz_pid" -gt 0 2>/dev/null; then ss -H -lntup | grep -F "pid=$qz_pid," | head -n 100 || true; fi; done; fi"#,
        ),
    ];
    let mut systemd = implementation(
        "systemd",
        &["systemctl", "ps", "journalctl"],
        systemd_steps,
        ResultParserKind::ServiceStatus,
    );
    systemd.compatibility.service_managers = vec!["systemd".into()];

    let traditional_steps = vec![
        step(
            "status",
            "服务状态",
            45,
            r#"for qz_service in {{services}}; do printf '== %s ==\n' "$qz_service"; service "$qz_service" status 2>&1 || true; done"#,
        ),
        step(
            "process",
            "服务相关进程",
            30,
            r#"for qz_service in {{services}}; do ps -eo pid,ppid,user,%cpu,%mem,stat,etime,args | grep -F -- "$qz_service" | grep -v '[g]rep -F' | head -n 100; done"#,
        ),
        step(
            "logs",
            "服务日志能力",
            30,
            "printf '%s\n' '__QZ_UNSUPPORTED__ per_service_journal'",
        ),
        step(
            "ports",
            "监听端口摘要",
            30,
            r#"if command -v ss >/dev/null 2>&1; then ss -H -lntup | head -n 200; elif command -v netstat >/dev/null 2>&1; then netstat -lntup | head -n 200; fi"#,
        ),
    ];
    let mut traditional = implementation(
        "service",
        &["service", "ps", "grep", "head"],
        traditional_steps,
        ResultParserKind::ServiceStatus,
    );
    traditional.compatibility.service_managers = vec!["service".into()];

    runbook(
        "runbook.service.incident",
        "指定服务故障排查",
        "对已发现的一个或多个服务检查状态、进程、日志和端口",
        150,
        parameters,
        vec![systemd, traditional],
    )
}

fn runbook(
    id: &str,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
) -> TaskDefinition {
    read_only_task(
        id,
        TaskCategory::Advanced,
        title,
        description,
        estimated_seconds,
        parameters,
        implementations,
    )
}

fn implementation(
    id: &str,
    required_commands: &[&str],
    steps: Vec<TaskStep>,
    parser: ResultParserKind,
) -> TaskImplementation {
    read_only_implementation(id, required_commands, steps, parser)
}

fn step(id: &str, title: &str, seconds: u64, command: &str) -> TaskStep {
    bounded_step(id, title, seconds, command)
}
