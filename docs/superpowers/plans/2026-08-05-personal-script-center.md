# 个人脚本中心 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在项目数据根内实现个人 Shell 脚本定义、不可变版本、强类型参数、分类收藏、JSON 导入导出和单服务器受控执行，并确保脚本始终高风险、不可自动回滚且正文不会泄漏到普通事件和浏览器存储。

**Architecture:** SQLite 保存脚本定义、不可变正文版本和运行引用；ScriptService 是唯一读取正文和发起执行的入口。前端编辑器只在 React 内存中持有当前正文，保存后即清空不需要的副本。参数在 Rust 中验证后映射为受限的 `QZ_PARAM_*` 环境变量，通过现有非交互 `sh` 执行通道运行，不替换脚本文本，不建立第二套脚本运行时。

**Tech Stack:** Rust、SQLx/SQLite、Serde JSON、SHA-256、现有 ExecutionService/SSH executor、Tauri IPC、React 19、TypeScript、Vitest、Testing Library。

---

## 文件结构

**创建：**

- `src-tauri/migrations/0007_personal_scripts.sql`
- `src-tauri/src/domain/script.rs`
- `src-tauri/src/repositories/script_repository.rs`
- `src-tauri/src/core/scripts/mod.rs`
- `src-tauri/src/core/scripts/validation.rs`
- `src-tauri/src/core/scripts/environment.rs`
- `src-tauri/src/core/scripts/package.rs`
- `src-tauri/src/services/script_service.rs`
- `src-tauri/src/commands/scripts.rs`
- `src-tauri/tests/script_repository_integration.rs`
- `src-tauri/tests/script_validation.rs`
- `src-tauri/tests/script_packages.rs`
- `src-tauri/tests/script_execution.rs`
- `src/features/tasks/scripts/ScriptCenter.tsx`
- `src/features/tasks/scripts/ScriptCenter.test.tsx`
- `src/features/tasks/scripts/ScriptList.tsx`
- `src/features/tasks/scripts/ScriptEditor.tsx`
- `src/features/tasks/scripts/ScriptRunDialog.tsx`
- `src/features/tasks/scripts/types.ts`

**修改：**

- `src-tauri/src/core/mod.rs`、`src-tauri/src/domain/mod.rs`、`src-tauri/src/repositories/mod.rs`、`src-tauri/src/services/mod.rs`：导出脚本模块。
- `src-tauri/src/services/app_services.rs`、`src-tauri/src/state.rs`：注册 ScriptService。
- `src-tauri/src/services/execution_service.rs`：接收后端内部的已验证脚本请求，不增加公开命令字段。
- `src-tauri/src/commands/mod.rs`、`src-tauri/src/lib.rs`：注册脚本 IPC。
- `src/api/contracts.ts`、`src/api/tauri.ts`、`src/api/preview.ts`、`src/api/tauri.test.ts`：脚本 DTO 和 API。
- `src/features/tasks/TaskPage.tsx`、`src/features/tasks/TaskPage.test.tsx`：挂载内置脚本/个人脚本入口，最终布局留给第五阶段整合。
- `src/styles/components.css`、`src/styles/responsive.css`：脚本列表、编辑器和确认弹窗样式。

## Task 1: 持久化定义、不可变版本和运行引用

**Files:**
- Create: `src-tauri/migrations/0007_personal_scripts.sql`
- Create: `src-tauri/src/domain/script.rs`
- Create: `src-tauri/src/repositories/script_repository.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/repositories/mod.rs`
- Create: `src-tauri/tests/script_repository_integration.rs`

- [ ] **Step 1: 写版本不可变和软删除失败测试**

```rust
#[tokio::test]
async fn saving_changes_creates_immutable_version_and_delete_preserves_runs() {
    let repo = fixture_repo().await;
    let definition = repo.create(new_script("巡检", "echo one")).await.unwrap();
    let v2 = repo.save_version(definition.id, "echo two", vec![]).await.unwrap();
    assert_eq!(v2.version_number, 2);
    assert_eq!(repo.get_version(definition.id, 1).await.unwrap().body, "echo one");
    repo.soft_delete(definition.id).await.unwrap();
    assert!(repo.get_for_editor(definition.id).await.unwrap().is_none());
    assert!(repo.get_version(definition.id, 1).await.is_ok());
}
```

- [ ] **Step 2: 运行并确认缺表失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_repository_integration
```

- [ ] **Step 3: 创建表和领域类型**

`script_definitions(id,title,category,tags_json,is_favorite,is_enabled,active_version_id,created_at,updated_at,deleted_at)`；`script_versions(id,definition_id,version_number,body,body_sha256,parameters_json,scan_summary_json,created_at)`；`script_runs(id,definition_id,version_id,operation_run_id,created_at)`。

数据库约束：同一定义的 version_number 唯一；版本行禁止 UPDATE/DELETE；删除定义只写 deleted_at 和 is_enabled=0；script_runs 引用固定 version_id，不因定义软删除丢失历史。列表 DTO 不含 body，只有 `get_script_for_editor` 返回当前正文。

- [ ] **Step 4: 实现事务写入和分页列表**

新建脚本在同一事务创建定义和 v1；编辑正文或参数始终创建新版本并更新 active_version_id；标题、分类、标签、收藏、启停只更新定义元数据。列表支持 query/category/tag/favorite/enabled，固定最多 100 条。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_repository_integration
git add -- src-tauri/migrations/0007_personal_scripts.sql src-tauri/src/domain/script.rs src-tauri/src/domain/mod.rs src-tauri/src/repositories/script_repository.rs src-tauri/src/repositories/mod.rs src-tauri/tests/script_repository_integration.rs
git commit -m "feat: persist immutable personal script versions"
```

## Task 2: 验证脚本元数据、正文和参数定义

**Files:**
- Create: `src-tauri/src/core/scripts/mod.rs`
- Create: `src-tauri/src/core/scripts/validation.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/tests/script_validation.rs`

- [ ] **Step 1: 写所有边界的失败测试**

```rust
#[test]
fn script_limits_and_parameter_names_are_enforced() {
    assert!(validate_parameter_name("HOST").is_ok());
    assert!(validate_parameter_name("DB_PORT_2").is_ok());
    assert!(validate_parameter_name("host").is_err());
    assert!(validate_parameter_name("A-B").is_err());
    assert!(validate_parameter_name("QZ_PARAM_HOST").is_err());
    assert!(validate_script_body(&"x".repeat(1024 * 1024 + 1)).is_err());
}
```

覆盖：标题 1–80 字符、分类 1–40、最多 20 个标签且单个 1–24、正文 1–1 MiB、无 NUL、最多 32 个参数、参数名 `^[A-Z][A-Z0-9_]{0,31}$` 且不能以 `QZ_` 开头、默认值同样通过强类型验证、超时 1–3600 秒。

- [ ] **Step 2: 运行并确认验证函数不存在**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation
```

- [ ] **Step 3: 复用任务参数类型并固定风险**

脚本参数只允许 String、Integer、Boolean、Enum、Host、Port、ServiceName、ContainerName、AbsolutePath；每个参数必须有中文 label 和是否必填。AbsolutePath 在脚本场景只代表远端绝对路径，不允许映射成本机路径。所有 ScriptSummary/ScriptDetails/ScriptRunPreview 的 riskLevel 后端固定为 dangerous，客户端无可写风险字段。

- [ ] **Step 4: 增加提示性静态扫描**

扫描 `rm -rf`、磁盘写入、用户/密码、网络配置、防火墙、服务停止、下载后执行、`eval`、`curl|sh` 等模式，只生成 warning 列表和内容摘要；扫描结果绝不能降低风险或作为执行成功依据。正文摘要仅包含行数、字符数、SHA-256 和提示数量。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation
git add -- src-tauri/src/core/scripts/mod.rs src-tauri/src/core/scripts/validation.rs src-tauri/src/core/mod.rs src-tauri/tests/script_validation.rs
git commit -m "feat: validate personal script definitions"
```

## Task 3: 通过 `QZ_PARAM_*` 环境变量安全传参

**Files:**
- Create: `src-tauri/src/core/scripts/environment.rs`
- Modify: `src-tauri/src/core/scripts/mod.rs`
- Test: `src-tauri/tests/script_validation.rs`

- [ ] **Step 1: 写 Shell 结构注入失败测试**

```rust
#[test]
fn parameter_values_cannot_change_script_structure() {
    let command = render_script_launcher(
        "printf '%s' \"$QZ_PARAM_HOST\"",
        &validated_params([("HOST", "x'; touch /tmp/pwn; #")]),
    ).unwrap();
    assert!(!command.contains("QZ_PARAM_HOST=x'; touch"));
    assert_eq!(extract_script_body(&command), "printf '%s' \"$QZ_PARAM_HOST\"");
}
```

还要覆盖换行、反引号、`$()`、Unicode、空字符串和接近 1 MiB 正文。测试断言参数值只出现在经过共享 shell_quote 的 env 赋值中，正文原样不替换。

- [ ] **Step 2: 运行并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation environment
```

- [ ] **Step 3: 实现固定 launcher**

参数名映射为 `QZ_PARAM_<NAME>`；按名称排序，使用共享 shell_quote 生成 `env KEY='value' ... sh -s`。脚本使用随机 heredoc delimiter，生成前循环确认正文不包含 delimiter。禁止参数模板语法、字符串 replace、`eval` 和动态 shell 变量名。

- [ ] **Step 4: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation
git add -- src-tauri/src/core/scripts/environment.rs src-tauri/src/core/scripts/mod.rs src-tauri/tests/script_validation.rs
git commit -m "feat: pass script parameters through safe environment"
```

## Task 4: 实现固定版本 JSON 导入导出

**Files:**
- Create: `src-tauri/src/core/scripts/package.rs`
- Modify: `src-tauri/src/core/scripts/mod.rs`
- Create: `src-tauri/tests/script_packages.rs`

- [ ] **Step 1: 写 schema、大小和敏感字段失败测试**

```rust
#[test]
fn import_rejects_unknown_version_and_external_state() {
    assert_code(import_json(r#"{"schemaVersion":2}"#), "unsupported_script_package");
    assert_code(import_json(package_with("serverId", "secret")), "forbidden_script_field");
    assert_code(import_json(package_with("localPath", "C:\\\\Users\\\\x")), "forbidden_script_field");
    assert_code(import_json(package_with("privateKey", "key")), "forbidden_script_field");
}
```

导入文件上限 2 MiB，解码后正文上限 1 MiB、参数最多 32、标签最多 20；拒绝 credentials、servers、history、runs、localPath、dataRoot、privateKey、password、token 等包字段。内容扫描发现明显私钥块或凭据赋值时拒绝并显示中文原因。

- [ ] **Step 2: 运行并确认失败**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_packages
```

- [ ] **Step 3: 实现 schemaVersion 1**

导出结构只含 `schemaVersion, exportedAt, script{title,category,tags,body,parameters}`，不含定义 UUID、版本历史、收藏、启用状态、服务器、运行和本地路径。导入忽略包外任何状态，重新生成 ID 和 v1，固定 `is_enabled=false`、`is_favorite=false`。

- [ ] **Step 4: 实现项目内原子文件导出**

后端生成 `downloads/scripts/script-<uuid>.json`，先写 `.partial`，flush、计算 SHA-256 后原子改名；API 不接受输出路径或自定义文件名。导出的正文是用户主动选择的脚本内容，因此只存在于该专用文件，不进入普通报告或事件。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_packages
git add -- src-tauri/src/core/scripts/package.rs src-tauri/src/core/scripts/mod.rs src-tauri/tests/script_packages.rs
git commit -m "feat: import and export personal script packages"
```

## Task 5: 实现脚本服务与高风险执行闭环

**Files:**
- Create: `src-tauri/src/services/script_service.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Create: `src-tauri/tests/script_execution.rs`

- [ ] **Step 1: 写禁用、确认、版本锁定和泄漏失败测试**

```rust
#[tokio::test]
async fn personal_script_is_dangerous_unrecoverable_and_body_is_not_logged() {
    let fixture = ScriptFixture::enabled("echo script-body-canary").await;
    let preview = fixture.preview().await.unwrap();
    assert_eq!(preview.risk_level, RiskLevel::Dangerous);
    assert!(!preview.automatic_rollback_available);
    assert!(preview.warning.contains("不可自动回滚"));
    let error = fixture.run_without_confirmation().await.unwrap_err();
    assert_eq!(error.code, "script_confirmation_required");
    assert!(!fixture.all_events_and_history().contains("script-body-canary"));
}
```

另测：禁用/删除脚本拒绝运行；预演后定义保存 v2，确认仍运行预演锁定的 v1；参数缺失/越界拒绝；只允许单服务器；断线为 uncertain；取消只在执行器确认停止后显示 cancelled。

- [ ] **Step 2: 运行并确认服务缺失**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_execution
```

- [ ] **Step 3: 实现 ScriptService**

`preview_script_run(script_id, server_id, parameter_values)` 固定读取并锁定 active version，返回行数/字符数/哈希/参数名/扫描提示/超时和“不可自动回滚”，不返回正文。`confirm_script_run(preview_id, confirmation_token)` 校验一次性 token、期限和服务器后，通过内部 API 调用 ExecutionService；不能调用公开 `start_custom_execution` 绕过版本与确认。

- [ ] **Step 4: 限制历史、事件和错误内容**

execution/task ID 使用 `script.personal`，历史只保存 scriptDefinitionId、scriptVersionId、摘要参数和 timeout；参数敏感值统一 `[REDACTED]`。事件不含命令、launcher、正文或 env 值。SSH stderr 经过 redactor 和长度限制；报告仅引用脚本标题、版本和正文哈希。

- [ ] **Step 5: 测试并提交**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_execution
git add -- src-tauri/src/services/script_service.rs src-tauri/src/services/execution_service.rs src-tauri/src/services/app_services.rs src-tauri/src/services/mod.rs src-tauri/src/state.rs src-tauri/tests/script_execution.rs
git commit -m "feat: execute personal scripts as unrecoverable high risk"
```

## Task 6: 暴露脚本 CRUD、包和运行 IPC

**Files:**
- Create: `src-tauri/src/commands/scripts.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/preview.ts`
- Modify: `src/api/tauri.test.ts`

- [ ] **Step 1: 写 DTO 失败测试**

覆盖 `list_scripts` 不返回正文；`get_script_for_editor` 才返回正文；创建/保存/复制/软删除/启停/收藏/导入/导出/预演/确认/取消命令。确认 request 只含 previewId 和 token，执行 request 不允许 riskLevel、rollbackAvailable、command、localPath 或 serverIds 数组。

- [ ] **Step 2: 运行并确认 API 缺失**

```powershell
pnpm exec vitest run src/api/tauri.test.ts
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_execution ipc
```

- [ ] **Step 3: 实现 Tauri 命令和 TypeScript 契约**

新增 `list_personal_scripts`、`get_personal_script_for_editor`、`create_personal_script`、`save_personal_script_version`、`update_personal_script_metadata`、`copy_personal_script`、`delete_personal_script`、`set_personal_script_enabled`、`import_personal_script`、`export_personal_script`、`preview_personal_script_run`、`confirm_personal_script_run`。

- [ ] **Step 4: 测试并提交**

```powershell
pnpm exec vitest run src/api/tauri.test.ts
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_execution
git add -- src-tauri/src/commands/scripts.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/api/contracts.ts src/api/tauri.ts src/api/preview.ts src/api/tauri.test.ts
git commit -m "feat: expose personal script center api"
```

## Task 7: 构建个人脚本管理界面

**Files:**
- Create: `src/features/tasks/scripts/ScriptCenter.tsx`
- Create: `src/features/tasks/scripts/ScriptCenter.test.tsx`
- Create: `src/features/tasks/scripts/ScriptList.tsx`
- Create: `src/features/tasks/scripts/ScriptEditor.tsx`
- Create: `src/features/tasks/scripts/ScriptRunDialog.tsx`
- Create: `src/features/tasks/scripts/types.ts`
- Modify: `src/features/tasks/TaskPage.tsx`
- Modify: `src/features/tasks/TaskPage.test.tsx`
- Modify: `src/styles/components.css`
- Modify: `src/styles/responsive.css`

- [ ] **Step 1: 写用户流程失败测试**

```tsx
it('导入脚本默认禁用，查看并启用后仍需不可回滚确认', async () => {
  render(<ScriptCenter api={fixtureApi()} />)
  await user.click(screen.getByRole('button', { name: '导入脚本' }))
  expect(await screen.findByText('已导入，默认未启用')).toBeVisible()
  expect(screen.getByRole('button', { name: '运行脚本' })).toBeDisabled()
  await user.click(screen.getByRole('button', { name: '启用脚本' }))
  await user.click(screen.getByRole('button', { name: '运行脚本' }))
  expect(screen.getByText('此脚本造成的修改无法由客户端自动回滚')).toBeVisible()
})
```

另测新建、编辑生成新版本、复制、软删除、分类/标签/收藏/搜索、查看历史版本、强类型参数表单、后端错误中文显示。

- [ ] **Step 2: 写浏览器存储泄漏测试**

在输入 `script-browser-canary` 后切换列表、保存、运行预演和卸载，spy `localStorage.setItem`、`sessionStorage.setItem`、console 和普通 operation event renderer，断言 canary 从未写入。

- [ ] **Step 3: 运行并确认组件缺失**

```powershell
pnpm exec vitest run src/features/tasks/scripts/ScriptCenter.test.tsx src/features/tasks/TaskPage.test.tsx
```

- [ ] **Step 4: 实现内置/个人脚本切换和编辑器**

脚本中心顶部显示“内置审核脚本 / 我的脚本”。内置脚本只读，可复制；个人脚本显示 high risk、启用状态和版本。编辑器离开时若未保存必须确认；保存成功后清理旧正文 state。确认弹窗只展示标题、版本、行数、字符数、参数摘要、扫描提示和不可回滚警告。

- [ ] **Step 5: 实现局部滚动和小窗口布局**

列表与编辑区各自局部滚动；960×640 下改为单栏分步视图，禁止固定大卡片横向挤压和全局横向滚动。延续现有白银渐变、立体阴影和中文可读性。

- [ ] **Step 6: 测试并提交**

```powershell
pnpm exec vitest run src/features/tasks/scripts/ScriptCenter.test.tsx src/features/tasks/TaskPage.test.tsx
git add -- src/features/tasks/scripts/ScriptCenter.tsx src/features/tasks/scripts/ScriptCenter.test.tsx src/features/tasks/scripts/ScriptList.tsx src/features/tasks/scripts/ScriptEditor.tsx src/features/tasks/scripts/ScriptRunDialog.tsx src/features/tasks/scripts/types.ts src/features/tasks/TaskPage.tsx src/features/tasks/TaskPage.test.tsx src/styles/components.css src/styles/responsive.css
git commit -m "feat: build personal script center interface"
```

## Task 8: 脚本中心安全审计与阶段回归

- [ ] **Step 1: 运行后端脚本专项测试**

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_repository_integration
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_validation
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_packages
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test script_execution
```

- [ ] **Step 2: 运行前端与全量回归**

```powershell
pnpm test
pnpm build
. .\scripts\dev-env.ps1 -Quiet
cargo fmt --manifest-path .\src-tauri\Cargo.toml --all -- --check
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
git diff --check
```

- [ ] **Step 3: 执行 canary 和路径扫描**

在测试数据库写入 `script-db-canary`，运行后检查普通事件、execution parameters、TXT/JSON 运维报告、诊断目录、浏览器存储和 console 均不含正文；只有 `script_versions.body` 与用户主动导出的专用脚本 JSON 可以包含。确认导出和数据库只位于项目数据根。

- [ ] **Step 4: 确认阶段边界**

确认没有自动回滚承诺、没有多服务器脚本运行、没有动态 JS 执行器、没有 C 盘 AppData/D 盘根写入；此阶段不打包、不发布，进入最终 UI 整合阶段。
