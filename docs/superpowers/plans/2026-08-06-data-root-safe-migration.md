# Safe Data Root Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** 允许用户随时在设置中修改轻舟 SSH 的数据目录，并通过“预检—退出客户端—完整复制—SHA-256 校验—切换指针—重启”的安全事务迁移全部数据；旧目录始终保留，不自动删除或清空。

**Architecture:** 数据目录解析拆成环境变量、便携版自定义指针、便携版默认目录和已安装版注册表指针四种来源。运行中的 Tauri 进程只负责预检、写入迁移事务和启动同一签名 GUI 程序的隐藏迁移模式；隐藏模式等待父进程退出后复制和校验，再原子更新持久指针并重启正常客户端。迁移状态同时写入源目录和目标目录，失败时保持旧指针并重启旧目录，成功时从新目录启动；任何分支都不删除源目录。

**Tech Stack:** Rust 2021、Tauri 2、Tokio、Serde、SHA-256、Windows HKCU、Windows GUI 子系统、React 19、TypeScript、Vitest、Testing Library、PowerShell 包装测试。

---

## 实施约束

- 迁移目标必须是绝对目录，不能是盘符根目录、源目录、源目录父级或子级、重解析点，也不能包含冲突数据；唯一例外是同一源目录留下且结构可验证的失败迁移目标，可由用户明确选择“重试此次迁移”。
- 源目录内发现重解析项时中止，不跟随链接复制到数据目录之外。
- 界面预检计算估算清单和空间；隐藏 worker 等父进程退出、SQLite 与 WebView2 关闭后重新生成权威清单并再次检查空间，预留至少 `max(64 MiB, 总大小的 10%)`。
- 复制后逐文件校验相对路径、大小和 SHA-256；全部通过后才允许切换指针。
- `QINGZHOU_DATA_ROOT` 始终最高优先级，并在设置中显示“由环境变量锁定”，不允许客户端覆盖。
- 便携版默认仍为 `<exe>\data`；自定义后使用程序旁的原子 JSON 指针；“恢复跟随程序目录”同样通过安全迁移完成。
- 已安装版继续使用 HKCU，不要求管理员权限。
- 不自动删除、重命名或清空旧目录；失败目标也不自动清理，只留下可识别的未完成标记。
- 本计划完成后仅提供项目目录内的本地测试包，不发布在线更新。

## Task 1: 扩展数据目录来源与持久指针抽象

**Files:**

- Modify: `src-tauri/src/core/data_root.rs`
- Modify: `src-tauri/src/core/root_registry.rs`
- Create: `src-tauri/src/core/portable_root.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/tests/data_root_resolution.rs`
- Modify: `src-tauri/src/commands/bootstrap.rs`

- [ ] **Step 1: 写来源优先级失败测试**

覆盖：

1. 环境变量覆盖所有来源。
2. 存在 `portable.flag` 时，便携版自定义指针覆盖 `<exe>\data`，且不读取注册表。
3. 便携版无自定义指针时使用 `<exe>\data`。
4. 非便携版使用 HKCU 指针；无指针时进入 `needs_selection`。
5. 无效或相对便携指针被拒绝，不回退到不相关目录。

目标输入：

```rust
pub struct DataRootInputs {
    pub env_override: Option<PathBuf>,
    pub portable_mode: bool,
    pub portable_custom_root: Option<PathBuf>,
    pub portable_default_root: Option<PathBuf>,
    pub registry_root: Option<PathBuf>,
}
```

```powershell
. .\scripts\dev-env.ps1 -Quiet
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_root_resolution
```

预期：字段和 `portable_root` 模块不存在而编译失败。

- [ ] **Step 2: 实现来源模型**

```rust
pub enum DataRootSource {
    Environment,
    PortableCustom,
    PortableDefault,
    Registry,
    NeedsSelection,
}

pub struct DataRootResolution {
    pub source: DataRootSource,
    pub path: Option<PathBuf>,
    pub mutable: bool,
}
```

`Environment` 的 `mutable=false`，其他 ready 来源为 `true`。解析函数保持纯函数，运行时函数只负责读取 exe 目录、flag、JSON 指针和注册表。

- [ ] **Step 3: 实现已安装版和便携版指针写入**

`root_registry.rs` 增加 `clear_data_root()`。`portable_root.rs` 使用现有 `atomicwrites` 写 `data-root.json`：

```json
{"schemaVersion":1,"dataRoot":"D:\\QingzhouData"}
```

提供 `load/save/clear`，只接受绝对路径，JSON 未知 schema 明确报错。不要把指针写入 C 盘 AppData。

- [ ] **Step 4: 扩展启动状态契约**

`BootstrapStatus::Ready` 增加 `dataRootSource` 和 `dataRootMutable`；现有 `dataRoot` 字段保持不变。更新序列化单元测试。

- [ ] **Step 5: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_root_resolution
cargo test --locked --manifest-path .\src-tauri\Cargo.toml commands::bootstrap::tests
git add src-tauri/src/core/data_root.rs src-tauri/src/core/root_registry.rs src-tauri/src/core/portable_root.rs src-tauri/src/core/mod.rs src-tauri/tests/data_root_resolution.rs src-tauri/src/commands/bootstrap.rs
git commit -m "feat: model mutable data root sources"
```

## Task 2: 实现迁移清单与路径预检

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Create: `src-tauri/src/core/data_migration/mod.rs`
- Create: `src-tauri/src/core/data_migration/model.rs`
- Create: `src-tauri/src/core/data_migration/preflight.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/tests/data_migration_preflight.rs`

- [ ] **Step 1: 写路径拒绝矩阵失败测试**

使用临时目录和可注入的 `PathInspector` fake 覆盖：相对路径、盘符根、同目录、父子目录、无关目标非空、目标或源重解析、源内重解析项、不可写目标、空间不足。合法空目录返回预检摘要；具有同一规范化源/目标和失败 journal、且无未知文件的目标返回 `retryable=true`。

```rust
pub struct DataMigrationPreview {
    pub preview_id: Uuid,
    pub source: PathBuf,
    pub target: PathBuf,
    pub file_count: u64,
    pub total_bytes: u64,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub old_root_will_be_kept: bool,
    pub retryable: bool,
}
```

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_migration_preflight
```

预期：模块不存在而失败。

- [ ] **Step 2: 添加磁盘空间依赖并定义允许项**

添加 `fs2 = "0.4"`，用 `fs2::available_space` 获取目标所在卷可用空间。根级允许项固定为：`app.db`、`vault`、`logs`、`downloads`、`backups`、`templates`、`cache`、`updates`，以及以后通过同一常量声明的目录。`app.db-wal/app.db-shm` 作为允许的数据库伴随文件：界面预检可用于估算，worker 在父进程退出后重新扫描，存在就复制，不存在就不伪造。

- [ ] **Step 3: 实现不跟随链接的清单扫描**

使用 `symlink_metadata` 和 Windows `MetadataExt::file_attributes()` 检测 `FILE_ATTRIBUTE_REPARSE_POINT (0x400)`。按规范化相对路径排序，记录文件与空目录；遇到源内重解析项立即失败。

- [ ] **Step 4: 实现目标预检**

规范化路径时对不存在目标解析最近存在父级，再拼接剩余组件，防止文本形式绕过父子关系。创建一次性写探针并删除。空间要求为 `total_bytes + max(64 MiB, total_bytes / 10)`。

- [ ] **Step 5: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_migration_preflight
cargo test --locked --manifest-path .\src-tauri\Cargo.toml core::data_root
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/core/data_migration src-tauri/src/core/mod.rs src-tauri/tests/data_migration_preflight.rs
git commit -m "feat: preflight safe data root migrations"
```

## Task 3: 实现复制、SHA-256 校验与事务日志

**Files:**

- Create: `src-tauri/src/core/data_migration/journal.rs`
- Create: `src-tauri/src/core/data_migration/copy.rs`
- Modify: `src-tauri/src/core/data_migration/mod.rs`
- Create: `src-tauri/tests/data_migration_copy.rs`

- [ ] **Step 1: 写事务状态与损坏检测失败测试**

目标状态机：

```rust
pub enum DataMigrationPhase {
    Prepared,
    Copying,
    Verifying,
    Switched,
    Completed,
    Failed,
}
```

测试：正常复制保持相对路径、空目录、大小和 SHA-256；测试钩子在复制后篡改文件会导致 `Failed`；校验失败时指针切换回调调用次数必须为 0；源文件始终存在。

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_migration_copy
```

预期：复制器和日志不存在而失败。

- [ ] **Step 2: 实现原子事务日志**

日志名固定 `.qingzhou-data-migration.json`，包含 migration ID、源/目标、来源模式、父 PID、清单摘要、phase、错误摘要、开始/更新时间、`acknowledged`。每次状态变化使用 `atomicwrites`；错误信息不得包含凭据或文件内容。

- [ ] **Step 3: 实现受限复制**

复制器只遍历传入的不可变清单；每个目标文件使用临时后缀写入、`sync_all` 后原子 rename。普通迁移遇到目标冲突立即失败。用户确认重试同一失败事务时，哈希一致的已复制文件直接复用；只有 journal 清单内且哈希不一致的目标文件可被临时文件原子替换，任何未知文件仍拒绝。复制进度按文件数和字节数写入日志。单元测试直接传固定清单；生产 worker 必须在父进程退出后生成权威清单，不能复用运行中进程的预检快照。

- [ ] **Step 4: 实现独立校验**

重新遍历目标并对比相对路径集合、文件类型、大小和 SHA-256；目标多出非日志文件也视为冲突。只有校验通过才返回 `VerifiedMigration` 类型，使指针切换 API 在类型层不能接收未验证事务。

- [ ] **Step 5: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_migration_copy
git add src-tauri/src/core/data_migration src-tauri/tests/data_migration_copy.rs
git commit -m "feat: copy and verify data root transactions"
```

## Task 4: 实现原子指针切换与隐藏迁移进程

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Create: `src-tauri/src/core/data_migration/worker.rs`
- Modify: `src-tauri/src/core/data_migration/mod.rs`
- Modify: `src-tauri/src/core/data_root.rs`
- Modify: `src-tauri/src/core/root_registry.rs`
- Modify: `src-tauri/src/core/portable_root.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/data_migration_worker.rs`
- Modify: `src-tauri/tests/desktop_entrypoint.rs`

- [ ] **Step 1: 写“校验前不得切换”失败测试**

通过 fake `DataRootPointer` 与 fake `ProcessLauncher` 验证：父进程未退出时不复制；复制或校验失败保持旧指针并启动旧根；目标完成标记必须先于指针切换；验证成功后恰好切换一次并正常重启；指针已切换但目标标记无效时启动解析会恢复旧指针；任何路径都不调用删除源目录。

- [ ] **Step 2: 添加 Windows 等待依赖**

添加目标限定依赖：

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = ["Win32_Foundation", "Win32_System_Threading"] }
```

使用 `OpenProcess(SYNCHRONIZE)` 与 `WaitForSingleObject` 等待父 PID，避免 sleep 猜测；打开失败必须进入 `Failed`，不能边运行客户端边复制数据库。

- [ ] **Step 3: 实现来源对应的指针提交**

定义：

```rust
pub trait DataRootPointer {
    fn commit(&self, verified: &VerifiedMigration) -> AppResult<()>;
}
```

已安装版写 HKCU；便携版写原子 JSON；目标为 `<exe>\data` 时清除便携自定义指针。环境变量来源在预检阶段已禁用。

- [ ] **Step 4: 实现隐藏进程入口**

`main.rs` 在进入 Tauri 前调用，不在返回 `()` 的 `main` 中使用 `?`：

```rust
match qingzhou_ssh_lib::run_process_mode(std::env::args_os()) {
    Ok(true) => return,
    Ok(false) => qingzhou_ssh_lib::run(),
    Err(_) => std::process::exit(1),
}
```

只接受内部参数 `--migrate-data-root <absolute-journal-path>`；日志必须位于当前数据根且 schema、源目录和父 PID 有效。GUI 子系统保持 `windows`，不得恢复命令行窗口。

- [ ] **Step 5: 实现成功/失败重启**

父进程退出后，worker 首先重新验证源/目标关系、目标冲突和可用空间，再重建权威清单；这一步失败仍保持旧指针。校验成功后先在目标写入带 migration ID、旧根和清单摘要的完成标记，再提交指针并记录 `Switched`，最后写 `Completed` 并以普通参数启动当前 exe。`resolve_runtime_data_root` 发现指针指向 `Failed` 或带有无效完成标记的迁移目标时，依据受验证的旧根记录恢复旧指针；合法的历史数据根没有迁移 journal 时不受影响。失败时源日志写 `Failed`，不动指针，启动旧根；目标和失败日志保留。

- [ ] **Step 6: 通过测试并提交**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_migration_worker
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test desktop_entrypoint
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/core/data_migration src-tauri/src/core/data_root.rs src-tauri/src/core/root_registry.rs src-tauri/src/core/portable_root.rs src-tauri/src/main.rs src-tauri/src/lib.rs src-tauri/tests/data_migration_worker.rs src-tauri/tests/desktop_entrypoint.rs
git commit -m "feat: migrate data after the gui exits"
```

## Task 5: 接入 Tauri 预检、确认和迁移状态命令

**Files:**

- Create: `src-tauri/src/services/data_migration_service.rs`
- Create: `src-tauri/src/commands/data_migration.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/services/app_services.rs`
- Modify: `src-tauri/src/services/execution_service.rs`
- Modify: `src-tauri/src/services/transfer_service.rs`
- Modify: `src-tauri/src/services/workflow_registry.rs`
- Modify: `src-tauri/src/services/update_service.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/bootstrap.rs`
- Create: `src-tauri/tests/data_migration_service.rs`

- [ ] **Step 1: 写一次性预检确认失败测试**

目标命令：

```rust
preflight_data_root_migration(target_path, state)
start_data_root_migration(preview_id, confirmation_token, app, state)
get_data_root_migration_status(state)
acknowledge_data_root_migration(migration_id, state)
open_data_root_folder(kind, state)
```

测试令牌五分钟过期、一次性、目标被替换、源目录变化、环境变量锁定和运行中任务存在时拒绝。开始迁移前不能关闭应用。

- [ ] **Step 2: 实现协调服务**

`AppState` 增加迁移预览注册表和 `migration_starting` 原子标志。预检调用 Task 2 生成仅供用户确认的估算清单；确认时重做路径、目标冲突和当前可用空间检查，写 `Prepared` 日志，再启动隐藏 worker。最终复制范围只以父进程退出后 worker 生成的权威清单为准。

- [ ] **Step 3: 有序退出客户端**

给执行、传输、工作流和更新服务增加只读 `is_idle` 查询，并由 `AppServices::ensure_idle_for_data_migration` 汇总；有活动任务时预检/确认返回可理解阻断。确认后先原子设置 `migration_starting`，`commands::services` 及更新命令入口在该标志为 true 时拒绝新执行。worker 成功启动后才调用 `app.exit(0)`；依赖进程退出自然关闭 SQLite、SSH 和 WebView2，不在运行中复制。worker 通过父 PID 等待确保资源完全释放。

- [ ] **Step 4: 读取并确认迁移结果**

启动状态读取当前根的迁移日志，将最近一次 `Completed/Failed` 摘要放入 `BootstrapStatus::Ready.lastDataMigration`。确认命令只把 `acknowledged=true` 写回日志，不删除日志或旧数据。`open_data_root_folder` 只接受 `current` 或 `last_source` 枚举，由后端解析可信路径并作为独立参数传给 `explorer.exe`，不接受前端任意可执行文件或 shell 字符串。

- [ ] **Step 5: 注册命令并通过测试**

```powershell
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test data_migration_service
cargo test --locked --manifest-path .\src-tauri\Cargo.toml commands::bootstrap::tests
git add src-tauri/src/services/data_migration_service.rs src-tauri/src/commands/data_migration.rs src-tauri/src/commands/mod.rs src-tauri/src/services/mod.rs src-tauri/src/services/app_services.rs src-tauri/src/services/execution_service.rs src-tauri/src/services/transfer_service.rs src-tauri/src/services/workflow_registry.rs src-tauri/src/services/update_service.rs src-tauri/src/state.rs src-tauri/src/lib.rs src-tauri/src/commands/bootstrap.rs src-tauri/tests/data_migration_service.rs
git commit -m "feat: coordinate confirmed data root migration"
```

## Task 6: 扩展前端契约、API 与预览模式

**Files:**

- Modify: `src/api/contracts.ts`
- Modify: `src/api/tauri.ts`
- Modify: `src/api/tauri.test.ts`
- Modify: `src/api/preview.ts`
- Modify: `src/api/preview.test.ts`
- Modify: `src/app/App.tsx`
- Modify: `src/app/App.test.tsx`
- Modify: `src/app/AppShell.tsx`
- Modify: `src/app/AppShell.test.tsx`

- [ ] **Step 1: 写契约与调用失败测试**

```ts
export interface DataMigrationPreview {
  previewId: string;
  confirmationToken: string;
  expiresAt: number;
  source: string;
  target: string;
  fileCount: number;
  totalBytes: number;
  requiredBytes: number;
  availableBytes: number;
  oldRootWillBeKept: true;
}
```

断言 API 命令名与 camelCase 参数、`BootstrapStatus.ready` 的 `dataRootSource/dataRootMutable/lastDataMigration`，以及 App 将完整信息传给 Settings。

```powershell
pnpm test -- src/api/tauri.test.ts src/app/App.test.tsx src/app/AppShell.test.tsx
```

预期：新字段和 API 不存在而失败。

- [ ] **Step 2: 实现 TypeScript 契约与 API**

增加 `preflightDataRootMigration`、`startDataRootMigration`、`getDataRootMigrationStatus`、`acknowledgeDataRootMigration`、`openDataRootFolder`。开始命令成功返回后 UI 进入“正在退出并迁移”不可操作状态，不自行写指针。

- [ ] **Step 3: 实现预览样本**

`preview.ts` 支持 ready、空间不足、环境变量锁定、上次迁移成功、上次迁移失败和可重试失败目标样本，便于无文件写入地验收界面。

- [ ] **Step 4: 通过测试并提交**

```powershell
pnpm test -- src/api/tauri.test.ts src/api/preview.test.ts src/app/App.test.tsx src/app/AppShell.test.tsx
git add src/api/contracts.ts src/api/tauri.ts src/api/tauri.test.ts src/api/preview.ts src/api/preview.test.ts src/app/App.tsx src/app/App.test.tsx src/app/AppShell.tsx src/app/AppShell.test.tsx
git commit -m "feat: expose data migration state to the client"
```

## Task 7: 在设置中实现数据目录迁移向导

**Files:**

- Create: `src/features/settings/DataRootMigrationDialog.tsx`
- Create: `src/features/settings/DataRootMigrationDialog.test.tsx`
- Modify: `src/features/settings/SettingsPage.tsx`
- Modify: `src/features/settings/SettingsPage.test.tsx`
- Modify: `src/styles/theme.css`

- [ ] **Step 1: 写完整用户流程失败测试**

断言：设置卡片有“更改数据目录”；系统文件夹选择器只允许目录；预检后显示源、目标、文件数、大小、可用空间和“旧目录不会删除”；未勾选确认前不能迁移；环境变量来源按钮禁用并解释原因；便携自定义来源提供“恢复跟随程序目录”。

- [ ] **Step 2: 实现目录选择和预检**

使用现有 `@tauri-apps/plugin-dialog` 的目录选择，不允许文本框直接提交未检查路径。预检错误转换为新手可读文案：目标冲突、空间不足、目录关系错误、无写权限、重解析路径。

- [ ] **Step 3: 实现二次确认与退出说明**

对话框明确说明客户端会自动退出并重启、期间不要移动目录、旧目录保留。确认按钮调用 `startDataRootMigration`；成功后覆盖界面显示“正在安全迁移数据，请等待客户端重新打开”。遇到同一事务的失败目标时提供“重试此次迁移”，不把无关非空目录当作可重试目标。

- [ ] **Step 4: 实现迁移结果通知**

App 顶部显示上次成功或失败横幅。成功显示新路径、旧路径仍保留和“打开旧目录”；失败显示仍在使用旧路径、失败原因和安全重试入口。用户关闭横幅后调用 acknowledge，不能删除任何目录。

- [ ] **Step 5: 通过测试并提交**

```powershell
pnpm test -- src/features/settings/DataRootMigrationDialog.test.tsx src/features/settings/SettingsPage.test.tsx src/app/App.test.tsx
git add src/features/settings/DataRootMigrationDialog.tsx src/features/settings/DataRootMigrationDialog.test.tsx src/features/settings/SettingsPage.tsx src/features/settings/SettingsPage.test.tsx src/styles/theme.css src/app/App.tsx src/app/App.test.tsx
git commit -m "feat: guide users through safe data migration"
```

## Task 8: 加固便携包、启动行为和失败恢复验收

**Files:**

- Modify: `scripts/tests/package-config.tests.ps1`
- Modify: `scripts/tests/desktop-ux.tests.ps1`
- Create: `scripts/tests/data-root-migration.tests.ps1`
- Modify: `scripts/package-portable.ps1`
- Modify: `release/portable/README-portable.txt`

- [ ] **Step 1: 先写打包与进程模式静态守卫**

PowerShell 测试要求便携包继续包含 `portable.flag`，默认不包含机器相关 `data-root.json`；代码保持 `windows_subsystem = "windows"`；隐藏模式只接受内部 flag；所有测试临时数据必须位于项目 `.local` 下。

- [ ] **Step 2: 添加可控的 worker 烟雾测试入口**

仅在测试构建或专用 fixture 中使用临时源/目标，启动同一 exe 的迁移模式，等待完成后验证目标哈希和状态日志。测试不得读取真实注册表值，使用注入 pointer fake 或临时 HKCU 测试键并在 `finally` 精确清理。

- [ ] **Step 3: 验证失败保持旧根**

通过预置目标冲突或复制后篡改 fixture 触发失败，断言旧指针和源文件未变、目标带 `Failed` 日志、没有删除命令。清理只允许已验证位于项目 `.local\data-root-migration-test` 的测试目录。

- [ ] **Step 4: 通过并提交**

```powershell
powershell -NoProfile -File .\scripts\tests\package-config.tests.ps1
powershell -NoProfile -File .\scripts\tests\desktop-ux.tests.ps1
powershell -NoProfile -File .\scripts\tests\data-root-migration.tests.ps1
git add scripts/tests/package-config.tests.ps1 scripts/tests/desktop-ux.tests.ps1 scripts/tests/data-root-migration.tests.ps1 scripts/package-portable.ps1 release/portable/README-portable.txt
git commit -m "test: verify portable data root migration recovery"
```

## Task 9: 文档、全量验证和本地可测试版本

**Files:**

- Modify: `README.md`
- Modify: `docs/data-and-updates.md`
- Modify: `docs/user-guide.md`
- Modify: `docs/security.md`
- Modify: `scripts/tests/public-docs.tests.ps1`

- [ ] **Step 1: 先扩展公开文档守卫**

要求文档出现：随时更改数据目录、退出后迁移、逐文件 SHA-256 校验、旧目录不自动删除、环境变量锁定、便携版恢复跟随程序目录、失败继续使用旧目录。

- [ ] **Step 2: 更新用户操作说明**

写清迁移前建议关闭占用文件的软件、迁移期间不要移动源/目标、成功后用户自行验证再手动处理旧目录。不要提供自动删除旧目录按钮或命令。

- [ ] **Step 3: 运行完整验证**

```powershell
. .\scripts\dev-env.ps1 -Quiet
pnpm test
pnpm build
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo clippy --locked --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
powershell -NoProfile -File .\scripts\tests\public-docs.tests.ps1
powershell -NoProfile -File .\scripts\tests\package-config.tests.ps1
powershell -NoProfile -File .\scripts\tests\data-root-migration.tests.ps1
git diff --check
```

预期：全部退出码为 0，无 clippy 警告、无工作区外数据写入。

- [ ] **Step 4: 生成合并功能的本地测试包**

```powershell
$sourceVersion = (Get-Content -Raw .\package.json | ConvertFrom-Json).version
$testVersion = "$sourceVersion-local.tool-library-data-root.1"
powershell -NoProfile -File .\scripts\build-local-test.ps1 -PackageVersion $testVersion
```

预期产物只在 `D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test\`。如同名产物已存在，递增后缀，不覆盖或删除旧产物。

- [ ] **Step 5: 手工验收清单**

1. 从本地测试包启动，确认没有命令行窗口。
2. 在设置中选择项目目录内新的空测试文件夹。
3. 核对预检大小、空间和旧目录保留提示。
4. 确认迁移，观察客户端退出后自动重启。
5. 验证服务器、执行记录、脚本、下载和设置仍存在。
6. 确认顶部路径为新目录，旧目录原样保留。
7. 使用目标冲突 fixture 做失败测试，确认仍从旧目录启动。
8. 不发布在线更新，等待用户独立验证结果。

- [ ] **Step 6: 本地提交，不推送**

```powershell
git add README.md docs/data-and-updates.md docs/user-guide.md docs/security.md scripts/tests/public-docs.tests.ps1
git commit -m "docs: explain safe data root migration"
git status --short --branch
```

预期：工作树干净且仅本地领先。不要执行 `git push`、GitHub Release、ModelScope 上传或在线更新清单修改。
