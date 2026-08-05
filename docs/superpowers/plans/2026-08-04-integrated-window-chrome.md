# Integrated Window Chrome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the Windows native title bar and make the existing blue application header the complete draggable desktop title bar with working window controls.

**Architecture:** Configure the Tauri `WebviewWindow` as undecorated but resizable, expose only the four required window permissions, and isolate native window calls behind a small frontend adapter. Render the controls inside the existing blue top bar so page layout and feature behavior remain unchanged.

**Tech Stack:** Tauri 2, Rust, React 19, TypeScript, Phosphor Icons, Vitest, Testing Library, PowerShell contract tests.

---

### Task 1: Lock the frameless-window contract with failing tests

**Files:**
- Modify: `scripts/tests/desktop-ux.tests.ps1`
- Modify: `src/app/App.test.tsx`

- [ ] **Step 1: Add the native-window source assertions**

Extend `desktop-ux.tests.ps1` to read `src-tauri/src/window.rs` and `src-tauri/capabilities/default.json`, then require:

```powershell
if ($windowSource -notmatch '\.decorations\(false\)') {
  throw 'The main window must not render the native title bar.'
}
if ($windowSource -notmatch '\.resizable\(true\)') {
  throw 'The frameless main window must remain resizable.'
}
foreach ($permission in @(
  'core:window:allow-minimize',
  'core:window:allow-toggle-maximize',
  'core:window:allow-close'
)) {
  if ($capabilities.permissions -notcontains $permission) {
    throw "Missing integrated title-bar permission: $permission"
  }
}
```

- [ ] **Step 2: Add the frontend behavior test**

Mock `getCurrentWindow()` in `App.test.tsx`, render `App`, and assert that clicking the three named controls calls `minimize`, `toggleMaximize`, and `close`; verify that the header exposes Tauri's official drag-region marker.

```tsx
expect(screen.getByTestId('window-drag-region')).toHaveAttribute('data-tauri-drag-region');

await user.click(screen.getByRole('button', { name: '最小化窗口' }));
await user.click(screen.getByRole('button', { name: '最大化或还原窗口' }));
await user.click(screen.getByRole('button', { name: '关闭窗口' }));
expect(windowMocks.minimize).toHaveBeenCalledTimes(1);
expect(windowMocks.toggleMaximize).toHaveBeenCalledTimes(1);
expect(windowMocks.close).toHaveBeenCalledTimes(1);
```

- [ ] **Step 3: Run the focused tests and verify RED**

Run:

```powershell
pnpm test -- src/app/App.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
```

Expected: the React test cannot find the new controls/drag region and the PowerShell test reports that `.decorations(false)` is missing.

### Task 2: Implement the native-window adapter and integrated controls

**Files:**
- Create: `src/app/nativeWindow.ts`
- Create: `src/app/WindowControls.tsx`
- Modify: `src/app/App.tsx`

- [ ] **Step 1: Add a minimal native-window adapter**

Create an adapter around `getCurrentWindow()`:

```ts
import { getCurrentWindow } from '@tauri-apps/api/window';

const currentWindow = () => getCurrentWindow();

export const windowControls = {
  minimize: () => currentWindow().minimize(),
  toggleMaximize: () => currentWindow().toggleMaximize(),
  close: () => currentWindow().close(),
};
```

- [ ] **Step 2: Add the three standard control buttons**

Create `WindowControls.tsx` with Phosphor `Minus`, `Square`, and `X` icons. Each button catches a rejected native promise so raw errors never replace the interface:

```tsx
function run(action: () => Promise<void>) {
  void action().catch(() => undefined);
}

export function WindowControls() {
  return (
    <div className="window-controls" aria-label="窗口控制">
      <button type="button" aria-label="最小化窗口" onClick={() => run(windowControls.minimize)}><Minus /></button>
      <button type="button" aria-label="最大化或还原窗口" onClick={() => run(windowControls.toggleMaximize)}><Square /></button>
      <button className="window-controls__close" type="button" aria-label="关闭窗口" onClick={() => run(windowControls.close)}><X /></button>
    </div>
  );
}
```

- [ ] **Step 3: Turn the blue header into the drag surface**

Update `App.tsx` so the brand and unused header space sit in a `data-tauri-drag-region` element. Tauri handles dragging and double-click maximize natively. Keep the data-root badge and window buttons outside that region.

- [ ] **Step 4: Run the React test and verify GREEN**

Run `pnpm test -- src/app/App.test.tsx`.

Expected: all App tests pass and each native action is called exactly once.

### Task 3: Remove native decorations and grant minimal permissions

**Files:**
- Modify: `src-tauri/src/window.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `scripts/tests/desktop-ux.tests.ps1`

- [ ] **Step 1: Configure the main window**

Add the following builder calls before the size constraints:

```rust
.decorations(false)
.resizable(true)
```

Do not change the current `1180 × 760` initial size or `960 × 640` minimum size.

- [ ] **Step 2: Grant only the required commands**

Add these capability identifiers:

```json
"core:window:allow-minimize",
"core:window:allow-toggle-maximize",
"core:window:allow-close"
```

- [ ] **Step 3: Run the native-window contract and verify GREEN**

Run `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1`.

Expected: `Desktop UX source contracts passed.`

### Task 4: Style and verify the unified client frame

**Files:**
- Modify: `src/styles/theme.css`
- Test: `src/app/App.test.tsx`
- Test: `scripts/tests/desktop-ux.tests.ps1`

- [ ] **Step 1: Add integrated title-bar styles**

Make `.app-topbar__drag-region` flexible and draggable, group the badge and controls in `.app-topbar__actions`, and style each window button as a fixed `44 × 40` transparent control. Use a subtle white hover for minimize/maximize and `#d94a55` for the close-button hover. Add `user-select: none` to the drag region and preserve truncation on the data-root badge.

- [ ] **Step 2: Preserve compact-window behavior**

At narrow widths, keep `.window-controls` visible, allow the data-root badge to shrink, and avoid the existing mobile rule that stacks the entire topbar vertically in a desktop window.

- [ ] **Step 3: Run all automated checks**

Run:

```powershell
pnpm test -- --run
pnpm build
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-d-drive.ps1
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
```

Expected: 75 existing tests plus the new title-bar test pass; frontend and Rust builds succeed; D-drive check passes.

- [ ] **Step 4: Build and visually inspect r5**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-local-test.ps1 -PackageVersion '0.1.5-local.20260804-r5'
```

Start the exact r5 EXE and verify the native white title bar is absent, the blue topbar touches the outer window edge, drag/double-click/control buttons work, edge resize remains functional, and the data root is `r5\data`.

- [ ] **Step 5: Stop for user validation**

Provide the expanded r5 EXE and portable ZIP paths. Do not modify online releases, Git tags, or the source version until the user validates the result.
