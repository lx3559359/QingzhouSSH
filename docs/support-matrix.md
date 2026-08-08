# 支持矩阵

## 桌面客户端

| 项目 | 支持状态 | 说明 |
| --- | --- | --- |
| Windows 10/11 x86_64 | 支持 | NSIS 安装版、便携版和在线更新已具备完整冒烟链路；需要 WebView2 Runtime |
| Windows 11 ARM64 | 发布候选 | 原生 NSIS、便携版和在线更新已进入六平台 CI；正式支持以首次标签构建及 ARM64 实机安装/凭据/更新冒烟通过为准 |
| macOS 13+ x86_64 / Apple Silicon | 发布候选 | 原生 DMG、Keychain、数据目录和 `.app.tar.gz` 在线更新已实现；正式支持以首次标签构建及两架构实机冒烟通过为准 |
| Ubuntu 22.04+ x86_64 / ARM64 | 发布候选 | 原生 AppImage、Secret Service、XDG 数据目录和在线更新已实现；正式支持以首次标签构建及两架构实机冒烟通过为准 |
| 其他 Windows、macOS 或 Linux 版本 | 尽力兼容 | 未进入固定发布矩阵，不承诺安装器、系统密钥环或 WebView 运行时兼容性 |

六个固定发布键为 `windows-x86_64-nsis`、`windows-aarch64-nsis`、`macos-x86_64-dmg`、`macos-aarch64-dmg`、`linux-x86_64-appimage` 和 `linux-aarch64-appimage`。Windows 使用 NSIS 当前用户安装；macOS 使用 DMG；Linux 使用 AppImage。标签发布必须在对应原生运行器构建、签名、汇总并逐平台验签，任一目标失败都会阻断整次发布。

“发布候选”表示代码、构建矩阵和发布契约已就绪，不等于已在本轮声明所有真实硬件与桌面环境均完成验收。企业安全策略、代理、SmartScreen、Gatekeeper、桌面 Secret Service 和系统 WebView 运行时不由项目控制。

## Auto detection（远端系统自动识别）

工具读取 `/etc/os-release`，再探测 `apt`、`dnf`、`yum`、`systemctl`、`service` 和任务所需命令。系统名称只是分类输入，真正能否执行某项功能以实时能力探测为准。

| 系统 ID / 系列 | 映射 | 验证状态 |
| --- | --- | --- |
| Ubuntu、Debian | Debian | 已有解析与真实 Ubuntu SSH 夹具测试 |
| Kylin（银河麒麟）、UOS | Debian | 已有国产系统探测夹具测试 |
| RHEL、CentOS、Rocky、AlmaLinux | RHEL | Rocky 有探测夹具；同族按 `ID`/`ID_LIKE` 与命令能力映射 |
| Anolis | RHEL | 已有国产系统探测夹具测试 |
| openEuler | openEuler | 独立系统族，已有探测夹具测试 |
| 其他 Linux | unknown | 可建立连接并显示探测结果；缺少能力的快捷任务会禁用或拒绝，不猜测包管理器 |
| FreeBSD、OpenBSD、NetBSD、DragonFly BSD | BSD | 通过 `uname`、`pkg`/`service` 和命令能力探测；系统概览、磁盘、进程和服务清单使用固定 POSIX/BSD 适配器 |
| Windows Server（OpenSSH + PowerShell） | Windows | 使用带边界的 PowerShell JSON 探测；系统概览、磁盘、进程和服务清单使用固定编码命令适配器 |
| 其他远端系统 | unknown | 仅保留 SSH/SFTP 基础能力；快捷任务必须有明确能力匹配，否则禁用 |

远端 Linux/BSD 架构通过 `uname -m` 展示，Windows 架构由 PowerShell 探测。SSH/SFTP 本身不限定 x86_64；快捷任务始终以系统族、远端 Shell、服务管理器和实时命令能力共同决定是否可用。

## SSH、日志与文件

- 支持密码认证，以及 PEM/OpenSSH 格式的带口令私钥；当前不代理 Pageant、Windows OpenSSH Agent 或硬件令牌。
- 首次主机密钥必须人工批准；指纹变化强制阻断。
- 日志检索支持普通 `.log` 和 `.gz`，远端需要相应的 `grep`/`gzip`/`awk` 等能力。
- 文件传输使用 SFTP；支持 POSIX 根目录与 Windows SFTP 盘符根目录。远端账号权限、SELinux、目录 ACL 和磁盘配额仍由服务器控制。
- 已认证 SSH 连接按服务器复用，各任务仍使用独立通道；失效连接会被淘汰，主机指纹和认证边界不变。
- 服务任务优先使用探测到的 `systemctl` 或 `service`；工具不会自动提权，也不假定用户拥有 sudo/root 权限。

## Operations Engine V2 当前支持范围

| 能力 | 当前状态 | 说明 |
| --- | --- | --- |
| 自动匹配实现 | 基础可用 | 依据系统族、服务管理器和远端命令能力选择后端固定实现；不根据系统名称猜测命令 |
| safe/caution 任务链 | 基础可用 | 支持预检、公开预览、受控执行、步骤/执行记录关联和失败状态 |
| dangerous 修改 | 暂未开放 | 当前仅生成只读预检与确认准备，不执行启动、停止、重启或配置修改 |
| 当前用户权限 | 支持 | 使用已登录 SSH 用户的现有权限，不自动扩大权限 |
| root / 免密 sudo | 策略已定义 | 后续任务只能使用 root 或 `sudo -n`；交互式 sudo、密码代填和权限绕过不支持 |
| 批量服务器 | 仅只读范围 | 任务必须同时标记为 safe 和 `read_only_batch`；修改类任务固定为单服务器 |

任务类别模型已覆盖系统、存储、网络、安全、服务、日志、Web、容器、脚本和高级操作。当前目录仍以原有系统/服务/日志任务的 V2 兼容桥接为主；新增目录任务会在后续阶段逐项加入兼容性、权限和真实发行版测试，不应把类别枚举理解为功能已经全部开放。

## 明确不支持

- 交互式 SSH 终端、PTY、持续 stdin、全屏 TUI；
- 自动获取管理员/root 权限或绕过服务器权限；
- 任意第三方插件节点和未审核的动态脚本市场；
- 静默自动安装更新或在用户未确认时重启应用。
