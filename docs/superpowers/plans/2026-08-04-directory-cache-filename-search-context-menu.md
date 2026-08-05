# Directory Cache, Filename Search, and Context Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Make repeated directory navigation immediate during one client session, add bounded fuzzy remote filename search, and provide safe context menus for files, folders, and log results.

**Architecture:** A shared in-memory directory cache wraps the current local and remote listing APIs and is reused by both SFTP views. The log pipeline gains a typed content/filename target and stores tagged content-line or remote-file results. A portal context menu receives page-owned actions, so existing upload, download, and log-search safety checks remain authoritative.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, Tauri 2, Rust, Tokio, existing SSH/SFTP execution and result-store infrastructure.

**Constraint:** Keep version 0.1.5. Do not commit, tag, push, or publish. Keep all generated data under D:\Codex Project\轻量化SSH快捷工具.

---

## File responsibilities

- Create src/features/file-browser/directorySessionCache.ts for the bounded shared cache and per-server last paths.
- Create src/features/file-browser/directorySessionCache.test.ts for cache behavior.
- Modify FileTransferPage.tsx and RemoteLogBrowserDialog.tsx to consume the cache.
- Modify Rust log request, command, parser, store, service, and workflow constructors for filename search.
- Modify frontend API contracts, preview API, LogSearchPage, and LogResultsTable for target-specific forms and results.
- Create src/components/ContextMenu.tsx and src/lib/clipboard.ts for safe desktop interactions.
- Modify AppShell.tsx to carry a one-shot “search this remote file” intent between pages.
- Modify theme.css and focused tests for loading, result, and menu presentation.

### Task 1: Shared session directory cache

**Files:**
- Create: src/features/file-browser/directorySessionCache.ts
- Create: src/features/file-browser/directorySessionCache.test.ts

- [ ] **Step 1: Write failing tests**

The tests instantiate DirectorySessionCache(128) and prove:
- two simultaneous loadRemote calls for the same server/path share one Promise;
- a later cache hit does not call the loader;
- refreshRemote forces one call;
- invalidating one server/path does not affect another server;
- lastRemotePath is stored per server;
- a cache with capacity two evicts the least recently used entry.

Core test shape:

    const cache = new DirectorySessionCache(128);
    const loader = vi.fn(async () => listing('/home'));
    await Promise.all([
      cache.loadRemote('server-1', '/home', loader),
      cache.loadRemote('server-1', '/home', loader),
    ]);
    await cache.loadRemote('server-1', '/home', loader);
    expect(loader).toHaveBeenCalledTimes(1);

- [ ] **Step 2: Verify RED**

Run:

    npm test -- --run src/features/file-browser/directorySessionCache.test.ts

Expected: FAIL because the module and class do not exist.

- [ ] **Step 3: Implement the minimal API**

DirectorySessionCache exposes:

    peekRemote(serverId, path): DirectoryListing | null
    peekLocal(path): DirectoryListing | null
    loadRemote(serverId, path, loader): Promise<DirectoryListing>
    refreshRemote(serverId, path, loader): Promise<DirectoryListing>
    loadLocal(path, loader): Promise<DirectoryListing>
    refreshLocal(path, loader): Promise<DirectoryListing>
    invalidateRemote(serverId, path): void
    invalidateLocal(path): void
    rememberRemotePath(serverId, path): void
    lastRemotePath(serverId): string

Use Map insertion order as LRU, a second Map for in-flight Promises, remote keys containing serverId and normalized path, and a singleton export named directorySessionCache. Cache only DirectoryListing values; never cache credentials, sessions, or file contents.

- [ ] **Step 4: Verify GREEN**

Run the Step 2 command. Expected: all cache tests PASS.

### Task 2: Cache-aware file and log directory browsers

**Files:**
- Modify: src/features/transfers/FileTransferPage.tsx
- Modify: src/features/logs/RemoteLogBrowserDialog.tsx
- Modify: src/features/transfers/FileTransferPage.test.tsx
- Modify: src/features/logs/LogSearchPage.test.tsx
- Modify: src/styles/theme.css

- [ ] **Step 1: Add failing integration tests**

Render FileTransferPage, wait for '/', unmount, render again, and expect listRemoteDirectory('server-1', '/') once. Click a remote refresh button with accessible name “刷新远程目录”; expect a second API call while the existing rows remain visible. Open, close, and reopen RemoteLogBrowserDialog at /var/log; expect one API call.

- [ ] **Step 2: Verify RED**

Run:

    npm test -- --run src/features/transfers/FileTransferPage.test.tsx src/features/logs/LogSearchPage.test.tsx

Expected: FAIL because remount and reopen call the API again and refresh replaces the list with a blank loading state.

- [ ] **Step 3: Integrate the cache**

Before a load, call peekRemote or peekLocal. A cache hit populates state immediately and skips the full loading state. Manual refresh and post-transfer refresh keep the old listing and show a compact refreshing indicator in the path bar. A failed refresh keeps the old rows and appends “当前显示上次读取结果” to the Chinese error.

Remember the successful path per server. On server selection, use lastRemotePath(serverId), defaulting to /. Upload success refreshes the current remote target directory. Download success refreshes the current local directory.

- [ ] **Step 4: Verify GREEN**

Run the Step 2 command. Expected: all focused tests PASS with no unhandled Promise warnings.

### Task 3: Typed and bounded filename search backend

**Files:**
- Modify: src-tauri/src/core/logs/request.rs
- Modify: src-tauri/src/core/logs/command.rs
- Modify: src-tauri/src/core/logs/parser.rs
- Modify: src-tauri/src/core/logs/result_store.rs
- Modify: src-tauri/src/core/logs/mod.rs
- Modify: src-tauri/src/services/log_service.rs
- Modify: src-tauri/src/core/workflows/validation.rs
- Modify: src-tauri/src/services/workflow_nodes/io.rs
- Modify: src-tauri/tests/log_search_integration.rs

- [ ] **Step 1: Add failing Rust tests**

Add LogSearchTarget::Content and LogSearchTarget::Filename to wished-for test requests. Assert that a filename request for requi:
- contains fixed roots /var/log, /opt, /srv, and /home;
- contains -maxdepth 6 and a safely quoted literal *requi* pattern;
- never contains find / followed by a root-wide scan;
- caps results at 200;
- parses a __QZ_FILE__ record into RemoteFileMatch for /home/app/requirements.txt;
- rejects empty, NUL-containing, or longer-than-256-byte filename keywords.

- [ ] **Step 2: Verify RED**

Run:

    . .\scripts\dev-env.ps1 -Quiet
    $env:CARGO_BUILD_JOBS='1'
    cargo test --locked --manifest-path .\src-tauri\Cargo.toml --test log_search_integration -- --nocapture

Expected: compilation FAIL because LogSearchTarget, SearchResultItem, and RemoteFileMatch do not exist.

- [ ] **Step 3: Add target and result types**

LogSearchRequest gains target: LogSearchTarget. Define:

    enum LogSearchTarget { Content, Filename }

    enum SearchResultItem {
        Content(LogMatch),
        File(RemoteFileMatch),
    }

    struct RemoteFileMatch {
        path: String,
        name: String,
        size: Option<u64>,
        modified_at: Option<u64>,
    }

Serialize SearchResultItem with a resultType tag. Filename validation requires empty path, 1..=256 keyword bytes, context_lines 0, limit 1..=200, no time range, and case_sensitive false. Existing workflow constructors explicitly set Content.

- [ ] **Step 4: Build a safe literal find command**

For Filename, build fixed find calls for /var/log, /opt, /srv, and /home with -maxdepth 6 -type f -iname shell_quote("*keyword*"). Deduplicate and cap records with fixed awk code. Emit __QZ_FILE__ plus path, optional stat size, and optional stat modified epoch. Ignore inaccessible roots. Do not accept regex, shell, glob, or root parameters from the user.

The parser rejects non-absolute paths, traversal, NUL, newline, unit separator, or trailing slash and derives the file name from the final path component.

- [ ] **Step 5: Store and page tagged results**

LogResultPage.items and LogResultStore::write use SearchResultItem. Content text export remains path:line [MATCH|CONTEXT] text. File text export is path [FILE] size=<bytes-or-unknown> modified=<epoch-or-unknown>. Success summaries use “N 个文件” for Filename and “N 条日志记录” for Content. Execution parameters include target.

- [ ] **Step 6: Verify GREEN**

Run the Step 2 command. Expected: every log-search integration test PASS.

### Task 4: Filename-search frontend

**Files:**
- Modify: src/api/contracts.ts
- Modify: src/api/preview.ts
- Modify: src/api/preview.test.ts
- Modify: src/api/tauri.test.ts
- Modify: src/features/logs/LogSearchPage.tsx
- Modify: src/features/logs/LogResultsTable.tsx
- Modify: src/features/logs/LogSearchPage.test.tsx
- Modify: src/styles/theme.css

- [ ] **Step 1: Add failing UI tests**

Select the “找文件名” radio and assert:
- the input label is “文件名包含” with a requi example;
- time, context, case, and exact-path controls are absent;
- submit uses target filename, path '', caseSensitive false, contextLines 0, limit 200, and null times;
- preview results render requirements.txt, path, size, and modified time;
- no “第 1 行” content-row label appears.

- [ ] **Step 2: Verify RED**

Run:

    npm test -- --run src/features/logs/LogSearchPage.test.tsx

Expected: FAIL because target selection and file results do not exist.

- [ ] **Step 3: Add frontend unions and preview behavior**

Add LogSearchTarget = 'content' | 'filename'. Define SearchResultItem as either a content-tagged LogMatch or a file result containing path, name, size, and modifiedAt. Add target to LogSearchRequest and change LogResultPage.items accordingly. Preview returns content records for Content and /home/app/requirements.txt for Filename.

- [ ] **Step 4: Implement target-specific form and result table**

Add “搜日志内容 / 找文件名” before the current smart/path controls. Filename mode hides content-only controls, explains fixed safe roots, caps the keyword at 256 bytes, uses button label “开始查找”, and renders a file table. Content mode preserves all current behavior.

- [ ] **Step 5: Verify GREEN**

Run:

    npm test -- --run src/features/logs/LogSearchPage.test.tsx src/api/preview.test.ts src/api/tauri.test.ts

Expected: all focused tests PASS.

### Task 5: Accessible context-menu component

**Files:**
- Create: src/components/ContextMenu.tsx
- Create: src/components/ContextMenu.test.tsx
- Create: src/lib/clipboard.ts
- Modify: src/styles/theme.css

- [ ] **Step 1: Write failing component tests**

Render a menu with an enabled “复制完整路径” item and a disabled “下载” item. Assert role menu/menuitem, first-enabled focus, disabled reason, action invocation, and close-after-action. Add tests for Escape, outside pointer down, and viewport clamping.

- [ ] **Step 2: Verify RED**

Run:

    npm test -- --run src/components/ContextMenu.test.tsx

Expected: FAIL because ContextMenu does not exist.

- [ ] **Step 3: Implement the component and clipboard boundary**

Render with createPortal into document.body. Measure in useLayoutEffect and clamp to an 8-pixel viewport margin. Focus the first enabled item. Close on successful selection, Escape, outside pointer down, page change, or unmount. Disabled items never invoke actions and expose the Chinese disabled reason.

clipboard.ts exports copyText(value), calls navigator.clipboard.writeText, and throws “当前系统不支持复制到剪贴板” when unavailable.

- [ ] **Step 4: Verify GREEN**

Run the Step 2 command. Expected: all context-menu tests PASS.

### Task 6: Wire approved safe actions

**Files:**
- Modify: src/features/transfers/FileTransferPage.tsx
- Modify: src/features/transfers/FileTransferPage.test.tsx
- Modify: src/features/logs/LogResultsTable.tsx
- Modify: src/features/logs/LogSearchPage.tsx
- Modify: src/features/logs/LogSearchPage.test.tsx
- Modify: src/app/AppShell.tsx
- Modify: src/app/AppShell.test.tsx

- [ ] **Step 1: Add failing action-scope tests**

Right-click each object and assert exact actions:
- local file: upload, copy name, copy path;
- local folder: open, refresh, copy path;
- remote file: download, search file content, copy name, copy path;
- remote folder: open, refresh, copy path;
- filename result: download, search file content, copy path;
- content row: copy row, copy log path.

Assert that no menu contains delete, rename, or create. Assert a right-click download uses the clicked entry even if it was not previously selected.

- [ ] **Step 2: Verify RED**

Run:

    npm test -- --run src/features/transfers/FileTransferPage.test.tsx src/features/logs/LogSearchPage.test.tsx src/app/AppShell.test.tsx

Expected: FAIL because rows have no context handlers or cross-page search intent.

- [ ] **Step 3: Add file-browser entry actions**

Right-click selects the entry and opens ContextMenu at clientX/clientY. Refactor upload and download functions to accept an explicit BrowserEntry, avoiding React selection-state races. Folder actions use the cache-aware open/refresh functions. Copy actions use copyText and display a short Chinese status.

- [ ] **Step 4: Add one-shot cross-page search intent**

AppShell owns optional { serverId, path, keyword }. FileTransferPage calls onSearchRemoteFile, and AppShell switches to logs. LogSearchPage consumes the intent once, selects Content and exact-path mode, and fills server/path. Filename-result “搜索文件内容” stays on the page, switches target to Content, fills exact path, and preserves the filename keyword.

- [ ] **Step 5: Add log result actions**

Content rows copy row text or path. File results download through the existing safe downloadFile API, switch to content search, or copy path. Download uses overwrite false and reports the project-relative produced-file path.

- [ ] **Step 6: Verify GREEN**

Run the Step 2 command. Expected: all focused tests PASS.

### Task 7: Full verification and local-only package

**Files:**
- Verify source without changing version metadata.
- Create local artifacts only under artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r3.

- [ ] **Step 1: Run frontend checks**

    npm test -- --run
    npm run build

Expected: every frontend test PASS and the production build completes.

- [ ] **Step 2: Run Rust checks serially**

    . .\scripts\dev-env.ps1 -Quiet
    $env:CARGO_BUILD_JOBS='1'
    cargo fmt --manifest-path .\src-tauri\Cargo.toml --all --check
    cargo clippy --locked --manifest-path .\src-tauri\Cargo.toml --all-targets -- -D warnings
    cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets -- --nocapture

Expected: format and Clippy PASS; every enabled Rust test PASS. Fixture-dependent live tests stay explicitly ignored until the user authorizes the lightweight server or starts the local fixture.

- [ ] **Step 3: Run project constraints**

    & .\scripts\tests\desktop-ux.tests.ps1
    & .\scripts\tests\dev-env.tests.ps1
    & .\scripts\tests\package-config.tests.ps1
    & .\scripts\tests\release-config.tests.ps1
    & .\scripts\verify-d-drive.ps1
    git diff --check

Expected: every script reports PASS and Git reports no whitespace errors.

- [ ] **Step 4: Perform visual and interaction QA**

Verify 900x700, 1150x775, and 1920x1080. Confirm cached revisits do not blank, refresh preserves rows, requi returns a file table, each object has only approved actions, Escape closes menus, and menus stay inside the viewport.

- [ ] **Step 5: Build and smoke-test r3**

Build target/release/qingzhou-ssh.exe with one Cargo job. Copy it with portable.flag, README-portable.txt, and LICENSE to the r3 directory, create the ZIP, verify GUI subsystem 2, launch hidden for five seconds, verify all data under r3/data, and stop only that exact process.

- [ ] **Step 6: Hand off**

Report clickable EXE and ZIP paths, SHA-256, test results, viewport results, and confirmation that version, Git remotes, GitHub, and ModelScope were unchanged.
