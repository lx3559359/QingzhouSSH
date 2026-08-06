# Unified Tool Library and Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** 将现有“内置任务 / 我的脚本 / 高级执行”分散入口改造成面向 Linux 新手的统一工具库，准确区分可直接运行、可补齐组件、权限受限和确实不支持，并通过明确确认的固定白名单安装流程补齐常见依赖。

**Architecture:** 后端新增纯函数能力评估层与固定包映射，`ExecutionService` 返回结构化可用性而不是布尔值；组件安装由独立 `TaskRemediationService` 负责预览、短期确认令牌、非交互提权和执行后重探测。前端将内置任务、审核命令模板和个人脚本投影为统一 `ToolLibraryItem`，使用“分类栏 + 工具列表 + 固定详情栏”的响应式工作区。危险任务仍走现有 Operation 保护链，个人脚本仍走现有脚本预演链，高级原始命令不混入默认工具列表。

**Tech Stack:** Rust 2021、Tauri 2、russh、Tokio、Serde、React 19、TypeScript 5、Vitest、Testing Library、现有银白渐变设计系统。

---

## 实施约束

- 不静默安装软件包，不收集或传递 sudo 密码，不在安装完成后自动运行原任务。
- 只有静态白名单中的命令与软件包映射可以进入补齐流程；用户输入永远不能拼接到安装命令。
- 通用只读任务在未知发行版上以“实际命令是否存在”为准；危险或配置相关任务继续保留发行版、服务管理器和恢复能力限制。
- `network.ip_change` 在延迟回滚与重连验证真正完成前保持 `unsupported`，并显示可理解原因。
- 默认只展示 `ready`、`remediable`；`permission_blocked` 和 `unsupported` 通过可用状态筛选主动查看。
- 本计划完成后只生成项目目录内的本地测试包，不推送分支、不创建 GitHub/ModelScope 在线版本。

## Task 1: 建立结构化可用性模型与能力评估纯函数

**Files:**

- Create: `src-tauri/src/core/tasks/availability.rs`
- Modify: `src-tauri/src/core/tasks/mod.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Create: `src-tauri/tests/task_availability.rs`
- Modify: `src/api/contracts.ts`

- [ ] **Step 1: 写出失败的后端状态测试**

在 `task_availability.rs` 先覆盖：命令与平台条件全部满足时为 `Ready`；缺少命令、服务管理器不匹配和 `network.ip_change` 为 `Unsupported`。`Remediable` 的失败测试留到 Task 3，在白名单类型出现后再加入。

```rust
use qingzhou_ssh_lib::core::tasks::{
    evaluate_task_availability, TaskAvailabilityState,
};

let availability = evaluate_task_availability(&definition, &capabilities);
assert_eq!(availability.state, TaskAvailabilityState::Ready);
```

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_availability
```

预期：编译失败，提示 `availability` 模块和类型尚不存在。

- [ ] **Step 2: 实现最小领域类型**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAvailabilityState {
    Ready,
    Remediable,
    PermissionBlocked,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAvailabilityEvaluation {
    pub state: TaskAvailabilityState,
    pub implementation_id: Option<String>,
    pub summary: String,
    pub missing_commands: Vec<String>,
    pub blocking_capabilities: Vec<String>,
}
```

逐个实现计算缺失命令、发行版和服务管理器阻断，选择阻断最少的实现。此步骤先产出 `Ready/Unsupported`，Task 3 再接入包补齐。

- [ ] **Step 3: 替换旧布尔 DTO**

将 `TaskAvailability` 先改为 `definition/state/summary/missingCommands/remediation`。本任务同时定义可序列化但暂不填充的摘要：

```rust
pub struct TaskRemediationSummary {
    pub package_manager: String,
    pub missing_commands: Vec<String>,
    pub packages: Vec<String>,
}
```

`remediation` 固定为 `None`，Task 3 再接入白名单结果；`library` 字段在 Task 2 的元数据类型落地后加入。同步修改 `contracts.ts`，不保留 `compatible/reason` 长期兼容字段。

- [ ] **Step 4: 运行测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_availability
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_catalog
pnpm test -- src/api/tauri.test.ts
git add src-tauri/src/core/tasks/availability.rs src-tauri/src/core/tasks/mod.rs src-tauri/src/services/execution_service.rs src-tauri/tests/task_availability.rs src/api/contracts.ts
git commit -m "feat: model structured task availability"
```

## Task 2: 添加新手分类、中文别名和统一来源元数据

**Files:**

- Create: `src-tauri/src/core/tasks/library.rs`
- Modify: `src-tauri/src/core/tasks/mod.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Create: `src-tauri/tests/tool_library_metadata.rs`
- Modify: `src/api/contracts.ts`

- [ ] **Step 1: 写元数据完整性失败测试**

断言每个内置任务都有来源、主分类、中文关键词，并验证：`网站打不开 -> runbook.web.gateway`、`端口被占用 -> network.port_process`、`磁盘满了 -> runbook.storage.capacity_io`、`服务器很慢 -> runbook.cpu.incident`、`登录失败 -> security.ssh_events`。

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test tool_library_metadata
```

预期：因 `library.rs` 不存在而失败。

- [ ] **Step 2: 实现稳定元数据映射**

```rust
pub enum ToolSource { BuiltInTask, ReviewedCommand }

pub enum ToolLibraryCategory {
    RecommendedRecent, DailyInspection, Performance, Storage, Network,
    WebService, SecurityLogin, ServiceManagement, Container, SystemSettings,
}

pub struct ToolLibraryMetadata {
    pub source: ToolSource,
    pub primary_category: ToolLibraryCategory,
    pub keywords: Vec<String>,
    pub novice_aliases: Vec<String>,
}
```

单步安全只读任务标为 `ReviewedCommand`；多步 runbook、谨慎和危险任务标为 `BuiltInTask`。显式任务 ID 映射提供新手别名，未知新任务按现有 `TaskCategory` 得到安全默认分类。

- [ ] **Step 3: 随任务列表返回元数据并同步 TypeScript 类型**

`list_task_definitions` 每项调用 `metadata_for(&definition)`，并在此时给 `TaskAvailability` 增加 `library` 字段；TypeScript 增加同名联合类型。

- [ ] **Step 4: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test tool_library_metadata
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
git add src-tauri/src/core/tasks/library.rs src-tauri/src/core/tasks/mod.rs src-tauri/src/services/execution_service.rs src-tauri/tests/tool_library_metadata.rs src/api/contracts.ts
git commit -m "feat: classify tools for novice discovery"
```

## Task 3: 放宽通用只读任务并建立固定包白名单

**Files:**

- Modify: `src-tauri/src/core/tasks/catalog/helpers.rs`
- Create: `src-tauri/src/core/tasks/remediation.rs`
- Modify: `src-tauri/src/core/tasks/availability.rs`
- Modify: `src-tauri/src/core/tasks/mod.rs`
- Modify: `src-tauri/tests/task_availability.rs`
- Modify: `src-tauri/tests/readonly_task_catalog.rs`

- [ ] **Step 1: 增加发行版与包映射失败测试**

覆盖：未知发行版且命令齐全为 `Ready`；apt 缺 `tcpdump/dig` 分别给出 `tcpdump/dnsutils`；dnf 缺 `ncat/iostat` 分别给出 `nmap-ncat/sysstat`；未知包管理器缺命令为 `Unsupported`。

- [ ] **Step 2: 仅放宽通用只读 helper**

将 `read_only_implementation` 的 `os_families` 设为空数组。不要修改 `dangerous_implementation`，不要删除危险任务的 `SUPPORTED_FAMILIES`。

- [ ] **Step 3: 实现静态白名单**

| 能力 | apt | dnf/yum |
|---|---|---|
| `nc` / `ncat` | `netcat-openbsd` | `nmap-ncat` |
| `tcpdump` | `tcpdump` | `tcpdump` |
| `lsof` | `lsof` | `lsof` |
| `iostat` | `sysstat` | `sysstat` |
| `dig` | `dnsutils` | `bind-utils` |

```rust
pub fn remediation_for(
    package_manager: Option<&str>,
    missing_commands: &[String],
) -> Option<TaskRemediationSummary>;
```

所有缺失命令都有映射时才返回 `Some`；包名去重、固定排序，禁止猜测。

- [ ] **Step 4: 接入评估并保留 IP 修改安全阻断**

仅缺白名单命令时标为 `Remediable`；发行版或服务管理器不匹配仍为 `Unsupported`。`network.ip_change` 显式返回“延迟回滚与重连验证尚不可用”。

- [ ] **Step 5: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_availability
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test readonly_task_catalog
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test dangerous_task_catalog
git add src-tauri/src/core/tasks/catalog/helpers.rs src-tauri/src/core/tasks/remediation.rs src-tauri/src/core/tasks/availability.rs src-tauri/src/core/tasks/mod.rs src-tauri/tests/task_availability.rs src-tauri/tests/readonly_task_catalog.rs
git commit -m "fix: evaluate read only tools by capabilities"
```

## Task 4: 实现显式确认的组件补齐服务

**Files:**

- Create: `src-tauri/src/services/task_remediation_service.rs`
- Create: `src-tauri/src/commands/remediation.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/task_remediation.rs`
- Modify: `src-tauri/tests/operation_privilege.rs`

- [ ] **Step 1: 写安全边界失败测试**

验证固定安装命令、无任意 shell 拼接、无 `sudo -S/SUDO_ASKPASS/--stdin`、令牌五分钟过期且一次性、服务器/任务/实现/包管理器/包集合任一不匹配都被拒绝、安装后只重探测而不运行任务。

```rust
pub struct TaskRemediationPreview {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
    pub expires_at: i64,
    pub task_id: String,
    pub implementation_id: String,
    pub missing_commands: Vec<String>,
    pub packages: Vec<String>,
    pub package_manager: String,
    pub permission_state: TaskAvailabilityState,
    pub command_summary: String,
}

pub struct ConfirmTaskRemediationRequest {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
}
```

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_remediation
```

预期：服务和命令尚不存在，编译失败。

- [ ] **Step 2: 实现固定安装命令构造器**

只接受内部 `PackageManagerKind` 与 `PackageId` 枚举，生成 `apt-get install -y --no-install-recommends`、`dnf install -y` 或 `yum install -y`。使用现有 `probe_privilege`、`elevate_fixed_command`。

- [ ] **Step 3: 实现短期一次性预览注册表**

`TaskRemediationService` 持有 `Arc<Mutex<HashMap<Uuid, StoredRemediationPreview>>>`。`preview` 连接、重新分析并探测权限；`confirm` 原子取出记录，再次连接确认能力未发生不一致变化。

- [ ] **Step 4: 执行安装并重新探测**

复用 `ExecutionService` 的受限流式执行，历史记录使用固定任务 ID `maintenance.package_install`，记录目标服务器、包管理器、固定包集合和结果；超时固定 10 分钟、输出上限 2 MiB。成功、失败或用户取消后都重新探测；取消提示包管理器可能已发生部分变化，不能宣称自动回滚。成功后返回刷新后的 `TaskAvailability`；不得自动调用目标任务，也不得通过卸载软件包伪装成自动回滚。

- [ ] **Step 5: 暴露命令并接线**

```rust
preview_task_remediation(server_id, task_id, state)
confirm_task_remediation(server_id, request, on_event, state)
```

在 `AppServices` 构造服务并在 `lib.rs` 注册。任务列表存在可补齐项时，每次连接只额外探测一次权限；无免密 sudo 时状态改为 `PermissionBlocked`，但保留缺失组件说明。

- [ ] **Step 6: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test task_remediation
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_privilege
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test execution_services
git add src-tauri/src/core/tasks/remediation.rs src-tauri/src/services/task_remediation_service.rs src-tauri/src/commands/remediation.rs src-tauri/src/commands/mod.rs src-tauri/src/services/mod.rs src-tauri/src/services/app_services.rs src-tauri/src/services/execution_service.rs src-tauri/src/lib.rs src-tauri/tests/task_remediation.rs src-tauri/tests/operation_privilege.rs
git commit -m "feat: add confirmed task dependency remediation"
```

## Task 5: 扩展前端契约、API 与预览数据

**Files:**

- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/tauri.test.ts`
- Modify: `src/api/preview.ts`
- Modify: `src/api/preview.test.ts`

- [ ] **Step 1: 写 API 调用失败测试**

```ts
await api.previewTaskRemediation('server-1', 'network.packet_capture');
await api.confirmTaskRemediation(
  'server-1',
  { previewId: 'preview-1', confirmationToken: 'token-1' },
  onEvent,
);
```

```powershell
pnpm test -- src/api/tauri.test.ts
```

预期：方法不存在而失败。

- [ ] **Step 2: 增加 TypeScript 契约和调用**

增加 `TaskAvailabilityState`、`TaskRemediationSummary/Preview`、`ConfirmTaskRemediationRequest`；`tauri.ts` 使用单调 Channel 转发安装事件。

- [ ] **Step 3: 完善本地预览样本**

`preview.ts` 至少提供一个 `ready`、一个 `remediable`、一个 `permission_blocked` 和一个隐藏的 `unsupported` 样本。

- [ ] **Step 4: 通过并提交**

```powershell
pnpm test -- src/api/tauri.test.ts src/api/preview.test.ts
git add src/api/contracts.ts src/api/tauri.ts src/api/tauri.test.ts src/api/preview.ts src/api/preview.test.ts
git commit -m "feat: expose task remediation contracts"
```

## Task 6: 实现统一工具投影、分类和搜索纯函数

**Files:**

- Create: `src/features/tasks/library/types.ts`
- Create: `src/features/tasks/library/toolLibrary.ts`
- Create: `src/features/tasks/library/toolLibrary.test.ts`
- Modify: `src/features/tasks/scripts/types.ts`

- [ ] **Step 1: 写统一投影失败测试**

```ts
export type ToolLibraryItem =
  | { source: 'builtin_task' | 'reviewed_command'; id: string; availability: TaskAvailability; searchText: string }
  | { source: 'personal_script'; id: string; script: PersonalScriptSummary; searchText: string };
```

测试内置任务与个人脚本同库、中文别名、默认隐藏 `permission_blocked/unsupported`，并验证分类、来源、风险、可用状态、收藏、最近使用和文本采用 AND 逻辑。

```powershell
pnpm test -- src/features/tasks/library/toolLibrary.test.ts
```

预期：模块不存在而失败。

- [ ] **Step 2: 实现纯函数**

```ts
buildToolLibrary(tasks, scripts): ToolLibraryItem[]
filterToolLibrary(items, filters): ToolLibraryItem[]
groupCounts(items): Record<ToolLibraryCategory | 'my_scripts', number>
```

搜索文本连接标题、描述、关键词、中文别名、个人脚本分类与 tags。最近使用只影响“推荐与最近”的排序。

- [ ] **Step 3: 通过并提交**

```powershell
pnpm test -- src/features/tasks/library/toolLibrary.test.ts
git add src/features/tasks/library src/features/tasks/scripts/types.ts
git commit -m "feat: build unified searchable tool model"
```

## Task 7: 重构快捷任务页为固定详情栏工具库

**Files:**

- Create: `src/features/tasks/library/ToolCategoryRail.tsx`
- Create: `src/features/tasks/library/ToolCatalogList.tsx`
- Create: `src/features/tasks/library/ToolDetailPane.tsx`
- Create: `src/features/tasks/library/ToolLibraryFilters.tsx`
- Modify: `src/features/tasks/TaskPage.tsx`
- Modify: `src/features/tasks/TaskPage.test.tsx`
- Modify: `src/features/tasks/ParameterForm.tsx`
- Modify: `src/features/tasks/ExecutionDrawer.tsx`

- [ ] **Step 1: 用可访问性断言锁定三栏交互**

```ts
expect(screen.getByLabelText('工具分类')).toBeVisible();
expect(screen.getByLabelText('工具列表')).toBeVisible();
expect(screen.getByLabelText('工具详情')).toBeVisible();
```

点击第一屏任意工具后，参数和运行按钮必须直接出现在详情栏；不得依赖 `scrollIntoView`。默认不渲染“权限受限”和“不支持”工具，选择对应状态筛选后才出现。

```powershell
pnpm test -- src/features/tasks/TaskPage.test.tsx
```

预期：旧卡片网格与底部详情不满足断言。

- [ ] **Step 2: 实现稳定数据与选择状态**

并行加载任务和 `listPersonalScripts({ enabled: true })`。服务器切换保留分类和搜索；当前项消失时选择第一个可见项；筛选不应反复清空当前参数和结果。

- [ ] **Step 3: 实现桌面三栏和固定详情**

```tsx
<div className="tool-library-workspace">
  <ToolCategoryRail />
  <ToolCatalogList />
  <ToolDetailPane />
</div>
```

不可运行项不能整体 `disabled`，用户仍需查看原因。只有详情执行按钮按状态禁用。详情内用“参数 / 运行结果”页签承载 `ParameterForm` 和 `ExecutionDrawer`。

- [ ] **Step 4: 保留安全边界**

危险任务不得再调用会拒绝危险任务的 `startTaskExecution`：先调用 `previewOperation` 展示当前状态、目标状态、备份和断连风险，再携带一次性令牌调用 `confirmOperation`。安全/谨慎任务继续使用 `startTaskExecution`。高级执行保留独立入口，但原始命令不进入默认搜索结果。

- [ ] **Step 5: 通过并提交**

```powershell
pnpm test -- src/features/tasks/TaskPage.test.tsx src/features/tasks/AdvancedExecutionPanel.test.tsx
git add src/features/tasks/TaskPage.tsx src/features/tasks/TaskPage.test.tsx src/features/tasks/ParameterForm.tsx src/features/tasks/ExecutionDrawer.tsx src/features/tasks/library
git commit -m "feat: show tools with a fixed detail workspace"
```

## Task 8: 接入个人脚本运行与组件补齐

**Files:**

- Create: `src/features/tasks/library/TaskRemediationDialog.tsx`
- Create: `src/features/tasks/library/TaskRemediationDialog.test.tsx`
- Create: `src/features/tasks/library/PersonalScriptToolDetail.tsx`
- Modify: `src/features/tasks/library/ToolDetailPane.tsx`
- Modify: `src/features/tasks/scripts/ScriptCenter.tsx`
- Modify: `src/features/tasks/scripts/ScriptRunDialog.tsx`
- Modify: `src/features/tasks/TaskPage.test.tsx`

- [ ] **Step 1: 写补齐确认失败测试**

对话框展示缺失命令、软件包、服务器，以及“不会询问 sudo 密码、不会自动运行原任务”。第一次点击只预览；明确点击“确认安装组件”才确认。成功后重新拉取任务，但不得调用原任务执行。

- [ ] **Step 2: 实现补齐对话框**

`remediable` 提供“查看并补齐组件”；`permission_blocked` 仅给解决建议。流式输出位于对话框内部，技术错误折叠显示。

- [ ] **Step 3: 接入个人脚本详情**

按需加载脚本详情，显示分类、tags、启用状态、版本和扫描警告；运行复用 `ScriptRunDialog`。编辑、新建、导入、删除进入 `ScriptCenter` 管理模式。

- [ ] **Step 4: 通过并提交**

```powershell
pnpm test -- src/features/tasks/library/TaskRemediationDialog.test.tsx src/features/tasks/TaskPage.test.tsx src/features/tasks/scripts/ScriptCenter.test.tsx
git add src/features/tasks/library src/features/tasks/scripts/ScriptCenter.tsx src/features/tasks/scripts/ScriptRunDialog.tsx src/features/tasks/TaskPage.test.tsx
git commit -m "feat: run scripts and remediate tools from one library"
```

## Task 9: 实现宽窄窗口响应式布局与立体视觉

**Files:**

- Modify: `src/styles/theme.css`
- Modify: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `scripts/tests/desktop-ux.tests.ps1`

- [ ] **Step 1: 先写静态响应式守卫**

要求宽屏三列、详情 `position: sticky` 且内部滚动；窄屏分类横向滚动、列表单列、详情为 fixed 右侧 drawer；不允许固定内容宽度引发横向溢出。

```powershell
powershell -NoProfile -File .\scripts\tests\responsive-layout.tests.ps1
```

预期：缺少新选择器而失败。

- [ ] **Step 2: 实现样式和键盘行为**

桌面列建议为 `180px minmax(320px, .9fr) minmax(380px, 1.1fr)`。窄屏抽屉不超过视口，有遮罩、关闭按钮；Esc 关闭并还原焦点。保持白银渐变、强阴影和深色可读文字，遵守 `prefers-reduced-motion`。

- [ ] **Step 3: 通过并提交**

```powershell
powershell -NoProfile -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -File .\scripts\tests\desktop-ux.tests.ps1
pnpm test -- src/features/tasks/TaskPage.test.tsx
git add src/styles/theme.css scripts/tests/responsive-layout.tests.ps1 scripts/tests/desktop-ux.tests.ps1 src/features/tasks/TaskPage.test.tsx
git commit -m "style: adapt unified tool library across window sizes"
```

## Task 10: 文档、全量验证与本地测试包

**Files:**

- Modify: `docs/user-guide.md`
- Modify: `docs/support-matrix.md`
- Modify: `docs/security.md`
- Modify: `scripts/tests/public-docs.tests.ps1`

- [ ] **Step 1: 扩展文档测试并更新说明**

文档必须包含四种状态、分类搜索、个人脚本统一入口，以及“不自动安装、不索取 sudo 密码、不在安装后自动运行”。支持矩阵明确未知发行版只读任务按能力判断、包补齐只覆盖白名单、`network.ip_change` 仍受安全恢复限制。

- [ ] **Step 2: 运行完整验证**

```powershell
. .\scripts\dev-env.ps1 -Quiet
pnpm test
pnpm build
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo clippy --locked --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
powershell -NoProfile -File .\scripts\tests\public-docs.tests.ps1
powershell -NoProfile -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -File .\scripts\tests\desktop-ux.tests.ps1
git diff --check
```

预期：全部退出码为 0，无 clippy 警告和空白错误。

- [ ] **Step 3: 生成项目目录内本地测试包**

```powershell
$sourceVersion = (Get-Content -Raw .\package.json | ConvertFrom-Json).version
$testVersion = "$sourceVersion-local.tool-library.1"
powershell -NoProfile -File .\scripts\build-local-test.ps1 -PackageVersion $testVersion
```

预期输出位于 `D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test\`。若同名包已存在，递增本地后缀，不删除既有包。不调用发布脚本。

- [ ] **Step 4: 本地提交，不推送**

```powershell
git add docs/user-guide.md docs/support-matrix.md docs/security.md scripts/tests/public-docs.tests.ps1
git commit -m "docs: explain unified tool compatibility"
git status --short --branch
```

预期：工作树干净，分支只显示本地领先；不要执行 `git push`、`build-release.ps1` 或 ModelScope 上传。
