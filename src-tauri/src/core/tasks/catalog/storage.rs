use crate::core::tasks::model::{
    BackupItemKind, OutputKind, ParameterDefinition, ResultParserKind, TaskCategory, TaskDefinition,
};

use super::helpers::{
    absolute_path_parameter, backup_item, bounded_step, dangerous_implementation, dangerous_task,
    enum_parameter, integer_parameter, read_only_implementation, read_only_task,
};

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![
        diagnostic(
            "storage.mounts_inode",
            "挂载与 inode",
            "查看挂载点、文件系统容量和 inode 使用率",
            30,
            Vec::new(),
            &["df"],
            30,
            r#"printf '== mounts ==\n'; if command -v findmnt >/dev/null 2>&1; then findmnt -rn -o TARGET,SOURCE,FSTYPE,OPTIONS | head -n 500; else sed -n '1,500p' /proc/mounts; fi; printf '== inode ==\n'; df -Pi"#,
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        diagnostic(
            "storage.io_latency",
            "磁盘 I/O 延迟",
            "短时采样磁盘吞吐、队列和等待指标",
            45,
            vec![integer_parameter(
                "samples",
                "采样次数",
                "每秒一次，最多五次",
                1,
                5,
                3,
            )],
            &[],
            45,
            r#"if command -v iostat >/dev/null 2>&1; then iostat -xz 1 {{samples}}; elif command -v vmstat >/dev/null 2>&1; then vmstat 1 {{samples}}; printf '== diskstats ==\n'; sed -n '1,200p' /proc/diskstats; else printf '%s\n' '__QZ_UNSUPPORTED__ io_sampler'; sed -n '1,200p' /proc/diskstats; fi"#,
            ResultParserKind::HealthSummary,
            OutputKind::KeyValue,
        ),
        diagnostic(
            "storage.large_directories",
            "大目录定位",
            "在指定挂载范围内查找占用空间较大的目录",
            120,
            vec![
                absolute_path_parameter("path", "起始目录", "只扫描这个绝对路径所在的文件系统"),
                integer_parameter("depth", "目录深度", "向下统计的最大目录层级", 1, 5, 2),
                integer_parameter("limit", "结果上限", "最多显示的目录数", 10, 200, 50),
            ],
            &["du", "sort", "head"],
            120,
            r#"du -x -B1 --max-depth={{depth}} -- {{path}} 2>/dev/null | sort -nr -k1,1 | head -n {{limit}}"#,
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        diagnostic(
            "storage.deleted_open_files",
            "已删除未释放文件",
            "查找已删除但仍被进程占用的文件",
            45,
            vec![integer_parameter(
                "limit",
                "结果上限",
                "最多显示的文件数",
                10,
                500,
                100,
            )],
            &[],
            45,
            r#"if command -v lsof >/dev/null 2>&1; then lsof -nP +L1 2>/dev/null | head -n {{limit}}; else printf '%s\n' '__QZ_UNSUPPORTED__ lsof'; fi"#,
            ResultParserKind::Table,
            OutputKind::Table,
        ),
        swap_manage(),
    ]
}

fn swap_manage() -> TaskDefinition {
    dangerous_task(
        "storage.swap_manage",
        TaskCategory::Storage,
        "管理 Swap",
        "创建、启用、停用或移除单个受控 Swap 文件，并保留原状态和 fstab",
        90,
        vec![
            enum_parameter(
                "action",
                "操作",
                "选择创建、启用、停用或移除",
                &["create", "enable", "disable", "remove"],
                None,
            ),
            absolute_path_parameter("path", "Swap 文件", "单个受控 Swap 文件绝对路径"),
            integer_parameter(
                "sizeMiB",
                "容量 MiB",
                "创建 Swap 时使用，范围 64 MiB 到 32 GiB",
                64,
                32_768,
                1024,
            ),
        ],
        vec![dangerous_implementation(
            "posix-swap",
            &[],
            &["swapon", "swapoff", "mkswap", "stat"],
            r#"swapon --show --bytes; grep -F -- {{path}} /etc/fstab || true; if test -e {{path}}; then stat -- {{path}}; fi"#,
            vec![
                backup_item(
                    "swap-state-before",
                    BackupItemKind::CommandSnapshot,
                    r#"test ! -L {{path}}; printf 'path=%s\n' {{path}}; if test -e {{path}}; then test -f {{path}}; printf 'exists=true\nsize=%s\n' "$(stat -Lc %s -- {{path}})"; else printf 'exists=false\nsize=0\n'; fi; if swapon --show=NAME --noheadings | grep -Fx -- {{path}} >/dev/null 2>&1; then printf 'active=true\n'; else printf 'active=false\n'; fi"#,
                ),
                backup_item("fstab-before", BackupItemKind::RemoteFile, "/etc/fstab"),
            ],
            r#"test ! -L {{path}} && case {{action}} in 'create') test ! -e {{path}} && case {{path}} in '/var/lib/qingzhou/swap/'*) mkdir -p -- '/var/lib/qingzhou/swap';; esac && fallocate -l {{sizeMiB}}M -- {{path}} && chmod 0600 -- {{path}} && mkswap -- {{path}} && swapon -- {{path}};; 'enable') test -f {{path}} && swapon -- {{path}};; 'disable') test -f {{path}} && swapoff -- {{path}};; 'remove') test -f {{path}} && (swapoff -- {{path}} 2>/dev/null || true) && rm -f -- {{path}};; *) exit 64;; esac"#,
            r#"swapon --show --bytes; if test {{action}} = 'enable' -o {{action}} = 'create'; then swapon --show=NAME --noheadings | grep -Fx -- {{path}}; else ! swapon --show=NAME --noheadings | grep -Fx -- {{path}}; fi"#,
            "{{restore:swap-state-before}}; {{restore:file:fstab-before}}",
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
        TaskCategory::Storage,
        title,
        description,
        estimated_seconds,
        parameters,
        vec![implementation],
    );
    task.output_kind = output_kind;
    task
}
