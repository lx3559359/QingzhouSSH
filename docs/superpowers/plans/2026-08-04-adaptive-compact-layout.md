# Adaptive Compact Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every client page remain readable and usable from 1920×1080 down to the existing 960×640 minimum window by progressively reducing layout density and reflowing page structures instead of squeezing full-size cards into a narrow viewport.

**Architecture:** Add one CSS-variable density system with standard, compact, and minimum-window modes. Keep global mode selection in viewport media queries, use container queries for page internals such as SFTP and workflow, preserve the native topbar/window controls, and confine horizontal scrolling to data tables, navigation strips, and the workflow canvas. Lock the behavior with PowerShell CSS source contracts, existing React tests, production builds, and manual viewport checks.

**Tech Stack:** React 19, TypeScript, CSS custom properties, CSS media queries, CSS container queries, Vitest, Testing Library, PowerShell source-contract tests, Tauri 2.

---

No commit, tag, push, release upload, GitHub update, ModelScope update, or online updater change is part of this plan. Preserve the dirty worktree and unrelated user changes. The only generated deliverable is a new r7 local-test package below the D-drive project folder; r6 must remain untouched.

## File structure

- Create `scripts/tests/responsive-layout.tests.ps1`: verify density variables, viewport modes, navigation reflow, page-specific stacking, internal overflow boundaries, and removal of fixed workflow widths.
- Modify `src/styles/theme.css`: define shared density tokens; implement standard, compact, and minimum modes; adapt navigation, common cards, dashboard, servers, tasks, logs, SFTP, settings, history, and downloads.
- Modify `src/features/workflows/workflow.css`: remove page-level fixed minimum widths, add compact editor columns, and reflow workflow records/editor/run panels at the minimum viewport.
- Verify `src/app/AppShell.test.tsx`: ensure all icon-and-label navigation items remain rendered and accessible after the CSS-only reflow.
- Verify `src/features/logs/LogSearchPage.test.tsx`: preserve search behavior while the result table gains internal scrolling.
- Verify `src/features/transfers/FileTransferPage.test.tsx`: preserve SFTP browsing and transfer behavior while panes reflow.
- Verify `src/features/workflows/WorkflowPage.test.tsx`, `src/features/workflows/WorkflowInspector.test.tsx`, and `src/features/workflows/WorkflowRunPanel.test.tsx`: preserve workflow behavior while the editor layout changes.
- Create locally `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r7/` and `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r7-windows-x86_64-portable.zip`.

### Task 1: Add responsive layout source contracts and density tokens

**Files:**
- Create: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `src/styles/theme.css`

- [ ] **Step 1: Create reusable contract helpers and the first density assertions**

Create `scripts/tests/responsive-layout.tests.ps1` with project-local path resolution and explicit positive/negative assertions:

```powershell
$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$themePath = Join-Path $projectRoot 'src\styles\theme.css'
$workflowPath = Join-Path $projectRoot 'src\features\workflows\workflow.css'
$theme = Get-Content -LiteralPath $themePath -Raw
$workflow = Get-Content -LiteralPath $workflowPath -Raw

function Assert-Contains {
  param([string]$Text, [string]$Needle, [string]$Message)
  if (-not $Text.Contains($Needle)) { throw $Message }
}

function Assert-NotMatches {
  param([string]$Text, [string]$Pattern, [string]$Message)
  if ($Text -match $Pattern) { throw $Message }
}

Assert-Contains $theme '--density-page-padding:' 'Shared page-padding density token is missing.'
Assert-Contains $theme '--density-shell-gap:' 'Shared shell-gap density token is missing.'
Assert-Contains $theme '--density-card-gap:' 'Shared card-gap density token is missing.'
Assert-Contains $theme '--density-card-padding:' 'Shared card-padding density token is missing.'
Assert-Contains $theme '--density-sidebar-width:' 'Shared sidebar-width density token is missing.'
Assert-Contains $theme '--density-nav-height:' 'Shared navigation-height density token is missing.'
Assert-Contains $theme '--density-control-height:' 'Shared control-height density token is missing.'
Assert-Contains $theme '@media (min-width: 1050px) and (max-width: 1359px)' 'Compact viewport mode is missing.'
Assert-Contains $theme '@media (min-width: 960px) and (max-width: 1049px)' 'Minimum viewport mode is missing.'

Write-Host 'Responsive layout source contracts passed.'
```

- [ ] **Step 2: Run the new contract and verify RED**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: the script exits non-zero because the shared density tokens and new viewport ranges do not exist yet.

- [ ] **Step 3: Define the standard density values and consume them in shared primitives**

Add these variables to `:root` in `src/styles/theme.css`:

```css
--density-page-padding: clamp(22px, 4vw, 48px);
--density-shell-gap: 30px;
--density-card-gap: 26px;
--density-card-padding: 24px;
--density-card-radius: 18px;
--density-sidebar-width: 224px;
--density-nav-height: 45px;
--density-control-height: 42px;
--density-feature-icon-size: 50px;
```

Replace the corresponding shared fixed values instead of duplicating mode-specific selectors:

```css
.app-content { padding: var(--density-page-padding); }
.silver-card { border-radius: var(--density-card-radius); }
.workspace-shell {
  grid-template-columns: var(--density-sidebar-width) minmax(0, 1fr);
  gap: var(--density-shell-gap);
}
.side-navigation__items button { min-height: var(--density-nav-height); }
.primary-button,
.secondary-button,
.success-button,
.danger-button { min-height: var(--density-control-height); }
.feature-icon {
  width: var(--density-feature-icon-size);
  height: var(--density-feature-icon-size);
}
```

Keep the native `.window-controls` dimensions unchanged; they are window chrome, not page-density controls.

- [ ] **Step 4: Add compact and minimum token overrides**

Replace the old `@media (max-width: 1280px)` density block with non-overlapping ranges:

```css
@media (min-width: 1050px) and (max-width: 1359px) {
  :root {
    --density-page-padding: 18px;
    --density-shell-gap: 16px;
    --density-card-gap: 16px;
    --density-card-padding: 18px;
    --density-card-radius: 15px;
    --density-sidebar-width: 188px;
    --density-nav-height: 40px;
    --density-control-height: 38px;
    --density-feature-icon-size: 44px;
  }
}

@media (min-width: 960px) and (max-width: 1049px) {
  :root {
    --density-page-padding: 12px;
    --density-shell-gap: 12px;
    --density-card-gap: 12px;
    --density-card-padding: 14px;
    --density-card-radius: 13px;
    --density-nav-height: 38px;
    --density-control-height: 36px;
    --density-feature-icon-size: 40px;
  }
}
```

Do not add `zoom`, `scale`, or a page-level `transform`; compact mode must reflow real layout geometry.

- [ ] **Step 5: Run the density contract and verify GREEN**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: the script prints `Responsive layout source contracts passed.`

### Task 2: Reflow the application shell and navigation at 960–1049px

**Files:**
- Modify: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `src/styles/theme.css`
- Verify: `src/app/AppShell.test.tsx`

- [ ] **Step 1: Add shell, navigation, and global-overflow contracts**

Before the final success message in the PowerShell test, add:

```powershell
Assert-Contains $theme 'grid-template-rows: auto minmax(0, 1fr);' 'Minimum mode must place navigation above page content.'
Assert-Contains $theme 'overflow-x: auto;' 'Compact navigation needs its own horizontal scrolling boundary.'
Assert-Contains $theme 'white-space: nowrap;' 'Compact navigation labels must stay readable on one line.'
Assert-Contains $theme 'max-width: 34vw;' 'The compact data-root badge must truncate before crowding window controls.'
Assert-Contains $theme '.workspace-content {' 'The main page scroll container is missing.'
Assert-NotMatches $theme '(?s)html,\s*body,\s*#root\s*\{[^}]*overflow-x:\s*auto' 'Global horizontal scrolling is forbidden.'
```

- [ ] **Step 2: Run the contract and verify RED**

Run the responsive contract again.

Expected: it fails on one or more shell/navigation assertions because minimum mode still lives at the unreachable 820px breakpoint and the data-root badge has no 34vw compact cap.

- [ ] **Step 3: Compact the persistent sidebar in the 1050–1359px range**

Inside the compact media range, reduce spacing without hiding labels:

```css
.side-navigation { padding: 13px 10px; }
.side-navigation__title { gap: 9px; padding: 5px 6px 14px; }
.side-navigation__items { gap: 5px; padding: 13px 0; }
.side-navigation__items button {
  gap: 8px;
  padding: 8px 9px;
  font-size: 0.84rem;
}
.side-navigation__items button svg { width: 19px; height: 19px; }
.data-root-badge { max-width: 34vw; padding: 8px 10px; }
.page-heading { gap: 18px; margin-bottom: 20px; }
.page-heading h1 { font-size: clamp(1.7rem, 3.4vw, 2.2rem); }
```

- [ ] **Step 4: Move navigation above content in the 960–1049px range**

Move the useful rules from the old `max-width: 820px` block into the reachable minimum range, preserving icon and Chinese label text:

```css
.workspace-shell {
  grid-template-columns: 1fr;
  grid-template-rows: auto minmax(0, 1fr);
  gap: var(--density-shell-gap);
}

.side-navigation {
  position: static;
  height: auto;
  min-height: 0;
  padding: 7px;
}

.side-navigation__title,
.side-navigation__security { display: none; }

.side-navigation__items {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding: 0 0 3px;
  scrollbar-width: thin;
}

.side-navigation__items button {
  min-width: max-content;
  flex: 0 0 auto;
  gap: 7px;
  padding: 7px 11px;
  font-size: 0.78rem;
  white-space: nowrap;
}

.side-navigation__items button svg { width: 18px; height: 18px; }
.workspace-content { padding: 2px 8px 24px 2px; }
.data-root-badge { max-width: 28vw; padding: 7px 9px; }
```

Leave `.app-topbar`, `.app-topbar__actions`, `.window-controls`, and the native drag handler unchanged. Remove or narrow the obsolete `max-width: 820px` shell rules so they cannot conflict with the new minimum mode.

- [ ] **Step 5: Run shell tests and verify GREEN**

Run:

```powershell
pnpm test -- src/app/AppShell.test.tsx src/app/App.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: all navigation labels remain accessible, native drag/window-control tests still pass, and the source contract is green.

### Task 3: Adapt common cards and non-specialized pages

**Files:**
- Modify: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `src/styles/theme.css`
- Verify: `src/features/servers/ServerListPage.test.tsx`
- Verify: `src/features/tasks/TaskPage.test.tsx`
- Verify: `src/features/settings/SettingsPage.test.tsx`
- Verify: `src/features/history/ExecutionHistoryPage.test.tsx`
- Verify: `src/features/downloads/DownloadsPage.test.tsx`

- [ ] **Step 1: Add contracts for flexible grids and control minimums**

Add these checks:

```powershell
Assert-Contains $theme 'min-height: var(--density-control-height);' 'Shared actions must consume the control-height token.'
Assert-Contains $theme 'grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));' 'Task cards need a compact auto-fit grid.'
Assert-Contains $theme 'grid-template-columns: repeat(3, minmax(0, 1fr));' 'Minimum history filters need three usable columns.'
Assert-Contains $theme '.settings-main-grid,' 'Settings and related detail layouts must share minimum-mode stacking.'
Assert-NotMatches $theme 'min-height:\s*(?:2[0-9]|3[0-5])px' 'Interactive controls below 36px are forbidden in the adaptive rules.'
```

Scope the last assertion to the newly added responsive section if existing decorative elements contain small `min-height` values; do not rewrite unrelated legacy styling merely to satisfy the contract.

- [ ] **Step 2: Run the contract and verify RED**

Run the responsive contract.

Expected: the task auto-fit and minimum-page stacking assertions fail.

- [ ] **Step 3: Apply compact card density before reducing columns**

Within 1050–1359px, add targeted reductions:

```css
.dashboard-grid,
.server-grid { gap: var(--density-card-gap); }

.dashboard-action {
  min-height: 128px;
  gap: 15px;
  padding: var(--density-card-padding);
}

.server-card,
.settings-panel,
.history-list,
.history-details { padding: var(--density-card-padding); }

.task-card-grid {
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: var(--density-card-gap);
}

.task-card {
  min-height: 166px;
  gap: 11px;
  padding: var(--density-card-padding);
}

.settings-main-grid,
.history-layout { gap: var(--density-card-gap); }
```

Keep two-column dashboard/server layouts when both cards remain above their usable minimum; do not force a column reduction merely because compact mode is active.

- [ ] **Step 4: Reflow detail-heavy pages in minimum mode**

Within 960–1049px, use the full width made available by the top navigation:

```css
.dashboard-grid,
.server-grid,
.task-card-grid,
.download-file-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--density-card-gap);
}

.dashboard-action,
.server-card,
.task-card,
.settings-panel,
.history-list,
.history-details,
.download-file-card { padding: var(--density-card-padding); }

.settings-main-grid,
.history-layout,
.advanced-execution-grid {
  grid-template-columns: 1fr;
  gap: var(--density-card-gap);
}

.history-filters {
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 9px;
  padding: var(--density-card-padding);
}

.settings-summary-card--path div > strong,
.download-file-card code {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
```

Use `font-size: 0.75rem` (12px at the root size) as the smallest body copy in these new rules, and retain at least 13px for primary form labels/actions where the existing size is larger.

- [ ] **Step 5: Run page tests and verify GREEN**

Run:

```powershell
pnpm test -- src/features/servers/ServerListPage.test.tsx src/features/tasks/TaskPage.test.tsx src/features/settings/SettingsPage.test.tsx src/features/history/ExecutionHistoryPage.test.tsx src/features/downloads/DownloadsPage.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: all behavior tests pass and responsive contracts are green.

### Task 4: Give log search and SFTP page-specific reflow rules

**Files:**
- Modify: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `src/styles/theme.css`
- Verify: `src/features/logs/LogSearchPage.test.tsx`
- Verify: `src/features/transfers/FileTransferPage.test.tsx`

- [ ] **Step 1: Add internal-overflow and page-stacking contracts**

Add:

```powershell
Assert-Contains $theme '.log-table-wrap {' 'Log results need a dedicated table scroll boundary.'
Assert-Contains $theme 'overflow: auto;' 'A page-internal data/canvas scrolling boundary is required.'
Assert-Contains $theme 'grid-template-areas:' 'SFTP pane ordering must be explicit.'
Assert-Contains $theme '"local"' 'Minimum SFTP mode must place the local pane first.'
Assert-Contains $theme '"actions"' 'Minimum SFTP mode must place transfer actions between panes.'
Assert-Contains $theme '"remote"' 'Minimum SFTP mode must place the remote pane last.'
```

- [ ] **Step 2: Run the contract and verify RED**

Run the responsive contract.

Expected: minimum SFTP ordering or log-page stacking coverage is absent from the 960–1049px mode.

- [ ] **Step 3: Compact logs while preserving side-by-side use above 1050px**

For compact mode, reduce gaps and padding while keeping proportional columns:

```css
.log-search-layout {
  grid-template-columns: minmax(250px, 0.7fr) minmax(0, 1.5fr);
  gap: var(--density-card-gap);
}
.log-search-form,
.log-results-panel { padding: var(--density-card-padding); }
.log-table-wrap {
  max-width: 100%;
  overflow: auto;
}
.log-results-table { min-width: 660px; }
```

The table may scroll inside `.log-table-wrap`; `.workspace-content` and the application body must not gain horizontal scrolling.

- [ ] **Step 4: Stack log search in minimum mode**

Add:

```css
.log-search-layout { grid-template-columns: 1fr; }
.log-form-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
.log-results-panel { min-width: 0; }
.log-results-header { gap: 12px; }
.log-download-controls input { width: min(170px, 24vw); }
```

This places the beginner-facing search form above results while retaining internal result-table scrolling.

- [ ] **Step 5: Keep SFTP compact at 1050–1359px and stack it at 960–1049px**

Preserve the existing `@container transfer-page (max-width: 920px)` two-pane arrangement for compact containers, but consume density tokens for pane spacing and padding. Then add this minimum viewport override after the container queries:

```css
@media (min-width: 960px) and (max-width: 1049px) {
  .sftp-workspace {
    grid-template-columns: 1fr;
    grid-template-areas:
      "local"
      "actions"
      "remote";
    gap: var(--density-card-gap);
  }

  .sftp-actions {
    width: 100%;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    align-items: center;
    gap: 8px;
    padding: 0;
  }

  .sftp-actions button,
  .sftp-actions .checkbox-field { min-height: 44px; }
  .sftp-actions > small { grid-column: 1 / -1; }
  .sftp-pane {
    height: clamp(300px, 42dvh, 420px);
    padding: var(--density-card-padding);
  }
}
```

Do not change SFTP commands, directory caching, right-click menus, selected-file state, upload/download rules, or D-drive download destinations.

- [ ] **Step 6: Run focused behavior tests and verify GREEN**

Run:

```powershell
pnpm test -- src/features/logs/LogSearchPage.test.tsx src/features/transfers/FileTransferPage.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: log-search and SFTP behavior tests pass; the responsive contract confirms internal scrolling and minimum-mode pane order.

### Task 5: Remove workflow page minimum widths and add editor reflow

**Files:**
- Modify: `scripts/tests/responsive-layout.tests.ps1`
- Modify: `src/features/workflows/workflow.css`
- Verify: `src/features/workflows/WorkflowPage.test.tsx`
- Verify: `src/features/workflows/WorkflowInspector.test.tsx`
- Verify: `src/features/workflows/WorkflowRunPanel.test.tsx`

- [ ] **Step 1: Add workflow overflow contracts**

Add:

```powershell
Assert-NotMatches $workflow 'min-width:\s*850px' 'Workflow must not force an 850px page width.'
Assert-NotMatches $workflow '(?s)\.workflow-page\s*\{[^}]*overflow-x:\s*auto' 'Workflow page must not own horizontal scrolling.'
Assert-Contains $workflow 'container-name: workflow-page;' 'Workflow needs a page-internal container query boundary.'
Assert-Contains $workflow 'grid-template-areas:' 'Minimum workflow mode must explicitly arrange editor regions.'
Assert-Contains $workflow '.workflow-canvas__viewport {' 'Workflow canvas must retain its own pan boundary.'
```

- [ ] **Step 2: Run the contract and verify RED**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: it fails because `.workflow-builder`, `.workflow-run-panel`, `.workflow-page__heading`, and `.workflow-records` still contain or inherit fixed 850px requirements and page-level overflow.

- [ ] **Step 3: Make the workflow editor intrinsically shrinkable**

At the top of `workflow.css`, establish a container and explicit areas:

```css
.workflow-page {
  width: 100%;
  min-width: 0;
  margin: 0 auto;
  container-name: workflow-page;
  container-type: inline-size;
}

.workflow-builder {
  min-width: 0;
  display: grid;
  grid-template-columns: minmax(150px, 184px) minmax(0, 1fr) minmax(176px, 205px);
  grid-template-areas: "library canvas inspector";
  align-items: stretch;
  gap: 18px;
}

.workflow-library { grid-area: library; }
.workflow-canvas { grid-area: canvas; }
.workflow-selection { grid-area: inspector; }
```

Keep `.workflow-canvas__viewport { overflow: auto; }` and the fixed logical board size so panning occurs only inside the canvas.

- [ ] **Step 4: Add compact container density**

Add a workflow container query for the narrower content area created by the compact sidebar:

```css
@container workflow-page (max-width: 880px) {
  .workflow-builder {
    grid-template-columns: 154px minmax(0, 1fr) 180px;
    gap: 12px;
  }

  .workflow-library { padding: 13px 10px; }
  .workflow-selection { padding: 14px 11px; }
  .workflow-canvas { min-height: 540px; }
  .workflow-canvas__viewport { height: 474px; }
}
```

Do not hide the library or inspector; compact mode keeps all three editor regions visible at reduced density.

- [ ] **Step 5: Reflow workflow regions in minimum viewport mode**

Replace the old `max-width: 1000px`, `max-width: 1180px`, and `max-width: 820px` fixed-width rules with:

```css
@media (min-width: 960px) and (max-width: 1049px) {
  .workflow-heading-actions { gap: 7px; }

  .workflow-records {
    grid-template-columns: auto minmax(0, 1fr);
    gap: 10px;
    padding: 12px;
  }

  .workflow-records__list { grid-column: 1 / -1; }
  .workflow-delete { grid-column: 2; grid-row: 1; justify-self: end; }

  .workflow-builder {
    grid-template-columns: minmax(0, 1fr) 190px;
    grid-template-areas:
      "library library"
      "canvas inspector";
    gap: 12px;
  }

  .workflow-library__items {
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  }

  .workflow-run-panel { min-width: 0; padding: 14px; }
  .workflow-run-panel__header { align-items: stretch; flex-direction: column; }
  .workflow-run-targets {
    min-width: 0;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
  .workflow-run-targets button { grid-column: 1 / -1; }
  .workflow-run-content { grid-template-columns: 1fr; }
}
```

Keep the editor canvas's own scrollbars/panning. Do not add overflow to `.workflow-page`, `.workflow-builder`, or `.workspace-content`.

- [ ] **Step 6: Run workflow tests and verify GREEN**

Run:

```powershell
pnpm test -- src/features/workflows/WorkflowPage.test.tsx src/features/workflows/WorkflowInspector.test.tsx src/features/workflows/WorkflowRunPanel.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
```

Expected: all workflow behavior tests pass and the source contract confirms there are no 850px page-level minimums or workflow-page horizontal scrolling.

### Task 6: Complete automated verification and visual viewport checks

**Files:**
- Verify: all files modified in Tasks 1–5
- Verify: `src-tauri/tauri.conf.json`
- Verify: existing r6 package remains unchanged

- [ ] **Step 1: Run the complete frontend suite and production build**

Run:

```powershell
pnpm test
pnpm build
```

Expected: all Vitest tests pass and the production frontend build exits with code 0.

- [ ] **Step 2: Run responsive, desktop, data-location, and local-build contracts**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\responsive-layout.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\local-build.tests.ps1
```

Expected: every script exits 0; the native minimum window remains 960×640; all generated data/build paths remain inside `D:\Codex Project\轻量化SSH快捷工具`.

- [ ] **Step 3: Run Rust checks**

Run:

```powershell
cargo fmt --check --manifest-path .\src-tauri\Cargo.toml
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
```

Expected: formatting is clean and all non-live Rust tests pass; fixture-dependent live tests remain ignored by design.

- [ ] **Step 4: Verify the four target viewport sizes against every page**

Launch the local app and inspect 1920×1080, 1366×768, 1180×760, and 960×640. At each size, open 首页、服务器、快捷任务、日志检索、文件传输、工作流、执行记录、下载文件、设置 and check:

- no page-level horizontal scrollbar appears;
- standard mode retains the accepted silver-card visual design;
- compact mode reduces padding/gaps/card dimensions before changing columns;
- minimum mode shows a top horizontal icon-and-Chinese-label navigation strip;
- log results scroll only inside the table wrapper;
- SFTP stacks local/actions/remote at 960×640 and all transfer controls remain visible;
- workflow library/canvas/inspector remain usable and only the canvas pans horizontally;
- the topbar remains draggable and minimize/maximize/close controls remain usable;
- buttons and inputs remain at least 36px high and beginner-facing text is not clipped.

If any viewport fails, return to the responsible task and add a contract before changing CSS.

### Task 7: Build the r7 self-test package without publishing

**Files:**
- Create locally: `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r7/`
- Create locally: `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r7-windows-x86_64-portable.zip`

- [ ] **Step 1: Confirm r7 targets do not already exist**

Run:

```powershell
$expanded = 'D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test\QingzhouSSH-v0.1.5-local.20260804-r7'
$archive = 'D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test\QingzhouSSH-v0.1.5-local.20260804-r7-windows-x86_64-portable.zip'
if ((Test-Path -LiteralPath $expanded) -or (Test-Path -LiteralPath $archive)) {
  throw 'r7 target already exists; choose a new local revision instead of overwriting it.'
}
```

Expected: no output and exit code 0. Do not delete or overwrite an existing artifact.

- [ ] **Step 2: Build the new local package**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-local-test.ps1 -PackageVersion '0.1.5-local.20260804-r7'
```

Expected: the expanded package and ZIP are created only below `D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test`; r6 remains unchanged.

- [ ] **Step 3: Verify contents and calculate hashes**

Run:

```powershell
$expanded = 'D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test\QingzhouSSH-v0.1.5-local.20260804-r7'
$exe = Join-Path $expanded 'QingzhouSSH.exe'
$data = Join-Path $expanded 'data'
$archive = 'D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test\QingzhouSSH-v0.1.5-local.20260804-r7-windows-x86_64-portable.zip'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'r7 executable is missing.' }
if (-not (Test-Path -LiteralPath $data -PathType Container)) { throw 'r7 project-local data directory is missing.' }
Get-FileHash -LiteralPath $exe -Algorithm SHA256
Get-FileHash -LiteralPath $archive -Algorithm SHA256
```

Expected: both SHA-256 hashes are returned. Provide the r7 EXE/ZIP paths to the user for manual validation. Do not publish, upload, enable an update manifest, or start another iteration automatically.

## Self-review

- Spec coverage: standard (≥1360px), compact (1050–1359px), and minimum (960–1049px) modes are each implemented and verified.
- Page coverage: dashboard, servers, tasks, logs, SFTP, workflow, history, downloads, and settings each have an explicit implementation or verification step.
- Overflow boundary: body/workspace never gain horizontal scrolling; navigation, log table, workflow record strip, and workflow canvas own only their necessary local scrolling.
- Beginner usability: icon-and-Chinese-label navigation remains visible, controls remain at least 36px high, and important copy is not reduced below the agreed readable floor.
- Native window safety: topbar dragging and window controls are explicitly regression-tested and are not resized by density tokens.
- Data safety: no existing local package is overwritten and every generated artifact remains under the D-drive project folder.
- Scope safety: no server logic, update logic, release metadata, remote project, color system, icon set, shadow style, or minimum native window size changes are included.
- Placeholder scan: the plan contains no deferred implementation markers or unresolved choices.
- Type consistency: this is a CSS-only production change; React component props and Rust command contracts remain unchanged and are covered by existing tests.
