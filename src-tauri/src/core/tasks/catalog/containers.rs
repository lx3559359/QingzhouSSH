use crate::core::tasks::model::{
    BackupItemKind, OutputKind, ResultParserKind, TaskCategory, TaskDefinition, TaskImplementation,
};

use super::helpers::{
    backup_item, bounded_step, container_parameter, dangerous_implementation, dangerous_task,
    enum_parameter, integer_parameter, read_only_implementation, read_only_task,
};

pub(super) fn tasks() -> Vec<TaskDefinition> {
    vec![health_storage(), inspect(), container_action()]
}

fn container_action() -> TaskDefinition {
    let implementations = ["docker", "podman"]
        .into_iter()
        .map(|runtime| {
            let preview = format!("{runtime} inspect -- {{{{container}}}}");
            let execute = format!("{runtime} {{{{action}}}} -- {{{{container}}}}");
            let snapshot = container_snapshot_command(runtime);
            let verify = container_verify_command(runtime);
            dangerous_implementation(
                runtime,
                &[],
                &[runtime],
                &preview,
                vec![backup_item(
                    "container-state-before",
                    BackupItemKind::RuntimeState,
                    &snapshot,
                )],
                &execute,
                &verify,
                &format!("{{{{restore:container:{runtime}:container-state-before}}}}"),
                ResultParserKind::ContainerStatus,
            )
        })
        .collect();
    dangerous_task(
        "container.action",
        TaskCategory::Container,
        "控制容器状态",
        "只对已发现的单个容器执行启动、停止、重启、暂停或继续，并保存原状态",
        75,
        vec![
            container_parameter(),
            enum_parameter(
                "action",
                "容器动作",
                "选择一个受控状态动作",
                &["start", "stop", "restart", "pause", "unpause"],
                None,
            ),
        ],
        implementations,
        OutputKind::KeyValue,
    )
}

fn container_snapshot_command(runtime: &str) -> String {
    format!(
        r#"qz_format=$(printf '{{%s}}' '{{.State.Status}}'); qz_state=$({runtime} inspect --format "$qz_format" -- {{{{container}}}}) || exit; case "$qz_state" in exited|stopped) qz_state=stopped;; running|paused) :;; *) exit 65;; esac; printf 'runtime={runtime}\ncontainer=%s\nstate=%s\n' {{{{container}}}} "$qz_state""#
    )
}

fn container_verify_command(runtime: &str) -> String {
    format!(
        r#"qz_action={{{{action}}}}; qz_format=$(printf '{{%s}}' '{{.State.Status}}'); qz_state=$({runtime} inspect --format "$qz_format" -- {{{{container}}}}) || exit; case "$qz_action:$qz_state" in start:running|restart:running|stop:exited|stop:stopped|pause:paused|unpause:running) exit 0;; *) printf 'expected action=%s, actual state=%s\n' "$qz_action" "$qz_state" >&2; exit 1;; esac"#
    )
}

fn health_storage() -> TaskDefinition {
    let implementations = ["docker", "podman"]
        .into_iter()
        .map(|runtime| {
            let command = format!(
                r#"printf '== version ==\n'; {runtime} version 2>&1 | head -n 100; printf '== containers ==\n'; {runtime} ps -a --no-trunc 2>&1 | head -n 101; printf '== stats ==\n'; {runtime} stats --no-stream 2>&1 | head -n 101; printf '== storage ==\n'; {runtime} system df 2>&1 | head -n 200"#
            );
            implementation(runtime, runtime, 60, &command)
        })
        .collect();
    let mut task = read_only_task(
        "container.health_storage",
        TaskCategory::Container,
        "容器健康与存储",
        "查看容器运行时、容器状态、资源和存储占用",
        60,
        Vec::new(),
        implementations,
    );
    task.output_kind = OutputKind::KeyValue;
    task
}

fn inspect() -> TaskDefinition {
    let implementations = ["docker", "podman"]
        .into_iter()
        .map(|runtime| {
            let command = format!(
                r#"qz_container={{{{container}}}}; qz_action={{{{action}}}}; qz_lines={{{{lines}}}}; case "$qz_action" in logs) {runtime} logs --tail "$qz_lines" -- "$qz_container" 2>&1;; inspect) {runtime} inspect -- "$qz_container";; stats) {runtime} stats --no-stream -- "$qz_container";; *) printf '%s\n' '__QZ_ERROR__ invalid_container_action'; exit 4;; esac"#
            );
            implementation(runtime, runtime, 60, &command)
        })
        .collect();
    let mut task = read_only_task(
        "container.inspect",
        TaskCategory::Container,
        "容器详情",
        "查看已发现容器的日志、配置或单次资源快照",
        60,
        vec![
            container_parameter(),
            enum_parameter(
                "action",
                "查看内容",
                "选择日志、配置或资源快照",
                &["logs", "inspect", "stats"],
                Some("logs"),
            ),
            integer_parameter("lines", "日志行数", "日志操作最多返回的行数", 10, 5000, 200),
        ],
        implementations,
    );
    task.output_kind = OutputKind::KeyValue;
    task
}

fn implementation(
    id: &str,
    executable: &str,
    timeout_seconds: u64,
    command: &str,
) -> TaskImplementation {
    read_only_implementation(
        id,
        &[executable, "head"],
        vec![bounded_step(
            "collect",
            "采集容器诊断",
            timeout_seconds,
            command,
        )],
        ResultParserKind::ContainerStatus,
    )
}
