# Novice Shell, Responsive SFTP, and Smart Log Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the desktop surface feel like one native client, keep the SFTP workspace usable in resized windows, and let novice users search common logs without knowing a remote path.

**Architecture:** The native title bar remains enabled while the WebView shell becomes edge-to-edge with a fixed application header and an internally scrolling workspace. The SFTP layout responds to its actual content width and uses a compact idle status strip. Log search treats an empty path as a bounded smart-search mode that scans only common log roots and emits the source path with every match; an explicit absolute path remains available as an advanced mode.

**Tech Stack:** React 19, TypeScript, CSS container queries, Tauri 2, Rust, Vitest, PowerShell contract tests.

**Release constraint:** Do not change version files, commit, push, tag, publish, or update online metadata before user validation.

---

### Task 1: Lock the desktop layout regressions

**Files:**
- Modify: `scripts/tests/desktop-ux.tests.ps1`

- [ ] **Step 1: Write failing source-contract assertions**

Require the outer shell to use zero padding, the inner app window to have no outer radius or shadow, the document to avoid body scrolling, the workspace content to own vertical scrolling, and the transfer page to expose content-width breakpoints rather than viewport-only breakpoints.

- [ ] **Step 2: Run the test and verify RED**

Run: `powershell -ExecutionPolicy Bypass -File scripts/tests/desktop-ux.tests.ps1`

Expected: FAIL because `.app-shell` still has `padding: 18px`, `.app-window` still has a 26px radius/shadow, and SFTP still relies on `@media (max-width: 1080px)`.

- [ ] **Step 3: Implement the minimal layout CSS**

In `src/styles/theme.css`, make `html`, `body`, and `#root` fill the viewport; make `.app-shell` and `.app-window` edge-to-edge; move scrolling to `.workspace-content`; add a reduced-width desktop shell breakpoint; add `container-type: inline-size` to `.transfer-page`; use container breakpoints for a two-pane/row-actions layout and a single-column compact layout.

- [ ] **Step 4: Run the contract test and verify GREEN**

Run the same PowerShell command and expect `Desktop UX source contracts passed.`

### Task 2: Keep the SFTP page complete when resized

**Files:**
- Modify: `src/features/transfers/FileTransferPage.test.tsx`
- Modify: `src/features/transfers/FileTransferPage.tsx`
- Modify: `src/styles/theme.css`

- [ ] **Step 1: Write a failing component test**

Assert that an untouched transfer page renders a compact idle status region and does not render empty source, target, progress, and metric blocks until a file operation exists.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `npm test -- src/features/transfers/FileTransferPage.test.tsx --run`

Expected: FAIL because the idle page currently renders the full 393px status card.

- [ ] **Step 3: Implement the compact status state**

Compute `const showTransferDetails = Boolean(source || target || status || running)` and add `sftp-status-card--idle` when false. Render paths, progress, metrics, hashes, messages, and cancel controls only when details exist.

- [ ] **Step 4: Run the focused test and verify GREEN**

Run the same Vitest command and expect both SFTP tests to pass.

### Task 3: Add novice-first smart log search

**Files:**
- Modify: `src-tauri/tests/log_search_integration.rs`
- Modify: `src-tauri/src/core/logs/request.rs`
- Modify: `src-tauri/src/core/logs/command.rs`
- Modify: `src-tauri/src/core/logs/parser.rs`
- Modify: `src-tauri/src/core/system_probe.rs`
- Modify: `src/features/logs/LogSearchPage.test.tsx`
- Modify: `src/features/logs/LogSearchPage.tsx`
- Modify: `src/styles/theme.css`

- [ ] **Step 1: Write failing Rust tests**

Add a request with an empty path and assert that it validates, requires `find`, scans only bounded common roots (`/var/log`, application log subdirectories under `/opt`, `/srv`, and `/home`), caps candidates, limits file size/age, safely quotes the content keyword, and never runs `find /`. Add parser coverage for machine records carrying a discovered source path.

- [ ] **Step 2: Run the focused Rust test and verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml --test log_search_integration -- --nocapture`

Expected: FAIL because an empty path is rejected and machine records cannot carry discovered paths.

- [ ] **Step 3: Write a failing React test**

Assert that smart search is selected by default, the exact path control is hidden, a content keyword can be submitted with `path: ''`, and switching to `指定日志文件` restores remote browsing and submits an absolute path.

- [ ] **Step 4: Run the focused React test and verify RED**

Run: `npm test -- src/features/logs/LogSearchPage.test.tsx --run`

Expected: FAIL because the current form requires `/var/log/syslog`.

- [ ] **Step 5: Implement the bounded smart-search command and parser**

Treat an empty path as smart mode. Build a static POSIX shell pipeline that discovers at most 120 recent files no larger than 32 MiB from common log locations, searches plain files with `grep`, searches gzip files only when `gzip` exists, emits `path`, `line`, `kind`, and `text` fields, and exits successfully when no match is found. Preserve the existing exact-file behavior.

- [ ] **Step 6: Implement the novice-first form**

Default to `智能搜索（推荐）`, show a plain-language bounded-scope explanation, require only server and content keyword, place exact path selection under `指定日志文件`, and update progress/error copy to explain automatic discovery.

- [ ] **Step 7: Run both focused suites and verify GREEN**

Run the focused Rust and Vitest commands; expect zero failures.

### Task 4: Verify and hand off a local-only build

**Files:**
- Verify: all modified source and test files
- Create locally: `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r2/`

- [ ] **Step 1: Run complete checks**

Run `npm test -- --run`, `npm run build`, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked --all-targets -- --nocapture`, and the project PowerShell path/UX tests.

- [ ] **Step 2: Verify responsive behavior visually**

At 1920×1080, 1150×775, and 900×700, verify that there is no nested outer frame or body scrollbar, the app header stays visible, SFTP controls remain reachable without horizontal overflow, and the smart log form does not ask for a path by default.

- [ ] **Step 3: Build a local portable executable**

Build without changing version metadata, place it only under the D-drive project artifacts directory, verify the Windows GUI subsystem and project-local data root, then stop all preview/fixture processes.

- [ ] **Step 4: Stop for user validation**

Report the local executable and ZIP paths plus SHA-256. Do not commit, push, tag, publish, or start another iteration.
