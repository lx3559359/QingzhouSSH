# QingzhouSSH（轻舟 SSH）

QingzhouSSH 是一款面向 Windows 的图形化服务器快捷运维工具。它通过 SSH 执行经过约束的快捷任务、脚本、日志检索、SFTP 文件传输和可视化工作流，但不提供交互式终端、PTY 或持续 stdin。

> **No SSH Terminal**：这是“无 SSH 终端”的安全边界，不是终端模拟器，也不会把远程 Shell 直接暴露给界面。

## 核心能力

- 自动探测 Ubuntu、Debian、RHEL 系、openEuler、UOS、Kylin（银河麒麟）和 Anolis 等 Linux 系统及可用命令。
- Windows DPAPI 用户级凭据保护；首次连接核对主机指纹，指纹变化时在认证前阻断。
- **Quick tasks**：系统概览、磁盘、进程、服务状态与启停等参数化任务。
- **Log search and download**：检索远程 `.log`/`.gz`，分页预览，并下载到所选数据目录。
- **SFTP**：分块上传/下载、单调进度、SHA-256 校验、临时文件和原子完成。
- **Workflow**：八类节点的可视化编排、条件分支、失败暂停、同节点重试、恢复点、逆序回滚和脱敏诊断包。
- 危险任务、自定义命令/脚本、服务启停和回滚均要求明确二次确认。
- **Online update**：GitHub Releases 主源与 ModelScope 镜像；下载后同时校验 Tauri 更新签名、SHA-256 和大小。

## 下载与安装

- [GitHub Releases](https://github.com/lx3559359/QingzhouSSH/releases/latest)：主下载入口、源码、Issue 和安全公告。
- [ModelScope](https://modelscope.cn/studios)：国内镜像入口；在 Studio 中搜索 `QingzhouSSH`，项目创建后 README 会补充直达地址。

Windows x64 安装版使用 NSIS `currentUser` 模式，不需要管理员权限。便携版解压后保留 `QingzhouSSH.exe` 旁的 `portable.flag`，应用数据会进入同目录的 `data`。

安装包和便携包均附带 `SHA256SUMS`、SPDX SBOM 与 Apache-2.0 许可证。更新签名不等于 Windows Authenticode 代码签名；当前发布如未配置商业代码签名证书，Windows 可能显示“未知发布者”。

## 第一次使用

1. 安装版首次启动时选择 **Data root（数据根目录）**；应用不会回退到 `%APPDATA%` 或 `%LOCALAPPDATA%` 保存业务数据。
2. 添加服务器地址、端口、用户名和密码或带口令 OpenSSH 私钥。
3. 第一次连接时，在可信渠道核对界面显示的 SHA-256 主机指纹后再批准。
4. 选择快捷任务、日志、文件传输或工作流；高风险操作按界面摘要二次确认。
5. 在“设置”页手工检查、下载并安装更新；工具不会静默安装更新。

完整操作见 [用户指南](docs/user-guide.md)，数据、升级、回退与卸载见 [数据与更新](docs/data-and-updates.md)，系统范围见 [支持矩阵](docs/support-matrix.md)。

## 安全与隐私

- 密码、私钥和私钥口令使用当前 Windows 用户的 DPAPI 加密，跨用户或跨电脑复制后需要重新录入。
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
