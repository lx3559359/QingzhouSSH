# QingzhouSSH Milestone 3：工作流与恢复实施计划

> 设计依据：`docs/superpowers/specs/2026-08-04-qingzhou-ssh-m3-workflows-recovery-design.md`

**目标：** 交付可持久化、可校验、可视化、串行执行、失败暂停、可重试、可取消、可恢复和可回滚的单服务器工作流。

**架构：** React 只编辑和展示强类型工作流 DTO；Tauri command 进入 Rust `WorkflowService`。图校验和条件求值在纯 Rust 核心层，持久化在专用 repository，节点执行复用 M2 的 execution/log/transfer 服务，恢复点通过受信 SFTP 会话写入数据根目录。

**约束：** 所有开发/测试/运行数据保持在 `D:\Codex Project\轻量化SSH快捷工具` 内；无循环、并行、多服务器、交互终端或伪成功；每项按失败测试 → 最小实现 → 回归 → 提交执行。

---

## Task 1：工作流迁移、领域类型与 repository

**Files:**

- Create: `src-tauri/migrations/0003_workflows.sql`
- Create: `src-tauri/src/domain/workflow.rs`
- Create: `src-tauri/src/repositories/workflow_repository.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Test: `src-tauri/tests/workflow_repository_integration.rs`

- [x] 写迁移失败测试：从 M2 数据库升级后保留服务器/执行记录，并生成 workflows、versions、runs、node_runs、restore_points、run_events；非法状态触发 CHECK。
- [x] 写 repository 失败测试：保存定义生成不可变版本、相同定义不重复增版、列表/详情、创建运行、节点 attempt、事件序号和运行筛选。
- [x] 写恢复失败测试：遗留 running workflow/node 启动后转为 uncertain，并保留 current node 与错误说明。
- [x] 运行 `cargo test --locked --test workflow_repository_integration -- --nocapture`，确认缺少实现。
- [x] 实现强类型状态、记录与 repository；JSON 必须规范化并计算 SHA-256，所有相对文件路径通过数据根约束。
- [x] 运行 repository 与 M1/M2 migration 回归测试。
- [x] 提交：`feat: persist versioned workflows and runs`

## Task 2：图校验与受限条件求值

**Files:**

- Create: `src-tauri/src/core/workflows/mod.rs`
- Create: `src-tauri/src/core/workflows/definition.rs`
- Create: `src-tauri/src/core/workflows/validation.rs`
- Create: `src-tauri/src/core/workflows/condition.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Test: `src-tauri/tests/workflow_graph.rs`

- [x] 写表驱动失败测试：开始/终止约束、缺失引用、自环、重复边、环、不可达节点、分支标签、100/200 上限和无终止路径。
- [x] 写节点参数失败测试：任务 ID/版本、日志、上传/下载、自定义超时和脚本长度复用 M2 校验；拒绝秘密字段和未知 JSON 字段。
- [x] 写条件求值失败测试：退出码、受限 JSON 点路径、固定文本 contains/notContains；拒绝正则、shell、动态路径和 512 字节以上文本。
- [x] 实现确定性拓扑校验、诊断 DTO 和条件求值器。
- [x] 运行 `cargo test --locked --test workflow_graph -- --nocapture`。
- [x] 提交：`feat: validate bounded workflow graphs`

## Task 3：工作流事件、状态机与运行注册表

**Files:**

- Create: `src-tauri/src/domain/workflow_events.rs`
- Create: `src-tauri/src/core/workflows/state_machine.rs`
- Create: `src-tauri/src/services/workflow_registry.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Test: `src-tauri/tests/workflow_state_machine.rs`

- [x] 写失败测试：合法/非法运行和节点转换、事件序号、32 KiB 上限、Redactor、取消 token、当前 child execution 注册与清理。
- [x] 实现 run/node 状态机与 `WorkflowEventEmitter`；状态先持久化再发事件。
- [x] 实现 registry 的 run cancellation token 与 child execution 映射，不向前端暴露句柄。
- [x] 运行状态机和 redaction 回归测试。
- [x] 提交：`feat: add workflow state and event contracts`

## Task 4：任务与高级执行节点适配器

**Files:**

- Create: `src-tauri/src/services/workflow_nodes/mod.rs`
- Create: `src-tauri/src/services/workflow_nodes/execution.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: `src-tauri/tests/workflow_execution_nodes.rs`

- [x] 写失败测试：内置任务和自定义命令/脚本节点调用现有 ExecutionService，关联 M2 execution ID，映射退出码/摘要/结果，敏感内容不进入 workflow event。
- [x] 为 M2 服务增加最小内部 trait/facade 以便工作流复用，不复制 SSH 执行逻辑。
- [x] 实现 task/custom 节点适配器和统一 `NodeOutcome`。
- [x] 运行节点测试及 M2 execution service 回归。
- [x] 提交：`feat: execute task nodes in workflows`

## Task 5：日志与 SFTP 节点适配器

**Files:**

- Create: `src-tauri/src/services/workflow_nodes/logs.rs`
- Create: `src-tauri/src/services/workflow_nodes/transfers.rs`
- Modify: `src-tauri/src/services/workflow_nodes/mod.rs`
- Test: `src-tauri/tests/workflow_io_nodes.rs`

- [x] 写失败测试：日志、上传、下载节点复用 M2 服务，关联 execution/file，校验失败准确映射，取消和哈希失败不显示成功。
- [x] 实现适配器；工作流只保存 data-root 相对文件引用。
- [x] 运行节点测试、日志和 SFTP 回归测试。
- [x] 提交：`feat: execute log and transfer workflow nodes`

## Task 6：恢复点创建与安全路径

**Files:**

- Create: `src-tauri/src/core/workflows/restore_paths.rs`
- Create: `src-tauri/src/services/restore_point_service.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: `src-tauri/tests/workflow_restore_points.rs`

- [x] 写路径失败测试：只能位于 `backups/workflows/<run>/<node>`，拒绝绝对路径、`..`、NUL 和数据库外文件。
- [x] 写 SFTP 失败测试：已有远端文件流式备份并 SHA-256 校验；不存在文件记录 create/delete 语义；取消/断线清理 partial。
- [x] 实现 restore-point create、available/failed 状态和元数据持久化。
- [x] 运行恢复点与 SFTP 回归测试。
- [x] 提交：`feat: capture verified workflow restore points`

## Task 7：串行运行器与条件分支

**Files:**

- Create: `src-tauri/src/services/workflow_service.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: `src-tauri/tests/workflow_runner.rs`

- [x] 写失败测试：start → task → condition → true/false → stop/finish 串行运行；未选分支标 skipped；节点失败进入 paused 且后续保持 pending/skipped。
- [x] 写校验失败测试：运行前重新检查服务器存在、主机已信任、系统兼容、危险确认和全部参数。
- [x] 实现 WorkflowService、node dispatcher、条件选择和完成判定；每一步状态/事件持久化。
- [x] 运行 runner、M2 service 和 repository 回归。
- [x] 提交：`feat: run linear conditional workflows`

## Task 8：取消、失败节点重试与崩溃恢复

**Files:**

- Modify: `src-tauri/src/services/workflow_service.rs`
- Modify: `src-tauri/src/services/workflow_registry.rs`
- Modify: `src-tauri/src/repositories/workflow_repository.rs`
- Test: `src-tauri/tests/workflow_recovery.rs`

- [x] 写失败测试：取消当前 child execution；确认停止为 cancelled，未确认远端为 uncertain；registry 完成后清理。
- [x] 写重试失败测试：仅 paused 的 failed 节点可重试，attempt 递增、新建 M2 execution，成功后继续；不可重试错误被拒绝。
- [x] 写启动恢复测试：running run/node → uncertain，不自动继续，不伪造 cancelled/succeeded。
- [x] 实现并运行 recovery 全套测试。
- [x] 提交：`feat: retry and recover interrupted workflows`

## Task 9：逆序回滚、清理与诊断包

**Files:**

- Modify: `src-tauri/src/services/restore_point_service.rs`
- Modify: `src-tauri/src/services/workflow_service.rs`
- Create: `src-tauri/src/services/workflow_diagnostics.rs`
- Test: `src-tauri/tests/workflow_rollback.rs`

- [x] 写失败测试：无危险确认拒绝回滚；恢复点逆序；已有文件临时上传+校验+原子替换；新文件删除；部分失败为 rollback_failed。
- [x] 写清理失败测试：运行中拒绝、只删登记路径、成功后 expired、重复清理幂等。
- [x] 写诊断失败测试：输出位于 downloads、只含相对路径、时间线/错误/校验和完整且 canary 被脱敏。
- [x] 实现回滚、cleanup 和 diagnostics export。
- [x] 运行 rollback、SFTP、redaction 与路径测试。
- [x] 提交：`feat: rollback workflows and export diagnostics`

## Task 10：Tauri 与 TypeScript 工作流契约

**Files:**

- Create: `src-tauri/src/commands/workflows.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/tauri.test.ts`

- [ ] 写前端 API 失败测试，覆盖 13 个命令名、camelCase 参数、Channel 单调事件和错误 DTO。
- [ ] 写 Rust DTO 序列化测试，确保节点、边、条件、状态和事件 discriminant 与 TypeScript 一致。
- [ ] 注册并实现设计中的全部 workflow commands。
- [ ] 运行前后端契约测试和 `pnpm build`。
- [ ] 提交：`feat: expose typed workflow commands`

## Task 11：预览 API 与可复现示例工作流

**Files:**

- Modify: `src/api/preview.ts`
- Create: `src/features/workflows/fixtures.ts`
- Test: `src/api/preview.test.ts`

- [ ] 写失败测试：预览保存/增版、校验诊断、成功运行、条件假分支、节点失败暂停、重试、取消、回滚和诊断下载均符合真实 DTO。
- [ ] 实现只用于浏览器视觉验收的内存预览；明确标识 preview，不写磁盘、不伪装 Tauri 成功。
- [ ] 运行 preview 和 API 测试。
- [ ] 提交：`test: model workflow preview flows`

## Task 12：工作流列表、步骤库与立体画布

**Files:**

- Create: `src/features/workflows/WorkflowPage.tsx`
- Create: `src/features/workflows/WorkflowLibrary.tsx`
- Create: `src/features/workflows/WorkflowCanvas.tsx`
- Create: `src/features/workflows/workflow.css`
- Modify: `src/app/AppShell.tsx`
- Modify: `src/styles/theme.css`
- Test: `src/features/workflows/WorkflowPage.test.tsx`
- Test: `src/app/AppShell.test.tsx`

- [ ] 写失败测试：工作流列表、创建/删除、八类步骤库、添加节点、选中、拖动、SVG 连线、缩放和无 M3 占位说明。
- [ ] 实现三栏布局；复用白银渐变、高光、强三层阴影和点阵背景，左右卡片与中间背景连续。
- [ ] 运行组件测试和生产构建。
- [ ] 提交：`feat: add dimensional workflow canvas`

## Task 13：节点参数、连接和校验诊断

**Files:**

- Create: `src/features/workflows/WorkflowInspector.tsx`
- Create: `src/features/workflows/WorkflowValidationPanel.tsx`
- Modify: `src/features/workflows/WorkflowPage.tsx`
- Test: `src/features/workflows/WorkflowInspector.test.tsx`

- [ ] 写失败测试：各节点表单、普通 next、条件 true/false、删除节点、缺失参数、环/不可达/不兼容诊断定位。
- [ ] 写安全测试：脚本文本不进 localStorage/sessionStorage，确认区只显示摘要，危险节点聚合显示。
- [ ] 实现 inspector、连接选择器、保存增版和 Rust 校验结果定位。
- [ ] 运行 workflow UI 全套测试和构建。
- [ ] 提交：`feat: edit and validate workflow nodes`

## Task 14：运行确认、时间线、重试与回滚 UI

**Files:**

- Create: `src/features/workflows/WorkflowRunPanel.tsx`
- Create: `src/features/workflows/WorkflowTimeline.tsx`
- Modify: `src/features/workflows/WorkflowPage.tsx`
- Test: `src/features/workflows/WorkflowRunPanel.test.tsx`

- [ ] 写失败测试：服务器选择、运行前诊断、危险摘要确认、节点流事件、暂停、重试、取消/uncertain、回滚二次确认、清理和诊断下载。
- [ ] 实现运行面板与重载后详情恢复；不得用前端计时器伪造终态。
- [ ] 运行组件测试、AppShell 回归和构建。
- [ ] 提交：`feat: control and recover workflow runs`

## Task 15：真实夹具工作流闭环

**Files:**

- Create: `src-tauri/tests/workflow_live.rs`
- Modify: `tests/fixtures/sshd/server.py`
- Modify: `scripts/test-ssh-live.ps1`
- Modify: `scripts/tests/ssh-fixture.tests.ps1`

- [ ] 扩展夹具部署目录、已有文件、服务和可注入失败点，数据仍只在项目 `.local`。
- [ ] 写 ignored live 测试：参考部署成功、条件两分支、任务/脚本/日志/上传/下载节点失败暂停、失败节点重试、覆盖恢复点、逆序回滚和诊断包。
- [ ] 扫描 workflow DB/events/backups/downloads，凭据与敏感脚本 canary 不得出现。
- [ ] 运行 fixture 生命周期与 `scripts/test-ssh-live.ps1 -SkipPythonDependencyInstall`。
- [ ] 提交：`test: cover recoverable workflow live flows`

## Task 16：M3 文档、视觉验收与完整门禁

**Files:**

- Modify: `README.md`
- Modify: `docs/development.md`
- Modify: `docs/security.md`
- Create: `docs/milestone-3-acceptance.md`
- Modify: 本计划

- [ ] 文档列出节点、图限制、条件、状态、重试、取消/uncertain、恢复点、回滚、诊断和数据目录。
- [ ] 运行占位扫描，只允许 M4 路线图说明和真实 HTML placeholder 属性。
- [ ] 运行 dev-env、D 盘路径、前端全量测试/构建、Rust fmt/Clippy/全量测试、Tauri 调试构建。
- [ ] 用内置浏览器人工验收工作流列表、画布阴影、节点拖动、右侧参数背景、条件分支、危险确认、失败暂停、重试、回滚和诊断下载，并保存截图。
- [ ] 精确扫描 C 盘 AppData、`D:\`、`D:\Codex Project`；所有项目数据保持在项目目录或用户选择的数据根目录。
- [ ] 把本计划全部完成项改为 `[x]`，记录命令、测试数、live 结果和截图。
- [ ] 提交：`docs: record milestone three acceptance`
