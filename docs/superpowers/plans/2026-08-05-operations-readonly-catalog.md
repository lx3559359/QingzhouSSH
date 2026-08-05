# 只读运维目录、Runbook、批量与报告 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在任务引擎 V2 上实现完整只读运维目录、9 个多步骤 Runbook、最多并发 3 台的只读批量执行和脱敏 TXT/JSON 报告。

**Architecture:** 按领域拆分 Rust catalog 文件，使用统一 helper 构造有界步骤；专用 result parser 把机器标记和受限原始输出转换成中文结论。批量服务只编排 safe 任务和只读内置 Runbook，每个服务器运行独立 OperationRun，报告服务只读取持久化结构化结果。

**Tech Stack:** Rust、Tokio semaphore、SQLx、Serde JSON、Tauri IPC、现有 SSH executor/redactor。

---

## 文件结构

**创建：**

- `src-tauri/src/core/tasks/catalog/system.rs`
- `src-tauri/src/core/tasks/catalog/storage.rs`
- `src-tauri/src/core/tasks/catalog/network.rs`
- `src-tauri/src/core/tasks/catalog/security.rs`
- `src-tauri/src/core/tasks/catalog/services.rs`
- `src-tauri/src/core/tasks/catalog/web.rs`
- `src-tauri/src/core/tasks/catalog/containers.rs`
- `src-tauri/src/core/tasks/catalog/runbooks.rs`
- `src-tauri/src/core/tasks/catalog/helpers.rs`
- `src-tauri/src/core/tasks/result.rs`
- `src-tauri/src/domain/operation_batch.rs`
- `src-tauri/src/repositories/operation_batch_repository.rs`
- `src-tauri/src/services/operation_batch_service.rs`
- `src-tauri/src/services/operation_report_service.rs`
- `src-tauri/migrations/0005_operation_batches.sql`
- `src-tauri/tests/readonly_task_catalog.rs`
- `src-tauri/tests/operation_results.rs`
- `src-tauri/tests/operation_batch_integration.rs`
- `src-tauri/tests/operation_reports.rs`

**修改：**

- `src-tauri/src/core/tasks/catalog.rs`：聚合各领域目录并检查重复 ID。
- `src-tauri/src/core/tasks/mod.rs`：导出结果类型。
- `src-tauri/src/domain/mod.rs`、`src-tauri/src/repositories/mod.rs`、`src-tauri/src/services/mod.rs`、`src-tauri/src/services/app_services.rs`：注册批量与报告服务。
- `src-tauri/src/services/operation_service.rs`：运行多步骤并保存结构化结果。
- `src-tauri/src/commands/operations.rs`、`src/api/contracts.ts`、`src/api/tauri.ts`、`src/api/preview.ts`：批次和报告 API。

### Task 1: 建立领域目录 helper 和稳定清单

**Files:**
- Create: `src-tauri/src/core/tasks/catalog/helpers.rs`
- Modify: `src-tauri/src/core/tasks/catalog.rs`
- Create: `src-tauri/tests/readonly_task_catalog.rs`

- [ ] **Step 1: 写完整 ID 失败测试**

测试必须逐项断言以下只读 ID，不能只检查数量：

```rust
const REQUIRED_READONLY_IDS: &[&str] = &[
    "system.overview", "system.cpu_pressure", "system.memory_oom",
    "system.process_top", "system.process_query", "system.process_detail",
    "system.kernel_events", "system.boot_history", "system.time",
    "system.disk_usage", "storage.mounts_inode", "storage.io_latency",
    "storage.large_directories", "storage.deleted_open_files",
    "network.interface_health", "network.tcp_states", "network.listening_ports",
    "network.port_process", "network.ip_route", "network.dns",
    "network.connectivity", "network.http", "network.tls", "network.udp",
    "network.packet_capture", "security.ssh_events", "security.firewall_exposure",
    "service.inventory", "service.failed_logs", "service.status",
    "service.scheduled_tasks", "logs.search", "web.config_check",
    "container.health_storage", "container.inspect",
];

#[test]
fn readonly_catalog_contains_every_stable_id_once() {
    let catalog = built_in_catalog();
    for id in REQUIRED_READONLY_IDS {
        assert_eq!(catalog.iter().filter(|item| item.id == *id).count(), 1, "{id}");
    }
}
```

- [ ] **Step 2: 运行并确认缺失 ID 导致失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
```

- [ ] **Step 3: 实现统一 helper**

```rust
pub fn read_only_task(
    id: &str,
    category: TaskCategory,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
) -> TaskDefinition {
    TaskDefinition {
        id: id.into(), version: 2, category, title: title.into(),
        description: description.into(), risk_level: RiskLevel::Safe,
        estimated_seconds, privilege: PrivilegeRequirement::CurrentUser,
        scope: ExecutionScope::ReadOnlyBatch, parameters, implementations,
        output_kind: OutputKind::KeyValue,
    }
}

pub fn bounded_step(id: &str, title: &str, seconds: u64, template: &str) -> TaskStep {
    TaskStep {
        id: id.into(), title: title.into(), timeout_seconds: seconds,
        output_limit_bytes: 1024 * 1024, command_template: template.into(),
    }
}
```

`catalog.rs::built_in_catalog()` 拼接所有模块后用 `BTreeSet` 检查重复 ID；重复时在测试构建中 panic，在生产中返回开发期不可达的固定目录。

- [ ] **Step 4: 提交 helper 和失败清单骨架**

```powershell
git add -- src-tauri/src/core/tasks/catalog/helpers.rs src-tauri/src/core/tasks/catalog.rs src-tauri/tests/readonly_task_catalog.rs
git commit -m "test: define readonly operations catalog contract"
```

### Task 2: 系统与存储只读任务

**Files:**
- Create: `src-tauri/src/core/tasks/catalog/system.rs`
- Create: `src-tauri/src/core/tasks/catalog/storage.rs`
- Modify: `src-tauri/src/core/tasks/catalog.rs`
- Test: `src-tauri/tests/readonly_task_catalog.rs`

- [ ] **Step 1: 增加命令与边界断言**

逐项断言 required commands 和参数：

```rust
#[test]
fn system_and_storage_tasks_are_bounded_and_safe() {
    let catalog = by_id();
    assert_eq!(catalog["storage.large_directories"].parameters.len(), 3);
    assert!(catalog["storage.large_directories"].implementations[0]
        .execution_steps[0].timeout_seconds <= 120);
    assert!(catalog["system.memory_oom"].implementations[0]
        .execution_steps.iter().all(|step| step.output_limit_bytes <= 1024 * 1024));
    assert!(catalog.values().filter(|item| matches!(item.category, TaskCategory::System | TaskCategory::Storage))
        .all(|item| item.risk_level == RiskLevel::Safe));
}
```

- [ ] **Step 2: 实现系统命令**

使用以下固定采集内容：

| ID | 参数 | 命令内容 |
|---|---|---|
| `system.overview` | 无 | `uname -a`、`uptime`、`/etc/os-release`、`who`、`free`、`df`、`ip -brief address` |
| `system.cpu_pressure` | 无 | `uptime`、CPU 数量、`ps --sort=-%cpu`、`/proc/pressure/cpu` |
| `system.memory_oom` | 无 | `free -b`、`vmstat 1 3`、内存热点、近 48 小时 OOM |
| `system.process_top` | limit 10–200 | CPU/内存排序，最多 limit 行 |
| `system.process_query` | query 1–128、limit 1–200 | `ps` 后 `grep -F -- {{query}}`，排除 grep 自身 |
| `system.process_detail` | pid 1–4194304 | `/proc/<pid>/status`、`cmdline`、`limits`、`ps -p` |
| `system.kernel_events` | hours 1–168 | `journalctl -k --since` 或有界 `dmesg` |
| `system.boot_history` | limit 10–100 | `who -b`、`last -x reboot shutdown` |
| `system.time` | 无 | `date -Is`、`timedatectl status`、`chronyc tracking` 或 `ntpq -pn` |

命令中所有 fallback 用显式 `if command -v`；不使用无界 `find /`、`du /` 或 `journalctl`。

- [ ] **Step 3: 实现存储命令**

| ID | 参数 | 命令内容 |
|---|---|---|
| `system.disk_usage` | 无 | `df -P -B1` |
| `storage.mounts_inode` | 无 | `findmnt` 或 `/proc/mounts`，`df -Pi` |
| `storage.io_latency` | samples 1–5 | `iostat -xz 1 N`，无 iostat 时 `vmstat 1 N` 和 `/proc/diskstats` |
| `storage.large_directories` | path、depth 1–5、limit 10–200 | `du -x -B1 --max-depth`、数值排序、tail limit |
| `storage.deleted_open_files` | limit 10–500 | `lsof -nP +L1`，缺失时返回 unsupported 机器标记 |

- [ ] **Step 4: 运行测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
```

```powershell
git add -- src-tauri/src/core/tasks/catalog/system.rs src-tauri/src/core/tasks/catalog/storage.rs src-tauri/src/core/tasks/catalog.rs src-tauri/tests/readonly_task_catalog.rs
git commit -m "feat: add system and storage diagnostics"
```

### Task 3: 网络与安全只读任务

**Files:**
- Create: `src-tauri/src/core/tasks/catalog/network.rs`
- Create: `src-tauri/src/core/tasks/catalog/security.rs`
- Modify: `src-tauri/src/core/tasks/catalog.rs`
- Test: `src-tauri/tests/readonly_task_catalog.rs`

- [ ] **Step 1: 写参数和风险失败测试**

```rust
#[test]
fn active_network_tasks_have_explicit_limits() {
    let catalog = by_id();
    assert_eq!(catalog["network.packet_capture"].risk_level, RiskLevel::Caution);
    assert_eq!(catalog["network.packet_capture"].scope, ExecutionScope::SingleServer);
    assert!(catalog["network.udp"].parameters.iter().any(|p| p.name == "attempts"));
    assert!(catalog["network.connectivity"].parameters.iter().any(|p| p.name == "host"));
}
```

- [ ] **Step 2: 实现固定网络任务**

| ID | 参数与限制 | 固定行为 |
|---|---|---|
| `network.interface_health` | 无 | `ip -brief link/address`、`ip -s link` |
| `network.tcp_states` | limit 20–500 | `ss -s` 和 established 汇总 |
| `network.listening_ports` | limit 20–500 | `ss -lntup` 或 `netstat -lntup` |
| `network.port_process` | port | 只过滤固定端口的监听进程 |
| `network.ip_route` | 无 | 地址、主路由、策略、邻居、MTU |
| `network.dns` | host | resolv.conf、getent、可选 resolvectl/dig |
| `network.connectivity` | host、count 1–20 | ping 和可选 tracepath，45 秒内 |
| `network.http` | host、port、tls | curl HEAD，8 秒 max-time，拒绝任意 URL 路径和参数 |
| `network.tls` | host、port | openssl s_client + x509 subject/issuer/dates，12 秒内 |
| `network.udp` | host、port、attempts 1–10、timeout 1–30 | listener、防火墙摘要、nc/ncat UDP 探测；无响应结果为 inconclusive |
| `network.packet_capture` | interface、host 可选、port 可选、count 1–200、seconds 1–30 | tcpdump 固定组合过滤，远端文件最大 16 MiB，下载后清理 |

抓包过滤器由 Rust 根据参数构造，不接受自定义 BPF 文本。

- [ ] **Step 3: 实现安全任务**

- `security.ssh_events`：优先 journalctl ssh/sshd 近 24 小时，fallback auth.log/secure，最多 300 行；附 `sshd -T` 受限字段、`last/lastb` 和 UID 0 账号摘要。
- `security.firewall_exposure`：监听端口和 firewalld/UFW/nftables/iptables 只读规则摘要；每类最多 300 行。

- [ ] **Step 4: 运行测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
```

```powershell
git add -- src-tauri/src/core/tasks/catalog/network.rs src-tauri/src/core/tasks/catalog/security.rs src-tauri/src/core/tasks/catalog.rs src-tauri/tests/readonly_task_catalog.rs
git commit -m "feat: add network and security diagnostics"
```

### Task 4: 服务、Web、容器与日志只读任务

**Files:**
- Create: `src-tauri/src/core/tasks/catalog/services.rs`
- Create: `src-tauri/src/core/tasks/catalog/web.rs`
- Create: `src-tauri/src/core/tasks/catalog/containers.rs`
- Modify: `src-tauri/src/core/tasks/catalog.rs`
- Test: `src-tauri/tests/readonly_task_catalog.rs`

- [ ] **Step 1: 写能力分支失败测试**

测试 systemd/service、Docker/Podman、Nginx/Apache 选择；选中的服务和容器必须存在于 capabilities discovery 中，否则 planner 拒绝。

- [ ] **Step 2: 实现目录**

| ID | 行为 |
|---|---|
| `service.inventory` | systemd 服务清单与 unit-file 状态；fallback `service --status-all` |
| `service.failed_logs` | 最多 20 个 failed unit，每个最多 100 行日志 |
| `service.status` | 保持旧 ID；只允许已发现服务，显示 Active/Sub/MainPID/ExecMainStatus |
| `service.scheduled_tasks` | systemd timers、当前用户 crontab、固定 cron 目录一级文件名 |
| `logs.search` | 保持旧 ID/API；卡片跳转现有智能日志页面，不重复实现内容搜索执行器 |
| `web.config_check` | nginx -t、apachectl configtest、80/443 监听摘要 |
| `container.health_storage` | Docker/Podman 版本、最多 100 容器、stats no-stream、system df |
| `container.inspect` | 已发现容器；action 仅 logs/inspect/stats；日志行数 10–5000 |

- [ ] **Step 3: 运行测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
```

```powershell
git add -- src-tauri/src/core/tasks/catalog/services.rs src-tauri/src/core/tasks/catalog/web.rs src-tauri/src/core/tasks/catalog/containers.rs src-tauri/src/core/tasks/catalog.rs src-tauri/tests/readonly_task_catalog.rs
git commit -m "feat: add service web and container diagnostics"
```

### Task 5: 结构化结果与中文建议

**Files:**
- Create: `src-tauri/src/core/tasks/result.rs`
- Modify: `src-tauri/src/core/tasks/mod.rs`
- Modify: `src-tauri/src/services/operation_service.rs`
- Create: `src-tauri/tests/operation_results.rs`

- [ ] **Step 1: 写 parser 失败测试**

```rust
#[test]
fn health_parser_returns_chinese_summary_and_bounded_details() {
    let raw = "__QZ_METRIC__ disk_percent=93\n__QZ_WARNING__ disk_usage\nsecret=token-canary";
    let result = parse_result(ResultParserKind::HealthSummary, raw, &redactor()).unwrap();
    assert_eq!(result.status, OperationConclusion::Warning);
    assert!(result.summary.contains("磁盘"));
    assert!(result.suggestions.iter().any(|item| item.contains("清理")));
    assert!(!serde_json::to_string(&result).unwrap().contains("token-canary"));
}

#[test]
fn udp_no_response_is_uncertain_not_failed() {
    let result = parse_result(ResultParserKind::NetworkProbe, "probe=no_response", &redactor()).unwrap();
    assert_eq!(result.status, OperationConclusion::Uncertain);
}
```

- [ ] **Step 2: 实现固定结构**

```rust
pub struct OperationResult {
    pub status: OperationConclusion,
    pub summary: String,
    pub findings: Vec<OperationFinding>,
    pub suggestions: Vec<String>,
    pub technical_details: String,
}

pub enum OperationConclusion { Normal, Warning, Failed, Uncertain }
pub struct OperationFinding { pub level: FindingLevel, pub title: String, pub detail: String }
```

机器标记只接受 `__QZ_METRIC__ key=value`、`__QZ_WARNING__ code`、`__QZ_ERROR__ code`、`__QZ_UNSUPPORTED__ capability`。未知标记保留在脱敏技术详情，不执行动态规则。

- [ ] **Step 3: 运行并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_results
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_service_integration
```

```powershell
git add -- src-tauri/src/core/tasks/result.rs src-tauri/src/core/tasks/mod.rs src-tauri/src/services/operation_service.rs src-tauri/tests/operation_results.rs
git commit -m "feat: parse operations into chinese findings"
```

### Task 6: 9 个内置多步骤 Runbook

**Files:**
- Create: `src-tauri/src/core/tasks/catalog/runbooks.rs`
- Modify: `src-tauri/src/core/tasks/catalog.rs`
- Test: `src-tauri/tests/readonly_task_catalog.rs`
- Test: `src-tauri/tests/operation_service_integration.rs`

- [ ] **Step 1: 写精确 Runbook 清单测试**

```rust
const RUNBOOK_IDS: &[&str] = &[
    "runbook.health.baseline", "runbook.cpu.incident", "runbook.memory.oom",
    "runbook.storage.capacity_io", "runbook.network.intermittent",
    "runbook.security.ssh_audit", "runbook.web.gateway",
    "runbook.container.runtime", "runbook.service.incident",
];
```

断言全部 safe、ReadOnlyBatch、2–6 步、每步 timeout ≤120 秒、output ≤1 MiB、总步骤 ≤6。

- [ ] **Step 2: 实现步骤组合**

- 综合巡检：system/resources/storage/network/services 五步。
- CPU：load/sampling/processes/scheduler 四步。
- 内存：overview/processes/oom/kernel 四步。
- 存储：capacity/growth/latency/open_deleted 四步。
- 网络：interfaces/route/dns/latency/path 五步，参数 host。
- SSH 审计：configuration/listeners/logins/accounts 四步。
- Web 502/504：services/listeners/configuration/logs/probe 五步，参数 host/port。
- 容器：runtime/inventory/resources/events 四步。
- 指定服务：status/process/logs/ports 四步，参数已发现服务多选，最多 10 个。

- [ ] **Step 3: 验证步骤失败即停止且历史完整**

服务测试使用 fake connector 让第二步失败，断言第三步保持 skipped、运行 failed、第一二步有独立 execution ID。

- [ ] **Step 4: 运行并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_service_integration
```

```powershell
git add -- src-tauri/src/core/tasks/catalog/runbooks.rs src-tauri/src/core/tasks/catalog.rs src-tauri/tests/readonly_task_catalog.rs src-tauri/tests/operation_service_integration.rs
git commit -m "feat: add builtin diagnostic runbooks"
```

### Task 7: 只读批量协调器

**Files:**
- Create: `src-tauri/migrations/0005_operation_batches.sql`
- Create: `src-tauri/src/domain/operation_batch.rs`
- Create: `src-tauri/src/repositories/operation_batch_repository.rs`
- Create: `src-tauri/src/services/operation_batch_service.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/commands/operations.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Create: `src-tauri/tests/operation_batch_integration.rs`

- [ ] **Step 1: 写并发和隔离失败测试**

```rust
#[tokio::test]
async fn readonly_batch_caps_concurrency_and_isolates_failures() {
    let fixture = BatchFixture::new(5).with_failure("server-3");
    let result = fixture.service.start(BatchRequest {
        server_ids: fixture.server_ids(),
        task_id: "system.overview".into(), task_version: 2, parameters: json!({}),
    }).await.unwrap();
    assert!(fixture.max_observed_concurrency() <= 3);
    assert_eq!(result.items.iter().filter(|item| item.status == BatchItemStatus::Failed).count(), 1);
    assert_eq!(result.items.iter().filter(|item| item.status == BatchItemStatus::Succeeded).count(), 4);
}

#[tokio::test]
async fn batch_rejects_caution_and_dangerous_tasks_before_creating_rows() {
    // network.packet_capture and service.restart must both be rejected.
}
```

- [ ] **Step 2: 创建 batch 表**

`operation_batches(id,task_id,task_version,status,created_at,finished_at)` 和 `operation_batch_items(batch_id,server_id,operation_run_id,status,error_message)`；状态严格限定 queued/running/succeeded/partial/failed/cancelled。

- [ ] **Step 3: 实现服务**

使用 `tokio::sync::Semaphore::new(3)`；先验证 server IDs 去重且 1–50 台、任务 risk=safe 且 scope=ReadOnlyBatch，再写数据库。取消 token 传给尚未开始和正在执行的子运行。

- [ ] **Step 4: 暴露 IPC**

增加 `start_operation_batch`、`cancel_operation_batch`、`get_operation_batch`；前端 request 不接受 concurrency 参数，固定为后端 3。

- [ ] **Step 5: 运行并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_batch_integration
pnpm exec vitest run src/api/tauri.test.ts
```

```powershell
git add -- src-tauri/migrations/0005_operation_batches.sql src-tauri/src/domain/operation_batch.rs src-tauri/src/repositories/operation_batch_repository.rs src-tauri/src/services/operation_batch_service.rs src-tauri/src/domain/mod.rs src-tauri/src/repositories/mod.rs src-tauri/src/services/mod.rs src-tauri/src/services/app_services.rs src-tauri/src/commands/operations.rs src/api/contracts.ts src/api/tauri.ts src/api/tauri.test.ts src-tauri/tests/operation_batch_integration.rs
git commit -m "feat: run readonly operations across servers"
```

### Task 8: TXT/JSON 脱敏报告

**Files:**
- Create: `src-tauri/src/services/operation_report_service.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/commands/operations.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Create: `src-tauri/tests/operation_reports.rs`

- [ ] **Step 1: 写路径、哈希和 canary 失败测试**

```rust
#[tokio::test]
async fn report_is_project_local_hashed_and_redacted() {
    let fixture = ReportFixture::with_result("password=report-canary").await;
    let file = fixture.service.export_run(fixture.run_id, ReportFormat::Json).await.unwrap();
    assert!(file.relative_path.starts_with("downloads/reports/"));
    let path = fixture.data_root.join(&file.relative_path);
    assert_eq!(file.sha256, sha256_local_file(&path).await.unwrap());
    let body = std::fs::read_to_string(path).unwrap();
    assert!(body.contains("[REDACTED]"));
    assert!(!body.contains("report-canary"));
    assert!(!body.contains(fixture.data_root.to_string_lossy().as_ref()));
}
```

- [ ] **Step 2: 实现报告服务**

后端生成 `downloads/reports/operation-<uuid>.json|txt` 和 `batch-<uuid>.json|txt`；先写 `.partial`，flush、计算 SHA-256 后原子改名。JSON 使用固定 schemaVersion=1；TXT 使用中文标题、结论、发现、建议和脱敏技术详情。

- [ ] **Step 3: 增加 IPC 并运行测试**

增加 `export_operation_report(run_id, format)` 和 `export_operation_batch_report(batch_id, format)`；前端不能传文件路径或文件名。

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_reports
pnpm exec vitest run src/api/tauri.test.ts
```

- [ ] **Step 4: 提交**

```powershell
git add -- src-tauri/src/services/operation_report_service.rs src-tauri/src/services/app_services.rs src-tauri/src/commands/operations.rs src/api/contracts.ts src/api/tauri.ts src/api/tauri.test.ts src-tauri/tests/operation_reports.rs
git commit -m "feat: export redacted operations reports"
```

### Task 9: 阶段回归

- [ ] 运行：

```powershell
pnpm test
pnpm build
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
git diff --check
```

- [ ] 确认无测试包、无 GitHub/魔塔改动、无项目外生成路径。
