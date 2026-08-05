# 运维任务引擎 V2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不破坏现有快捷任务、工作流和执行历史的前提下，建立可表达多步骤、预检、预演、权限、结果和恢复计划的 Rust 强类型任务引擎 V2。

**Architecture:** 扩展 `core::tasks` 的定义和参数模型，新增纯函数 planner；用独立 `operation_runs`/`operation_steps` 持久化高层运维生命周期，每个真实 SSH 步骤仍引用现有 `executions`。新增 `OperationService` 和 Tauri IPC，同时保留现有 `ExecutionService::execute_task` 作为兼容入口。

**Tech Stack:** Rust、Serde、Tokio、SQLx/SQLite、Tauri IPC、TypeScript。

---

## 文件结构

**修改：**

- `src-tauri/src/core/tasks/model.rs`：V2 任务、步骤、权限、结果和恢复定义。
- `src-tauri/src/core/tasks/parameters.rs`：扩展参数类型和 Shell 环境变量名称校验。
- `src-tauri/src/core/tasks/mod.rs`：导出新类型与能力匹配函数。
- `src-tauri/src/core/tasks/catalog.rs`：把现有 8 个任务迁移到 V2 helper。
- `src-tauri/src/services/execution_service.rs`：提供单步骤执行适配，不改变旧 API 行为。
- `src-tauri/src/services/app_services.rs`、`src-tauri/src/services/mod.rs`：注册 OperationService。
- `src-tauri/src/commands/mod.rs`、`src-tauri/src/lib.rs`：注册 IPC。
- `src-tauri/src/repositories/mod.rs`：导出 OperationRepository。
- `src/api/contracts.ts`、`src/api/tauri.ts`、`src/api/tauri.test.ts`、`src/api/preview.ts`：同步 IPC DTO。

**创建：**

- `src-tauri/src/core/tasks/planner.rs`：纯函数参数校验、实现选择和计划生成。
- `src-tauri/src/domain/operation.rs`：运行、步骤、预演、结果和状态 DTO。
- `src-tauri/src/repositories/operation_repository.rs`：运维运行持久化。
- `src-tauri/src/services/operation_service.rs`：预检、预演和运行编排。
- `src-tauri/src/commands/operations.rs`：Tauri IPC。
- `src-tauri/migrations/0004_operations.sql`：运行和步骤表。
- `src-tauri/tests/task_planner.rs`：planner 单元集成测试。
- `src-tauri/tests/operation_repository_integration.rs`：迁移和状态测试。
- `src-tauri/tests/operation_service_integration.rs`：服务编排测试。

### Task 1: 扩展任务定义与分类

**Files:**
- Modify: `src-tauri/src/core/tasks/model.rs`
- Modify: `src-tauri/src/core/tasks/mod.rs`
- Test: `src-tauri/tests/task_catalog.rs`

- [ ] **Step 1: 写失败测试**

在 `task_catalog.rs` 增加：

```rust
#[test]
fn v2_task_definition_serializes_safe_metadata_but_not_commands() {
    let definition = built_in_catalog()
        .into_iter()
        .find(|item| item.id == "system.overview")
        .unwrap();
    assert_eq!(definition.category, TaskCategory::System);
    assert_eq!(definition.privilege, PrivilegeRequirement::CurrentUser);
    assert_eq!(definition.scope, ExecutionScope::ReadOnlyBatch);
    assert!(!definition.implementations[0].execution_steps.is_empty());

    let encoded = serde_json::to_string(&definition).unwrap();
    assert!(encoded.contains("estimatedSeconds"));
    assert!(!encoded.contains("uname -a"));
    assert!(!encoded.contains("commandTemplate"));
}

#[test]
fn task_categories_cover_the_operations_center() {
    for category in [
        TaskCategory::System,
        TaskCategory::Storage,
        TaskCategory::Network,
        TaskCategory::Security,
        TaskCategory::Service,
        TaskCategory::Logs,
        TaskCategory::Web,
        TaskCategory::Container,
        TaskCategory::Script,
        TaskCategory::Advanced,
    ] {
        assert!(!category.as_str().is_empty());
    }
}
```

- [ ] **Step 2: 运行测试并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_catalog v2_task_definition -- --nocapture
```

预期：因 `PrivilegeRequirement`、`ExecutionScope` 和 V2 字段不存在而编译失败。

- [ ] **Step 3: 实现精确模型**

在 `model.rs` 增加并导出：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeRequirement {
    CurrentUser,
    RootOrPasswordlessSudo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    SingleServer,
    ReadOnlyBatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStep {
    pub id: String,
    pub title: String,
    pub timeout_seconds: u64,
    pub output_limit_bytes: u64,
    #[serde(skip_serializing)]
    pub command_template: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupItemKind {
    RemoteFile,
    CommandSnapshot,
    ManagedBlock,
    RuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupItemDefinition {
    pub id: String,
    pub kind: BackupItemKind,
    #[serde(skip_serializing, skip_deserializing)]
    pub target_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPlan {
    pub items: Vec<BackupItemDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPlan {
    pub steps: Vec<TaskStep>,
    pub automatic_on_failure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultParserKind {
    Text,
    KeyValue,
    Table,
    HealthSummary,
    NetworkProbe,
    ServiceStatus,
    ContainerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskImplementation {
    pub id: String,
    pub compatibility: CompatibilityPredicate,
    pub preflight_steps: Vec<TaskStep>,
    pub preview_steps: Vec<TaskStep>,
    pub backup_plan: Option<BackupPlan>,
    pub execution_steps: Vec<TaskStep>,
    pub verify_steps: Vec<TaskStep>,
    pub rollback_plan: Option<RollbackPlan>,
    pub result_parser: ResultParserKind,
}
```

把 `TaskDefinition` 扩展为：

```rust
pub struct TaskDefinition {
    pub id: String,
    pub version: i32,
    pub category: TaskCategory,
    pub title: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub estimated_seconds: u32,
    pub privilege: PrivilegeRequirement,
    pub scope: ExecutionScope,
    pub parameters: Vec<ParameterDefinition>,
    pub implementations: Vec<TaskImplementation>,
    pub output_kind: OutputKind,
}
```

扩展 `TaskCategory` 及 `as_str()`，使用固定字符串 `system/storage/network/security/service/logs/web/container/script/advanced`。

- [ ] **Step 4: 迁移现有 catalog helper**

`catalog.rs` 的现有任务默认使用：

```rust
estimated_seconds: 30,
privilege: PrivilegeRequirement::CurrentUser,
scope: if risk_level == RiskLevel::Safe {
    ExecutionScope::ReadOnlyBatch
} else {
    ExecutionScope::SingleServer
},
```

把现有单个 `command_template` 放入一个 ID 为 `execute` 的 `execution_steps`，并把同一只读状态读取放入 `preview_steps`；现有系统/服务/日志任务分别使用 `KeyValue`、`ServiceStatus`、`Text` parser。现有 safe 任务的 backup/rollback 为 None，verify_steps 为空；危险任务将在第三阶段逐项补齐非空 backup/verify/rollback。步骤元数据可以进入 IPC，但 `command_template` 和 `target_template` 必须跳过序列化。

- [ ] **Step 5: 运行目录测试**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_catalog
```

预期：全部通过；旧 8 个稳定 ID 不变。

- [ ] **Step 6: 提交**

```powershell
git add -- src-tauri/src/core/tasks/model.rs src-tauri/src/core/tasks/mod.rs src-tauri/src/core/tasks/catalog.rs src-tauri/tests/task_catalog.rs
git commit -m "feat: define operations task model v2"
```

### Task 2: 扩展强类型参数验证

**Files:**
- Modify: `src-tauri/src/core/tasks/model.rs`
- Modify: `src-tauri/src/core/tasks/parameters.rs`
- Test: `src-tauri/tests/task_catalog.rs`

- [ ] **Step 1: 写恶意输入失败测试**

```rust
#[test]
fn operations_parameters_reject_shell_structure_and_out_of_range_values() {
    let definition = parameter_fixture(vec![
        kind("host", ParameterKind::Host),
        kind("port", ParameterKind::Port),
        kind("interface", ParameterKind::InterfaceName),
        kind("cidr", ParameterKind::Cidr),
        kind("container", ParameterKind::ContainerName),
        kind("mode", ParameterKind::FileMode),
        kind("cron", ParameterKind::CronExpression),
    ]);
    for bad in [
        json!({"host":"a;id","port":22,"interface":"eth0","cidr":"10.0.0.1/24","container":"web","mode":"0644","cron":"0 2 * * *"}),
        json!({"host":"example.com","port":0,"interface":"eth0","cidr":"10.0.0.1/24","container":"web","mode":"0644","cron":"0 2 * * *"}),
        json!({"host":"example.com","port":22,"interface":"../../x","cidr":"10.0.0.1/24","container":"web","mode":"0644","cron":"0 2 * * *"}),
        json!({"host":"example.com","port":22,"interface":"eth0","cidr":"10.0.0.1/99","container":"web","mode":"0644","cron":"0 2 * * *"}),
        json!({"host":"example.com","port":22,"interface":"eth0","cidr":"10.0.0.1/24","container":"$(id)","mode":"0644","cron":"0 2 * * *"}),
        json!({"host":"example.com","port":22,"interface":"eth0","cidr":"10.0.0.1/24","container":"web","mode":"4777","cron":"0 2 * * *"}),
        json!({"host":"example.com","port":22,"interface":"eth0","cidr":"10.0.0.1/24","container":"web","mode":"0644","cron":"@reboot id"}),
    ] {
        assert!(validate_parameters(&definition, &bad).is_err());
    }
}
```

- [ ] **Step 2: 运行并确认新枚举不存在**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_catalog operations_parameters -- --nocapture
```

- [ ] **Step 3: 实现参数类型**

给 `ParameterKind` 增加：

```rust
Host,
Port,
InterfaceName,
Cidr,
ContainerName,
FileMode,
CronExpression,
MultiSelect { options: Vec<String>, max_items: usize },
```

固定校验规则：

- Host：1–253 字符，只允许 ASCII 字母数字、点、短横线、冒号；拒绝首尾短横线和空标签。
- Port：JSON 整数 1–65535。
- InterfaceName：1–32 字符，只允许字母数字、点、冒号、下划线、短横线和 `any`。
- Cidr：使用 `std::net::IpAddr` 解析地址；IPv4 前缀 0–32，IPv6 前缀 0–128。
- ContainerName：1–128 字符，只允许字母数字、点、下划线、短横线、冒号。
- FileMode：字符串 `0000`–`0777`，不允许 setuid/setgid/sticky。
- CronExpression：恰好五个空白分隔字段；每个字段只允许数字、`*,-/`。
- MultiSelect：数组、去重、1 到 `max_items`，每项必须来自 options。

每种值都返回 `shell_quote()` 后的 `shell_value`；不得返回未引用用户文本。

- [ ] **Step 4: 增加个人脚本环境变量名校验函数**

```rust
pub fn script_parameter_env_name(name: &str) -> AppResult<String> {
    if name.is_empty()
        || name.len() > 32
        || name.starts_with("QZ_")
        || !name.as_bytes()[0].is_ascii_uppercase()
        || !name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(AppError::Validation("脚本参数名称无效".into()));
    }
    Ok(format!("QZ_PARAM_{name}"))
}
```

- [ ] **Step 5: 运行测试**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_catalog
```

- [ ] **Step 6: 提交**

```powershell
git add -- src-tauri/src/core/tasks/model.rs src-tauri/src/core/tasks/parameters.rs src-tauri/tests/task_catalog.rs
git commit -m "feat: validate operations task parameters"
```

### Task 3: 创建纯函数 planner

**Files:**
- Create: `src-tauri/src/core/tasks/planner.rs`
- Modify: `src-tauri/src/core/tasks/mod.rs`
- Create: `src-tauri/tests/task_planner.rs`

- [ ] **Step 1: 写 planner 失败测试**

```rust
#[test]
fn planner_selects_capability_and_never_exposes_commands() {
    let definition = built_in_catalog().into_iter()
        .find(|item| item.id == "service.status").unwrap();
    let plan = plan_task(
        &definition,
        &capabilities("openeuler", "openeuler", "systemd", &["systemctl"]),
        &json!({"service":"nginx.service"}),
    ).unwrap();
    assert_eq!(plan.implementation_id, "systemd-status");
    assert_eq!(plan.execution_steps.len(), 1);
    assert_eq!(plan.execution_steps[0].title, "执行任务");
    assert!(!serde_json::to_string(&plan.public_summary()).unwrap().contains("systemctl"));
}

#[test]
fn planner_rejects_batch_for_non_safe_tasks() {
    let definition = dangerous_fixture();
    assert!(validate_scope(&definition, 2).is_err());
}
```

- [ ] **Step 2: 运行并确认 `plan_task` 不存在**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_planner
```

- [ ] **Step 3: 实现 planner 类型与函数**

```rust
pub struct PlannedTask {
    pub definition_id: String,
    pub definition_version: i32,
    pub implementation_id: String,
    pub risk_level: RiskLevel,
    pub privilege: PrivilegeRequirement,
    pub parameters: ValidatedParameters,
    pub preflight_steps: Vec<RenderedTaskStep>,
    pub execution_steps: Vec<RenderedTaskStep>,
    pub result_parser: ResultParserKind,
}

pub struct RenderedTaskStep {
    pub id: String,
    pub title: String,
    pub command: String,
    pub timeout_seconds: u64,
    pub output_limit_bytes: u64,
}

pub fn plan_task(
    definition: &TaskDefinition,
    capabilities: &SystemCapabilities,
    input: &serde_json::Value,
) -> AppResult<PlannedTask>;

pub fn validate_scope(definition: &TaskDefinition, server_count: usize) -> AppResult<()>;
```

`public_summary()` 只返回 ID、版本、风险、权限、步骤标题和预计耗时，不返回 command。

- [ ] **Step 4: 运行 planner 与目录测试**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_planner
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_catalog
```

- [ ] **Step 5: 提交**

```powershell
git add -- src-tauri/src/core/tasks/planner.rs src-tauri/src/core/tasks/mod.rs src-tauri/tests/task_planner.rs
git commit -m "feat: plan compatible operations tasks"
```

### Task 4: 持久化运维运行和步骤状态

**Files:**
- Create: `src-tauri/migrations/0004_operations.sql`
- Create: `src-tauri/src/domain/operation.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Create: `src-tauri/src/repositories/operation_repository.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Create: `src-tauri/tests/operation_repository_integration.rs`

- [ ] **Step 1: 写迁移与状态失败测试**

测试创建运行、严格状态转换、步骤引用 execution、重启恢复：

```rust
#[tokio::test]
async fn operation_state_is_strict_and_interrupted_work_is_uncertain() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let repo = OperationRepository::new(database.pool().clone());
    let run = repo.create(NewOperationRun {
        server_id: "server-1".into(),
        task_id: "system.overview".into(),
        task_version: 2,
        risk_level: RiskLevel::Safe,
        parameters_summary: None,
    }).await.unwrap();
    repo.transition(run.id, OperationStatus::Preflighting).await.unwrap();
    repo.transition(run.id, OperationStatus::Running).await.unwrap();
    assert!(repo.transition(run.id, OperationStatus::PreviewReady).await.is_err());
    assert_eq!(repo.recover_interrupted().await.unwrap(), 1);
    assert_eq!(repo.get(run.id).await.unwrap().unwrap().run.status, OperationStatus::Uncertain);
}
```

- [ ] **Step 2: 运行并确认 migration/table 不存在**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_repository_integration
```

- [ ] **Step 3: 创建 migration**

`0004_operations.sql` 必须创建：

```sql
CREATE TABLE operation_runs (
  id TEXT PRIMARY KEY NOT NULL,
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  task_id TEXT NOT NULL,
  task_version INTEGER NOT NULL CHECK (task_version > 0),
  risk_level TEXT NOT NULL CHECK (risk_level IN ('safe','caution','dangerous')),
  status TEXT NOT NULL CHECK (status IN (
    'validating','preflighting','preview_ready','waiting_confirmation',
    'backing_up','running','verifying','succeeded','failed','cancelled',
    'uncertain','rollback_available','rolling_back','rolled_back',
    'rollback_partial','rollback_failed'
  )),
  parameters_summary TEXT CHECK (parameters_summary IS NULL OR length(CAST(parameters_summary AS BLOB)) <= 8192),
  result_json TEXT CHECK (result_json IS NULL OR length(CAST(result_json AS BLOB)) <= 65536),
  error_category TEXT,
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  finished_at INTEGER
);

CREATE TABLE operation_steps (
  run_id TEXT NOT NULL REFERENCES operation_runs(id) ON DELETE CASCADE,
  phase TEXT NOT NULL CHECK (phase IN ('preflight','backup','execute','verify','rollback')),
  step_index INTEGER NOT NULL CHECK (step_index >= 0),
  step_id TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','running','succeeded','failed','cancelled','uncertain','skipped')),
  execution_id TEXT REFERENCES executions(id) ON DELETE SET NULL,
  output_summary TEXT CHECK (output_summary IS NULL OR length(CAST(output_summary AS BLOB)) <= 8192),
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  started_at INTEGER,
  finished_at INTEGER,
  PRIMARY KEY (run_id, phase, step_index)
);

CREATE INDEX idx_operation_runs_created ON operation_runs(created_at DESC);
CREATE INDEX idx_operation_runs_server_status ON operation_runs(server_id,status,created_at DESC);
```

- [ ] **Step 4: 实现 domain 与 repository**

`OperationStatus::can_transition_to` 必须以显式 match 表达允许边，不能用序数比较。`recover_interrupted()` 只把 `preflighting/backing_up/running/verifying/rolling_back` 改为 `uncertain`。

- [ ] **Step 5: 运行测试**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_repository_integration
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test execution_repository_integration
```

- [ ] **Step 6: 提交**

```powershell
git add -- src-tauri/migrations/0004_operations.sql src-tauri/src/domain/operation.rs src-tauri/src/domain/mod.rs src-tauri/src/repositories/operation_repository.rs src-tauri/src/repositories/mod.rs src-tauri/tests/operation_repository_integration.rs
git commit -m "feat: persist operations runs and steps"
```

### Task 5: 实现 OperationService 的预检与只读运行骨架

**Files:**
- Create: `src-tauri/src/services/operation_service.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Create: `src-tauri/tests/operation_service_integration.rs`

- [ ] **Step 1: 写服务失败测试**

覆盖：未知任务在建 run 前拒绝、预检返回公开摘要、不泄漏 command、safe 运行引用现有 execution、dangerous 未确认不执行。

```rust
#[tokio::test]
async fn preflight_returns_public_plan_without_commands() {
    let services = fixture_services().await;
    let preview = services.operation_service().preflight(
        "server-1",
        OperationPreflightRequest {
            task_id: "system.overview".into(),
            task_version: 2,
            parameters: json!({}),
        },
    ).await.unwrap();
    let encoded = serde_json::to_string(&preview).unwrap();
    assert_eq!(preview.risk_level, RiskLevel::Safe);
    assert!(!encoded.contains("uname -a"));
}
```

- [ ] **Step 2: 运行并确认服务不存在**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_service_integration
```

- [ ] **Step 3: 提取底层步骤执行入口**

在 `ExecutionService` 增加 `execute_planned_step()`，参数固定为后端构造的 `RenderedTaskStep`，并创建 `executions` 记录；不要提供接受前端 command 的 IPC。

```rust
pub(crate) async fn execute_planned_step<E: EventSink>(
    &self,
    server_id: &str,
    task_id: &str,
    task_version: i32,
    category: &str,
    step: &RenderedTaskStep,
    parameters: &[ExecutionParameter],
    events: &mut E,
) -> AppResult<ExecutionDetails>;
```

- [ ] **Step 4: 实现 OperationService**

首阶段只执行 `safe`/`caution` 且无 backup 的任务；`dangerous` 返回 `preview_ready`，由危险恢复子计划接管后续。服务必须先 connect 获取能力，再 `plan_task`，disconnect 后返回公开预演。

- [ ] **Step 5: 运行服务与旧执行测试**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_service_integration
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test execution_services
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test workflow_execution_nodes
```

- [ ] **Step 6: 提交**

```powershell
git add -- src-tauri/src/services/operation_service.rs src-tauri/src/services/execution_service.rs src-tauri/src/services/app_services.rs src-tauri/src/services/mod.rs src-tauri/tests/operation_service_integration.rs
git commit -m "feat: preflight and run planned operations"
```

### Task 6: 添加 IPC 与 TypeScript 契约

**Files:**
- Create: `src-tauri/src/commands/operations.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/tauri.test.ts`
- Modify: `src/api/preview.ts`

- [ ] **Step 1: 先写前端 IPC 失败测试**

```ts
it('uses typed operations IPC without accepting command text', async () => {
  await api.preflightOperation('server-1', {
    taskId: 'system.overview',
    taskVersion: 2,
    parameters: {},
  });
  expect(invoke).toHaveBeenCalledWith('preflight_operation', {
    serverId: 'server-1',
    request: { taskId: 'system.overview', taskVersion: 2, parameters: {} },
  });

  await api.startOperation('server-1', {
    taskId: 'system.overview', taskVersion: 2, parameters: {}, confirmedPreviewId: null,
  }, expect.any(Function));
  expect(JSON.stringify(invoke.mock.calls)).not.toContain('commandTemplate');
});
```

- [ ] **Step 2: 运行并确认 API 不存在**

```powershell
pnpm exec vitest run src/api/tauri.test.ts
```

- [ ] **Step 3: 创建 Rust commands**

提供固定命令：

```rust
list_operations_tasks(server_id)
preflight_operation(server_id, request)
start_operation(server_id, request, channel)
cancel_operation(run_id)
get_operation(run_id)
list_operations(filter)
```

`start_operation` 的 request 只能包含 taskId、taskVersion、parameters、confirmedPreviewId；不能包含 command、script、backup 或 rollback 字段。

- [ ] **Step 4: 同步 TypeScript DTO 和 preview backend**

TypeScript `TaskDefinition` 增加 category/estimatedSeconds/privilege/scope；添加 `OperationPreview`、`OperationRunDetails`、`OperationEvent`。preview 数据至少提供一个 safe task 和一个 dangerous task，使 UI 测试不依赖 Tauri。

- [ ] **Step 5: 运行 IPC、前端和 Rust command contract 测试**

```powershell
pnpm exec vitest run src/api/tauri.test.ts src/api/preview.test.ts
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml commands::executions
```

- [ ] **Step 6: 提交**

```powershell
git add -- src-tauri/src/commands/operations.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/api/contracts.ts src/api/tauri.ts src/api/tauri.test.ts src/api/preview.ts src/api/preview.test.ts
git commit -m "feat: expose typed operations ipc"
```

### Task 7: 基础阶段回归和文档

**Files:**
- Modify: `docs/development.md`
- Modify: `docs/security.md`
- Modify: `docs/support-matrix.md`

- [ ] **Step 1: 更新文档**

写明任务 V2 的后端命令边界、root/`sudo -n` 策略、operation 与 execution 的关系、当前阶段尚未启用危险修改。

- [ ] **Step 2: 运行阶段全量验证**

```powershell
pnpm test
pnpm build
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
git diff --check
```

预期：前端、构建、全部非 live Rust 测试通过；不产生 C 盘或 D 盘根目录文件。

- [ ] **Step 3: 提交**

```powershell
git add -- docs/development.md docs/security.md docs/support-matrix.md
git commit -m "docs: document operations engine v2"
```
