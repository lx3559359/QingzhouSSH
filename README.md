# QingzhouSSH（轻舟 SSH）

QingzhouSSH 是一款面向 Windows、macOS 与 Linux 的图形化服务器快捷运维工具。它通过 SSH 执行经过约束的快捷任务、脚本、日志检索、SFTP 文件传输和可视化工作流，但不提供交互式终端、PTY 或持续 stdin。

> **No SSH Terminal**：这是“无 SSH 终端”的安全边界，不是终端模拟器，也不会把远程 Shell 直接暴露给界面。

## 核心能力

- 自动探测 Linux、BSD 与 Windows Server（OpenSSH + PowerShell）及其可用命令。
- Windows DPAPI、macOS Keychain、Linux Secret Service 凭据保护；首次连接核对主机指纹，指纹变化时在认证前阻断。
- **Quick tasks**：系统概览、磁盘、进程、服务状态与启停等参数化任务；脚本支持 Bash、POSIX sh 与 PowerShell 能力匹配。
- **Log search and download**：检索远程 `.log`/`.gz`，分页预览，并下载到所选数据目录。
- **SFTP**：复用可信 SSH 连接、持久队列、有界并发与流水线、速度/ETA、均衡或严格 SHA-256 校验，以及安全的新建、重命名和删除操作。
- **Workflow**：八类节点的可视化编排、条件分支、失败暂停、同节点重试、恢复点、逆序回滚和脱敏诊断包。
- 危险任务、自定义命令/脚本、服务启停和回滚均要求明确二次确认。
- **Online update**：GitHub Releases 主源与 ModelScope 镜像；下载后同时校验 Tauri 更新签名、SHA-256 和大小。
- **Data root migration**：设置页可随时更改数据目录；退出后完整复制并逐文件校验，成功才切换，失败继续使用原目录，旧目录不自动删除。

## 下载与安装

- [GitHub Releases](https://github.com/lx3559359/QingzhouSSH/releases/latest)：主下载入口、源码、Issue 和安全公告。
- [ModelScope](https://modelscope.cn/models/lx3559359/QingzhouSSH)：国内发布镜像与同字节发布物；[Studio](https://modelscope.cn/studios/lx3559359/QingzhouSSH) 保留为项目展示页。

发布契约覆盖 Windows x86_64/ARM64（NSIS `currentUser` + 便携 ZIP）、macOS x86_64/Apple Silicon（DMG）和 Linux x86_64/ARM64（AppImage）。Windows x64 已完成完整冒烟；其余五个目标目前为发布候选，以首次标签原生构建和对应平台实机安装/凭据/更新冒烟通过为正式支持条件。详见 [支持矩阵](docs/support-matrix.md)。

Windows 便携版完整解压后，应保留 `QingzhouSSH.exe` 旁的 `portable.flag`；未另选目录时，业务数据会进入同目录的 `data`。

发布物附带 `SHA256SUMS`、SPDX SBOM 与 Apache-2.0 许可证。更新签名不等于 Windows Authenticode、Apple notarization 或 Linux 发行版仓库签名；系统仍可能按本机安全策略显示额外警告。

## 第一次使用

1. 首次启动时选择 **Data root（数据根目录）**；数据库、日志、下载、备份和更新文件只进入这个明确路径。
2. 添加服务器地址、端口、用户名和密码或带口令 OpenSSH 私钥。
3. 第一次连接时，在可信渠道核对界面显示的 SHA-256 主机指纹后再批准。
4. 选择快捷任务、日志、文件传输或工作流；高风险操作按界面摘要二次确认。
5. 在“设置”页手工检查、下载并安装更新；工具不会静默安装更新。

完整操作见 [用户指南](docs/user-guide.md)，数据、升级、回退与卸载见 [数据与更新](docs/data-and-updates.md)，系统范围见 [支持矩阵](docs/support-matrix.md)。

## 安全与隐私

- 密码、私钥和私钥口令使用当前系统的 DPAPI/Keychain/Secret Service；跨用户或跨电脑后通常需要重新录入。
- 主机指纹变化会在发送凭据之前阻断连接。
- 运行事件、历史和诊断包会脱敏；自定义脚本文本不会出现在确认弹窗和诊断包中。
- 当前没有遥测和自动崩溃上传。
- 漏洞请按 [SECURITY.md](SECURITY.md) 私下报告，不要在公开 Issue 中粘贴凭据、私钥、服务器地址或利用细节。

技术边界见 [安全说明](docs/security.md)。

## 开发

本仓库的开发数据和构建产物全部约束在项目目录内。Windows PowerShell 会话先加载：

```powershell
& .\scripts\dev-env.ps1 -Quiet
pnpm install --frozen-lockfile
pnpm test
pnpm build
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
```

在当前工作区，项目位于 `D:\Codex Project\轻量化SSH快捷工具`；依赖、缓存、测试数据和产物只能进入其 `.local`、`target`、`dist`、`artifacts` 或用户明确选择的数据根目录。详见 [开发指南](docs/development.md)。

## 许可证

[Apache License 2.0](LICENSE)
