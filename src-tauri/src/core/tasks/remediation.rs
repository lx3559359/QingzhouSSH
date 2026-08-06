use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    core::tasks::TaskRemediationSummary,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageManagerKind {
    Apt,
    Dnf,
    Yum,
}

impl TryFrom<&str> for PackageManagerKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "apt" => Ok(Self::Apt),
            "dnf" => Ok(Self::Dnf),
            "yum" => Ok(Self::Yum),
            _ => Err(AppError::Security("包管理器不在内置白名单中".into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageId {
    NetcatOpenbsd,
    NmapNcat,
    Tcpdump,
    Lsof,
    Sysstat,
    Dnsutils,
    BindUtils,
}

impl PackageId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NetcatOpenbsd => "netcat-openbsd",
            Self::NmapNcat => "nmap-ncat",
            Self::Tcpdump => "tcpdump",
            Self::Lsof => "lsof",
            Self::Sysstat => "sysstat",
            Self::Dnsutils => "dnsutils",
            Self::BindUtils => "bind-utils",
        }
    }
}

impl TryFrom<&str> for PackageId {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "netcat-openbsd" => Ok(Self::NetcatOpenbsd),
            "nmap-ncat" => Ok(Self::NmapNcat),
            "tcpdump" => Ok(Self::Tcpdump),
            "lsof" => Ok(Self::Lsof),
            "sysstat" => Ok(Self::Sysstat),
            "dnsutils" => Ok(Self::Dnsutils),
            "bind-utils" => Ok(Self::BindUtils),
            _ => Err(AppError::Security("软件包不在内置白名单中".into())),
        }
    }
}

pub fn fixed_install_command(
    package_manager: PackageManagerKind,
    packages: &[PackageId],
) -> AppResult<String> {
    if packages.is_empty() {
        return Err(AppError::Validation("没有需要安装的软件包".into()));
    }
    let mut packages = packages.to_vec();
    packages.sort_by_key(|package| package.as_str());
    packages.dedup();
    if packages
        .iter()
        .any(|package| !package_allowed(package_manager, *package))
    {
        return Err(AppError::Security("软件包与服务器包管理器不匹配".into()));
    }
    let names = packages
        .into_iter()
        .map(PackageId::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(match package_manager {
        PackageManagerKind::Apt => {
            format!("apt-get install -y --no-install-recommends {names}")
        }
        PackageManagerKind::Dnf => format!("dnf install -y {names}"),
        PackageManagerKind::Yum => format!("yum install -y {names}"),
    })
}

fn package_allowed(package_manager: PackageManagerKind, package: PackageId) -> bool {
    match package_manager {
        PackageManagerKind::Apt => matches!(
            package,
            PackageId::NetcatOpenbsd
                | PackageId::Tcpdump
                | PackageId::Lsof
                | PackageId::Sysstat
                | PackageId::Dnsutils
        ),
        PackageManagerKind::Dnf | PackageManagerKind::Yum => matches!(
            package,
            PackageId::NmapNcat
                | PackageId::Tcpdump
                | PackageId::Lsof
                | PackageId::Sysstat
                | PackageId::BindUtils
        ),
    }
}

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
