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
cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path .\src-tauri\Cargo.toml
```

Tauri 调试构建（不生成安装包）：

```powershell
. .\scripts\dev-env.ps1
pnpm tauri build --debug --no-bundle
```

## 真实 SSH 集成测试

推荐使用项目内 AsyncSSH 夹具。脚本会把 Python 包、pip 缓存、字节码、日志、主机密钥和带口令 Ed25519 测试密钥写到 `.local`，并在结束时关闭夹具进程：

```powershell
.\scripts\test-ssh-live.ps1
```

依赖已经安装到 `.local\python-packages` 后可跳过安装：

```powershell
.\scripts\test-ssh-live.ps1 -SkipPythonDependencyInstall
```

测试覆盖密码认证、带口令私钥认证和错误主机指纹优先拦截。

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
