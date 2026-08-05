use crate::core::tasks::model::{
    BackupItemKind, ExecutionScope, OutputKind, ParameterDefinition, ParameterKind,
    ResultParserKind, RiskLevel, TaskCategory, TaskDefinition, TaskImplementation,
};

use super::helpers::{
    backup_item, boolean_parameter, bounded_step, dangerous_implementation, dangerous_task,
    enum_parameter, host_parameter, integer_parameter, interface_parameter, parameter,
    port_parameter, read_only_implementation, read_only_task,
};

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![
        diagnostic(
            "network.interface_health",
            "网卡健康",
            "查看接口状态、地址、丢包和错误计数",
            30,
            Vec::new(),
            vec![implementation(
                "iproute2",
                &["ip"],
                30,
                r#"printf '== links ==\n'; ip -brief link; printf '== addresses ==\n'; ip -brief address; printf '== counters ==\n'; ip -s link"#,
                ResultParserKind::HealthSummary,
            )],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "network.tcp_states",
            "TCP 连接状态",
            "汇总 TCP 状态并查看有限数量的已建立连接",
            30,
            vec![integer_parameter(
                "limit",
                "结果上限",
                "最多显示的已建立连接数",
                20,
                500,
                100,
            )],
            vec![implementation(
                "ss",
                &["ss", "head"],
                30,
                r#"printf '== summary ==\n'; ss -s; printf '== established ==\n'; ss -Htn state established | head -n {{limit}}"#,
                ResultParserKind::Table,
            )],
            OutputKind::Table,
        ),
        diagnostic(
            "network.listening_ports",
            "监听端口",
            "查看监听地址、端口和关联进程",
            30,
            vec![integer_parameter(
                "limit",
                "结果上限",
                "最多显示的监听项",
                20,
                500,
                100,
            )],
            vec![
                implementation(
                    "ss",
                    &["ss", "head"],
                    30,
                    "ss -H -lntup | head -n {{limit}}",
                    ResultParserKind::Table,
                ),
                implementation(
                    "netstat",
                    &["netstat", "head"],
                    30,
                    "netstat -lntup | head -n {{limit}}",
                    ResultParserKind::Table,
                ),
            ],
            OutputKind::Table,
        ),
        diagnostic(
            "network.port_process",
            "端口进程",
            "查看指定本地端口的监听进程",
            30,
            vec![port_parameter(
                "port",
                "端口",
                "要检查的本地端口",
                true,
                None,
            )],
            vec![
                implementation(
                    "ss",
                    &["ss", "awk"],
                    30,
                    r#"ss -H -lntup | awk -v qz_port={{port}} '{ local=$5; sub(/^.*:/,"",local); if (local == qz_port) print }'"#,
                    ResultParserKind::Table,
                ),
                implementation(
                    "netstat",
                    &["netstat", "awk"],
                    30,
                    r#"netstat -lntup | awk -v qz_port={{port}} 'NR > 2 { local=$4; sub(/^.*:/,"",local); if (local == qz_port) print }'"#,
                    ResultParserKind::Table,
                ),
            ],
            OutputKind::Table,
        ),
        diagnostic(
            "network.ip_route",
            "地址与路由",
            "查看 IP 地址、主路由、策略路由、邻居和 MTU",
            30,
            Vec::new(),
            vec![implementation(
                "iproute2",
                &["ip"],
                30,
                r#"printf '== address ==\n'; ip -brief address; printf '== route ==\n'; ip route show table main; printf '== rules ==\n'; ip rule show; printf '== neighbours ==\n'; ip neigh show; printf '== mtu ==\n'; ip -o link show"#,
                ResultParserKind::Table,
            )],
            OutputKind::Table,
        ),
        diagnostic(
            "network.dns",
            "DNS 解析",
            "查看解析器配置和指定主机的解析结果",
            30,
            vec![host_parameter("host", "主机", "要解析的主机名或 IP", true)],
            vec![implementation(
                "libc",
                &["getent"],
                30,
                r#"printf '== resolv.conf ==\n'; sed -n '1,120p' /etc/resolv.conf; printf '== getent ==\n'; getent ahosts {{host}} | head -n 100; printf '== resolver ==\n'; if command -v resolvectl >/dev/null 2>&1; then resolvectl query {{host}} 2>/dev/null | head -n 100 || true; elif command -v dig >/dev/null 2>&1; then dig +time=3 +tries=1 {{host}} A {{host}} AAAA | head -n 160; fi"#,
                ResultParserKind::NetworkProbe,
            )],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "network.connectivity",
            "网络连通性",
            "对指定主机执行有界 ping 和可选路径探测",
            45,
            vec![
                host_parameter("host", "目标主机", "要探测的主机名或 IP", true),
                integer_parameter("count", "探测次数", "ICMP 探测包数量", 1, 20, 4),
            ],
            vec![implementation(
                "ping",
                &["ping"],
                45,
                r#"printf '== ping ==\n'; ping -n -c {{count}} -W 3 -- {{host}}; qz_ping=$?; printf '== path ==\n'; if command -v tracepath >/dev/null 2>&1; then timeout 20 tracepath -n {{host}} 2>/dev/null | head -n 40 || true; fi; if test "$qz_ping" -ne 0; then printf '%s\n' '__QZ_WARNING__ connectivity_failed'; fi"#,
                ResultParserKind::NetworkProbe,
            )],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "network.http",
            "HTTP 探测",
            "只探测目标根路径的响应头，不接受自定义 URL",
            30,
            vec![
                host_parameter("host", "目标主机", "Web 服务主机名或 IP", true),
                port_parameter("port", "目标端口", "Web 服务端口", false, Some(80)),
                boolean_parameter("tls", "使用 HTTPS", "启用 TLS 连接", false),
            ],
            vec![implementation(
                "curl",
                &["curl"],
                30,
                r#"qz_host={{host}}; qz_port={{port}}; if {{tls}}; then qz_scheme=https; else qz_scheme=http; fi; case "$qz_host" in *:*) qz_authority="[$qz_host]";; *) qz_authority="$qz_host";; esac; curl --head --silent --show-error --location --max-redirs 3 --connect-timeout 5 --max-time 8 "$qz_scheme://$qz_authority:$qz_port/""#,
                ResultParserKind::NetworkProbe,
            )],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "network.tls",
            "TLS 证书",
            "查看证书主题、签发者、有效期和指纹",
            30,
            vec![
                host_parameter("host", "目标主机", "TLS 服务主机名或 IP", true),
                port_parameter("port", "目标端口", "TLS 服务端口", false, Some(443)),
            ],
            vec![implementation(
                "openssl",
                &["openssl", "timeout"],
                30,
                r#"qz_host={{host}}; qz_port={{port}}; case "$qz_host" in *:*) qz_target="[$qz_host]:$qz_port";; *) qz_target="$qz_host:$qz_port";; esac; timeout 12 openssl s_client -connect "$qz_target" -servername "$qz_host" </dev/null 2>/dev/null | openssl x509 -noout -subject -issuer -dates -serial -fingerprint -sha256"#,
                ResultParserKind::NetworkProbe,
            )],
            OutputKind::KeyValue,
        ),
        diagnostic(
            "network.udp",
            "UDP 探测",
            "结合监听、防火墙摘要和多次 UDP 探测判断可达性",
            60,
            vec![
                host_parameter("host", "目标主机", "UDP 服务主机名或 IP", true),
                port_parameter("port", "目标端口", "UDP 服务端口", true, None),
                integer_parameter("attempts", "尝试次数", "最多探测十次", 1, 10, 3),
                integer_parameter("timeout", "单次超时", "每次等待秒数", 1, 30, 3),
            ],
            vec![
                udp_implementation("nc", "nc"),
                udp_implementation("ncat", "ncat"),
            ],
            OutputKind::KeyValue,
        ),
        packet_capture(),
        hosts_manage(),
        ip_change(),
    ]
}

fn hosts_manage() -> TaskDefinition {
    dangerous_task(
        "network.hosts_manage",
        TaskCategory::Network,
        "管理 Hosts 映射",
        "只添加或移除带轻舟标识的单条映射，并保留完整 hosts 文件备份",
        60,
        vec![
            enum_parameter(
                "action",
                "操作",
                "添加或移除工具管理的映射",
                &["add", "remove"],
                None,
            ),
            host_parameter("address", "IP 地址", "映射目标 IPv4 或 IPv6 地址", true),
            host_parameter("hostname", "主机名", "要解析的主机名", true),
        ],
        vec![dangerous_implementation(
            "hosts-managed-block",
            &[],
            &["getent"],
            "test ! -L /etc/hosts; sed -n '1,300p' /etc/hosts; getent hosts {{hostname}} || true",
            vec![backup_item(
                "hosts-before",
                BackupItemKind::RemoteFile,
                "/etc/hosts",
            )],
            "{{managed:hosts:action}}",
            "test ! -L /etc/hosts; {{managed:hosts:verify}}; getent hosts {{hostname}} || true",
            "{{restore:file:hosts-before}}",
            ResultParserKind::NetworkProbe,
        )],
        OutputKind::KeyValue,
    )
}

fn ip_change() -> TaskDefinition {
    let parameters = vec![
        interface_parameter("interface", "网络接口", "从服务器接口中选择目标网卡"),
        parameter(
            "cidr",
            "新地址",
            "带前缀长度的 IPv4 或 IPv6 地址",
            ParameterKind::Cidr,
            true,
            None,
        ),
        host_parameter("gateway", "默认网关", "目标默认网关地址", true),
        integer_parameter(
            "rollbackSeconds",
            "自动恢复等待秒数",
            "修改后未验证成功时自动恢复，范围 60 到 300 秒",
            60,
            300,
            120,
        ),
    ];
    dangerous_task(
        "network.ip_change",
        TaskCategory::Network,
        "修改 IP 地址",
        "先安排超时自动恢复，再修改地址并通过独立连接核验",
        180,
        parameters,
        vec![
            ip_implementation(
                "network-manager",
                &["ip", "nmcli", "systemd-run"],
                vec![backup_item(
                    "network-before",
                    BackupItemKind::CommandSnapshot,
                    "ip -details address show dev {{interface}}; ip route show table main; nmcli -g all connection show",
                )],
            ),
            ip_implementation(
                "netplan",
                &["ip", "netplan", "systemd-run"],
                vec![
                    backup_item(
                        "network-before",
                        BackupItemKind::CommandSnapshot,
                        "ip -details address show dev {{interface}}; ip route show table main",
                    ),
                    backup_item(
                        "netplan-managed-before",
                        BackupItemKind::RemoteFile,
                        "/etc/netplan/99-qingzhou.yaml",
                    ),
                ],
            ),
            ip_implementation(
                "legacy-ifcfg",
                &["ip", "systemd-run"],
                vec![
                    backup_item(
                        "network-before",
                        BackupItemKind::CommandSnapshot,
                        "ip -details address show dev {{interface}}; ip route show table main",
                    ),
                    backup_item(
                        "ifcfg-before",
                        BackupItemKind::RemoteFile,
                        "/etc/sysconfig/network-scripts/ifcfg-{{interface}}",
                    ),
                ],
            ),
        ],
        OutputKind::KeyValue,
    )
}

fn ip_implementation(
    id: &str,
    required_commands: &[&str],
    backup_items: Vec<crate::core::tasks::model::BackupItemDefinition>,
) -> TaskImplementation {
    dangerous_implementation(
        id,
        &[],
        required_commands,
        "ip -details address show dev {{interface}}; ip route show table main",
        backup_items,
        "{{managed:network:arm-rollback}}; {{managed:network:apply}}",
        "{{managed:network:verify-independent-connection}}",
        "{{restore:network:network-before}}",
        ResultParserKind::NetworkProbe,
    )
}

#[allow(clippy::too_many_arguments)]
fn diagnostic(
    id: &str,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
    output_kind: OutputKind,
) -> TaskDefinition {
    let mut task = read_only_task(
        id,
        TaskCategory::Network,
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
    required_commands: &[&str],
    timeout_seconds: u64,
    command: &str,
    parser: ResultParserKind,
) -> TaskImplementation {
    read_only_implementation(
        id,
        required_commands,
        vec![bounded_step(
            "collect",
            "采集网络诊断",
            timeout_seconds,
            command,
        )],
        parser,
    )
}

fn udp_implementation(id: &str, executable: &str) -> TaskImplementation {
    let command = format!(
        r#"qz_host={{{{host}}}}; qz_port={{{{port}}}}; qz_attempts={{{{attempts}}}}; qz_timeout={{{{timeout}}}}; printf '== listener ==\n'; if command -v ss >/dev/null 2>&1; then ss -H -lunp 2>/dev/null | awk -v qz_port="$qz_port" '{{ local=$5; sub(/^.*:/,"",local); if (local == qz_port) print }}' | head -n 50; fi; printf '== probe ==\n'; qz_i=1; qz_ok=0; while test "$qz_i" -le "$qz_attempts"; do if {executable} -z -u -w "$qz_timeout" "$qz_host" "$qz_port" >/dev/null 2>&1; then qz_ok=1; break; fi; qz_i=$((qz_i + 1)); done; if test "$qz_ok" -eq 1; then printf '%s\n' 'probe=reachable'; else printf '%s\n' 'probe=no_response' '__QZ_WARNING__ udp_inconclusive'; fi"#
    );
    implementation(
        id,
        &[executable, "awk", "head"],
        60,
        &command,
        ResultParserKind::NetworkProbe,
    )
}

fn packet_capture() -> TaskDefinition {
    let command = r#"qz_interface={{interface}}; qz_host={{?host}}; qz_port={{?port}}; qz_file="/tmp/qingzhou-pcap-$$.pcap"; trap 'rm -f -- "$qz_file"' EXIT HUP INT TERM; set --; if test -n "$qz_host"; then set -- "$@" host "$qz_host"; fi; if test -n "$qz_port"; then if test "$#" -gt 0; then set -- "$@" and port "$qz_port"; else set -- port "$qz_port"; fi; fi; if test "$#" -gt 0; then timeout {{seconds}} tcpdump -i "$qz_interface" -nn -s 128 -c {{count}} -w "$qz_file" -- "$@" >/dev/null 2>&1; else timeout {{seconds}} tcpdump -i "$qz_interface" -nn -s 128 -c {{count}} -w "$qz_file" >/dev/null 2>&1; fi; qz_rc=$?; if test "$qz_rc" -ne 0 -a "$qz_rc" -ne 124; then printf '%s\n' '__QZ_ERROR__ packet_capture_failed'; exit "$qz_rc"; fi; qz_size=$(wc -c < "$qz_file"); if test "$qz_size" -gt 16777216; then printf '%s\n' '__QZ_ERROR__ capture_too_large'; exit 5; fi; printf '__QZ_METRIC__ capture_bytes=%s\n' "$qz_size"; tcpdump -nn -r "$qz_file" -c {{count}} 2>/dev/null"#;
    let mut task = diagnostic(
        "network.packet_capture",
        "限时抓包摘要",
        "使用固定 host/port 过滤组合抓取有限数据包并返回摘要",
        45,
        vec![
            interface_parameter("interface", "网络接口", "要采集的网卡名称"),
            host_parameter("host", "主机过滤", "可选的固定主机过滤条件", false),
            port_parameter("port", "端口过滤", "可选的固定端口过滤条件", false, None),
            integer_parameter("count", "数据包数量", "最多采集的数据包数", 1, 200, 50),
            integer_parameter("seconds", "最长秒数", "最长采集时间", 1, 30, 10),
        ],
        vec![implementation(
            "tcpdump",
            &["tcpdump", "timeout", "wc"],
            45,
            command,
            ResultParserKind::NetworkProbe,
        )],
        OutputKind::Text,
    );
    task.risk_level = RiskLevel::Caution;
    task.scope = ExecutionScope::SingleServer;
    task
}
