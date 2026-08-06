use std::collections::BTreeSet;

use crate::core::tasks::TaskRemediationSummary;

pub fn remediation_for(
    package_manager: Option<&str>,
    missing_commands: &[String],
) -> Option<TaskRemediationSummary> {
    if missing_commands.is_empty() {
        return None;
    }
    let package_manager = package_manager?;
    let mut packages = BTreeSet::new();
    for command in missing_commands {
        packages.insert(package_for(package_manager, command)?);
    }

    let mut normalized_commands = missing_commands.to_vec();
    normalized_commands.sort();
    normalized_commands.dedup();
    Some(TaskRemediationSummary {
        package_manager: package_manager.into(),
        missing_commands: normalized_commands,
        packages: packages.into_iter().map(str::to_string).collect(),
    })
}

fn package_for(package_manager: &str, command: &str) -> Option<&'static str> {
    match (package_manager, command) {
        ("apt", "nc" | "ncat") => Some("netcat-openbsd"),
        ("dnf" | "yum", "nc" | "ncat") => Some("nmap-ncat"),
        ("apt" | "dnf" | "yum", "tcpdump") => Some("tcpdump"),
        ("apt" | "dnf" | "yum", "lsof") => Some("lsof"),
        ("apt" | "dnf" | "yum", "iostat") => Some("sysstat"),
        ("apt", "dig") => Some("dnsutils"),
        ("dnf" | "yum", "dig") => Some("bind-utils"),
        _ => None,
    }
}
