# 运维快捷任务与脚本中心总实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在现有轻舟 SSH 客户端中完成 Rust 强类型运维任务引擎、全量任务目录、只读批量执行、危险操作恢复、个人脚本库和面向小白的完整运维界面，最后只生成一个新的本地测试包。

**Architecture:** 保留现有 `ExecutionService` 作为底层单条 SSH 执行与历史记录入口，在其上增加任务规划、运维运行、批次、恢复点和脚本版本服务。内置命令模板只存在于 Rust 后端；前端只提交稳定 ID、版本和强类型参数。所有子计划按依赖顺序实施，每个子计划必须保持主分支持续可测试，但中途不打用户测试包。

**Tech Stack:** Rust、Tokio、russh、SQLx/SQLite、Tauri 2、React 19、TypeScript、Vitest、Testing Library、PowerShell、本地 AsyncSSH 夹具。

---

## 0. 执行前基线

当前分支 `codex/adaptive-compact-layout` 包含已经通过 r7 验证但仍未全部提交的既有实现。开始本计划前不得创建一个缺少这些改动的 Git worktree。

- [ ] 运行 `git status --short --branch`，保存当前文件清单。
- [ ] 运行 `pnpm test`，预期 20 个测试文件、76 个测试通过。
- [ ] 运行 `pnpm build`，预期 TypeScript 与 Vite 构建成功。
- [ ] 在 PowerShell 当前会话运行：

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
```

预期：格式检查通过；所有非 live Rust 测试通过，依赖 SSH 夹具的测试保持 ignored。

- [ ] 仅在确认现有 r7 改动均属于已验收工作后，建立一个本地基线提交；不得推送、发布或改写历史。
- [ ] 如果使用 worktree，在基线提交后调用 `superpowers:using-git-worktrees` 创建隔离工作区；如果继续使用当前工作区，必须保留所有既有未提交文件并逐文件暂存。

## 1. 子计划与依赖顺序

### 阶段一：任务引擎 V2 基础

执行：[2026-08-05-operations-engine-v2.md](2026-08-05-operations-engine-v2.md)

产出：强类型任务模型、扩展参数、规划/预检、运维状态机、SQLite 持久化、IPC 和现有任务兼容适配。

进入下一阶段的门槛：旧任务与工作流回归通过；新 planner/repository/IPC 测试通过。

### 阶段二：只读任务、Runbook、批量与报告

执行：[2026-08-05-operations-readonly-catalog.md](2026-08-05-operations-readonly-catalog.md)

产出：系统、存储、网络、安全、服务、Web、容器只读任务；9 个内置 Runbook；并发上限 3 的只读批次；TXT/JSON 报告。

进入下一阶段的门槛：只读任务目录、解析器、批量隔离、报告脱敏和国产系统兼容测试通过。

### 阶段三：危险维护与恢复

执行：[2026-08-05-operations-dangerous-recovery.md](2026-08-05-operations-dangerous-recovery.md)

产出：root/免密 sudo 预检、预演、任务恢复点、验证、回滚、网络修改超时自动回滚以及全部内置修改任务。

进入下一阶段的门槛：所有内置危险任务都有明确 backup/verify/rollback 定义；断线、部分回滚和重复回滚测试通过。

### 阶段四：个人脚本库

执行：[2026-08-05-personal-script-center.md](2026-08-05-personal-script-center.md)

产出：个人脚本定义和不可变版本、参数环境变量、导入导出、受控执行、前端脚本中心和安全测试。

进入下一阶段的门槛：个人脚本始终高风险并明确不可自动回滚；正文不进入浏览器存储、事件、报告或诊断。

### 阶段五：运维中心 UI、全量验收与单包交付

执行：[2026-08-05-operations-ui-final-integration.md](2026-08-05-operations-ui-final-integration.md)

产出：任务搜索/分类/收藏/右键菜单、任务向导、结构化结果、批量面板、恢复入口、自适应布局、文档、全量测试与 r8 本地便携包。

完成门槛：规格第 18 节全部满足；只生成 `0.1.5-local.20260805-r8` 本地测试包，不覆盖 r7，不自动发布。

## 2. 跨阶段固定约束

- [ ] 每个实现任务严格遵循红—绿—重构：先写失败测试，确认失败原因正确，再做最小实现。
- [ ] 每个任务只提交该任务涉及的文件；不得使用 `git add .` 混入用户或其他阶段改动。
- [ ] 每个阶段结束运行前端全量测试、Rust 非 live 全量测试和 `git diff --check`。
- [ ] 所有本地数据和测试产物位于 `D:\Codex Project\轻量化SSH快捷工具` 下的 `.local`、`data`、`target`、`dist` 或 `artifacts`。
- [ ] 不向 C 盘 AppData、D 盘根目录或 `D:\Codex Project` 父目录写入可控数据。
- [ ] 不把用户提供过的 API key、服务器凭据、私钥或脚本正文写进源码、计划、测试快照、日志或提交。
- [ ] 内置任务不能接受前端命令模板；个人脚本不能被错误标记为可自动回滚。
- [ ] 中途不生成用户测试包，不提交 GitHub、不更新魔塔、不修改在线更新清单。

## 3. 最终统一验证顺序

- [ ] 运行前端源码契约：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\local-build.tests.ps1
```

- [ ] 运行前端测试和生产构建：

```powershell
pnpm test
pnpm build
```

- [ ] 运行 Rust 格式与非 live 全量测试：

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
```

- [ ] 启动项目内 SSH/SFTP 夹具并运行 live 闭环：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\ssh-fixture.ps1 -Action Start
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test ssh_live -- --ignored --nocapture
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test sftp_live -- --ignored --nocapture
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operations_live -- --ignored --nocapture
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\ssh-fixture.ps1 -Action Stop
```

- [ ] 在 1920×1080、1366×768、1180×760、960×640 原生窗口中检查：首页、服务器、快捷任务、日志检索、文件传输、工作流、历史、下载和设置。
- [ ] 运行 `git diff --check`，预期退出码 0；仅允许现有 CRLF 提示，不允许空白错误。
- [ ] 构建唯一测试包：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-local-test.ps1 -PackageVersion '0.1.5-local.20260805-r8'
```

- [ ] 验证目录、ZIP、`portable.flag`、嵌入式生产前端和 SHA-256；确认所有路径位于项目 `artifacts\local-test`。
- [ ] 停止继续迭代，向用户提供 EXE、ZIP、哈希和验证摘要，等待用户自行测试。
