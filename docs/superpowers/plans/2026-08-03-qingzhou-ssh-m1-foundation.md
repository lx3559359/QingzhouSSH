# QingzhouSSH Milestone 1: Secure Connection Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a runnable Windows Tauri application that keeps its controllable data on the selected drive, securely stores credentials, verifies SSH host identity, authenticates with password or private key, and reports detected Linux capabilities.

**Architecture:** React is a presentation-only WebView. Typed Tauri commands call focused Rust services for data-root resolution, SQLite, DPAPI, SSH, and system probing. Development commands run through a project-local PowerShell environment so caches, targets, test data, and artifacts remain under the D-drive workspace.

**Tech Stack:** Tauri 2, React 19, TypeScript, Rust, SQLx SQLite, `ssh2` 0.9.6 with vendored OpenSSL, `windows-dpapi` 0.2, `winreg` 0.56, Vitest, Testing Library, Docker Ubuntu SSH fixture.

---

## Scope and execution preconditions

This plan implements only Milestone 1 from `docs/superpowers/plans/2026-08-03-qingzhou-ssh-roadmap.md`. Do not add quick-task templates, log search, SFTP UI, workflow nodes, updater code, GitHub releases, or ModelScope synchronization in this milestone.

At execution time:

1. Use `superpowers:using-git-worktrees` before editing.
2. Create a feature worktree on the D drive.
3. Run every project command after dot-sourcing `scripts/dev-env.ps1`.
4. Use test-driven development for every behavior below.
5. Stop at the milestone gate for user review.

## File map

### Development environment and project shell

- `scripts/dev-env.ps1` — defines D-local Cargo, pnpm, temp, test-data, and artifact paths.
- `scripts/tests/dev-env.tests.ps1` — verifies every controllable path stays inside the worktree.
- `scripts/verify-d-drive.ps1` — checks Cargo/pnpm metadata and application data-root configuration.
- `.npmrc` — project-local pnpm store/cache configuration.
- `.cargo/config.toml` — project-local Cargo target directory.
- `package.json`, `pnpm-lock.yaml`, `vite.config.ts`, `tsconfig*.json`, `index.html` — frontend toolchain.
- `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json` — Rust/Tauri toolchain and permission boundary.

### Rust core

- `src-tauri/src/error.rs` — serializable application error taxonomy.
- `src-tauri/src/core/data_root.rs` — data-root precedence, validation, directory creation.
- `src-tauri/src/core/root_registry.rs` — HKCU storage for the non-secret installed-mode root pointer.
- `src-tauri/src/window.rs` — incognito first-run WebView and selected-root WebView2 data directory.
- `src-tauri/src/core/database.rs` — SQLx pool and migrations.
- `src-tauri/migrations/0001_foundation.sql` — server and host-key schema.
- `src-tauri/src/core/vault.rs` — atomic encrypted credential files.
- `src-tauri/src/core/secret_protector.rs` — DPAPI adapter and test double boundary.
- `src-tauri/src/domain/server.rs` — server and credential request types plus validation.
- `src-tauri/src/repositories/server_repository.rs` — SQLite server and host-key persistence.
- `src-tauri/src/core/ssh/fingerprint.rs` — SHA-256 host fingerprint formatting.
- `src-tauri/src/core/ssh/transport.rs` — host inspection, authentication, command execution.
- `src-tauri/src/core/ssh/trust.rs` — first-use, match, and mismatch decisions.
- `src-tauri/src/core/system_probe.rs` — remote probe command and parser.
- `src-tauri/src/services/app_services.rs` — initialized service bundle for one data root.
- `src-tauri/src/commands/bootstrap.rs` — first-run and data-root commands.
- `src-tauri/src/commands/servers.rs` — server CRUD, fingerprint, trust, and connection-test commands.
- `src-tauri/src/state.rs` — managed optional service state.

### React UI

- `src/api/contracts.ts` — JSON contracts matching Rust command payloads.
- `src/api/tauri.ts` — the only frontend module allowed to call `invoke`.
- `src/features/bootstrap/DataRootGate.tsx` — first-run directory selection.
- `src/features/servers/ServerListPage.tsx` — list and connection status.
- `src/features/servers/AddServerDialog.tsx` — server and credential form.
- `src/features/servers/HostKeyDialog.tsx` — explicit first-use fingerprint approval and mismatch block.
- `src/app/App.tsx` — small route/state composition only.
- `src/styles/theme.css` — approved white-silver cards, three-layer shadows, gradients, and contrast tokens.

### Tests and fixtures

- Rust unit tests remain beside focused modules under `#[cfg(test)]`.
- `src-tauri/tests/foundation_integration.rs` — database, vault, repository, trust, and probe service integration.
- `src-tauri/tests/ssh_live.rs` — ignored live SSH test.
- `tests/fixtures/sshd/Dockerfile`, `tests/fixtures/sshd/compose.yml` — deterministic Ubuntu SSH target.
- `src/**/*.test.tsx` — frontend behavior tests.

## Task 1: Lock all controllable development paths to the D-drive worktree

**Files:**

- Create: `scripts/tests/dev-env.tests.ps1`
- Create: `scripts/dev-env.ps1`
- Create: `.npmrc`
- Create: `.cargo/config.toml`
- Modify: `.gitignore`

- [ ] **Step 1: Write the failing environment-path test**

```powershell
# scripts/tests/dev-env.tests.ps1
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

. (Join-Path $repoRoot 'scripts\dev-env.ps1') -Quiet

$paths = @(
  $env:CARGO_HOME,
  $env:CARGO_TARGET_DIR,
  $env:NPM_CONFIG_CACHE,
  $env:COREPACK_HOME,
  $env:PNPM_HOME,
  $env:PNPM_STORE_DIR,
  $env:TEMP,
  $env:TMP,
  $env:QINGZHOU_DATA_ROOT,
  $env:QINGZHOU_ARTIFACTS_DIR
)

foreach ($path in $paths) {
  if (-not $path.StartsWith($repoRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Path escaped repository: $path"
  }
  if ((Split-Path -Qualifier $path) -ne (Split-Path -Qualifier $repoRoot)) {
    throw "Path is on another drive: $path"
  }
}

Write-Host 'PASS: all controllable development paths are project-local'
```

- [ ] **Step 2: Run the test and verify it fails because the environment script does not exist**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
```

Expected: FAIL with `dev-env.ps1` not found.

- [ ] **Step 3: Implement the project-local development environment**

```powershell
# scripts/dev-env.ps1
param([switch]$Quiet)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$localRoot = Join-Path $projectRoot '.local'

$env:CARGO_HOME = Join-Path $localRoot 'cargo-home'
$env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
$env:NPM_CONFIG_CACHE = Join-Path $localRoot 'npm-cache'
$env:COREPACK_HOME = Join-Path $localRoot 'corepack'
$env:PNPM_HOME = Join-Path $localRoot 'pnpm-home'
$env:PNPM_STORE_DIR = Join-Path $localRoot 'pnpm-store'
$env:TEMP = Join-Path $localRoot 'tmp'
$env:TMP = $env:TEMP
$env:QINGZHOU_DATA_ROOT = Join-Path $localRoot 'dev-data'
$env:QINGZHOU_ARTIFACTS_DIR = Join-Path $projectRoot 'artifacts'

@(
  $env:CARGO_HOME,
  $env:CARGO_TARGET_DIR,
  $env:NPM_CONFIG_CACHE,
  $env:COREPACK_HOME,
  $env:PNPM_HOME,
  $env:PNPM_STORE_DIR,
  $env:TEMP,
  $env:QINGZHOU_DATA_ROOT,
  $env:QINGZHOU_ARTIFACTS_DIR
) | ForEach-Object { New-Item -ItemType Directory -Force -Path $_ | Out-Null }

if (-not $Quiet) {
  Write-Host "QingzhouSSH development environment: $projectRoot"
  Write-Host "Cargo home: $env:CARGO_HOME"
  Write-Host "Data root:  $env:QINGZHOU_DATA_ROOT"
}

if (($env:Path -split ';') -notcontains $env:PNPM_HOME) {
  $env:Path = "$env:PNPM_HOME;$env:Path"
}
```

```ini
# .npmrc
store-dir=.local/pnpm-store
cache-dir=.local/pnpm-cache
state-dir=.local/pnpm-state
virtual-store-dir=node_modules/.pnpm
```

```toml
# .cargo/config.toml
[build]
target-dir = "target"
```

Append to `.gitignore`:

```gitignore
.local/
artifacts/
target/
node_modules/
```

- [ ] **Step 4: Re-run the environment test**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
```

Expected: `PASS: all controllable development paths are project-local`.

- [ ] **Step 5: Commit the environment boundary**

```powershell
git add scripts .npmrc .cargo .gitignore
git commit -m "build: keep development data inside workspace"
```

## Task 2: Create the runnable Tauri shell and visual baseline

**Files:**

- Create: `package.json`
- Create: `vite.config.ts`
- Create: `tsconfig.json`
- Create: `tsconfig.app.json`
- Create: `tsconfig.node.json`
- Create: `index.html`
- Create: `src/main.tsx`
- Create: `src/app/App.test.tsx`
- Create: `src/app/App.tsx`
- Create: `src/styles/theme.css`
- Create: `assets/app-icon.svg`
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/error.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/default.json`
- Create: `LICENSE`

- [ ] **Step 1: Add package manifests and install into the project-local pnpm store**

```json
{
  "name": "qingzhou-ssh",
  "private": true,
  "version": "0.1.0",
  "packageManager": "pnpm@10.14.0",
  "type": "module",
  "scripts": {
    "dev": "vite --port 1420 --strictPort",
    "build": "tsc -b && vite build",
    "test": "vitest run",
    "test:watch": "vitest",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.11.0",
    "@tauri-apps/plugin-dialog": "^2.7.0",
    "react": "^19.1.0",
    "react-dom": "^19.1.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.0",
    "@testing-library/jest-dom": "^6.6.0",
    "@testing-library/react": "^16.3.0",
    "@testing-library/user-event": "^14.6.0",
    "@types/react": "^19.1.0",
    "@types/react-dom": "^19.1.0",
    "@vitejs/plugin-react": "^4.6.0",
    "jsdom": "^26.1.0",
    "typescript": "^5.8.0",
    "vite": "^7.0.0",
    "vitest": "^3.2.0"
  }
}
```

Run:

```powershell
. .\scripts\dev-env.ps1
corepack enable --install-directory $env:PNPM_HOME
corepack prepare pnpm@10.14.0 --activate
pnpm install
```

Expected: `pnpm-lock.yaml` and `node_modules` are created under the worktree; `pnpm store path` resolves under `.local`.

- [ ] **Step 2: Write the failing React shell test**

```tsx
// src/app/App.test.tsx
import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App';

describe('App', () => {
  it('presents QingzhouSSH as a task tool without a terminal entry', () => {
    render(<App />);
    expect(screen.getByRole('heading', { name: '轻舟 SSH' })).toBeVisible();
    expect(screen.getByText('安全地完成 Linux 操作')).toBeVisible();
    expect(screen.queryByText('打开终端')).not.toBeInTheDocument();
  });
});
```

Run: `pnpm test src/app/App.test.tsx`

Expected: FAIL because `App.tsx` does not exist.

- [ ] **Step 3: Add the minimal React/Vite shell**

```tsx
// src/app/App.tsx
import '../styles/theme.css';

export function App() {
  return (
    <main className="app-shell">
      <section className="welcome-card silver-card">
        <div className="app-mark">Q</div>
        <div>
          <h1>轻舟 SSH</h1>
          <p>安全地完成 Linux 操作</p>
        </div>
      </section>
    </main>
  );
}
```

```tsx
// src/main.tsx
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app/App';

createRoot(document.getElementById('root')!).render(
  <StrictMode><App /></StrictMode>,
);
```

```css
/* src/styles/theme.css */
:root {
  font-family: "Segoe UI", "Microsoft YaHei UI", sans-serif;
  color: #18243a;
  background: radial-gradient(circle at 10% 5%, #d8edff, transparent 32%), #e7edf4;
}
* { box-sizing: border-box; }
body { margin: 0; min-width: 960px; min-height: 100vh; }
.app-shell { min-height: 100vh; display: grid; place-items: center; padding: 32px; }
.silver-card {
  overflow: hidden;
  border: 1px solid #fff;
  border-right-color: #b7bec8;
  border-bottom-color: #aeb6c1;
  border-radius: 18px;
  background: linear-gradient(138deg, #fff 0%, #f8f9fa 25%, #e5e8ed 58%, #ccd2da 82%, #fafafa 100%);
  box-shadow: 0 28px 44px rgb(36 51 79 / 30%), 0 10px 15px rgb(38 54 83 / 24%), 0 3px 5px rgb(39 53 78 / 20%);
}
.welcome-card { display: flex; align-items: center; gap: 16px; min-width: 420px; padding: 28px; }
.app-mark { width: 52px; height: 52px; display: grid; place-items: center; border-radius: 15px; color: white; font-weight: 800; background: linear-gradient(145deg, #72c4ff, #4a62dc); }
h1, p { margin: 0; }
p { margin-top: 6px; color: #607188; }
```

```html
<!-- index.html -->
<!doctype html>
<html lang="zh-CN">
  <head><meta charset="UTF-8" /><meta name="viewport" content="width=device-width, initial-scale=1.0" /><title>轻舟 SSH</title></head>
  <body><div id="root"></div><script type="module" src="/src/main.tsx"></script></body>
</html>
```

```ts
// vite.config.ts
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { host: '127.0.0.1', port: 1420, strictPort: true },
  test: { environment: 'jsdom', globals: true },
});
```

```json
// tsconfig.json
{
  "files": [],
  "references": [{ "path": "./tsconfig.app.json" }, { "path": "./tsconfig.node.json" }]
}
```

```json
// tsconfig.app.json
{
  "compilerOptions": {
    "target": "ES2022", "useDefineForClassFields": true, "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext", "skipLibCheck": true, "moduleResolution": "Bundler", "allowImportingTsExtensions": true,
    "verbatimModuleSyntax": true, "moduleDetection": "force", "noEmit": true, "jsx": "react-jsx",
    "strict": true, "noUnusedLocals": true, "noUnusedParameters": true, "types": ["vitest/globals"]
  },
  "include": ["src"]
}
```

```json
// tsconfig.node.json
{
  "compilerOptions": {
    "target": "ES2023", "lib": ["ES2023"], "module": "ESNext", "skipLibCheck": true,
    "moduleResolution": "Bundler", "allowImportingTsExtensions": true, "verbatimModuleSyntax": true,
    "moduleDetection": "force", "noEmit": true, "strict": true
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 4: Verify the React test passes**

Run: `pnpm test src/app/App.test.tsx`

Expected: 1 test PASS.

- [ ] **Step 5: Add the Rust/Tauri shell**

```toml
# src-tauri/Cargo.toml
[package]
name = "qingzhou-ssh"
version = "0.1.0"
edition = "2021"

[lib]
name = "qingzhou_ssh_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
ssh2 = { version = "0.9.6", features = ["vendored-openssl"] }
sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite", "migrate", "macros"] }
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2.7"
thiserror = "2"
tokio = { version = "1", features = ["rt-multi-thread", "sync"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
windows-dpapi = "0.2"
winreg = "0.56"
zeroize = { version = "1", features = ["derive"] }

[dev-dependencies]
tempfile = "3"
```

```rust
// src-tauri/src/error.rs
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("输入无效：{0}")]
    Validation(String),
    #[error("应用尚未完成数据目录初始化")]
    NotReady,
    #[error("安全检查失败：{0}")]
    Security(String),
    #[error("I/O 操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据库操作失败：{0}")]
    Database(#[from] sqlx::Error),
    #[error("数据库迁移失败：{0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("SSH 操作失败：{0}")]
    Ssh(#[from] ssh2::Error),
    #[error("桌面窗口操作失败：{0}")]
    Tauri(#[from] tauri::Error),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        #[derive(Serialize)]
        struct Payload<'a> { code: &'a str, message: String }
        let code = match self {
            AppError::Validation(_) => "validation",
            AppError::NotReady => "not_ready",
            AppError::Security(_) => "security",
            AppError::Io(_) => "io",
            AppError::Database(_) => "database",
            AppError::Migration(_) => "migration",
            AppError::Ssh(_) => "ssh",
            AppError::Tauri(_) => "tauri",
        };
        Payload { code, message: self.to_string() }.serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

```rust
// src-tauri/src/lib.rs
mod error;

use tauri::{WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("轻舟 SSH")
                .inner_size(1180.0, 760.0)
                .min_inner_size(960.0, 640.0)
                .incognito(true)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run QingzhouSSH");
}

#[cfg(test)]
mod tests {
    #[test]
    fn package_identity_is_stable() {
        assert_eq!(env!("CARGO_PKG_NAME"), "qingzhou-ssh");
    }
}
```

```rust
// src-tauri/src/main.rs
fn main() { qingzhou_ssh_lib::run(); }
```

```rust
// src-tauri/build.rs
fn main() { tauri_build::build() }
```

```json
// src-tauri/tauri.conf.json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "轻舟 SSH",
  "version": "0.1.0",
  "identifier": "com.qingzhoussh.desktop",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [],
    "security": { "csp": null }
  },
  "bundle": { "active": true, "targets": "all", "icon": ["icons/icon.ico"] }
}
```

```json
// src-tauri/capabilities/default.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "QingzhouSSH main-window permissions",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:allow-open"]
}
```

- [ ] **Step 6: Generate icons and add Apache-2.0 license**

Create `assets/app-icon.svg` before generating platform icons:

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#72c4ff"/><stop offset="1" stop-color="#5c4fe4"/></linearGradient></defs>
  <rect width="512" height="512" rx="112" fill="url(#g)"/><path d="M142 252c0-75 47-124 116-124s116 49 116 124c0 45-17 82-47 103l42 37-42 44-55-55c-5 1-10 1-14 1-69 0-116-51-116-130Zm70 0c0 44 17 70 46 70s46-26 46-70-17-66-46-66-46 22-46 66Z" fill="#fff"/>
</svg>
```

Then run:

```powershell
pnpm tauri icon .\assets\app-icon.svg --output .\src-tauri\icons
Invoke-WebRequest 'https://www.apache.org/licenses/LICENSE-2.0.txt' -OutFile '.\LICENSE'
```

- [ ] **Step 7: Verify frontend, Rust, and Tauri shell**

Run:

```powershell
pnpm test
cargo test --manifest-path .\src-tauri\Cargo.toml
pnpm build
pnpm tauri build --debug --no-bundle
```

Expected: all commands exit 0; build output remains under the worktree.

- [ ] **Step 8: Commit the shell**

```powershell
git add package.json pnpm-lock.yaml vite.config.ts tsconfig*.json index.html src assets src-tauri LICENSE
git commit -m "feat: scaffold QingzhouSSH desktop shell"
```

## Task 3: Resolve and initialize the data root without default AppData writes

**Files:**

- Create: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/src/core/data_root.rs`
- Create: `src-tauri/src/core/root_registry.rs`
- Create: `src-tauri/src/window.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing precedence and directory tests**

```rust
// append in src-tauri/src/core/data_root.rs under #[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn environment_override_wins_over_portable_and_registry() {
        let input = DataRootInputs {
            env_override: Some(r"D:\work\data".into()),
            portable_root: Some(r"D:\app\data".into()),
            registry_root: Some(r"E:\saved".into()),
        };
        let resolved = resolve_data_root(input);
        assert_eq!(resolved.source, DataRootSource::Environment);
        assert_eq!(resolved.path.unwrap(), std::path::PathBuf::from(r"D:\work\data"));
    }

    #[test]
    fn no_source_requires_user_selection_instead_of_appdata_fallback() {
        let resolved = resolve_data_root(DataRootInputs::default());
        assert_eq!(resolved.source, DataRootSource::NeedsSelection);
        assert!(resolved.path.is_none());
    }

    #[test]
    fn initialization_creates_only_declared_subdirectories() {
        let temp = tempdir().unwrap();
        initialize_data_root(temp.path()).unwrap();
        for name in ["vault", "logs", "downloads", "backups", "templates", "cache", "updates"] {
            assert!(temp.path().join(name).is_dir(), "missing {name}");
        }
        assert!(!temp.path().join("AppData").exists());
    }
}
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml data_root::tests`

Expected: FAIL because the data-root types and functions are undefined.

- [ ] **Step 3: Implement pure resolution and validated initialization**

```rust
// src-tauri/src/core/data_root.rs
use std::{fs, path::{Path, PathBuf}};
use serde::Serialize;
use crate::error::{AppError, AppResult};

#[derive(Debug, Default)]
pub struct DataRootInputs {
    pub env_override: Option<PathBuf>,
    pub portable_root: Option<PathBuf>,
    pub registry_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataRootSource { Environment, Portable, Registry, NeedsSelection }

#[derive(Debug, Serialize)]
pub struct DataRootResolution { pub source: DataRootSource, pub path: Option<PathBuf> }

pub fn resolve_data_root(input: DataRootInputs) -> DataRootResolution {
    if let Some(path) = input.env_override { return DataRootResolution { source: DataRootSource::Environment, path: Some(path) }; }
    if let Some(path) = input.portable_root { return DataRootResolution { source: DataRootSource::Portable, path: Some(path) }; }
    if let Some(path) = input.registry_root { return DataRootResolution { source: DataRootSource::Registry, path: Some(path) }; }
    DataRootResolution { source: DataRootSource::NeedsSelection, path: None }
}

pub fn initialize_data_root(root: &Path) -> AppResult<()> {
    if !root.is_absolute() { return Err(AppError::Validation("数据目录必须是绝对路径".into())); }
    fs::create_dir_all(root)?;
    for name in ["vault", "logs", "downloads", "backups", "templates", "cache", "updates"] {
        fs::create_dir_all(root.join(name))?;
    }
    let probe = root.join(".write-probe");
    fs::write(&probe, b"qingzhou")?;
    fs::remove_file(probe)?;
    Ok(())
}

pub fn resolve_runtime_data_root() -> AppResult<DataRootResolution> {
    let executable = std::env::current_exe()?;
    let executable_directory = executable.parent().ok_or_else(|| AppError::Validation("无法确定程序目录".into()))?;
    let portable_root = executable_directory.join("portable.flag").is_file().then(|| executable_directory.join("data"));
    Ok(resolve_data_root(DataRootInputs {
        env_override: std::env::var_os("QINGZHOU_DATA_ROOT").map(PathBuf::from),
        portable_root,
        registry_root: crate::core::root_registry::load_data_root()?,
    }))
}
```

```rust
// src-tauri/src/core/root_registry.rs
use std::path::{Path, PathBuf};
use winreg::HKCU;
use crate::error::AppResult;

const KEY: &str = r"Software\QingzhouSSH";
const VALUE: &str = "DataRoot";

pub fn load_data_root() -> AppResult<Option<PathBuf>> {
    match HKCU.open_subkey(KEY) {
        Ok(key) => Ok(key.get_value::<String, _>(VALUE).ok().map(PathBuf::from)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn save_data_root(path: &Path) -> AppResult<()> {
    let (key, _) = HKCU.create_subkey(KEY)?;
    key.set_value(VALUE, &path.to_string_lossy().to_string())?;
    Ok(())
}
```

Runtime precedence is: `QINGZHOU_DATA_ROOT` → `portable.flag` beside executable → HKCU pointer → needs user selection. The registry stores only the non-secret path pointer.

Do not let Tauri create a default configured window. Replace the temporary Task 2 setup with this focused window builder:

```rust
// src-tauri/src/window.rs
use std::path::Path;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use crate::error::AppResult;

pub fn build_main_window(app: &AppHandle, data_root: Option<&Path>) -> AppResult<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("轻舟 SSH")
        .inner_size(1180.0, 760.0)
        .min_inner_size(960.0, 640.0);
    let builder = match data_root {
        Some(root) => builder.data_directory(root.join("cache").join("webview2")),
        None => builder.incognito(true),
    };
    Ok(builder.build()?)
}
```

In `lib.rs` setup, call `resolve_runtime_data_root`; initialize an existing resolved root, then call `build_main_window(app.handle(), resolution.path.as_deref())`. A first-run window is incognito, so directory-selection UI can render without creating persistent WebView2 data on C. After a directory is selected, the app may continue incognito for that one run; every subsequent launch must use `<data-root>\cache\webview2`.

- [ ] **Step 4: Run data-root tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml data_root::tests`

Expected: 3 tests PASS.

- [ ] **Step 5: Commit data-root behavior**

```powershell
git add src-tauri/src/core src-tauri/src/lib.rs
git commit -m "feat: add explicit data root resolution"
```

## Task 4: Add SQLite migrations and server persistence

**Files:**

- Create: `src-tauri/migrations/0001_foundation.sql`
- Create: `src-tauri/src/core/database.rs`
- Create: `src-tauri/src/domain/mod.rs`
- Create: `src-tauri/src/domain/server.rs`
- Create: `src-tauri/src/repositories/mod.rs`
- Create: `src-tauri/src/repositories/server_repository.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Write failing domain-validation and repository tests**

```rust
// src-tauri/src/domain/server.rs tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_host_and_zero_port() {
        let request = CreateServerRequest {
            name: "测试".into(), host: " ".into(), port: 0,
            username: "root".into(), credential: CredentialInput::Password { password: "secret".into() },
        };
        assert!(request.validate().is_err());
    }
}
```

```rust
// src-tauri/src/repositories/server_repository.rs tests
#[sqlx::test(migrations = "./migrations")]
async fn inserts_and_lists_server_without_secret_columns(pool: sqlx::SqlitePool) {
    let repository = ServerRepository::new(pool.clone());
    let server = ServerProfile::new("网站服务器", "127.0.0.1", 22, "tester", AuthKind::Password, "cred-1");
    repository.insert(&server).await.unwrap();
    let listed = repository.list().await.unwrap();
    assert_eq!(listed, vec![server]);

    let secret_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('servers') WHERE lower(name) IN ('password','private_key','passphrase')"
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(secret_columns, 0);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml domain::server
cargo test --manifest-path src-tauri/Cargo.toml repositories::server_repository
```

Expected: FAIL because schema and types do not exist.

- [ ] **Step 3: Add the foundation migration**

```sql
-- src-tauri/migrations/0001_foundation.sql
PRAGMA foreign_keys = ON;

CREATE TABLE servers (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  host TEXT NOT NULL,
  port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
  username TEXT NOT NULL,
  auth_kind TEXT NOT NULL CHECK (auth_kind IN ('password', 'private_key')),
  credential_id TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE host_keys (
  server_id TEXT PRIMARY KEY NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  algorithm TEXT NOT NULL,
  fingerprint_sha256 TEXT NOT NULL,
  raw_key_base64 TEXT NOT NULL,
  trusted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_servers_name ON servers(name COLLATE NOCASE);
```

- [ ] **Step 4: Implement database opening and migration**

```rust
// src-tauri/src/core/database.rs
use std::path::Path;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, SqlitePool};
use crate::error::AppResult;

#[derive(Clone)]
pub struct Database { pool: SqlitePool }

impl Database {
    pub async fn open(data_root: &Path) -> AppResult<Self> {
        let options = SqliteConnectOptions::new()
            .filename(data_root.join("app.db"))
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
    pub fn pool(&self) -> &SqlitePool { &self.pool }
}
```

Add these exact domain contracts; credential-bearing enums intentionally do not derive `Debug`:

```rust
// src-tauri/src/domain/server.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind { Password, PrivateKey }

impl AuthKind {
    pub fn as_str(self) -> &'static str {
        match self { Self::Password => "password", Self::PrivateKey => "private_key" }
    }
}

impl TryFrom<&str> for AuthKind {
    type Error = AppError;
    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "password" => Ok(Self::Password),
            "private_key" => Ok(Self::PrivateKey),
            other => Err(AppError::Validation(format!("未知认证类型：{other}"))),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CredentialInput {
    Password { password: String },
    PrivateKey { private_key: String, passphrase: Option<String> },
}

impl CredentialInput {
    pub fn auth_kind(&self) -> AuthKind {
        match self { Self::Password { .. } => AuthKind::Password, Self::PrivateKey { .. } => AuthKind::PrivateKey }
    }
    fn is_empty(&self) -> bool {
        match self {
            Self::Password { password } => password.is_empty(),
            Self::PrivateKey { private_key, .. } => private_key.is_empty(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServerRequest {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credential: CredentialInput,
}

impl CreateServerRequest {
    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() || self.host.trim().is_empty() || self.username.trim().is_empty() {
            return Err(AppError::Validation("名称、地址和用户名不能为空".into()));
        }
        if self.name.len() > 128 || self.host.len() > 255 || self.username.len() > 128 {
            return Err(AppError::Validation("服务器字段超过长度限制".into()));
        }
        if self.name.contains('\0') || self.host.contains('\0') || self.username.contains('\0') {
            return Err(AppError::Validation("服务器字段包含无效字符".into()));
        }
        if self.port == 0 { return Err(AppError::Validation("端口必须在 1 到 65535 之间".into())); }
        if self.credential.is_empty() { return Err(AppError::Validation("认证凭据不能为空".into())); }
        match &self.credential {
            CredentialInput::Password { password } if password.len() > 16 * 1024 => return Err(AppError::Validation("密码超过长度限制".into())),
            CredentialInput::PrivateKey { private_key, passphrase } if private_key.len() > 2 * 1024 * 1024 || passphrase.as_ref().is_some_and(|value| value.len() > 16 * 1024) => return Err(AppError::Validation("私钥或口令超过长度限制".into())),
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: AuthKind,
    pub credential_id: String,
}

impl ServerProfile {
    pub fn new(name: &str, host: &str, port: u16, username: &str, auth_kind: AuthKind, credential_id: &str) -> Self {
        Self { id: Uuid::new_v4().to_string(), name: name.into(), host: host.into(), port, username: username.into(), auth_kind, credential_id: credential_id.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredHostKey {
    pub server_id: String,
    pub algorithm: String,
    pub fingerprint_sha256: String,
    pub raw_key_base64: String,
}
```

Implement `ServerRepository` with the following fixed API and SQL statements. Every value is bound through SQLx; no request value is interpolated into SQL:

```rust
pub fn new(pool: SqlitePool) -> Self;
pub async fn insert(&self, server: &ServerProfile) -> AppResult<()>;
pub async fn list(&self) -> AppResult<Vec<ServerProfile>>;
pub async fn get(&self, id: &str) -> AppResult<Option<ServerProfile>>;
pub async fn upsert_host_key(&self, key: &StoredHostKey) -> AppResult<()>;
pub async fn get_host_key(&self, server_id: &str) -> AppResult<Option<StoredHostKey>>;
```

Use only these query shapes in `server_repository.rs`:

```sql
INSERT INTO servers (id,name,host,port,username,auth_kind,credential_id) VALUES (?,?,?,?,?,?,?)
SELECT id,name,host,port,username,auth_kind,credential_id FROM servers ORDER BY name COLLATE NOCASE,id
SELECT id,name,host,port,username,auth_kind,credential_id FROM servers WHERE id = ?
INSERT INTO host_keys (server_id,algorithm,fingerprint_sha256,raw_key_base64) VALUES (?,?,?,?)
  ON CONFLICT(server_id) DO UPDATE SET algorithm=excluded.algorithm,fingerprint_sha256=excluded.fingerprint_sha256,raw_key_base64=excluded.raw_key_base64,trusted_at=CURRENT_TIMESTAMP
SELECT server_id,algorithm,fingerprint_sha256,raw_key_base64 FROM host_keys WHERE server_id = ?
```

Map `port` through `u16::try_from`, map `auth_kind` through `AuthKind::try_from`, and return `AppError::Validation` for corrupt rows rather than truncating values.

- [ ] **Step 5: Run domain and repository tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml domain::server
cargo test --manifest-path src-tauri/Cargo.toml repositories::server_repository
```

Expected: validation and SQLx repository tests PASS.

- [ ] **Step 6: Commit persistence**

```powershell
git add src-tauri/migrations src-tauri/src/core/database.rs src-tauri/src/domain src-tauri/src/repositories
git commit -m "feat: persist server profiles and host keys"
```

## Task 5: Add the DPAPI credential vault

**Files:**

- Create: `src-tauri/src/core/secret_protector.rs`
- Create: `src-tauri/src/core/vault.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Write failing vault and DPAPI tests**

```rust
// src-tauri/src/core/vault.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct XorProtector;
    impl SecretProtector for XorProtector {
        fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> { Ok(value.iter().map(|byte| byte ^ 0xA5).collect()) }
        fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> { Ok(value.iter().map(|byte| byte ^ 0xA5).collect()) }
    }

    #[test]
    fn writes_atomic_encrypted_blob_and_round_trips() {
        let temp = tempdir().unwrap();
        let vault = Vault::new(temp.path(), Arc::new(XorProtector));
        vault.put("cred-1", b"canary-password").unwrap();
        let stored = std::fs::read(temp.path().join("vault/cred-1.bin")).unwrap();
        assert!(!stored.windows(b"canary-password".len()).any(|w| w == b"canary-password"));
        assert_eq!(&*vault.get("cred-1").unwrap(), b"canary-password");
        assert!(!temp.path().join("vault/cred-1.tmp").exists());
    }
}
```

```rust
// src-tauri/src/core/secret_protector.rs tests
#[cfg(all(test, windows))]
#[test]
fn dpapi_user_scope_round_trips() {
    let protector = DpapiProtector;
    let encrypted = protector.protect(b"dpapi-canary").unwrap();
    assert_ne!(encrypted, b"dpapi-canary");
    assert_eq!(protector.unprotect(&encrypted).unwrap(), b"dpapi-canary");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml core::vault
cargo test --manifest-path src-tauri/Cargo.toml core::secret_protector
```

Expected: FAIL because vault types are undefined.

- [ ] **Step 3: Implement the safe protector boundary and atomic vault**

```rust
// src-tauri/src/core/secret_protector.rs
use crate::error::{AppError, AppResult};

pub trait SecretProtector: Send + Sync {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>>;
    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>>;
}

pub struct DpapiProtector;

impl SecretProtector for DpapiProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        windows_dpapi::encrypt_data(value, windows_dpapi::Scope::User, None)
            .map_err(|error| AppError::Security(error.to_string()))
    }
    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        windows_dpapi::decrypt_data(value, windows_dpapi::Scope::User, None)
            .map_err(|error| AppError::Security(error.to_string()))
    }
}
```

```rust
// src-tauri/src/core/vault.rs
use std::{fs, path::{Path, PathBuf}, sync::Arc};
use zeroize::Zeroizing;
use crate::{core::secret_protector::SecretProtector, error::{AppError, AppResult}};

#[derive(Clone)]
pub struct Vault { directory: PathBuf, protector: Arc<dyn SecretProtector> }

impl Vault {
    pub fn new(root: &Path, protector: Arc<dyn SecretProtector>) -> Self {
        Self { directory: root.join("vault"), protector }
    }
    fn path_for(&self, id: &str) -> AppResult<PathBuf> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(AppError::Validation("凭据标识格式无效".into()));
        }
        Ok(self.directory.join(format!("{id}.bin")))
    }
    pub fn put(&self, id: &str, secret: &[u8]) -> AppResult<()> {
        fs::create_dir_all(&self.directory)?;
        let final_path = self.path_for(id)?;
        if final_path.exists() { return Err(AppError::Validation("凭据标识已经存在".into())); }
        let temp_path = self.directory.join(format!("{id}.tmp"));
        let encrypted = self.protector.protect(secret)?;
        fs::write(&temp_path, encrypted)?;
        fs::rename(temp_path, final_path)?;
        Ok(())
    }
    pub fn get(&self, id: &str) -> AppResult<Zeroizing<Vec<u8>>> {
        let encrypted = fs::read(self.path_for(id)?)?;
        Ok(Zeroizing::new(self.protector.unprotect(&encrypted)?))
    }
    pub fn delete(&self, id: &str) -> AppResult<()> {
        let path = self.path_for(id)?;
        if path.exists() { fs::remove_file(path)?; }
        Ok(())
    }
}
```

- [ ] **Step 4: Run vault tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml core::vault
cargo test --manifest-path src-tauri/Cargo.toml core::secret_protector
```

Expected: vault and DPAPI round-trip tests PASS; canary text is absent from disk.

- [ ] **Step 5: Commit the vault**

```powershell
git add src-tauri/src/core
git commit -m "feat: protect credentials with DPAPI vault"
```

## Task 6: Implement SSH host inspection and trust decisions

**Files:**

- Create: `src-tauri/src/core/ssh/mod.rs`
- Create: `src-tauri/src/core/ssh/fingerprint.rs`
- Create: `src-tauri/src/core/ssh/trust.rs`
- Create: `src-tauri/src/core/ssh/transport.rs`
- Modify: `src-tauri/src/core/mod.rs`

- [ ] **Step 1: Write failing fingerprint and trust tests**

```rust
// src-tauri/src/core/ssh/fingerprint.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_openssh_style_sha256_fingerprint() {
        assert_eq!(sha256_fingerprint(b"host-key"), "SHA256:CfEOS9w3pHE4KlqjcQFwWyWMmyRvvPoehydyMhTxpzg");
    }
}
```

```rust
// src-tauri/src/core/ssh/trust.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn new_host_requires_approval() { assert_eq!(decide(None, "SHA256:new"), TrustDecision::NeedsApproval); }
    #[test] fn matching_host_is_trusted() { assert_eq!(decide(Some("SHA256:x"), "SHA256:x"), TrustDecision::Trusted); }
    #[test] fn changed_host_is_blocked() { assert_eq!(decide(Some("SHA256:old"), "SHA256:new"), TrustDecision::Changed); }
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml core::ssh`

Expected: FAIL because SSH modules are undefined.

- [ ] **Step 3: Implement fingerprint and trust logic**

```rust
// src-tauri/src/core/ssh/fingerprint.rs
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use sha2::{Digest, Sha256};

pub fn sha256_fingerprint(raw_key: &[u8]) -> String {
    format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(raw_key)))
}
```

```rust
// src-tauri/src/core/ssh/trust.rs
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustDecision { Trusted, NeedsApproval, Changed }

pub fn decide(stored: Option<&str>, observed: &str) -> TrustDecision {
    match stored {
        None => TrustDecision::NeedsApproval,
        Some(value) if value == observed => TrustDecision::Trusted,
        Some(_) => TrustDecision::Changed,
    }
}
```

- [ ] **Step 4: Implement host-key inspection without authentication**

```rust
// src-tauri/src/core/ssh/transport.rs
use std::{net::{TcpStream, ToSocketAddrs}, time::Duration};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use ssh2::Session;
use crate::{core::ssh::fingerprint::sha256_fingerprint, error::{AppError, AppResult}};

#[derive(Debug, Clone)]
pub struct SshEndpoint { pub host: String, pub port: u16, pub timeout: Duration }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyObservation { pub algorithm: String, pub fingerprint_sha256: String, pub raw_key_base64: String }

fn open_session(endpoint: &SshEndpoint) -> AppResult<Session> {
    let address = (endpoint.host.as_str(), endpoint.port).to_socket_addrs()?
        .next().ok_or_else(|| AppError::Validation("服务器地址无法解析".into()))?;
    let tcp = TcpStream::connect_timeout(&address, endpoint.timeout)?;
    tcp.set_read_timeout(Some(endpoint.timeout))?;
    tcp.set_write_timeout(Some(endpoint.timeout))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.set_timeout(endpoint.timeout.as_millis().min(u32::MAX as u128) as u32);
    session.handshake()?;
    Ok(session)
}

pub fn inspect_host_key(endpoint: &SshEndpoint) -> AppResult<HostKeyObservation> {
    let session = open_session(endpoint)?;
    let (raw, algorithm) = session.host_key().ok_or_else(|| AppError::Security("服务器没有提供主机密钥".into()))?;
    Ok(HostKeyObservation {
        algorithm: format!("{algorithm:?}"),
        fingerprint_sha256: sha256_fingerprint(raw),
        raw_key_base64: STANDARD.encode(raw),
    })
}
```

- [ ] **Step 5: Run SSH unit tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml core::ssh`

Expected: fingerprint and trust tests PASS.

- [ ] **Step 6: Commit host identity behavior**

```powershell
git add src-tauri/src/core/ssh src-tauri/src/core/mod.rs
git commit -m "feat: inspect and verify SSH host identity"
```

## Task 7: Authenticate and parse Linux capabilities

**Files:**

- Modify: `src-tauri/src/domain/server.rs`
- Modify: `src-tauri/src/core/ssh/transport.rs`
- Create: `src-tauri/src/core/system_probe.rs`
- Create: `src-tauri/tests/fixtures/ubuntu_probe.txt`
- Create: `src-tauri/tests/fixtures/kylin_probe.txt`
- Create: `src-tauri/tests/fixtures/rocky_probe.txt`
- Create: `src-tauri/tests/fixtures/openeuler_probe.txt`
- Create: `src-tauri/tests/fixtures/anolis_probe.txt`
- Create: `src-tauri/tests/fixtures/uos_probe.txt`

- [ ] **Step 1: Write failing probe-parser tests**

```rust
// src-tauri/src/core/system_probe.rs tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ubuntu_capabilities() {
        let result = parse_probe(include_str!("../../tests/fixtures/ubuntu_probe.txt")).unwrap();
        assert_eq!(result.os_id, "ubuntu");
        assert_eq!(result.os_family, "debian");
        assert_eq!(result.package_manager.as_deref(), Some("apt"));
        assert_eq!(result.service_manager, "systemd");
        assert_eq!(result.architecture, "x86_64");
    }

    #[test]
    fn maps_kylin_by_id_like_and_detected_tools() {
        let result = parse_probe(include_str!("../../tests/fixtures/kylin_probe.txt")).unwrap();
        assert_eq!(result.os_id, "kylin");
        assert_eq!(result.os_family, "debian");
        assert_eq!(result.package_manager.as_deref(), Some("apt"));
    }

    #[test]
    fn maps_rhel_openeuler_and_domestic_variants() {
        let cases = [
            (include_str!("../../tests/fixtures/rocky_probe.txt"), "rocky", "rhel", "dnf"),
            (include_str!("../../tests/fixtures/openeuler_probe.txt"), "openeuler", "openeuler", "dnf"),
            (include_str!("../../tests/fixtures/anolis_probe.txt"), "anolis", "rhel", "dnf"),
            (include_str!("../../tests/fixtures/uos_probe.txt"), "uos", "debian", "apt"),
        ];
        for (fixture, id, family, package_manager) in cases {
            let result = parse_probe(fixture).unwrap();
            assert_eq!(result.os_id, id);
            assert_eq!(result.os_family, family);
            assert_eq!(result.package_manager.as_deref(), Some(package_manager));
        }
    }
}
```

Fixtures use this exact sentinel format:

```text
__QZ_OS_BEGIN__
ID=ubuntu
ID_LIKE=debian
VERSION_ID="24.04"
__QZ_OS_END__
PKG=apt
SERVICE=systemd
ARCH=x86_64
SHELL=/bin/bash
```

Create the other fixtures with the same sentinels and these exact values (all use `SERVICE=systemd`, `ARCH=x86_64`, and `SHELL=/bin/bash`):

| Fixture | `ID` | `ID_LIKE` | `VERSION_ID` | `PKG` | Expected family |
|---|---|---|---|---|---|
| `kylin_probe.txt` | `kylin` | `debian ubuntu` | `V10` | `apt` | `debian` |
| `rocky_probe.txt` | `rocky` | `rhel centos fedora` | `9.6` | `dnf` | `rhel` |
| `openeuler_probe.txt` | `openEuler` | `rhel fedora` | `24.03` | `dnf` | `openeuler` |
| `anolis_probe.txt` | `anolis` | `rhel centos fedora` | `8.10` | `dnf` | `rhel` |
| `uos_probe.txt` | `uos` | `debian` | `20` | `apt` | `debian` |

- [ ] **Step 2: Run parser tests and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml system_probe::tests`

Expected: FAIL because parser and capability types are undefined.

- [ ] **Step 3: Implement the bounded probe and parser**

```rust
// src-tauri/src/core/system_probe.rs
use std::collections::HashMap;
use serde::Serialize;
use crate::error::{AppError, AppResult};

pub const PROBE_COMMAND: &str = r#"printf '__QZ_OS_BEGIN__\n'; cat /etc/os-release; printf '__QZ_OS_END__\n'; if command -v apt >/dev/null 2>&1; then echo PKG=apt; elif command -v dnf >/dev/null 2>&1; then echo PKG=dnf; elif command -v yum >/dev/null 2>&1; then echo PKG=yum; else echo PKG=; fi; if command -v systemctl >/dev/null 2>&1; then echo SERVICE=systemd; else echo SERVICE=service; fi; printf 'ARCH='; uname -m; printf 'SHELL='; printf '%s\n' "${SHELL:-unknown}""#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCapabilities {
    pub os_id: String,
    pub os_family: String,
    pub version_id: Option<String>,
    pub package_manager: Option<String>,
    pub service_manager: String,
    pub architecture: String,
    pub shell: String,
}

fn unquote(value: &str) -> String { value.trim().trim_matches('"').to_string() }

pub fn parse_probe(output: &str) -> AppResult<SystemCapabilities> {
    let start = output.find("__QZ_OS_BEGIN__").ok_or_else(|| AppError::Validation("探测输出缺少开始标记".into()))?;
    let end = output.find("__QZ_OS_END__").ok_or_else(|| AppError::Validation("探测输出缺少结束标记".into()))?;
    let os = &output[start + "__QZ_OS_BEGIN__".len()..end];
    let mut values = HashMap::new();
    for line in os.lines().chain(output[end + "__QZ_OS_END__".len()..].lines()) {
        if let Some((key, value)) = line.split_once('=') { values.insert(key.trim().to_string(), unquote(value)); }
    }
    let os_id = values.remove("ID").ok_or_else(|| AppError::Validation("无法识别系统 ID".into()))?.to_ascii_lowercase();
    let id_like = values.remove("ID_LIKE").unwrap_or_default();
    let os_family = if os_id == "openeuler" { "openeuler" }
        else if ["debian", "ubuntu", "kylin", "uos"].contains(&os_id.as_str()) || id_like.contains("debian") { "debian" }
        else if ["rhel", "centos", "rocky", "almalinux", "anolis"].contains(&os_id.as_str()) || id_like.contains("rhel") || id_like.contains("fedora") { "rhel" }
        else { "unknown" }.to_string();
    Ok(SystemCapabilities {
        os_id,
        os_family,
        version_id: values.remove("VERSION_ID"),
        package_manager: values.remove("PKG").filter(|v| !v.is_empty()),
        service_manager: values.remove("SERVICE").unwrap_or_else(|| "unknown".into()),
        architecture: values.remove("ARCH").unwrap_or_else(|| "unknown".into()),
        shell: values.remove("SHELL").unwrap_or_else(|| "unknown".into()),
    })
}
```

- [ ] **Step 4: Add credential material and authenticated command execution**

In `domain/server.rs`, add this credential-at-rest payload. It is serialized only immediately before vault encryption and zeroized on drop; it intentionally does not derive `Debug`:

```rust
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoredCredential {
    Password { password: String },
    PrivateKey { private_key: String, passphrase: Option<String> },
}

impl From<CredentialInput> for StoredCredential {
    fn from(value: CredentialInput) -> Self {
        match value {
            CredentialInput::Password { password } => Self::Password { password },
            CredentialInput::PrivateKey { private_key, passphrase } => Self::PrivateKey { private_key, passphrase },
        }
    }
}
```

In `transport.rs`, add `execute(endpoint, username, credential, expected_fingerprint, command)`. It must:

1. open a new session;
2. compare the observed fingerprint before authentication;
3. return `AppError::Security` on mismatch;
4. call `userauth_password` or `userauth_pubkey_memory`;
5. execute without allocating a PTY;
6. read stdout and stderr with a 1 MiB combined limit;
7. wait for close and return stdout, stderr, and exit status.

Use `PROBE_COMMAND` and `parse_probe` to implement `probe_system`.

Expose these exact transport contracts so the service and live fixture use one path for both authentication modes:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput { pub stdout: String, pub stderr: String, pub exit_status: i32 }

pub fn execute(
    endpoint: &SshEndpoint,
    username: &str,
    credential: &StoredCredential,
    expected_fingerprint: &str,
    command: &str,
) -> AppResult<CommandOutput>;

pub fn probe_system(
    endpoint: &SshEndpoint,
    username: &str,
    credential: &StoredCredential,
    expected_fingerprint: &str,
) -> AppResult<SystemCapabilities> {
    let output = execute(endpoint, username, credential, expected_fingerprint, PROBE_COMMAND)?;
    if output.exit_status != 0 {
        return Err(AppError::SshCommand { exit_status: output.exit_status, stderr: output.stderr });
    }
    parse_probe(&output.stdout)
}
```

Add `SshCommand { exit_status: i32, stderr: String }` to `AppError`, serialize it with code `ssh_command`, and cap `stderr` in the error to 8 KiB. Never include command text or credential material in the error.

- [ ] **Step 5: Run parser and transport unit tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml system_probe::tests
cargo test --manifest-path src-tauri/Cargo.toml core::ssh
```

Expected: all focused tests PASS.

- [ ] **Step 6: Commit authentication and probing**

```powershell
git add src-tauri/src src-tauri/tests/fixtures
git commit -m "feat: authenticate and detect Linux capabilities"
```

## Task 8: Compose services and expose typed Tauri commands

**Files:**

- Create: `src-tauri/src/services/mod.rs`
- Create: `src-tauri/src/services/app_services.rs`
- Create: `src-tauri/src/state.rs`
- Create: `src-tauri/src/commands/mod.rs`
- Create: `src-tauri/src/commands/bootstrap.rs`
- Create: `src-tauri/src/commands/servers.rs`
- Create: `src-tauri/tests/foundation_integration.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Write the failing service integration test**

```rust
// src-tauri/tests/foundation_integration.rs
use std::sync::Arc;
use tempfile::TempDir;
use qingzhou_ssh_lib::{
    core::secret_protector::SecretProtector,
    domain::server::{CreateServerRequest, CredentialInput},
    error::AppResult,
    services::app_services::AppServices,
};

struct XorProtector;
impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> { Ok(value.iter().map(|byte| byte ^ 0xA5).collect()) }
    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> { Ok(value.iter().map(|byte| byte ^ 0xA5).collect()) }
}

struct TestHarness { root: TempDir, services: AppServices }
impl TestHarness {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector)).await.unwrap();
        Self { root, services }
    }
}

#[tokio::test]
async fn creates_server_with_encrypted_credential_and_pending_host_trust() {
    let harness = TestHarness::new().await;
    let created = harness.services.create_server(CreateServerRequest {
        name: "网站服务器".into(), host: "127.0.0.1".into(), port: 2222,
        username: "testuser".into(),
        credential: CredentialInput::Password { password: "canary-password".into() },
    }).await.unwrap();

    assert_eq!(harness.services.list_servers().await.unwrap().len(), 1);
    let vault_blob = std::fs::read(harness.root.path().join(format!("vault/{}.bin", created.credential_id))).unwrap();
    assert!(!String::from_utf8_lossy(&vault_blob).contains("canary-password"));
    assert!(harness.services.get_trusted_host_key(&created.id).await.unwrap().is_none());
}
```

The harness never touches HKCU or real DPAPI. The XOR protector is only a deterministic test double that proves service wiring never writes the canary as plaintext; DPAPI itself remains covered by the Windows-only round-trip test in Task 5.

- [ ] **Step 2: Run the integration test and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test foundation_integration`

Expected: FAIL because `AppServices` and harness do not exist.

- [ ] **Step 3: Implement `AppServices` with transaction-like cleanup**

`AppServices::open(root)` initializes directories, database, repository, and a `DpapiProtector` vault. `open_with_protector(root, protector)` performs the same initialization for integration tests. `create_server` must validate first, generate independent server and credential UUIDs, serialize and encrypt the credential, then insert the server; if database insertion fails, delete the just-created vault entry. Add methods:

```rust
pub async fn open(root: &Path) -> AppResult<Self>;
pub async fn open_with_protector(root: &Path, protector: Arc<dyn SecretProtector>) -> AppResult<Self>;
pub async fn create_server(&self, request: CreateServerRequest) -> AppResult<ServerProfile>;
pub async fn list_servers(&self) -> AppResult<Vec<ServerProfile>>;
pub async fn get_trusted_host_key(&self, server_id: &str) -> AppResult<Option<StoredHostKey>>;
pub async fn inspect_host_key(&self, server_id: &str) -> AppResult<HostKeyCheck>;
pub async fn trust_host_key(&self, server_id: &str, observation: HostKeyObservation) -> AppResult<()>;
pub async fn test_connection(&self, server_id: &str) -> AppResult<SystemCapabilities>;
```

`trust_host_key` must perform a fresh host-key inspection and compare algorithm, fingerprint, and raw key with the observation supplied by the UI before persisting it; this closes the inspect/approve race. `test_connection` must also open a fresh session and reject missing trust or a changed fingerprint before authentication.

`HostKeyCheck` is produced in Rust by combining the observation with the persisted key and `trust::decide`; the frontend never computes trust:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyCheck {
    pub decision: TrustDecision,
    pub observed: HostKeyObservation,
    pub trusted: Option<StoredHostKey>,
}
```

For the integration test only, expose focused Rust modules from `lib.rs`:

```rust
pub mod core;
pub mod domain;
pub mod error;
pub mod repositories;
pub mod services;
mod commands;
mod state;
```

- [ ] **Step 4: Implement optional managed state and commands**

```rust
// src-tauri/src/state.rs
use tokio::sync::RwLock;
use crate::services::app_services::AppServices;

pub struct AppState { pub services: RwLock<Option<AppServices>> }
impl Default for AppState { fn default() -> Self { Self { services: RwLock::new(None) } } }
```

Commands return serializable contracts and never return paths outside the selected root, vault bytes, database handles, or SSH sessions:

```rust
bootstrap_status() -> AppResult<BootstrapStatus>
initialize_data_root(path: String) -> AppResult<BootstrapStatus>
list_servers() -> AppResult<Vec<ServerProfile>>
create_server(request: CreateServerRequest) -> AppResult<ServerProfile>
inspect_server_host_key(server_id: String) -> AppResult<HostKeyCheck>
trust_server_host_key(server_id: String, observation: HostKeyObservation) -> AppResult<()>
test_server_connection(server_id: String) -> AppResult<SystemCapabilities>
```

Register all commands in one `tauri::generate_handler![]` call and manage one `AppState`.

- [ ] **Step 5: Run integration and Rust tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test foundation_integration
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all tests PASS.

- [ ] **Step 6: Commit services and IPC**

```powershell
git add src-tauri/src src-tauri/tests/foundation_integration.rs
git commit -m "feat: expose secure server connection services"
```

## Task 9: Build the first-run and add-server UI

**Files:**

- Create: `src/api/contracts.ts`
- Create: `src/api/tauri.ts`
- Create: `src/api/tauri.test.ts`
- Create: `src/features/bootstrap/DataRootGate.tsx`
- Create: `src/features/bootstrap/DataRootGate.test.tsx`
- Create: `src/features/servers/ServerListPage.tsx`
- Create: `src/features/servers/AddServerDialog.tsx`
- Create: `src/features/servers/AddServerDialog.test.tsx`
- Create: `src/features/servers/HostKeyDialog.tsx`
- Create: `src/features/servers/HostKeyDialog.test.tsx`
- Modify: `src/app/App.tsx`
- Modify: `src/styles/theme.css`

- [ ] **Step 1: Define matching frontend contracts and one invoke wrapper**

```ts
// src/api/contracts.ts
export type BootstrapStatus =
  | { state: 'needs_selection' }
  | { state: 'ready'; dataRoot: string };

export type CredentialInput =
  | { kind: 'password'; password: string }
  | { kind: 'private_key'; privateKey: string; passphrase: string | null };

export interface CreateServerRequest {
  name: string; host: string; port: number; username: string; credential: CredentialInput;
}

export interface ServerProfile {
  id: string; name: string; host: string; port: number; username: string;
  authKind: 'password' | 'private_key'; credentialId: string;
}

export interface HostKeyObservation {
  algorithm: string; fingerprintSha256: string; rawKeyBase64: string;
}

export interface HostKeyCheck {
  decision: 'trusted' | 'needs_approval' | 'changed';
  observed: HostKeyObservation;
  trusted: (HostKeyObservation & { serverId: string }) | null;
}

export interface SystemCapabilities {
  osId: string; osFamily: string; versionId: string | null; packageManager: string | null;
  serviceManager: string; architecture: string; shell: string;
}
```

```ts
// src/api/tauri.ts
import { invoke } from '@tauri-apps/api/core';
import type { BootstrapStatus, CreateServerRequest, HostKeyCheck, HostKeyObservation, ServerProfile, SystemCapabilities } from './contracts';

export const api = {
  bootstrapStatus: () => invoke<BootstrapStatus>('bootstrap_status'),
  initializeDataRoot: (path: string) => invoke<BootstrapStatus>('initialize_data_root', { path }),
  listServers: () => invoke<ServerProfile[]>('list_servers'),
  createServer: (request: CreateServerRequest) => invoke<ServerProfile>('create_server', { request }),
  inspectHostKey: (serverId: string) => invoke<HostKeyCheck>('inspect_server_host_key', { serverId }),
  trustHostKey: (serverId: string, observation: HostKeyObservation) => invoke<void>('trust_server_host_key', { serverId, observation }),
  testConnection: (serverId: string) => invoke<SystemCapabilities>('test_server_connection', { serverId }),
};
```

Add a unit test mocking `@tauri-apps/api/core` and assert each wrapper sends the exact command name and camelCase argument key.

- [ ] **Step 2: Write failing first-run UI test**

```tsx
// src/features/bootstrap/DataRootGate.test.tsx
it('requires an explicit data directory and never offers an AppData default', async () => {
  render(<DataRootGate status={{ state: 'needs_selection' }} onReady={vi.fn()} />);
  expect(screen.getByRole('heading', { name: '选择数据存储位置' })).toBeVisible();
  expect(screen.getByRole('button', { name: '选择文件夹' })).toBeVisible();
  expect(screen.queryByText(/AppData/i)).not.toBeInTheDocument();
});
```

Run: `pnpm test src/features/bootstrap/DataRootGate.test.tsx`

Expected: FAIL because the component does not exist.

- [ ] **Step 3: Implement the data-root gate**

```tsx
// src/features/bootstrap/DataRootGate.tsx
import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api } from '../../api/tauri';
import type { BootstrapStatus } from '../../api/contracts';

type ReadyStatus = Extract<BootstrapStatus, { state: 'ready' }>;

export function DataRootGate({ status, onReady }: { status: BootstrapStatus; onReady: (status: ReadyStatus) => void }) {
  const [selectedPath, setSelectedPath] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function chooseDirectory() {
    setError('');
    const path = await open({ directory: true, multiple: false, title: '选择轻舟 SSH 数据目录' });
    if (!path) return;
    setSelectedPath(path);
    setBusy(true);
    try {
      const ready = await api.initializeDataRoot(path);
      if (ready.state !== 'ready') throw new Error('数据目录初始化未完成');
      onReady(ready);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }

  if (status.state === 'ready') return null;
  return (
    <main className="app-shell">
      <section className="silver-card data-root-card">
        <h1>选择数据存储位置</h1>
        <p>数据库、凭据密文、日志、下载和更新文件都将保存在这里。</p>
        {selectedPath && <output>{selectedPath}</output>}
        {error && <p role="alert">{error}</p>}
        <button type="button" disabled={busy} onClick={chooseDirectory}>{busy ? '正在初始化…' : '选择文件夹'}</button>
      </section>
    </main>
  );
}
```

Extend the test by mocking both `open` and `api.initializeDataRoot`: when initialization rejects, assert the selected path remains visible and the error has `role="alert"`; when it succeeds, assert `onReady` receives the returned ready status.

- [ ] **Step 4: Write failing add-server and host-key tests**

Test these exact behaviors:

- port defaults to 22 and rejects 0/greater than 65535;
- password and private-key inputs are mutually exclusive;
- submitting creates a server, then fetches the host key before authentication;
- a new host shows algorithm and SHA-256 fingerprint with an explicit “信任并继续” button;
- a changed host key shows old/new fingerprints, disables continuation, and never displays a trust button.

- [ ] **Step 5: Implement focused server components**

`AddServerDialog` owns only form state and returns a `CreateServerRequest`. It must never log props or state, and it clears password, private-key, and passphrase state immediately after a successful submit or cancel.

`HostKeyDialog` receives `HostKeyCheck` directly. Render behavior is fixed:

| Decision | Content | Allowed actions |
|---|---|---|
| `needs_approval` | algorithm + observed SHA-256 fingerprint | `信任并继续`, `取消` |
| `trusted` | observed fingerprint + “身份已验证” | `继续`, `取消` |
| `changed` | old and new fingerprints + red warning | `关闭` only; no trust or continue action |

`ServerListPage` orchestrates this exact sequence: `createServer` → `inspectHostKey` → show `HostKeyDialog`; only `needs_approval` approval calls `trustHostKey`; only `trusted` or successfully persisted approval calls `testConnection`. It then renders OS family, package manager, service manager, and architecture. Any `changed` result terminates the sequence before authentication.

Apply the approved visual tokens: white-silver gradient cards, clipped internal highlight, three external shadow layers, blue-purple primary button, green success, orange warning, red mismatch, and high-contrast text.

- [ ] **Step 6: Compose the small app state machine**

`App.tsx` has only three states: loading bootstrap, selecting data root, ready server page. It does not contain SSH, vault, validation, or database logic.

- [ ] **Step 7: Run frontend tests and build**

Run:

```powershell
pnpm test
pnpm build
```

Expected: all frontend tests PASS and TypeScript build exits 0.

- [ ] **Step 8: Commit the first-run UI**

```powershell
git add src
git commit -m "feat: add data root and server connection UI"
```

## Task 10: Add a deterministic live SSH fixture

**Files:**

- Create: `tests/fixtures/sshd/Dockerfile`
- Create: `tests/fixtures/sshd/compose.yml`
- Create: `src-tauri/tests/ssh_live.rs`

- [ ] **Step 1: Write the ignored live integration test**

```rust
// src-tauri/tests/ssh_live.rs
use qingzhou_ssh_lib::{
    core::{ssh::transport::{inspect_host_key, SshEndpoint}, system_probe::probe_system},
    domain::server::StoredCredential,
    error::AppError,
};

fn endpoint() -> SshEndpoint {
    SshEndpoint { host: "127.0.0.1".into(), port: 2222, timeout: std::time::Duration::from_secs(10) }
}

#[test]
#[ignore = "requires tests/fixtures/sshd container"]
fn password_auth_and_probe_work_against_fixture() {
    let endpoint = endpoint();
    let observed = inspect_host_key(&endpoint).unwrap();
    let credential = StoredCredential::Password { password: "testpass".into() };
    let capabilities = probe_system(&endpoint, "testuser", &credential, &observed.fingerprint_sha256).unwrap();
    assert_eq!(capabilities.os_id, "ubuntu");
    assert_eq!(capabilities.os_family, "debian");
    assert_eq!(capabilities.service_manager, "systemd");
}

#[test]
#[ignore = "requires tests/fixtures/sshd container and generated test key"]
fn private_key_auth_and_probe_work_against_fixture() {
    let endpoint = endpoint();
    let observed = inspect_host_key(&endpoint).unwrap();
    let credential = StoredCredential::PrivateKey {
        private_key: std::fs::read_to_string(".local/test-keys/id_ed25519").unwrap(),
        passphrase: Some("fixture-passphrase".into()),
    };
    let capabilities = probe_system(&endpoint, "testuser", &credential, &observed.fingerprint_sha256).unwrap();
    assert_eq!(capabilities.os_family, "debian");
}

#[test]
#[ignore = "requires tests/fixtures/sshd container"]
fn wrong_fingerprint_blocks_before_authentication() {
    let endpoint = endpoint();
    let credential = StoredCredential::Password { password: "deliberately-wrong".into() };
    let error = probe_system(&endpoint, "testuser", &credential, "SHA256:not-the-server").unwrap_err();
    assert!(matches!(error, AppError::Security(_)));
}
```

- [ ] **Step 2: Run the ignored test without a fixture and confirm it fails when forced**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test ssh_live -- --ignored`

Expected: FAIL with connection refused on `127.0.0.1:2222`.

- [ ] **Step 3: Add the Ubuntu OpenSSH fixture**

```dockerfile
# tests/fixtures/sshd/Dockerfile
FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y openssh-server sudo \
 && mkdir -p /run/sshd \
 && useradd -m -s /bin/bash testuser \
 && install -d -m 700 -o testuser -g testuser /home/testuser/.ssh \
 && echo 'testuser:testpass' | chpasswd \
 && printf '\nPasswordAuthentication yes\nPubkeyAuthentication yes\nStrictModes no\nPermitRootLogin no\n' >> /etc/ssh/sshd_config \
 && rm -rf /var/lib/apt/lists/*
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
```

```yaml
# tests/fixtures/sshd/compose.yml
services:
  sshd:
    build: .
    ports:
      - "127.0.0.1:2222:22"
    volumes:
      - ../../../.local/test-keys/authorized_keys:/home/testuser/.ssh/authorized_keys:ro
```

- [ ] **Step 4: Run the live test**

```powershell
New-Item -ItemType Directory -Force .\.local\test-keys | Out-Null
if (-not (Test-Path .\.local\test-keys\id_ed25519)) {
  ssh-keygen -q -t ed25519 -N 'fixture-passphrase' -C 'qingzhou-fixture-only' -f .\.local\test-keys\id_ed25519
}
Copy-Item .\.local\test-keys\id_ed25519.pub .\.local\test-keys\authorized_keys -Force
try {
  docker compose -f .\tests\fixtures\sshd\compose.yml up -d --build
  if ($LASTEXITCODE -ne 0) { throw 'SSH fixture startup failed' }
  cargo test --manifest-path .\src-tauri\Cargo.toml --test ssh_live -- --ignored
  if ($LASTEXITCODE -ne 0) { throw 'Live SSH tests failed' }
} finally {
  docker compose -f .\tests\fixtures\sshd\compose.yml down
}
```

Expected: live test PASS; container is removed.

- [ ] **Step 5: Verify the mismatch layers**

The Task 6 unit test must still assert stored `SHA256:old` plus observed `SHA256:new` returns `TrustDecision::Changed`. The live `wrong_fingerprint_blocks_before_authentication` test above uses an intentionally wrong password as a canary: it must return `AppError::Security`, not an authentication failure, proving fingerprint rejection occurs first.

- [ ] **Step 6: Commit the fixture**

```powershell
git add tests src-tauri/tests/ssh_live.rs
git commit -m "test: add live SSH foundation fixture"
```

## Task 11: Add D-drive audit and milestone documentation

**Files:**

- Create: `scripts/verify-d-drive.ps1`
- Create: `docs/development.md`
- Create: `docs/security.md`
- Create: `README.md`

- [ ] **Step 1: Write the failing path-audit script**

```powershell
# scripts/verify-d-drive.ps1
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$rootPrefix = $repoRoot.TrimEnd('\') + '\'
$forbidden = @(
  (Join-Path $env:APPDATA 'QingzhouSSH'),
  (Join-Path $env:LOCALAPPDATA 'QingzhouSSH')
)
$before = @{}
foreach ($path in $forbidden) { $before[$path] = Test-Path -LiteralPath $path }

. (Join-Path $PSScriptRoot 'dev-env.ps1') -Quiet

function Assert-UnderRepo([string]$name, [string]$path) {
  if ([string]::IsNullOrWhiteSpace($path)) { throw "$name is empty" }
  $full = [IO.Path]::GetFullPath($path)
  if ($full -ne $repoRoot -and -not $full.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$name escaped repository: $full"
  }
}

$metadata = cargo metadata --manifest-path (Join-Path $repoRoot 'src-tauri\Cargo.toml') --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
$pnpmStore = (pnpm store path).Trim()
if ($LASTEXITCODE -ne 0) { throw 'pnpm store path failed' }

Assert-UnderRepo 'Cargo target' $metadata.target_directory
Assert-UnderRepo 'Cargo home' $env:CARGO_HOME
Assert-UnderRepo 'Corepack home' $env:COREPACK_HOME
Assert-UnderRepo 'pnpm home' $env:PNPM_HOME
Assert-UnderRepo 'pnpm store' $pnpmStore
Assert-UnderRepo 'temporary directory' $env:TEMP
Assert-UnderRepo 'development data root' $env:QINGZHOU_DATA_ROOT
Assert-UnderRepo 'artifacts directory' $env:QINGZHOU_ARTIFACTS_DIR

$tauriConfig = Get-Content -Raw (Join-Path $repoRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
if (@($tauriConfig.app.windows).Count -ne 0) { throw 'Static Tauri windows can create WebView2 data before root resolution' }
$windowSource = Get-Content -Raw (Join-Path $repoRoot 'src-tauri\src\window.rs')
if ($windowSource -notmatch '\.data_directory\(' -or $windowSource -notmatch '\.incognito\(true\)') {
  throw 'Window builder must route persistent WebView2 data to the selected root and use incognito first-run mode'
}

foreach ($path in $forbidden) {
  if (-not $before[$path] -and (Test-Path -LiteralPath $path)) {
    throw "Development audit created forbidden AppData path: $path"
  }
  if ($before[$path]) { Write-Warning "Pre-existing path was not created by this audit: $path" }
}

Write-Host 'PASS: controllable development and application paths remain inside the repository'
```

- [ ] **Step 2: Run the audit before adding any fixes**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-d-drive.ps1`

Expected: either PASS immediately or a precise failure naming the escaping path. If it fails, fix only the responsible config and rerun.

- [ ] **Step 3: Document the development and security contracts**

`docs/development.md` includes exact environment, test, Docker fixture, frontend build, Rust build, and Tauri debug commands. `docs/security.md` documents DPAPI user scope, host-key trust, no-secret logging, registry path pointer, and the portable-mode re-entry limitation. `README.md` identifies the milestone as foundation-only and does not claim quick tasks or workflows exist.

- [ ] **Step 4: Run the complete milestone verification**

```powershell
. .\scripts\dev-env.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-d-drive.ps1
pnpm test
pnpm build
cargo fmt --manifest-path .\src-tauri\Cargo.toml -- --check
cargo clippy --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path .\src-tauri\Cargo.toml
New-Item -ItemType Directory -Force .\.local\test-keys | Out-Null
if (-not (Test-Path .\.local\test-keys\id_ed25519)) {
  ssh-keygen -q -t ed25519 -N 'fixture-passphrase' -C 'qingzhou-fixture-only' -f .\.local\test-keys\id_ed25519
}
Copy-Item .\.local\test-keys\id_ed25519.pub .\.local\test-keys\authorized_keys -Force
try {
  docker compose -f .\tests\fixtures\sshd\compose.yml up -d --build
  if ($LASTEXITCODE -ne 0) { throw 'SSH fixture startup failed' }
  cargo test --manifest-path .\src-tauri\Cargo.toml --test ssh_live -- --ignored
  if ($LASTEXITCODE -ne 0) { throw 'Live SSH tests failed' }
} finally {
  docker compose -f .\tests\fixtures\sshd\compose.yml down
}
pnpm tauri build --debug --no-bundle
git status --short
```

Expected:

- environment and D-drive audits PASS;
- all frontend and Rust tests PASS;
- formatting and Clippy report no issues;
- live SSH tests PASS;
- Tauri debug build exits 0;
- `git status --short` is empty after the documentation commit.

- [ ] **Step 5: Commit documentation and audit**

```powershell
git add scripts/verify-d-drive.ps1 docs README.md
git commit -m "docs: document foundation development and security"
```

## Milestone 1 acceptance checklist

- [ ] Data-root precedence is environment → portable → registry → explicit selection.
- [ ] No code falls back to AppData for application data.
- [ ] First-run WebView is incognito; subsequent WebView2 cache resolves to `<data-root>\cache\webview2`.
- [ ] SQLite stores no password, private-key, or passphrase columns.
- [ ] Vault files do not contain known plaintext canaries.
- [ ] DPAPI blobs decrypt only for the current Windows user/machine context.
- [ ] First-use host keys require explicit approval.
- [ ] Changed host keys block authentication.
- [ ] Password and private-key authentication both pass against controlled fixtures.
- [ ] Ubuntu/Debian, RHEL-like, openEuler, and domestic-system fixture outputs map to the intended system family.
- [ ] UI displays detected capabilities without exposing raw SSH sessions or credential material.
- [ ] All project-local path, frontend, Rust, Docker SSH, lint, and build checks pass.
- [ ] The user reviews the runnable foundation before Milestone 2 planning begins.
