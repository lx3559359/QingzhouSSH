# 危险运维任务与可验证恢复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为全部内置修改类运维任务实现 root/免密 sudo 预检、只读预演、项目内恢复点、执行后验证、显式回滚和网络断线保护，任何无法确认的修改都显示为状态不确定。

**Architecture:** 在 OperationService 与现有 ExecutionService 之间加入危险任务状态机和恢复服务。普通文件/状态恢复点复用工作流已有的 SFTP、SHA-256 和路径约束能力；网络与关键防火墙操作额外建立远端一次性超时回滚资产。前端仍只提交任务 ID、版本、参数和确认令牌，不提交命令、备份路径或回滚内容。

**Tech Stack:** Rust、Tokio、SQLx/SQLite、russh/SFTP、SHA-256、Tauri IPC、本地 AsyncSSH 夹具。

---

## 文件结构

**创建：**

- `src-tauri/migrations/0006_operation_restore_points.sql`
- `src-tauri/src/core/tasks/recovery.rs`
- `src-tauri/src/domain/operation_restore.rs`
- `src-tauri/src/repositories/operation_restore_repository.rs`
- `src-tauri/src/services/operation_restore_service.rs`
- `src-tauri/src/services/remote_recovery_service.rs`
- `src-tauri/tests/operation_privilege.rs`
- `src-tauri/tests/dangerous_task_catalog.rs`
- `src-tauri/tests/operation_restore_points.rs`
- `src-tauri/tests/operation_network_recovery.rs`
- `src-tauri/tests/operations_live.rs`

**修改：**

- `src-tauri/src/core/tasks/model.rs`：危险步骤、备份、验证和回滚合同。
- `src-tauri/src/core/tasks/catalog/system.rs`：主机名与时区修改任务。
- `src-tauri/src/core/tasks/catalog/storage.rs`：Swap 修改任务。
- `src-tauri/src/core/tasks/catalog/network.rs`：hosts 与 IP 修改任务。
- `src-tauri/src/core/tasks/catalog/security.rs`：文件权限与防火墙任务。
- `src-tauri/src/core/tasks/catalog/services.rs`：服务与 Cron 修改任务。
- `src-tauri/src/core/tasks/catalog/containers.rs`：容器动作任务。
- `src-tauri/src/core/tasks/mod.rs`、`src-tauri/src/domain/mod.rs`、`src-tauri/src/repositories/mod.rs`：导出新类型。
- `src-tauri/src/services/operation_service.rs`：危险任务生命周期、确认和恢复调用。
- `src-tauri/src/services/app_services.rs`、`src-tauri/src/services/mod.rs`：注册恢复服务。
- `src-tauri/src/commands/operations.rs`、`src-tauri/src/commands/mod.rs`、`src-tauri/src/lib.rs`：预演、确认、回滚和清理 IPC。
- `src/api/contracts.ts`、`src/api/tauri.ts`、`src/api/preview.ts`：恢复状态契约。
- `src-tauri/src/services/restore_point_service.rs`、`src-tauri/src/core/workflows/restore_paths.rs`：提取可复用的安全备份原语，不改变工作流行为。

## Task 1: 固化权限预检与危险状态机

**Files:**
- Modify: `src-tauri/src/core/tasks/model.rs`
- Modify: `src-tauri/src/services/operation_service.rs`
- Create: `src-tauri/tests/operation_privilege.rs`

- [ ] **Step 1: 先写 root、sudo 和无权限失败测试**

```rust
#[tokio::test]
async fn dangerous_task_accepts_only_root_or_passwordless_sudo() {
    assert_eq!(preflight_for("0\n", "").await, PrivilegeMode::Root);
    assert_eq!(preflight_for("1000\n", "sudo-ok").await, PrivilegeMode::PasswordlessSudo);
    let error = preflight_for("1000\n", "sudo-failed").await.unwrap_err();
    assert_eq!(error.code, "passwordless_sudo_required");
    assert!(!error.user_message.contains("sudo -S"));
}

#[test]
fn dangerous_lifecycle_rejects_running_before_backup() {
    assert!(OperationStatus::BackingUp.can_transition_to(OperationStatus::Running));
    assert!(!OperationStatus::WaitingConfirmation.can_transition_to(OperationStatus::Running));
}
```

- [ ] **Step 2: 运行并确认测试因缺少预检/状态失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_privilege
```

- [ ] **Step 3: 实现固定权限探测**

先运行 `id -u`；非 0 时只运行 `sudo -n true`。后续需要提升权限的内置命令由 planner 在命令前加固定 `sudo -n --`，禁止 `sudo -S`、`SUDO_ASKPASS`、密码参数和交互 stdin。预检失败返回中文说明“当前账号不是 root，且未配置免密 sudo；服务器未发生修改”。

- [ ] **Step 4: 扩展状态转换**

状态必须包含 `validating -> preflighting -> preview_ready -> waiting_confirmation -> backing_up -> running -> verifying`，以及 `succeeded|failed|uncertain|rollback_available|rolling_back|rolled_back|rollback_partial|rollback_failed`。网络断开、进程结果未知或应用重启后无法核验时只能进入 `uncertain`。

- [ ] **Step 5: 启用危险任务合同并增加一致性校验**

沿用第一阶段已经建立的恢复合同字段；步骤标题和恢复能力元数据可以序列化，但具体命令模板与备份目标模板继续禁止序列化：

```rust
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

pub struct BackupPlan {
    pub items: Vec<BackupItemDefinition>,
}

pub struct RollbackPlan {
    pub steps: Vec<TaskStep>,
    pub automatic_on_failure: bool,
}
```

增加目录一致性测试：`BackupItemDefinition` 只允许 `RemoteFile`、`CommandSnapshot`、`ManagedBlock`、`RuntimeState` 四类，目标由后端模板和已验证参数生成。safe/caution 任务使用空 backup/rollback；所有任务都必须有至少一个无副作用 preview step。

- [ ] **Step 6: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_privilege
git add -- src-tauri/src/core/tasks/model.rs src-tauri/src/services/operation_service.rs src-tauri/tests/operation_privilege.rs
git commit -m "feat: enforce dangerous operation privilege lifecycle"
```

## Task 2: 持久化任务恢复点和恢复项

**Files:**
- Create: `src-tauri/migrations/0006_operation_restore_points.sql`
- Create: `src-tauri/src/domain/operation_restore.rs`
- Create: `src-tauri/src/repositories/operation_restore_repository.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Create: `src-tauri/tests/operation_restore_points.rs`

- [ ] **Step 1: 写迁移与状态约束失败测试**

测试创建恢复点和多个恢复项、按运行查询、重启后恢复、成功回滚后二次消费拒绝，以及非法状态无法写入。

```rust
#[tokio::test]
async fn consumed_restore_point_cannot_be_rolled_back_twice() {
    let repo = fixture_repo().await;
    let point = repo.create(fixture_restore_point()).await.unwrap();
    repo.mark_rolled_back(point.id).await.unwrap();
    let error = repo.begin_rollback(point.id).await.unwrap_err();
    assert_eq!(error.code(), "restore_point_already_consumed");
}
```

- [ ] **Step 2: 运行并确认缺表失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
```

- [ ] **Step 3: 创建严格数据模型**

`operation_restore_points` 至少包含 `id, operation_run_id, server_id, task_id, status, local_relative_dir, remote_asset_id, expires_at, created_at, updated_at`；`operation_restore_items` 至少包含 `id, restore_point_id, ordinal, item_kind, remote_target, local_relative_path, sha256, original_metadata_json, status, error_summary`。状态限定为 `creating|available|rolling_back|rolled_back|partial|failed|expired|cleanup_pending`。

恢复项只允许引用任务定义声明的目标；数据库中只保存数据根的相对路径，不保存本机绝对路径。删除 operation run 不得级联删除仍可恢复的恢复点。

- [ ] **Step 4: 实现原子状态更新与幂等占用**

`begin_rollback` 使用带当前状态条件的单条 UPDATE；只能从 `available|partial|failed` 进入 `rolling_back`。保留部分失败项和错误摘要，成功消费后仍保留审计元数据，但不能再次应用。

- [ ] **Step 5: 运行并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
git add -- src-tauri/migrations/0006_operation_restore_points.sql src-tauri/src/domain/operation_restore.rs src-tauri/src/domain/mod.rs src-tauri/src/repositories/operation_restore_repository.rs src-tauri/src/repositories/mod.rs src-tauri/tests/operation_restore_points.rs
git commit -m "feat: persist operation restore points"
```

## Task 3: 提取项目内备份原语并实现恢复服务

**Files:**
- Create: `src-tauri/src/core/tasks/recovery.rs`
- Create: `src-tauri/src/services/operation_restore_service.rs`
- Modify: `src-tauri/src/services/restore_point_service.rs`
- Modify: `src-tauri/src/core/workflows/restore_paths.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: `src-tauri/tests/operation_restore_points.rs`
- Test: `src-tauri/tests/workflow_restore_points.rs`

- [ ] **Step 1: 写路径逃逸、符号链接和哈希失败测试**

```rust
#[test]
fn task_backup_path_is_confined_to_data_root() {
    let relative = task_restore_dir(Uuid::nil()).unwrap();
    assert_eq!(relative, PathBuf::from("backups/tasks/00000000-0000-0000-0000-000000000000"));
    assert!(validate_restore_relative_path(Path::new("../../escape")).is_err());
    assert!(validate_restore_relative_path(Path::new("C:/escape")).is_err());
}
```

异步测试还必须覆盖：远端目标为符号链接时拒绝、SFTP 下载后 SHA-256 不匹配时恢复点失败、`.partial` 不被当作可用备份、工作流原恢复测试保持通过。

- [ ] **Step 2: 运行并确认缺少任务恢复实现**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test workflow_restore_points
```

- [ ] **Step 3: 提取共享安全原语**

把数据根路径拼接、`.partial` 原子改名、SHA-256、SFTP metadata 和路径约束提取为工作流与任务共同调用的函数。任务备份固定写入 `backups/tasks/<run_uuid>/`；前端不得传本地路径。不能改变现有 `backups/workflows` 目录和恢复语义。

- [ ] **Step 4: 实现备份、验证和逆序回滚**

`OperationRestoreService` 按声明顺序备份，全部成功后才把恢复点标为 available；回滚按 ordinal 逆序执行，每项恢复后重新读取远端文件或状态并核验 SHA-256/元数据。部分失败保留未完成项和本地资产，返回 `rollback_partial`。

- [ ] **Step 5: 运行并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test workflow_restore_points
git add -- src-tauri/src/core/tasks/recovery.rs src-tauri/src/services/operation_restore_service.rs src-tauri/src/services/restore_point_service.rs src-tauri/src/core/workflows/restore_paths.rs src-tauri/src/services/app_services.rs src-tauri/src/services/mod.rs src-tauri/tests/operation_restore_points.rs src-tauri/tests/workflow_restore_points.rs
git commit -m "feat: create verified operation backups"
```

## Task 4: 固化全部危险任务的恢复合同

**Files:**
- Modify: `src-tauri/src/core/tasks/catalog/system.rs`
- Modify: `src-tauri/src/core/tasks/catalog/storage.rs`
- Modify: `src-tauri/src/core/tasks/catalog/network.rs`
- Modify: `src-tauri/src/core/tasks/catalog/security.rs`
- Modify: `src-tauri/src/core/tasks/catalog/services.rs`
- Modify: `src-tauri/src/core/tasks/catalog/containers.rs`
- Create: `src-tauri/tests/dangerous_task_catalog.rs`

- [ ] **Step 1: 写逐 ID 完整性测试**

```rust
const REQUIRED_DANGEROUS_IDS: &[&str] = &[
    "system.hostname_change", "system.timezone_change", "storage.swap_manage",
    "security.file_permissions", "network.hosts_manage", "network.ip_change",
    "security.firewall_open_port", "service.start", "service.stop",
    "service.restart", "service.boot_policy", "service.cron_manage",
    "container.action",
];

#[test]
fn every_builtin_dangerous_task_has_recovery_contract() {
    for id in REQUIRED_DANGEROUS_IDS {
        let task = built_in_catalog().get(id).unwrap();
        assert_eq!(task.risk_level, RiskLevel::Dangerous, "{id}");
        assert_eq!(task.scope, ExecutionScope::SingleServer, "{id}");
        for implementation in &task.implementations {
            assert!(!implementation.preview_steps.is_empty(), "{id}");
            assert!(implementation.backup_plan.is_some(), "{id}");
            assert!(!implementation.verify_steps.is_empty(), "{id}");
            assert!(implementation.rollback_plan.is_some(), "{id}");
        }
    }
}
```

- [ ] **Step 2: 增加恢复矩阵断言**

| 任务 | 恢复点 | 成功验证 | 回滚验证 |
|---|---|---|---|
| hostname/timezone | 原值快照 | 新值精确匹配 | 原值恢复 |
| swap | `swapon`、fstab 条目、受控 swap 文件元数据 | 目标状态/容量匹配 | 原状态和 fstab 恢复 |
| file permissions | 路径、uid、gid、mode | `stat` 匹配 | 原元数据恢复 |
| hosts | `/etc/hosts` 文件与哈希 | 目标映射唯一且可解析 | 原文件哈希恢复 |
| firewall port | 当前后端与规则快照 | 规则存在且 SSH 复连 | 原规则集恢复 |
| service state/policy | active/enabled 状态 | 目标状态匹配 | 原状态恢复 |
| cron | 仅工具标识块/文件 | 条目标识和语法检查通过 | 原块恢复 |
| container action | running/paused/exit 状态 | 目标状态匹配 | 原状态恢复 |
| IP | 地址、路由和配置文件 | 新连接、地址、路由均通过 | 原网络恢复 |

- [ ] **Step 3: 运行并确认目录不完整导致失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test dangerous_task_catalog
```

- [ ] **Step 4: 仅补齐声明，不先写执行代码**

每个实现明确 compatibility、权限、参数、预演、备份、执行、验证和回滚；禁止通配符备份、任意路径或前端回滚命令。`service.cron_manage` 只能修改包含 `# qingzhou:<uuid>` 标识的工具条目，不允许删除系统其他 Cron。

- [ ] **Step 5: 提交合同**

```powershell
git add -- src-tauri/src/core/tasks/catalog/system.rs src-tauri/src/core/tasks/catalog/storage.rs src-tauri/src/core/tasks/catalog/network.rs src-tauri/src/core/tasks/catalog/security.rs src-tauri/src/core/tasks/catalog/services.rs src-tauri/src/core/tasks/catalog/containers.rs src-tauri/tests/dangerous_task_catalog.rs
git commit -m "test: define dangerous operation recovery contracts"
```

## Task 5: 实现系统、Swap 与文件权限修改闭环

**Files:**
- Modify: `src-tauri/src/core/tasks/catalog/system.rs`
- Modify: `src-tauri/src/core/tasks/catalog/storage.rs`
- Modify: `src-tauri/src/core/tasks/catalog/security.rs`
- Modify: `src-tauri/src/services/operation_service.rs`
- Test: `src-tauri/tests/dangerous_task_catalog.rs`
- Test: `src-tauri/tests/operation_restore_points.rs`

- [ ] **Step 1: 增加预演无副作用和回滚失败测试**

覆盖 hostname、timezone、swap create/enable/disable/remove、chmod/chown。预演只读取当前状态；Swap 文件只能在 `/swapfile` 或用户从任务提供的受限绝对路径中选择，创建大小限定 64 MiB–32 GiB；文件权限目标拒绝 `/`、`/etc`、`/usr` 等高层目录和符号链接。

- [ ] **Step 2: 运行目标测试并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test dangerous_task_catalog system_storage_permissions
```

- [ ] **Step 3: 实现发行版能力分支和验证**

hostname 优先 `hostnamectl`，timezone 优先 `timedatectl`；缺少能力时任务不可用，不自行安装工具。Swap 只改任务声明的文件和精确 fstab 行；权限操作使用经验证的 uid/gid/mode 和单一路径，不递归。

- [ ] **Step 4: 实现自动回滚触发**

执行步骤失败或验证失败时保留恢复点并返回中文影响说明；只有在连接仍可靠且回滚前置检查通过时自动尝试恢复。回滚结果分别映射为 rolled_back、rollback_partial、rollback_failed，不把“命令退出 0”直接等同于恢复成功。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test dangerous_task_catalog
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
git add -- src-tauri/src/core/tasks/catalog/system.rs src-tauri/src/core/tasks/catalog/storage.rs src-tauri/src/core/tasks/catalog/security.rs src-tauri/src/services/operation_service.rs src-tauri/tests/dangerous_task_catalog.rs src-tauri/tests/operation_restore_points.rs
git commit -m "feat: recover system storage and permission changes"
```

## Task 6: 实现服务、Cron 与容器动作闭环

**Files:**
- Modify: `src-tauri/src/core/tasks/catalog/services.rs`
- Modify: `src-tauri/src/core/tasks/catalog/containers.rs`
- Test: `src-tauri/tests/dangerous_task_catalog.rs`
- Test: `src-tauri/tests/operation_restore_points.rs`

- [ ] **Step 1: 写状态组合失败测试**

覆盖 systemd 与传统 service；Docker 与 Podman；active/inactive/failed、enabled/disabled/masked；容器 running/stopped/paused。断言只接受已通过现有 ServiceName/ContainerName 验证的单个目标。

- [ ] **Step 2: 写 Cron 所有权测试**

新增任务必须带工具 UUID 标识；停用/移除只能匹配同一标识；导入或手输的任意 crontab 行不能被内置任务删除。Cron 五段表达式和命令意图参数在 Rust 中验证，命令仅从受控快捷任务引用生成。

- [ ] **Step 3: 运行并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test dangerous_task_catalog service_cron_container
```

- [ ] **Step 4: 实现状态快照、动作、验证和恢复**

服务动作恢复到执行前 active/enabled 状态；容器动作恢复到执行前运行状态，不删除或重建容器；Cron 使用受控临时文件、语法校验和原子替换。任何 manager/runtime 缺失都在预检阶段结束。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test dangerous_task_catalog
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
git add -- src-tauri/src/core/tasks/catalog/services.rs src-tauri/src/core/tasks/catalog/containers.rs src-tauri/tests/dangerous_task_catalog.rs src-tauri/tests/operation_restore_points.rs
git commit -m "feat: recover service cron and container actions"
```

## Task 7: 实现 hosts 与防火墙修改闭环

**Files:**
- Modify: `src-tauri/src/core/tasks/catalog/network.rs`
- Modify: `src-tauri/src/core/tasks/catalog/security.rs`
- Create: `src-tauri/src/services/remote_recovery_service.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Test: `src-tauri/tests/operation_network_recovery.rs`

- [ ] **Step 1: 写后端识别和禁止性测试**

测试 firewalld、UFW、nftables、iptables 四种夹具；一次只允许 add/remove 一条由该任务管理的 TCP/UDP 端口规则。明确拒绝 disable firewall、flush、默认策略修改、任意规则文本和会阻断当前 SSH 端口的操作。

- [ ] **Step 2: 写远端资产安全测试**

恢复目录固定为 `${XDG_RUNTIME_DIR:-/tmp}/qingzhou-recovery/<run_uuid>` 的后端计算结果，必须由当前用户拥有且为 0700；文件 0600、脚本 0700。拒绝已存在目录、符号链接、所有者不匹配、路径穿越和错误校验和。

- [ ] **Step 3: 运行并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_network_recovery hosts_and_firewall
```

- [ ] **Step 4: 实现 hosts 原子写入和防火墙适配**

hosts 只管理 `# qingzhou:<uuid>` 标记的映射，写入前备份完整文件并校验目标不是符号链接，写入后解析并确认恰好一条目标映射。防火墙使用探测出的单一后端、保存可验证快照，并在执行后从规则列表重新确认。

- [ ] **Step 5: 为关键规则先安排自动回滚**

如果防火墙变化可能影响 SSH 连接，先创建和安排一次性恢复脚本，确认调度成功后才应用规则；验证原连接和新 SSH 连接都可用后取消。取消失败时结果为 warning 并保留 cleanup_pending，不谎报已清理。

- [ ] **Step 6: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_network_recovery hosts_and_firewall
git add -- src-tauri/src/core/tasks/catalog/network.rs src-tauri/src/core/tasks/catalog/security.rs src-tauri/src/services/remote_recovery_service.rs src-tauri/src/services/app_services.rs src-tauri/src/services/mod.rs src-tauri/tests/operation_network_recovery.rs
git commit -m "feat: protect hosts and firewall changes"
```

## Task 8: 实现 IP 修改的超时自动回滚

**Files:**
- Modify: `src-tauri/src/core/tasks/catalog/network.rs`
- Modify: `src-tauri/src/services/remote_recovery_service.rs`
- Modify: `src-tauri/src/services/operation_service.rs`
- Test: `src-tauri/tests/operation_network_recovery.rs`

- [ ] **Step 1: 写顺序与断连失败测试**

```rust
#[tokio::test]
async fn ip_change_arms_rollback_before_mutation_and_keeps_it_on_disconnect() {
    let trace = run_ip_change(FixtureOutcome::DisconnectAfterApply).await;
    assert!(trace.position("rollback_armed") < trace.position("network_applied"));
    assert!(!trace.contains("rollback_cancelled"));
    assert_eq!(trace.final_status(), OperationStatus::Uncertain);
}
```

再覆盖：新连接成功后才取消；地址验证失败触发回滚；客户端崩溃后调度仍生效；二次消费拒绝；NetworkManager、netplan 和 legacy ifcfg 只有能力明确时才出现对应实现。

- [ ] **Step 2: 运行并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_network_recovery ip_change
```

- [ ] **Step 3: 实现一次性恢复脚本与调度器选择**

恢复脚本内嵌执行 ID、原地址/路由/配置哈希和一次性标志。调度优先 `systemd-run --on-active`，其次能力探测后的 `at`；两者都不可用时任务灰显。默认 120 秒，允许 60–300 秒。没有成功安排调度不得修改网络。

- [ ] **Step 4: 实现双连接验证与核验恢复**

应用后用独立 SSH 连接验证目标地址、默认路由和 SSH 指纹；全部成功才取消回滚。断线时本地记录 `uncertain` 和远端资产期限；重新连接后读取一次性标志、当前网络和脚本状态，得出 succeeded、rolled_back 或仍 uncertain。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_network_recovery ip_change
git add -- src-tauri/src/core/tasks/catalog/network.rs src-tauri/src/services/remote_recovery_service.rs src-tauri/src/services/operation_service.rs src-tauri/tests/operation_network_recovery.rs
git commit -m "feat: auto rollback protected ip changes"
```

## Task 9: 暴露预演、确认、回滚和清理 API

**Files:**
- Modify: `src-tauri/src/commands/operations.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/preview.ts`
- Modify: `src/api/tauri.test.ts`

- [ ] **Step 1: 写 IPC 边界失败测试**

测试 `preview_operation` 返回服务器、权限、当前摘要、目标摘要、备份与断连风险；`confirm_operation` 只接受对应 run 的一次性确认 token；`rollback_operation` 只接受 restorePointId；`cleanup_operation_restore_assets` 只清理已过期/已消费资产。请求类型不得包含 command、localPath、remoteScript 或 sudoPassword。

- [ ] **Step 2: 运行并确认契约失败**

```powershell
pnpm exec vitest run src/api/tauri.test.ts
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points ipc
```

- [ ] **Step 3: 实现命令和 DTO**

增加 `preview_operation`、`confirm_operation`、`rollback_operation`、`inspect_uncertain_operation`、`cleanup_operation_restore_assets`。错误 DTO 固定回答 `whatHappened`、`serverMayHaveChanged`、`stateConfirmed`、`nextStep`、`restorePoint`，技术详情单独脱敏并限长。

- [ ] **Step 4: 测试并提交**

```powershell
pnpm exec vitest run src/api/tauri.test.ts
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_restore_points
git add -- src-tauri/src/commands/operations.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/api/contracts.ts src/api/tauri.ts src/api/preview.ts src/api/tauri.test.ts src-tauri/tests/operation_restore_points.rs
git commit -m "feat: expose dangerous operation recovery api"
```

## Task 10: Live 恢复闭环和阶段回归

**Files:**
- Create: `src-tauri/tests/operations_live.rs`
- Modify: `scripts/ssh-fixture.ps1`
- Modify: `scripts/dev-env.ps1`

- [ ] **Step 1: 扩展项目内夹具**

夹具只能在项目 `.local/ssh-fixture` 下创建模拟 hosts、服务、Cron、网络和容器状态，不修改开发机真实网络、防火墙、服务或系统文件。测试账号覆盖 root 模拟、`sudo -n` 成功和无权限三种响应。

- [ ] **Step 2: 写 ignored live 测试**

至少覆盖：危险预演零副作用；文件备份与哈希；执行/验证/回滚；验证失败后的恢复；断连 uncertain；IP 回滚先安排后修改；过期资产清理。所有测试结束核对 `backups/tasks` 位于 fixture 数据根。

- [ ] **Step 3: 运行目标 live 测试**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\ssh-fixture.ps1 -Action Start
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operations_live -- --ignored --nocapture
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\ssh-fixture.ps1 -Action Stop
```

- [ ] **Step 4: 运行阶段回归**

```powershell
pnpm test
pnpm build
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
git diff --check
```

- [ ] **Step 5: 路径与安全审计后提交**

确认没有 sudo 密码字段、`sudo -S`、任意回滚命令、C 盘 AppData 或 D 盘根路径；确认此阶段没有生成测试包或发布文件。

```powershell
git add -- src-tauri/tests/operations_live.rs scripts/ssh-fixture.ps1 scripts/dev-env.ps1
git commit -m "test: verify dangerous operation recovery live"
```
