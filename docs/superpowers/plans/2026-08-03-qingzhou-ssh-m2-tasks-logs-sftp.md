# QingzhouSSH Milestone 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成快捷任务、实时 SSH 输出、日志检索下载、SFTP 单文件传输和脱敏执行历史的完整 UI → Rust → SSH/SFTP → SQLite/文件闭环。

**Architecture:** React 仅负责类型化表单、确认和事件展示；Tauri commands 把 DTO 交给 `AppServices`；`core/tasks`、`core/logs`、`core/ssh`、`core/sftp` 负责纯领域规则和远端 I/O；repository 只保存有限元数据，完整输出与结果文件只写入用户数据根目录。所有普通模板参数必须在 Rust 侧验证和转义，凭据、私钥和敏感参数在进入数据库、日志或 IPC 前统一脱敏。

**Tech Stack:** Tauri 2 Channels, React 19, TypeScript 5, Rust 2021, SQLx/SQLite, russh, russh-sftp, Tokio, SHA-256, Vitest, Rust unit/integration tests, controlled OpenSSH fixture.

---

## 执行约定

- 每个任务严格执行红灯测试、最小实现、绿灯验证、提交四步。
- PowerShell 中先运行 `& .\scripts\dev-env.ps1`；Cargo 产物必须位于项目内 `target`。
- 单元测试命令默认在 `D:\Codex Project\轻量化SSH快捷工具\.worktrees\full-product` 执行。
- 不在数据库中保存完整 stdout/stderr，不在前端暴露凭据、数据库句柄、文件句柄或 SSH 会话。
- 事件片段上限 32 KiB，完整命令输出上限 32 MiB，SFTP 块大小 64 KiB。

## Task 1: M2 数据库迁移与执行领域模型

**Files:**

- Create: `src-tauri/migrations/0002_tasks_and_executions.sql`
- Create: `src-tauri/src/domain/execution.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Create: `src-tauri/src/repositories/execution_repository.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Modify: `src-tauri/tests/foundation_integration.rs`
- Create: `src-tauri/tests/execution_repository_integration.rs`

- [x] 先写迁移测试：从只有 `0001_foundation.sql` 的数据库升级后应保留 servers/host_keys，并生成 `task_definitions`、`executions`、`execution_parameters`、`execution_files`；非法状态必须触发 CHECK 约束。
- [x] 写 repository 失败测试：创建 queued 记录、转为 running/终态、读取筛选列表、保存脱敏参数和相对文件引用；启动恢复必须把遗留 running 改成 uncertain。
- [x] 运行 `cargo test --locked execution_repository -- --nocapture`，确认因迁移和类型缺失失败。
- [x] 实现 `ExecutionStatus::{Queued,Running,Succeeded,Failed,Cancelled,Uncertain}`、`ExecutionRecord`、`ExecutionFile`、`ExecutionFilter`，并提供稳定的 snake_case 序列化。
- [x] 实现 repository API：

```rust
pub async fn create(&self, draft: NewExecution) -> Result<ExecutionRecord, AppError>;
pub async fn mark_running(&self, id: Uuid, started_at: i64) -> Result<(), AppError>;
pub async fn finish(&self, finish: FinishExecution) -> Result<(), AppError>;
pub async fn add_file(&self, id: Uuid, file: ExecutionFile) -> Result<(), AppError>;
pub async fn list(&self, filter: ExecutionFilter) -> Result<Vec<ExecutionRecord>, AppError>;
pub async fn get(&self, id: Uuid) -> Result<ExecutionDetails, AppError>;
pub async fn recover_interrupted(&self) -> Result<u64, AppError>;
```

- [x] 运行 `cargo test --locked execution_repository -- --nocapture` 和 `cargo test --locked --test foundation_integration -- --nocapture`，确认迁移、约束和恢复测试通过。
- [x] 提交：`feat: persist task execution history`

## Task 2: 脱敏、事件序列与有界 UTF-8 分片

**Files:**

- Create: `src-tauri/src/core/redaction.rs`
- Create: `src-tauri/src/domain/events.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/error.rs`

- [x] 写失败测试：口令、私钥块、`password=...`、`token=...` 和显式敏感值均替换成 `[REDACTED]`；分片不得破坏 UTF-8；序号必须从 1 单调递增；摘要不得超过 8 KiB。
- [x] 写输出上限失败测试：累计超过 32 MiB 返回 `OutputLimitExceeded`，且已写片段不包含 canary secret。
- [x] 运行 `cargo test --locked redaction -- --nocapture` 和 `cargo test --locked events -- --nocapture`，确认缺少实现。
- [x] 实现 `Redactor::new(runtime_secrets)`、`redact(&str)`、`redact_json(&Value)`；实现 `SequencedEventEmitter` 与：

```rust
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExecutionEventPayload {
    Started { execution_id: Uuid, started_at: i64 },
    Stdout { text: String, total_bytes: u64 },
    Stderr { text: String, total_bytes: u64 },
    Progress { transferred: u64, total: Option<u64>, percent: Option<f64> },
    FileProduced { file: ExecutionFile },
    Finished { status: ExecutionStatus, exit_code: Option<i32>, duration_ms: u64, result: Option<Value> },
    Failed { category: String, message: String, retryable: bool },
}
```

- [x] 在 `AppError` 增加参数、输出上限、传输、校验、权限、磁盘空间和取消状态分类，并保持 Tauri 错误 DTO 不泄露内部值。
- [x] 运行 `cargo test --locked redaction -- --nocapture` 和 `cargo test --locked events -- --nocapture`，确认全部通过。
- [x] 提交：`feat: add redacted execution events`

## Task 3: 任务定义、参数验证与兼容实现选择

**Files:**

- Create: `src-tauri/src/core/tasks/mod.rs`
- Create: `src-tauri/src/core/tasks/model.rs`
- Create: `src-tauri/src/core/tasks/parameters.rs`
- Create: `src-tauri/src/core/tasks/catalog.rs`
- Create: `src-tauri/src/core/tasks/render.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [x] 写表驱动失败测试，覆盖字符串、整数、布尔、枚举、绝对路径、服务名、进程过滤词、时间范围；拒绝 NUL、相对路径、越界数值、未知字段和无效服务名。
- [x] 写 shell 单引号转义失败测试，至少覆盖空串、空格、单引号、换行和命令替换字符。
- [x] 写目录测试，要求稳定 ID/版本并包含：system overview、disk usage、process query、service status/start/stop/restart、log search；危险服务操作 riskLevel 为 dangerous。
- [x] 写国产化兼容测试，覆盖 Ubuntu/Debian、Rocky/Anolis、openEuler、Kylin、UOS，并根据 `systemd`、`service`、`grep`、`gzip` 能力选择实现。
- [x] 运行 `cargo test --locked tasks:: -- --nocapture`，确认失败。
- [x] 实现 `TaskDefinition`、`ParameterDefinition`、`CompatibilityPredicate`、`TaskImplementation`、`OutputKind`，并实现：

```rust
pub fn validate_parameters(definition: &TaskDefinition, input: &Value) -> Result<ValidatedParameters, AppError>;
pub fn shell_quote(value: &str) -> String;
pub fn select_implementation<'a>(definition: &'a TaskDefinition, probe: &SystemProbe) -> Result<&'a TaskImplementation, AppError>;
pub fn render_command(implementation: &TaskImplementation, parameters: &ValidatedParameters) -> Result<String, AppError>;
pub fn built_in_catalog() -> Vec<TaskDefinition>;
```

- [x] 运行 `cargo test --locked tasks:: -- --nocapture`，确认普通模板无未经验证的原始 shell 插值。
- [x] 提交：`feat: add compatible quick task catalog`

## Task 4: 认证 SSH 会话复用与流式命令执行

**Files:**

- Modify: `src-tauri/src/core/ssh/transport.rs`
- Create: `src-tauri/src/core/ssh/executor.rs`
- Modify: `src-tauri/src/core/ssh/mod.rs`
- Create: `src-tauri/tests/ssh_streaming_integration.rs`

- [x] 先为现有 transport 写回归测试，确保 host-key 不受信或变化时在认证和建 channel 前终止。
- [x] 写执行器失败测试：stdout/stderr 独立事件、有序序号、32 KiB UTF-8 分片、退出码、超时、32 MiB 上限、输出文件流式写入和取消结果。
- [x] 运行 `cargo test --locked ssh_streaming -- --nocapture`，确认缺少 API。
- [x] 将连接和认证提取为仅 Rust 内部可持有的 `AuthenticatedSshSession`；保留 M1 `test_connection` 行为，SFTP 与执行器共享安全握手路径。
- [x] 实现：

```rust
pub async fn execute_streaming<E: EventSink>(
    session: &mut AuthenticatedSshSession,
    request: CommandRequest,
    output_file: &Path,
    redactor: &Redactor,
    events: &mut E,
    cancel: CancellationToken,
) -> Result<CommandOutcome, AppError>;
```

- [x] 使用 `tokio::select!` 同时处理 stdout、stderr、退出状态、超时和取消；不读取完整输出到内存。
- [x] 运行 `cargo test --locked ssh_streaming -- --nocapture`、`cargo test --locked foundation -- --nocapture` 和 `cargo test --locked --test ssh_live -- --nocapture`；普通测试中 live 用例保持 ignored，显式 fixture 才执行。
- [x] 提交：`feat: stream bounded ssh command output`

## Task 5: SFTP 单文件上传、下载与 SHA-256 校验

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Create: `src-tauri/src/core/sftp/mod.rs`
- Create: `src-tauri/src/core/sftp/transfer.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/tests/sftp_live.rs`

- [x] 添加 `russh-sftp = "2.3.0"`、Tokio `fs`/`io-util` 特性和 `tokio-util` cancellation 依赖；更新锁文件且不得引入系统 OpenSSL 构建依赖。
- [x] 写纯逻辑失败测试：远程路径必须绝对且无 NUL，本地目标必须位于 data root/downloads 或来自用户明确选择，临时文件名不可碰撞。
- [x] 写受控 SFTP fixture 失败测试：上传/下载大于 128 KiB 文件、64 KiB 分块进度、SHA-256 一致、原子重命名、断网/取消/哈希失败清理 `.partial` 和远端临时文件。
- [x] 运行 `cargo test --locked sftp -- --nocapture`，确认失败。
- [x] 通过已认证 russh channel 请求 `sftp` subsystem，使用 `russh_sftp::client::SftpSession::new(channel.into_stream())`。
- [x] 实现 `upload`、`download`、`hash_remote_file`；哈希远端文件优先流式读取 SFTP 内容，不依赖远端安装 `sha256sum`。
- [x] 运行 `cargo test --locked sftp -- --nocapture`；再在 fixture 可用时运行 `cargo test --locked --test sftp_live -- --ignored --nocapture`。
- [x] 提交：`feat: transfer verified files over sftp`

## Task 6: 日志检索命令、解析、结果文件和分页

**Files:**

- Create: `src-tauri/src/core/logs/mod.rs`
- Create: `src-tauri/src/core/logs/request.rs`
- Create: `src-tauri/src/core/logs/command.rs`
- Create: `src-tauri/src/core/logs/parser.rs`
- Create: `src-tauri/src/core/logs/result_store.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/tests/log_search_integration.rs`

- [x] 写请求失败测试：绝对 `.log`/`.gz` 路径、关键词、大小写、context 0..20、limit 1..10000、可选时间范围；拒绝 NUL 和相对路径。
- [x] 写命令渲染失败测试：普通文件使用 `grep`，gzip 文件使用 `gzip -cd -- <path> | grep`；所有用户值经过 `shell_quote`；缺能力返回明确兼容错误。
- [x] 写 parser 失败测试：匹配行、上下文、行号、UTF-8 替换、空结果和 limit 截断。
- [x] 写 result-store 失败测试：同一执行生成 UTF-8 JSONL 与 text 文件，cursor/page_size=50 分页不重复、不重跑远端命令，文件引用只保存 data-root 相对路径。
- [x] 运行 `cargo test --locked logs:: -- --nocapture` 和 `cargo test --locked --test log_search_integration -- --nocapture`，确认失败。
- [x] 实现 `LogSearchRequest`、`LogMatch`、固定记录分隔协议、`LogResultWriter` 和 `read_page(execution_id, cursor, page_size)`。
- [x] 所有预览和下载内容进入统一 Redactor；结果超过 32 MiB 时终止并记录明确错误。
- [x] 运行 `cargo test --locked logs:: -- --nocapture` 和 `cargo test --locked --test log_search_integration -- --nocapture`，确认 `.log`、`.gz`、上下文、limit 与分页通过。
- [x] 提交：`feat: search and page remote logs`

## Task 7: 执行服务、任务编排、历史和取消注册表

**Files:**

- Create: `src-tauri/src/services/execution_service.rs`
- Create: `src-tauri/src/services/transfer_service.rs`
- Create: `src-tauri/src/services/log_service.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/state.rs`

- [x] 写服务失败测试：服务端重新校验 DTO，创建 queued/running 记录，建立受信 SSH 会话，流式执行，保存终态和文件；任一步失败也必须留下准确历史。
- [x] 写敏感 canary 测试：凭据和 sensitive 参数不能出现在 execution log、SQLite、事件或错误中。
- [x] 写取消失败测试：运行中的 execution 可查到 token；本地已确认关闭 channel 记 cancelled，无法确认远端终止记 uncertain；完成后清理注册表。
- [x] 分别运行 `cargo test --locked services::execution -- --nocapture`、`cargo test --locked services::transfer -- --nocapture` 和 `cargo test --locked services::logs -- --nocapture`，确认失败。
- [x] 实现 `ExecutionService`、`TransferService`、`LogService` 和 `ExecutionRegistry`，初始化时调用 `recover_interrupted()`。
- [x] 自定义单条命令与多行脚本仅接受非交互模式；脚本通过固定 `sh -s`/base64 安全载荷执行，禁止持续 stdin、PTY 和全屏程序。
- [x] 运行 `cargo test --locked services:: -- --nocapture`，确认成功、失败、取消、uncertain 和脱敏路径通过。
- [x] 提交：`feat: orchestrate tasks logs and transfers`

## Task 8: Tauri 命令、Channel 与 TypeScript 契约

**Files:**

- Create: `src-tauri/src/commands/executions.rs`
- Create: `src-tauri/src/commands/logs.rs`
- Create: `src-tauri/src/commands/transfers.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/tauri.test.ts`

- [x] 先写前端 API 失败测试，断言所有 command 名、snake_case Rust 参数映射和 Channel 消息处理；重复/倒序 event 被客户端忽略。
- [x] 写 Rust command DTO 序列化测试，确保错误 DTO 和事件 discriminant 与 TypeScript 联合类型一致。
- [x] 运行 `pnpm test -- src/api/tauri.test.ts` 与 `cargo test --locked commands:: -- --nocapture`，确认失败。
- [x] 注册规格中的 11 个命令：任务列表/执行、自定义执行、取消、日志搜索/分页/下载、上传/下载、执行列表/详情。
- [x] 前端通过 `new Channel<ExecutionEvent>()` 接收事件，并返回可取消的 execution handle；不得把凭据或本地真实绝对数据根路径加入 DTO。
- [x] 运行上述测试，再运行 `pnpm build`，确认 Rust/TypeScript 契约一致。
- [x] 提交：`feat: expose typed milestone two commands`

## Task 9: 应用壳层、导航、服务器选择与快捷任务运行器

**Files:**

- Modify: `src/app/App.tsx`
- Modify: `src/app/App.test.tsx`
- Create: `src/app/AppShell.tsx`
- Create: `src/app/AppShell.test.tsx`
- Create: `src/features/tasks/TaskPage.tsx`
- Create: `src/features/tasks/TaskPage.test.tsx`
- Create: `src/features/tasks/ParameterForm.tsx`
- Create: `src/features/tasks/ExecutionDrawer.tsx`
- Modify: `src/styles/theme.css`

- [x] 写失败测试：首页、服务器、快捷任务、工作流、执行记录、下载文件、设置导航存在；工作流明确标注下一里程碑且没有伪运行按钮。
- [x] 写任务页失败测试：服务器选择、白银渐变立体卡片、国产系统兼容标签、参数生成、危险操作二次确认、运行/取消和 stdout/stderr 流式展示。
- [x] 运行 `pnpm test -- src/app src/features/tasks`，确认失败。
- [x] 实现 AppShell 与任务页；沿用批准的白银渐变、边缘高光和三层阴影，保证正文对比度和左右面板背景一致。
- [x] ExecutionDrawer 只保留有限 UI 缓冲，完成后从历史 API 恢复终态；重载不伪造仍在运行状态。
- [x] 运行 `pnpm test -- src/app src/features/tasks` 和 `pnpm build`。
- [x] 提交：`feat: add quick task runner interface`

## Task 10: 日志检索、分页预览与结果下载界面

**Files:**

- Create: `src/features/logs/LogSearchPage.tsx`
- Create: `src/features/logs/LogSearchPage.test.tsx`
- Create: `src/features/logs/LogResultsTable.tsx`
- Modify: `src/app/AppShell.tsx`
- Modify: `src/styles/theme.css`

- [x] 写失败测试：路径/关键词/时间/大小写/context/limit 字段，流进度，50 条分页，空结果、权限不足、缺 grep/gzip、超限和下载位置反馈。
- [x] 运行 `pnpm test -- src/features/logs`，确认失败。
- [x] 实现检索表单、流状态、分页表格和下载动作；翻页只能调用 `read_log_result_page`，不能重新执行搜索。
- [x] 下载成功显示数据根目录内相对位置；错误信息使用分类文案而不是原始异常堆栈。
- [x] 运行 `pnpm test -- src/features/logs` 和 `pnpm build`。
- [x] 提交：`feat: add searchable downloadable log results`

## Task 11: 文件传输、下载文件与执行历史界面

**Files:**

- Create: `src/features/transfers/FileTransferPage.tsx`
- Create: `src/features/transfers/FileTransferPage.test.tsx`
- Create: `src/features/downloads/DownloadsPage.tsx`
- Create: `src/features/downloads/DownloadsPage.test.tsx`
- Create: `src/features/history/ExecutionHistoryPage.tsx`
- Create: `src/features/history/ExecutionHistoryPage.test.tsx`
- Create: `src/features/history/ExecutionDetails.tsx`
- Modify: `src/app/AppShell.tsx`
- Modify: `src/styles/theme.css`

- [x] 写失败测试：上传/下载显示本地/远程路径、字节进度、速度和 SHA-256 状态；历史按服务器、类别、状态、时间筛选；详情显示参数摘要、时间线、退出码、脱敏日志和文件。
- [x] 写下载文件页测试：仅列出后端返回的数据根目录文件，不从 WebView 扫描任意路径。
- [x] 运行 `pnpm test -- src/features/transfers src/features/downloads src/features/history`，确认失败。
- [x] 实现三页并接入 AppShell；取消和校验失败状态不得显示成功。
- [x] 运行上述测试和 `pnpm build`。
- [x] 提交：`feat: add transfer downloads and history views`

## Task 12: 高级命令与多行脚本界面

**Files:**

- Create: `src/features/tasks/AdvancedExecutionPanel.tsx`
- Create: `src/features/tasks/AdvancedExecutionPanel.test.tsx`
- Modify: `src/features/tasks/TaskPage.tsx`

- [x] 写失败测试：单条命令/多行脚本切换、服务器/超时/内容摘要、非交互提示、危险确认、运行/取消和结果事件。
- [x] 写安全测试：确认弹窗不展示完整敏感参数，前端不自动保存脚本文本到 localStorage/sessionStorage。
- [x] 运行 `pnpm test -- src/features/tasks/AdvancedExecutionPanel.test.tsx`，确认失败。
- [x] 实现高级执行面板并复用 ExecutionDrawer，不增加 Web 终端、PTY 或持续 stdin。
- [x] 运行任务页全套测试和 `pnpm build`。
- [x] 提交：`feat: add controlled advanced executions`

## Task 13: 真实 SSH/SFTP 夹具与端到端闭环测试

**Files:**

- Modify: `scripts/ssh-fixture.ps1`
- Modify: `scripts/tests/ssh-fixture.tests.ps1`
- Modify: `src-tauri/tests/ssh_live.rs`
- Modify: `src-tauri/tests/sftp_live.rs`
- Create: `src-tauri/tests/m2_live.rs`
- Create: `tests/m2-canary.txt`

- [x] 扩展 fixture 测试，使受控服务器具备普通 `.log`、`.gz`、可写临时目录、可查询服务和密码/私钥两种账号；fixture 数据只落在项目目录。
- [x] 写 ignored live 测试：内置系统/服务任务、高级命令/脚本、`.log`/`.gz` 检索下载、SFTP 上传下载和历史闭环。
- [x] 写 canary 测试：执行后递归扫描项目 data-root 测试目录、SQLite、日志和捕获事件，凭据 canary 不得出现。
- [x] 先运行 live tests，确认 fixture 扩展前失败；完成脚本后运行 `cargo test --locked --test m2_live -- --ignored --nocapture`。
- [x] 运行 `powershell -File scripts/tests/ssh-fixture.tests.ps1`，确认夹具启动、停止和清理幂等。
- [x] 提交：`test: cover milestone two live flows`

## Task 14: M2 文档、安全审计与完整门禁

**Files:**

- Modify: `README.md`
- Modify: `docs/development.md`
- Create: `docs/milestone-2-acceptance.md`
- Modify: `docs/superpowers/plans/2026-08-03-qingzhou-ssh-m2-tasks-logs-sftp.md`

- [x] 文档列出支持系统家族、内置任务、日志和传输限制、危险确认、数据目录、取消/uncertain 语义和 fixture 使用方法。
- [x] 运行占位扫描：`rg -n "TODO|TBD|implement later|placeholder|mock success|假成功|待实现" src src-tauri scripts README.md docs`；只允许路线图中明确属于 M3/M4 的说明。
- [x] 运行 `powershell -File scripts/tests/dev-env.tests.ps1` 和 `powershell -File scripts/verify-d-drive.ps1`。
- [x] 运行 `pnpm test`、`pnpm build`、`cargo fmt --all -- --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked --all-targets -- --nocapture`。
- [x] 启动应用并人工验收服务器选择、快捷任务、危险确认、日志分页/下载、SFTP、历史筛选和高级脚本；截图记录在 `docs/milestone-2-acceptance.md`。
- [x] 扫描 C 盘和 `D:\Codex Project` 根目录，不得发现本项目新生成的缓存、target、SQLite、日志或下载；所有可控数据必须在项目目录或用户选择的数据根目录。
- [x] 把本计划所有已完成 checkbox 改为 `[x]`，记录命令、测试数和 live fixture 结果。
- [x] 提交：`docs: record milestone two acceptance`

## 规格覆盖自检

- 内置任务、国产系统自动兼容：Task 3、4、9、13。
- 高级命令与多行脚本：Task 7、8、12、13。
- 流式事件、退出码、取消与输出上限：Task 2、4、7、8、9。
- `.log`/`.gz`、分页和下载：Task 6、7、8、10、13。
- SFTP 临时文件、SHA-256 和清理：Task 5、7、8、11、13。
- SQLite 历史、重启恢复和脱敏：Task 1、2、7、8、11、14。
- 视觉一致性与无占位功能：Task 9–12、14。
- D 盘数据约束和完整安全门禁：Task 13、14。
