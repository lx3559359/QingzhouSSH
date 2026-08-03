# QingzhouSSH（轻舟 SSH）

QingzhouSSH 是一个面向 Windows 的无交互式 SSH 终端工具。它把服务器连接、Linux 能力识别、快捷任务、日志检索下载和 SFTP 文件传输包装成图形界面，不提供交互式终端、PTY 或持续 stdin。

当前开发版本已完成 **Milestone 2：快捷任务、日志与文件传输**，仍不是完整产品发行版。

## 当前已实现

- 首次启动选择数据根目录；开发环境的依赖、缓存、测试数据和构建产物均约束在项目所在盘。
- SQLite 服务器资料库与 Windows DPAPI 用户级凭据保险库。
- 首次连接主机指纹确认，以及指纹变化时在认证前强制拦截。
- 密码和带口令 OpenSSH 私钥认证，不创建明文私钥临时文件。
- 自动识别 Ubuntu/Debian、RHEL 系、openEuler、UOS、银河麒麟和 Anolis 等系统家族与基础能力。
- React + Tauri 桌面界面，展示服务器、信任状态和检测到的系统能力。
- 参数化内置任务：系统概览、磁盘使用、进程查询、服务状态/启动/停止/重启与日志检索。
- 受控单条命令与多行脚本执行；每次执行都有超时、输出上限、脱敏事件和危险操作二次确认。
- 普通 `.log`/`.gz` 日志搜索、50 条分页预览和项目数据目录内下载。
- SFTP 分块上传/下载、进度、SHA-256 校验、临时文件清理和原子完成。
- 可筛选执行历史、`uncertain` 中断恢复语义和后端登记的下载文件清单。
- 前端、Rust、D 盘路径审计及真实 SSH/SFTP/M2 闭环集成测试。

## 后续里程碑

- Milestone 3：可视化工作流、条件分支、重试、恢复点与运行控制。
- Milestone 4：Windows 安装包、在线更新，以及 GitHub Releases + ModelScope（魔搭）双源发布。

界面中的工作流入口会明确显示里程碑状态，不提供伪运行按钮。

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

真实 SSH/SFTP/M2 闭环测试使用项目内的隔离夹具，生成的 Python 包、日志、远端模拟目录和测试密钥全部写入 `.local`：

```powershell
.\scripts\test-ssh-live.ps1
```

完整命令和环境要求见 [开发指南](docs/development.md)，M2 验收证据见 [Milestone 2 验收记录](docs/milestone-2-acceptance.md)，安全边界见 [安全说明](docs/security.md)。

## 数据位置

公开版本不假设用户一定有 D 盘。安装版首次启动会要求用户明确选择数据根目录；便携版可通过程序目录旁的 `portable.flag` 使用同目录下的 `data`。本开发工作区固定在 `D:\Codex Project\轻量化SSH快捷工具`，开发数据、缓存、测试夹具和产物只能进入该项目目录内的 `.local`、`target`、`dist` 或 `artifacts`；`scripts/verify-d-drive.ps1` 会检查可控路径没有逃逸到 C 盘 AppData、D 盘根目录或项目父目录。

## 许可证

[Apache License 2.0](LICENSE)
