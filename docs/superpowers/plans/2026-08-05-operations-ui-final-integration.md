# 运维中心 UI、全量验收与单包交付 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把任务引擎、全量目录、批量执行、危险恢复和脚本库整合为面向 Linux 小白的完整原生客户端流程，完成自适应布局、中文错误、全量回归与视觉验收后，只生成一个 r8 本地测试包。

**Architecture:** `OperationsCenter` 作为快捷任务页面容器，目录、向导、运行结果、批次、恢复和脚本中心拆成独立组件；所有能力与状态来自 Rust API。收藏和最近使用写入项目数据根 SQLite，不使用浏览器存储。页面在宽窗口使用目录+工作区，在小窗口切换为单栏分步导航，不按比例缩放文字，而是重排并保持局部滚动。

**Tech Stack:** React 19、TypeScript、Vitest、Testing Library、CSS container/media queries、Rust/SQLx、Tauri 2、PowerShell、本地原生窗口。

---

## 文件结构

**创建：**

- `src-tauri/migrations/0008_operation_preferences.sql`
- `src-tauri/src/domain/operation_preference.rs`
- `src-tauri/src/repositories/operation_preference_repository.rs`
- `src-tauri/tests/operation_preferences.rs`
- `src/features/tasks/OperationsCenter.tsx`
- `src/features/tasks/OperationsCenter.test.tsx`
- `src/features/tasks/TaskCatalog.tsx`
- `src/features/tasks/TaskCatalog.test.tsx`
- `src/features/tasks/TaskWizard.tsx`
- `src/features/tasks/TaskWizard.test.tsx`
- `src/features/tasks/OperationResultPanel.tsx`
- `src/features/tasks/OperationResultPanel.test.tsx`
- `src/features/tasks/OperationBatchPanel.tsx`
- `src/features/tasks/OperationBatchPanel.test.tsx`
- `src/features/tasks/OperationRecoveryPanel.tsx`
- `src/features/tasks/OperationRecoveryPanel.test.tsx`
- `src/features/tasks/fixtures.ts`
- `docs/design-qa/r8-operations-1920x1080.png`
- `docs/design-qa/r8-operations-1366x768.png`
- `docs/design-qa/r8-operations-1180x760.png`
- `docs/design-qa/r8-operations-960x640.png`
- `docs/milestone-5-operations-acceptance.md`

**修改：**

- `src-tauri/src/domain/mod.rs`、`src-tauri/src/repositories/mod.rs`、`src-tauri/src/services/app_services.rs`：注册任务偏好仓库。
- `src-tauri/src/commands/operations.rs`、`src-tauri/src/lib.rs`：偏好、复制为个人脚本和整合查询 API。
- `src/api/contracts.ts`、`src/api/errors.ts`、`src/api/tauri.ts`、`src/api/preview.ts`、`src/api/preview.test.ts`、`src/api/tauri.test.ts`：最终 DTO、中文错误和 preview 夹具。
- `src/features/tasks/TaskPage.tsx`、`src/features/tasks/TaskPage.test.tsx`：替换为 OperationsCenter 入口并保留受控高级执行。
- `src/features/tasks/ParameterForm.tsx`、`src/features/tasks/ExecutionDrawer.tsx`：复用扩展参数和技术详情。
- `src/features/tasks/scripts/ScriptCenter.tsx`：接入统一目录和结果流程。
- `src/features/tasks/scripts/ScriptCenter.test.tsx`：脚本整合回归。
- `src/features/tasks/scripts/ScriptList.tsx`：统一目录筛选与选择。
- `src/features/tasks/scripts/ScriptEditor.tsx`：正文生命周期回归。
- `src/features/tasks/scripts/ScriptRunDialog.tsx`：统一预演、确认和结果入口。
- `src/components/ContextMenu.tsx`、`src/components/ContextMenu.test.tsx`：任务对象右键菜单。
- `src/app/AppShell.tsx`、`src/app/AppShell.test.tsx`：小窗口内容区和自定义标题栏回归。
- `src/styles/theme.css`、`src/styles/components.css`、`src/styles/responsive.css`：最终立体化、自适应和滚动边界。
- `scripts/tests/responsive-layout.tests.ps1`、`scripts/tests/desktop-ux.tests.ps1`：源码契约。
- `docs/user-guide.md`、`docs/security.md`、`docs/support-matrix.md`：运维中心文档。

## Task 1: 把收藏和最近使用持久化到数据根

**Files:**
- Create: `src-tauri/migrations/0008_operation_preferences.sql`
- Create: `src-tauri/src/domain/operation_preference.rs`
- Create: `src-tauri/src/repositories/operation_preference_repository.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/commands/operations.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Create: `src-tauri/tests/operation_preferences.rs`

- [ ] **Step 1: 写持久化、排序和未知 ID 失败测试**

```rust
#[tokio::test]
async fn favorites_and_recent_tasks_survive_restart_without_browser_storage() {
    let db = fixture_db().await;
    let repo = OperationPreferenceRepository::new(db.pool());
    repo.set_favorite("system.overview", true).await.unwrap();
    repo.record_used("network.dns").await.unwrap();
    drop(repo);
    let reopened = OperationPreferenceRepository::new(db.reopen().await);
    assert_eq!(reopened.list_favorites().await.unwrap(), vec!["system.overview"]);
    assert_eq!(reopened.list_recent(8).await.unwrap()[0].task_id, "network.dns");
}
```

测试未知/已移除 task ID 不返回前端；最近使用按 last_used_at、use_count 排序，最多 12 条；收藏幂等。

- [ ] **Step 2: 运行并确认缺表失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_preferences
```

- [ ] **Step 3: 实现表和 API**

`operation_task_preferences(task_id PRIMARY KEY,is_favorite,use_count,last_used_at,updated_at)`。增加 `get_operation_preferences`、`set_operation_task_favorite`；任务成功启动时由后端 record_used，前端不能伪造使用次数。API 返回的 ID 与当前内置目录求交集。

- [ ] **Step 4: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operation_preferences
pnpm exec vitest run src/api/tauri.test.ts
git add -- src-tauri/migrations/0008_operation_preferences.sql src-tauri/src/domain/operation_preference.rs src-tauri/src/domain/mod.rs src-tauri/src/repositories/operation_preference_repository.rs src-tauri/src/repositories/mod.rs src-tauri/src/services/app_services.rs src-tauri/src/commands/operations.rs src-tauri/src/lib.rs src/api/contracts.ts src/api/tauri.ts src/api/tauri.test.ts src-tauri/tests/operation_preferences.rs
git commit -m "feat: persist operation favorites and recents"
```

## Task 2: 建立运维中心页面壳和完整目录

**Files:**
- Create: `src/features/tasks/OperationsCenter.tsx`
- Create: `src/features/tasks/OperationsCenter.test.tsx`
- Create: `src/features/tasks/TaskCatalog.tsx`
- Create: `src/features/tasks/TaskCatalog.test.tsx`
- Create: `src/features/tasks/fixtures.ts`
- Modify: `src/features/tasks/TaskPage.tsx`
- Modify: `src/features/tasks/TaskPage.test.tsx`
- Modify: `src/api/preview.ts`
- Modify: `src/styles/components.css`

- [ ] **Step 1: 写目录流程失败测试**

```tsx
it('按中文意图搜索并解释任务为何不可用', async () => {
  render(<OperationsCenter api={previewApi()} />)
  await user.type(screen.getByRole('searchbox', { name: '搜索运维任务' }), '证书到期')
  expect(screen.getByRole('button', { name: /TLS 证书检查/ })).toBeVisible()
  await user.click(screen.getByRole('button', { name: /防火墙放行端口/ }))
  expect(screen.getByText('当前服务器未检测到受支持的防火墙管理工具')).toBeVisible()
  expect(screen.queryByText(/sudo -n|firewall-cmd/)).not.toBeInTheDocument()
})
```

另测推荐、系统、存储、网络、安全、服务、Web、容器、脚本、收藏、最近分类；任务卡显示用途、safe/caution/dangerous、预计耗时、权限和兼容性；搜索匹配标题、描述、中文别名和标签，不搜索隐藏命令。

- [ ] **Step 2: 运行并确认组件缺失**

```powershell
pnpm exec vitest run src/features/tasks/OperationsCenter.test.tsx src/features/tasks/TaskCatalog.test.tsx src/features/tasks/TaskPage.test.tsx
```

- [ ] **Step 3: 实现页面壳和服务器目标模式**

顶部提供单服务器选择；只有后端标记为 `ReadOnlyBatch` 的 safe 任务显示“批量服务器”切换。caution 抓包、dangerous 任务和个人脚本始终单机。无服务器时显示“先添加服务器”的单一下一步，不渲染空参数表单。

- [ ] **Step 4: 实现卡片和兼容说明**

卡片保持白银渐变、边缘高光和清晰阴影，文字对比度符合现有主题；危险色只用于风险徽标和确认，不把整张卡染红。不兼容卡可查看说明但不能进入运行步骤。

- [ ] **Step 5: 测试并提交**

```powershell
pnpm exec vitest run src/features/tasks/OperationsCenter.test.tsx src/features/tasks/TaskCatalog.test.tsx src/features/tasks/TaskPage.test.tsx
git add -- src/features/tasks/OperationsCenter.tsx src/features/tasks/OperationsCenter.test.tsx src/features/tasks/TaskCatalog.tsx src/features/tasks/TaskCatalog.test.tsx src/features/tasks/fixtures.ts src/features/tasks/TaskPage.tsx src/features/tasks/TaskPage.test.tsx src/api/preview.ts src/styles/components.css
git commit -m "feat: build novice operations catalog"
```

## Task 3: 实现安全右键菜单和复制为个人脚本

**Files:**
- Modify: `src/components/ContextMenu.tsx`
- Modify: `src/components/ContextMenu.test.tsx`
- Modify: `src/features/tasks/TaskCatalog.tsx`
- Modify: `src/features/tasks/TaskCatalog.test.tsx`
- Modify: `src-tauri/src/commands/operations.rs`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/contracts.ts`
- Test: `src-tauri/tests/script_validation.rs`

- [ ] **Step 1: 写菜单白名单失败测试**

右键任务卡只出现“查看说明”“收藏/取消收藏”“复制为个人脚本”；不可出现“直接执行”“跳过确认”“以 root 运行”“复制命令”。键盘 Shift+F10 和 ContextMenu 键可打开，Escape/点击外部关闭，菜单保持窗口内。

- [ ] **Step 2: 写后端复制安全测试**

`copy_builtin_task_to_personal_script(taskId,serverId)` 由后端选择兼容实现、生成使用 `QZ_PARAM_*` 的可查看脚本并直接保存为个人脚本 v1；新脚本固定 disabled、dangerous、不可自动回滚。前端响应只得到 ScriptSummary，不得到内置命令模板。

- [ ] **Step 3: 运行并确认失败**

```powershell
pnpm exec vitest run src/components/ContextMenu.test.tsx src/features/tasks/TaskCatalog.test.tsx
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation copy_builtin
```

- [ ] **Step 4: 实现菜单和复制操作**

说明面板使用中文用途、参数、权限、影响和不兼容原因；复制成功跳转“我的脚本”并显示“副本默认未启用，个人脚本始终高风险”。

- [ ] **Step 5: 测试并提交**

```powershell
pnpm exec vitest run src/components/ContextMenu.test.tsx src/features/tasks/TaskCatalog.test.tsx
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation
git add -- src/components/ContextMenu.tsx src/components/ContextMenu.test.tsx src/features/tasks/TaskCatalog.tsx src/features/tasks/TaskCatalog.test.tsx src-tauri/src/commands/operations.rs src/api/tauri.ts src/api/contracts.ts src-tauri/tests/script_validation.rs
git commit -m "feat: add safe operation card context menu"
```

## Task 4: 构建中文参数、预检、预演和确认向导

**Files:**
- Create: `src/features/tasks/TaskWizard.tsx`
- Create: `src/features/tasks/TaskWizard.test.tsx`
- Modify: `src/features/tasks/ParameterForm.tsx`
- Modify: `src/features/tasks/OperationsCenter.tsx`
- Modify: `src/api/errors.ts`
- Modify: `src/styles/components.css`

- [ ] **Step 1: 写小白流程失败测试**

测试从任务选择到参数、预检、预演、确认、运行的步骤条；参数显示中文标签、示例、范围和即时错误，不要求输入 Linux 命令。safe 任务预演后可直接运行；dangerous 任务必须勾选“我已了解影响和恢复方式”并输入目标服务器显示名确认。

```tsx
it('危险任务确认前说明会改什么以及如何恢复', async () => {
  render(<TaskWizard task={dangerousTaskFixture()} api={previewApi()} />)
  await fillValidParameters()
  await user.click(screen.getByRole('button', { name: '检查并预演' }))
  expect(await screen.findByText('服务器是否可能断开')).toBeVisible()
  expect(screen.getByText('已创建备份后才会执行')).toBeVisible()
  expect(screen.getByRole('button', { name: '确认执行' })).toBeDisabled()
})
```

- [ ] **Step 2: 写过期与状态竞争测试**

服务器选择、参数或任务改变后，旧 preview/token 立即失效；重复点击只创建一次运行；预检期间可以取消返回目录；dangerous 任务不能批量。

- [ ] **Step 3: 运行并确认失败**

```powershell
pnpm exec vitest run src/features/tasks/TaskWizard.test.tsx src/features/tasks/OperationsCenter.test.tsx
```

- [ ] **Step 4: 实现向导**

步骤为“选择目标—填写信息—检查影响—执行—查看结果”。默认折叠技术命令；不可用/无权限/参数错误在当前步骤用中文说明，禁止展示 `[object Object]`。确认成功后锁定输入，只从事件更新状态。

- [ ] **Step 5: 测试并提交**

```powershell
pnpm exec vitest run src/features/tasks/TaskWizard.test.tsx src/features/tasks/OperationsCenter.test.tsx
git add -- src/features/tasks/TaskWizard.tsx src/features/tasks/TaskWizard.test.tsx src/features/tasks/ParameterForm.tsx src/features/tasks/OperationsCenter.tsx src/api/errors.ts src/styles/components.css
git commit -m "feat: guide novice users through operation preview"
```

## Task 5: 构建结构化结果、错误和恢复入口

**Files:**
- Create: `src/features/tasks/OperationResultPanel.tsx`
- Create: `src/features/tasks/OperationResultPanel.test.tsx`
- Create: `src/features/tasks/OperationRecoveryPanel.tsx`
- Create: `src/features/tasks/OperationRecoveryPanel.test.tsx`
- Modify: `src/features/tasks/ExecutionDrawer.tsx`
- Modify: `src/features/tasks/TaskWizard.tsx`
- Modify: `src/api/errors.ts`
- Modify: `src/styles/components.css`

- [ ] **Step 1: 写四种结论和中文错误失败测试**

normal、warning、failed、uncertain 各有独立标题、解释和下一步；错误区固定回答“发生了什么”“服务器可能已改变吗”“当前状态能确认吗”“下一步怎么做”“是否有恢复点”。技术详情默认折叠并脱敏、限高局部滚动。

- [ ] **Step 2: 写恢复状态失败测试**

只有 available/partial/failed 恢复点显示“开始回滚”；确认弹窗列出恢复对象和风险；rolling_back 禁止重复点击；rolled_back 不再显示回滚按钮；rollback_partial 保留失败项与再次核验入口；uncertain 显示“重新连接并核验”。

- [ ] **Step 3: 运行并确认失败**

```powershell
pnpm exec vitest run src/features/tasks/OperationResultPanel.test.tsx src/features/tasks/OperationRecoveryPanel.test.tsx
```

- [ ] **Step 4: 实现结果和下载操作**

优先显示中文摘要、关键指标、异常项和建议；TXT/JSON 下载调用后端生成文件，不让前端指定路径。技术详情复用 ExecutionDrawer，但移除原始对象直接字符串化路径，统一走 `toUserFacingError`。

- [ ] **Step 5: 测试并提交**

```powershell
pnpm exec vitest run src/features/tasks/OperationResultPanel.test.tsx src/features/tasks/OperationRecoveryPanel.test.tsx
git add -- src/features/tasks/OperationResultPanel.tsx src/features/tasks/OperationResultPanel.test.tsx src/features/tasks/OperationRecoveryPanel.tsx src/features/tasks/OperationRecoveryPanel.test.tsx src/features/tasks/ExecutionDrawer.tsx src/features/tasks/TaskWizard.tsx src/api/errors.ts src/styles/components.css
git commit -m "feat: explain operation results and recovery in chinese"
```

## Task 6: 构建只读批量选择与聚合结果

**Files:**
- Create: `src/features/tasks/OperationBatchPanel.tsx`
- Create: `src/features/tasks/OperationBatchPanel.test.tsx`
- Modify: `src/features/tasks/OperationsCenter.tsx`
- Modify: `src/features/tasks/TaskWizard.tsx`
- Modify: `src/styles/components.css`

- [ ] **Step 1: 写批量限制失败测试**

最多选择 50 台，去重；safe 只读任务才显示批量入口；UI 不提供并发数输入。dangerous/caution 有持久远端影响或个人脚本不能批量。测试后端拒绝后，UI显示“此任务会修改服务器，只能逐台执行”。

- [ ] **Step 2: 写聚合状态测试**

每台显示等待、运行、正常、警告、失败、取消、状态不确定；一台失败不移除其他结果。取消批次后等待项为 cancelled，运行项等待后端确认；“重试失败服务器”只提交 failed，不自动重试 uncertain。

- [ ] **Step 3: 运行并确认失败**

```powershell
pnpm exec vitest run src/features/tasks/OperationBatchPanel.test.tsx src/features/tasks/OperationsCenter.test.tsx
```

- [ ] **Step 4: 实现批量面板和汇总报告**

进度摘要显示 `完成/总数` 与正常/警告/失败计数；服务器列表局部滚动。支持取消全部、查看单机详情、重试失败服务器和下载 TXT/JSON 汇总；不把服务器凭据或本地绝对路径放入 UI/报告。

- [ ] **Step 5: 测试并提交**

```powershell
pnpm exec vitest run src/features/tasks/OperationBatchPanel.test.tsx src/features/tasks/OperationsCenter.test.tsx
git add -- src/features/tasks/OperationBatchPanel.tsx src/features/tasks/OperationBatchPanel.test.tsx src/features/tasks/OperationsCenter.tsx src/features/tasks/TaskWizard.tsx src/styles/components.css
git commit -m "feat: present readonly batch operation results"
```

## Task 7: 整合脚本中心与受控高级执行

**Files:**
- Modify: `src/features/tasks/OperationsCenter.tsx`
- Modify: `src/features/tasks/OperationsCenter.test.tsx`
- Modify: `src/features/tasks/scripts/ScriptCenter.tsx`
- Modify: `src/features/tasks/scripts/ScriptCenter.test.tsx`
- Modify: `src/features/tasks/AdvancedExecutionPanel.tsx`
- Modify: `src/features/tasks/AdvancedExecutionPanel.test.tsx`
- Modify: `src/features/tasks/TaskPage.tsx`

- [ ] **Step 1: 写统一入口失败测试**

“脚本”分类内显示 9 个内置只读 Runbook 和“我的脚本”；内置脚本进入统一 TaskWizard，可批量；个人脚本进入 ScriptRunDialog，只能单机且显示不可自动回滚。受控高级执行保留在“高级执行”二级入口，不与已保存脚本混为安全任务。

- [ ] **Step 2: 写正文隔离回归**

输入脚本 canary 后切换目录、搜索、执行和查看技术详情，断言普通 OperationsCenter state 快照、错误、结果组件、localStorage/sessionStorage 与 console 不包含正文。

- [ ] **Step 3: 运行并确认失败**

```powershell
pnpm exec vitest run src/features/tasks/OperationsCenter.test.tsx src/features/tasks/scripts/ScriptCenter.test.tsx src/features/tasks/AdvancedExecutionPanel.test.tsx src/features/tasks/TaskPage.test.tsx
```

- [ ] **Step 4: 实现最终导航关系**

OperationsCenter 只持有当前 view/selection/run ID，不提升个人脚本正文 state；ScriptEditor 自己获取与释放正文。高级执行继续二次确认并标记“不会自动保存为脚本、不可自动回滚”。

- [ ] **Step 5: 测试并提交**

```powershell
pnpm exec vitest run src/features/tasks/OperationsCenter.test.tsx src/features/tasks/scripts/ScriptCenter.test.tsx src/features/tasks/AdvancedExecutionPanel.test.tsx src/features/tasks/TaskPage.test.tsx
git add -- src/features/tasks/OperationsCenter.tsx src/features/tasks/OperationsCenter.test.tsx src/features/tasks/scripts/ScriptCenter.tsx src/features/tasks/scripts/ScriptCenter.test.tsx src/features/tasks/AdvancedExecutionPanel.tsx src/features/tasks/AdvancedExecutionPanel.test.tsx src/features/tasks/TaskPage.tsx src/features/tasks/TaskPage.test.tsx
git commit -m "feat: integrate builtin and personal operation scripts"
```

## Task 8: 完成原生窗口自适应与立体视觉

**Files:**
- Modify: `src/app/AppShell.tsx`
- Modify: `src/app/AppShell.test.tsx`
- Modify: `src/styles/theme.css`
- Modify: `src/styles/components.css`
- Modify: `src/styles/responsive.css`
- Modify: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `scripts/tests/desktop-ux.tests.ps1`

- [ ] **Step 1: 写四档布局源码契约和组件失败测试**

宽度断点覆盖 ≥1440、1180–1439、960–1179、<960；窗口 960×640 时目录变为可返回的单栏视图，卡片宽度变为 `minmax(0,1fr)`，页面无全局横向滚动。不能使用 `transform: scale()` 缩小整个客户端，也不能维持桌面卡片宽度后挤压。

- [ ] **Step 2: 写自定义标题栏拖动回归**

缩小/还原窗口后，顶部品牌空白区域仍有 `data-tauri-drag-region`，交互控件有 no-drag；最小化、最大化/还原、关闭可用，不出现系统浏览器地址栏、网页错误页或第二个外部命令行窗口。

- [ ] **Step 3: 运行并确认失败**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
pnpm exec vitest run src/app/AppShell.test.tsx src/features/tasks/OperationsCenter.test.tsx
```

- [ ] **Step 4: 实现重排、局部滚动和视觉层级**

大屏为左侧目录+右侧工作区；中屏收窄目录与两列卡片；960×640 为目录/向导/结果互斥单页并提供明确返回。顶部、目录、内容区背景连续，卡片使用白银渐变、内高光、强阴影和 hover/focus 抬升；正文保持深色高对比，禁用态仍可读。

- [ ] **Step 5: 测试并提交**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
pnpm exec vitest run src/app/AppShell.test.tsx src/features/tasks/OperationsCenter.test.tsx
git add -- src/app/AppShell.tsx src/app/AppShell.test.tsx src/styles/theme.css src/styles/components.css src/styles/responsive.css scripts/tests/responsive-layout.tests.ps1 scripts/tests/desktop-ux.tests.ps1
git commit -m "fix: adapt operations center to native window sizes"
```

## Task 9: 文档、全量自动化和安全路径审计

**Files:**
- Create: `docs/milestone-5-operations-acceptance.md`
- Modify: `docs/user-guide.md`
- Modify: `docs/security.md`
- Modify: `docs/support-matrix.md`

- [ ] **Step 1: 更新小白用户文档**

按“选择服务器—选择想做什么—填写中文信息—查看影响—执行—看结论/恢复”写操作指南；说明 root/免密 sudo、批量只读限制、个人脚本不可自动回滚、报告/备份位置和状态不确定处理。不以 Linux 命令作为主要步骤。

- [ ] **Step 2: 更新安全与兼容矩阵**

列出 Debian/RHEL/openEuler/银河麒麟/UOS/Anolis 映射，systemd/service、防火墙和 Docker/Podman 能力；明确缺能力时灰显、不自动安装。记录恢复资产权限、保留期、脚本正文边界、并发/输出/超时限制。

- [ ] **Step 3: 运行源码、前端和 Rust 全量测试**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\local-build.tests.ps1
pnpm test
pnpm build
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
git diff --check
```

- [ ] **Step 4: 运行项目内 live 闭环**

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\ssh-fixture.ps1 -Action Start
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test ssh_live -- --ignored --nocapture
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test sftp_live -- --ignored --nocapture
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test operations_live -- --ignored --nocapture
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\ssh-fixture.ps1 -Action Stop
```

- [ ] **Step 5: 扫描敏感信息和项目外写入**

检查源码、测试快照、数据库摘要、报告、诊断、浏览器存储和 console，不得包含用户 API key、服务器密码/私钥、script canary 或未脱敏参数。运行 `scripts/verify-d-drive.ps1` 并检查新文件只位于工作区下的 `.local`、`data`、`target`、`dist`、`artifacts`；不得写 C 盘 AppData、D 盘根或 `D:\Codex Project` 父目录。

- [ ] **Step 6: 提交文档**

```powershell
git add -- docs/milestone-5-operations-acceptance.md docs/user-guide.md docs/security.md docs/support-matrix.md
git commit -m "docs: document operations and script center"
```

## Task 10: 原生视觉验收并只生成一个 r8 测试包

**Files:**
- Create: `docs/design-qa/r8-operations-1920x1080.png`
- Create: `docs/design-qa/r8-operations-1366x768.png`
- Create: `docs/design-qa/r8-operations-1180x760.png`
- Create: `docs/design-qa/r8-operations-960x640.png`
- Modify: `docs/milestone-5-operations-acceptance.md`

- [ ] **Step 1: 启动原生开发客户端而非浏览器页面**

```powershell
. .\scripts\dev-env.ps1 -Quiet
pnpm tauri dev
```

确认只出现轻舟 SSH 原生窗口，不弹出外部 Edge/Chrome、localhost 错误页或额外命令行窗口。开发服务未就绪时客户端显示中文启动错误并可重试，不暴露浏览器错误页。

- [ ] **Step 2: 完成四档截图和交互验收**

在 1920×1080、1366×768、1180×760、960×640 检查任务目录、safe 向导、dangerous 预演/确认、批量结果、回滚、脚本编辑/执行和技术详情。逐档验证标题栏可拖动、按钮可见、文字不截断、卡片不挤压、局部滚动可达、无全局横向滚动；截图写入列出的 `docs/design-qa` 文件。

- [ ] **Step 3: 回归其他客户端页面**

同一四档检查首页、服务器、日志检索、SFTP 文件传输、工作流、执行历史、下载和设置，确认没有破坏已有完整客户端边缘、右键菜单和自适应行为。

- [ ] **Step 4: 在所有门槛通过后构建唯一测试包**

本计划之前不得执行此命令。执行一次：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-local-test.ps1 -PackageVersion '0.1.5-local.20260805-r8'
```

- [ ] **Step 5: 验证交付物**

确认 EXE、ZIP、`portable.flag`、嵌入式生产前端和 SHA-256 均在 `D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test`；版本不覆盖 r7。冷启动 EXE，关闭任何开发服务后仍能打开，且不连接 localhost 开发页面。

- [ ] **Step 6: 提交视觉证据并停止迭代**

```powershell
git add -- docs/design-qa/r8-operations-1920x1080.png docs/design-qa/r8-operations-1366x768.png docs/design-qa/r8-operations-1180x760.png docs/design-qa/r8-operations-960x640.png docs/milestone-5-operations-acceptance.md
git commit -m "test: record r8 operations acceptance"
```

向用户提供 EXE、ZIP、SHA-256、自动化结果、live 测试结果和四档视觉验收摘要。不得自动发布 GitHub、魔塔或更新清单；停止继续迭代，等待用户自行验证。
