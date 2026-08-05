use crate::core::tasks::model::{
    OutputKind, ResultParserKind, TaskCategory, TaskDefinition, TaskImplementation,
};

use super::helpers::{bounded_step, read_only_implementation, read_only_task};

pub(super) fn tasks() -> Vec<TaskDefinition> {
    let implementations = vec![
        implementation(
            "nginx",
            &["nginx", "ss", "head"],
            r#"printf '== config ==\n'; nginx -t 2>&1; qz_rc=$?; printf '== listeners ==\n'; ss -H -lntp '( sport = :80 or sport = :443 )' 2>/dev/null | head -n 200 || true; exit "$qz_rc""#,
        ),
        implementation(
            "apache",
            &["apachectl", "ss", "head"],
            r#"printf '== config ==\n'; apachectl configtest 2>&1; qz_rc=$?; printf '== listeners ==\n'; ss -H -lntp '( sport = :80 or sport = :443 )' 2>/dev/null | head -n 200 || true; exit "$qz_rc""#,
        ),
    ];
    let mut task = read_only_task(
        "web.config_check",
        TaskCategory::Web,
        "Web 配置检查",
        "检查 Nginx 或 Apache 配置语法及 80/443 监听情况",
        45,
        Vec::new(),
        implementations,
    );
    task.output_kind = OutputKind::KeyValue;
    vec![task]
}

fn implementation(id: &str, commands: &[&str], command: &str) -> TaskImplementation {
    read_only_implementation(
        id,
        commands,
        vec![bounded_step("check", "检查 Web 配置", 45, command)],
        ResultParserKind::HealthSummary,
    )
}
