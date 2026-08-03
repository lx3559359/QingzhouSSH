# QingzhouSSH（轻舟 SSH）

QingzhouSSH 是一个面向 Windows 的无交互式 SSH 终端工具。它把服务器连接与 Linux 能力识别包装成图形界面，并为后续快捷任务、日志检索下载、文件传输和可视化工作流保留扩展接口。

当前版本是 **Milestone 1：安全连接基础**，不是完整产品发行版。

## 当前已实现

- 首次启动选择数据根目录；开发环境的依赖、缓存、测试数据和构建产物均约束在项目所在盘。
- SQLite 服务器资料库与 Windows DPAPI 用户级凭据保险库。
- 首次连接主机指纹确认，以及指纹变化时在认证前强制拦截。
- 密码和带口令 OpenSSH 私钥认证，不创建明文私钥临时文件。
- 自动识别 Ubuntu/Debian、RHEL 系、openEuler、UOS、银河麒麟和 Anolis 等系统家族与基础能力。
- React + Tauri 桌面界面，展示服务器、信任状态和检测到的系统能力。
- 前端、Rust、D 盘路径审计及真实 SSH 集成测试。

## 尚未实现

日志搜索与下载、快捷命令/脚本、SFTP 文件传输、可视化工作流、安装包、在线更新，以及 GitHub Releases + ModelScope（魔搭）双源发布仍在后续里程碑中。本仓库当前不会把这些规划能力显示为可用功能。

## 开发快速开始

在 PowerShell 中进入仓库后执行：

```powershell
. .\scripts\dev-env.ps1
pnpm install --frozen-lockfile
pnpm test
pnpm build
cargo test --manifest-path .\src-tauri\Cargo.toml
pnpm tauri dev
```

真实 SSH 测试使用项目内的隔离夹具，生成的 Python 包、日志和测试密钥全部写入 `.local`：

```powershell
.\scripts\test-ssh-live.ps1
```

完整命令和环境要求见 [开发指南](docs/development.md)，安全边界见 [安全说明](docs/security.md)。

## 数据位置

公开版本不假设用户一定有 D 盘。安装版首次启动会要求用户明确选择数据根目录；便携版可通过程序目录旁的 `portable.flag` 使用同目录下的 `data`。本开发工作区位于 D 盘，`scripts/verify-d-drive.ps1` 会检查可控路径没有逃逸到 C 盘 AppData。

## 许可证

[Apache License 2.0](LICENSE)
