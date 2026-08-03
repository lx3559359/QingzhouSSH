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

脚本依次执行 `ssh_live`、`sftp_live` 和 `m2_live`。覆盖密码认证、带口令私钥认证、错误主机指纹优先拦截、128 KiB 以上 SFTP 往返、内置/高级任务、普通与 gzip 日志检索下载、历史闭环以及凭据 canary 扫描。

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
