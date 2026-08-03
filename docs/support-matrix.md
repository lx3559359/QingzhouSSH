# 支持矩阵

## Windows 客户端

| 项目 | 支持状态 | 说明 |
| --- | --- | --- |
| Windows 11 x86_64 | 支持 | 安装版、便携版和在线更新的主要目标 |
| Windows 10 x86_64 | 支持 | 需要系统可用的 WebView2 Runtime |
| Windows ARM64 | 暂不支持 | 当前只发布 `windows-x86_64` 更新包 |
| macOS / Linux 客户端 | 暂不支持 | 当前 UI、DPAPI、注册表和安装器均为 Windows 实现 |

安装版为 NSIS 当前用户安装，不要求管理员权限。Windows 的系统级 WebView2 安装、企业安全策略、代理和 SmartScreen 不由本项目控制。

## Auto detection（远端 Linux 自动识别）

工具读取 `/etc/os-release`，再探测 `apt`、`dnf`、`yum`、`systemctl`、`service` 和任务所需命令。系统名称只是分类输入，真正能否执行某项功能以实时能力探测为准。

| 系统 ID / 系列 | 映射 | 验证状态 |
| --- | --- | --- |
| Ubuntu、Debian | Debian | 已有解析与真实 Ubuntu SSH 夹具测试 |
| Kylin（银河麒麟）、UOS | Debian | 已有国产系统探测夹具测试 |
| RHEL、CentOS、Rocky、AlmaLinux | RHEL | Rocky 有探测夹具；同族按 `ID`/`ID_LIKE` 与命令能力映射 |
| Anolis | RHEL | 已有国产系统探测夹具测试 |
| openEuler | openEuler | 独立系统族，已有探测夹具测试 |
| 其他 Linux | unknown | 可建立连接并显示探测结果；缺少能力的快捷任务会禁用或拒绝，不猜测包管理器 |

远端架构会通过 `uname -m` 展示。SSH 命令本身不限定 x86_64，但发布验收主要覆盖 x86_64 Linux 夹具；在 aarch64/ARM 服务器上应先用低风险系统概览确认命令兼容性。

## SSH、日志与文件

- 支持密码认证，以及 PEM/OpenSSH 格式的带口令私钥；当前不代理 Pageant、Windows OpenSSH Agent 或硬件令牌。
- 首次主机密钥必须人工批准；指纹变化强制阻断。
- 日志检索支持普通 `.log` 和 `.gz`，远端需要相应的 `grep`/`gzip`/`awk` 等能力。
- 文件传输使用 SFTP；远端账号权限、SELinux、目录 ACL 和磁盘配额仍由服务器控制。
- 服务任务优先使用探测到的 `systemctl` 或 `service`；工具不会自动提权，也不假定用户拥有 sudo/root 权限。

## 明确不支持

- 交互式 SSH 终端、PTY、持续 stdin、全屏 TUI；
- 自动获取管理员/root 权限或绕过服务器权限；
- 任意第三方插件节点和未审核的动态脚本市场；
- 静默自动安装更新或在用户未确认时重启应用。
