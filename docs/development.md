# 开发指南

## 环境要求

- Windows 10/11 x64 与 PowerShell 5.1 或更高版本。
- Rust stable（`russh 0.62` 最低要求 Rust 1.85）。
- Node.js 与 `pnpm 10.14.0`。
- 运行真实 SSH 夹具时需要 Python 3.12+ 和 `ssh-keygen`。
- Docker 仅作为可选的 Ubuntu OpenSSH 夹具；使用前必须把 Docker Desktop/WSL 的镜像与虚拟磁盘数据位置配置到 D 盘或其他明确允许的位置。

不要在本项目中执行全局依赖安装。每个 PowerShell 会话先加载开发环境：

```powershell
. .\scripts\dev-env.ps1
```

该脚本把 Cargo、Rustup、pnpm、npm、Corepack、临时目录、测试数据、运行数据和构建产物全部指向仓库内的 `.local`、`target` 或 `artifacts`。`CARGO_TARGET_DIR` 直接使用当前仓库的物理 `target` 目录，不会在项目父目录或其他盘符创建兼容目录与 junction。

## 安装依赖与运行界面

```powershell
. .\scripts\dev-env.ps1
pnpm install --frozen-lockfile
pnpm dev
```

Vite 默认监听 `http://localhost:1420`。运行完整桌面应用：

```powershell
. .\scripts\dev-env.ps1
pnpm tauri dev
```

开发环境默认把应用数据放到 `.local\dev-data`。若要测试首次选择界面，请在新的隔离工作副本中清除 `QINGZHOU_DATA_ROOT`，不要删除真实用户数据。

## 自动化检查

```powershell
. .\scripts\dev-env.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-d-drive.ps1
pnpm test
pnpm build
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo clippy --locked --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets -- --nocapture
```

Tauri 调试构建（不生成安装包）：

```powershell
. .\scripts\dev-env.ps1
pnpm tauri build --debug --no-bundle
```

## 真实 SSH 集成测试

推荐使用项目内 AsyncSSH 夹具。脚本会把 Python 包、pip 缓存、字节码、日志、远端模拟目录、主机密钥和带口令 Ed25519 测试密钥写到 `.local`，并在结束时关闭夹具进程：

```powershell
.\scripts\test-ssh-live.ps1
```

依赖已经安装到 `.local\python-packages` 后可跳过安装：

```powershell
.\scripts\test-ssh-live.ps1 -SkipPythonDependencyInstall
```

脚本依次执行 `ssh_live`、`sftp_live`、`m2_live` 和 `workflow_live`。除密码/私钥认证、主机指纹、SFTP、任务、日志和历史外，工作流 live 用例还覆盖失败暂停、同节点重试、条件真假分支、上传覆盖恢复点、逆序回滚、诊断包及凭据/脚本 canary 扫描。

需要单独控制夹具时使用：

```powershell
.\scripts\ssh-fixture.ps1 -Action Start -SkipPythonDependencyInstall
.\scripts\ssh-fixture.ps1 -Action Status
.\scripts\ssh-fixture.ps1 -Action Stop
```

夹具生命周期幂等性检查：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\ssh-fixture.tests.ps1
```

若开发机已经确认 Docker 的数据根目录不会写入 C 盘，可使用 Ubuntu OpenSSH 夹具：

```powershell
New-Item -ItemType Directory -Force .\.local\test-keys | Out-Null
if (-not (Test-Path .\.local\test-keys\id_ed25519)) {
  ssh-keygen -q -t ed25519 -N 'fixture-passphrase' -C 'qingzhou-fixture-only' -f .\.local\test-keys\id_ed25519
}
Copy-Item .\.local\test-keys\id_ed25519.pub .\.local\test-keys\authorized_keys -Force
try {
  docker compose -f .\tests\fixtures\sshd\compose.yml up -d --build
  cargo test --manifest-path .\src-tauri\Cargo.toml --test ssh_live -- --ignored --test-threads=1
} finally {
  docker compose -f .\tests\fixtures\sshd\compose.yml down
}
```

## 目录约定

- `.local`：工具链、包缓存、测试密钥、夹具日志和开发数据；不提交。
- `target`：Rust 构建产物；不提交。
- `dist`：前端生产构建；不提交。
- `artifacts`：调试包、安装包和后续发布产物；不提交。
- 用户选择的数据根目录：`app.db`、`vault`、`logs`、`downloads`、`backups`、`templates`、`cache` 和 `updates`。

## M2 运行限制与状态语义

- 单个 stdout/stderr 事件最多 32 KiB，单次任务或日志捕获最多 32 MiB，历史摘要最多 8 KiB。
- 日志路径必须是远端绝对 `.log` 或 `.gz` 路径；关键词最多 512 字节，上下文 0–20 行，结果上限 1–10,000，界面每页 50 条。
- SFTP 以 64 KiB 分块，下载只能写入数据根目录下的 `downloads`；上传/下载完成前使用 `.partial` 临时文件，成功后执行 SHA-256 校验。
- `cancelled` 表示已经确认本地执行通道终止；若无法确认远端进程是否停止，或应用重启时发现遗留 `running` 记录，则状态为 `uncertain`，不会误报成功。
- 服务启动、停止、重启以及高级命令/脚本都要求二次确认。高级模式仍是一次性非交互执行，不提供 SSH 终端。

## Operations Engine V2 开发边界

- 前端只提交固定的 `taskId`、`taskVersion`、结构化参数和可选的 `confirmedPreviewId`。Tauri IPC 使用拒绝未知字段的强类型 DTO，不接受 `command`、`script`、`backup`、`rollback` 或命令模板。
- 命令模板和目标模板只存在于 Rust 任务目录中，并通过 Serde 跳过序列化。参数先按类型校验，再由后端安全渲染；前端返回值只包含步骤标题、风险、权限、范围和预计耗时等公开元数据。
- 一次 `operation_run` 表示用户看到的完整运维任务生命周期；每个真正发起 SSH 命令的步骤仍创建一条现有 `execution`，并由 `operation_step.execution_id` 关联。这样既能展示高层预检/执行/验证状态，也保留原有执行历史、输出限额和脱敏链路。
- 权限需求分为当前用户和 `root_or_passwordless_sudo`。需要提权的实现只能使用 root 会话或非交互 `sudo -n`；不得请求、保存或通过 stdin 传递 sudo 密码。
- 当前基础阶段只开放 safe/caution 的兼容执行链。dangerous 任务可以生成只读预检和确认摘要，但尚不执行远端修改；备份、验证、回滚和确认恢复链完成前不得把它们标记为可用。

## M3 工作流约束与恢复语义

- 节点类型固定为开始、快捷任务、自定义命令/脚本、上传、下载、日志检索、条件和停止；不接受任意插件节点或交互式终端节点。
- 每个定义只能有一个开始节点；停止节点无出边；普通节点最多一条 `success`；条件节点必须各有一条 `true` 与 `false`。所有节点必须可达且不能成环。
- 单个工作流最多 100 个节点和 200 条边。条件只支持退出码、结构化结果受限字段路径和脱敏输出摘要固定文本，不接受正则、Shell 或动态属性访问。
- 保存会创建不可变版本；已开始的运行始终引用原版本。画布有未保存改动时，界面不会把旧版本当作新配置运行。
- 执行严格串行。可重试节点失败后工作流为 `paused`；重试只创建该节点的新 attempt，成功后沿原定义继续。
- `cancelled` 只表示后端已确认当前子执行停止；无法确认时为 `uncertain`。应用启动会把遗留 `running` 运行和节点恢复为 `uncertain`，不会自动续跑。
- 上传覆盖可在变更前把原文件流式保存到 `<data-root>\backups\workflows\<run-id>\<node-id>`。回滚要求二次确认，按成功变更逆序恢复；原文件不存在时删除本次新建文件。
- 诊断文件写入 `<data-root>\downloads`，只向前端返回相对路径；包含版本校验和、运行/节点时间线、错误和恢复点元数据，不包含凭据、完整敏感参数、脚本文本或任意本地绝对路径。
