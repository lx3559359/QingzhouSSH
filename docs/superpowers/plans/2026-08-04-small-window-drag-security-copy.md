# Small Window Drag and Security Copy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the complete blue title area draggable at reduced window sizes and replace the cramped technical sidebar status with one clear safety message.

**Architecture:** Replace the narrow declarative drag strip with one explicit topbar mouse handler that calls Tauri's native `startDragging()` API and ignores window-control buttons. Keep the change inside the existing window API adapter, add the required Tauri capability, and verify the beginner-facing copy through the existing AppShell component test.

**Tech Stack:** React 19, TypeScript, Tauri 2 window API, Vitest, Testing Library, CSS, PowerShell source-contract tests.

---

No commit, tag, push, release upload, or online update is part of this plan. The only output is an r6 local-test package under the project folder.

## File structure

- Modify `src/app/App.test.tsx`: prove that every non-control part of the blue topbar starts native dragging and controls remain clickable.
- Modify `src/app/AppShell.test.tsx`: lock the simplified beginner-facing safety message.
- Modify `scripts/tests/desktop-ux.tests.ps1`: require the native drag capability.
- Modify `src/app/nativeWindow.ts`: expose Tauri `startDragging()` through the existing window adapter.
- Modify `src/app/App.tsx`: make the entire topbar the drag surface and exclude window-control buttons.
- Modify `src/app/AppShell.tsx`: replace the two-line technical status with a single clear message.
- Modify `src/styles/theme.css`: remove obsolete narrow drag-region rules and keep the compact safety row readable.
- Modify `src-tauri/capabilities/default.json`: allow native start-dragging for the main window.
- Create locally `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r6/` and its portable ZIP.

### Task 1: Add failing behavior and capability tests

**Files:**
- Modify: `src/app/App.test.tsx`
- Modify: `src/app/AppShell.test.tsx`
- Modify: `scripts/tests/desktop-ux.tests.ps1`

- [ ] **Step 1: Extend the mocked Tauri window API**

Add `startDragging` beside the existing window mocks:

```ts
const windowMocks = vi.hoisted(() => ({
  startDragging: vi.fn().mockResolvedValue(undefined),
  minimize: vi.fn().mockResolvedValue(undefined),
  toggleMaximize: vi.fn().mockResolvedValue(undefined),
  close: vi.fn().mockResolvedValue(undefined),
}));
```

- [ ] **Step 2: Replace the obsolete drag-marker assertion with interaction assertions**

Use `fireEvent` to prove a left-button press on the topbar calls native dragging, a right-button press does not, and a left-button press on the minimize button does not:

```ts
const topbar = screen.getByTestId('window-drag-region');
fireEvent.mouseDown(topbar, { button: 0 });
expect(windowMocks.startDragging).toHaveBeenCalledTimes(1);

fireEvent.mouseDown(topbar, { button: 2 });
fireEvent.mouseDown(screen.getByRole('button', { name: '最小化窗口' }), { button: 0 });
expect(windowMocks.startDragging).toHaveBeenCalledTimes(1);
```

Keep the existing click assertions for minimize, maximize/restore, and close.

- [ ] **Step 3: Add the safety-copy assertion**

In `AppShell.test.tsx`, render the shell and assert:

```ts
expect(screen.getByText('本地安全保护已开启')).toBeVisible();
expect(screen.queryByText(/WebView/i)).not.toBeInTheDocument();
```

- [ ] **Step 4: Require the Tauri drag capability**

Add `core:window:allow-start-dragging` to the permission list checked in `scripts/tests/desktop-ux.tests.ps1`.

- [ ] **Step 5: Run focused tests and verify RED**

Run:

```powershell
pnpm test -- src/app/App.test.tsx src/app/AppShell.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
```

Expected: React fails because `startDragging` is not called and the new safety copy is absent; PowerShell fails because the drag permission is absent.

### Task 2: Implement reliable topbar dragging

**Files:**
- Modify: `src/app/nativeWindow.ts`
- Modify: `src/app/App.tsx`
- Modify: `src/styles/theme.css`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Expose the native drag operation**

Add the following adapter member in `src/app/nativeWindow.ts`:

```ts
export const windowControls = {
  startDragging: () => currentWindow().startDragging(),
  minimize: () => currentWindow().minimize(),
  toggleMaximize: () => currentWindow().toggleMaximize(),
  close: () => currentWindow().close(),
};
```

- [ ] **Step 2: Make the entire topbar the drag surface**

Add a typed handler in `src/app/App.tsx` and attach it to the header:

```ts
import type { MouseEvent } from 'react';
import { windowControls } from './nativeWindow';

function startWindowDrag(event: MouseEvent<HTMLElement>) {
  if (event.button !== 0) return;
  if ((event.target as HTMLElement).closest('.window-controls')) return;
  void windowControls.startDragging().catch(() => undefined);
}
```

```tsx
<header
  className="app-topbar"
  data-testid="window-drag-region"
  onMouseDown={startWindowDrag}
>
```

Remove `data-tauri-drag-region` and the nested `.app-topbar__drag-region` wrapper so the brand, data path badge, and blank blue space all behave consistently. Keep `<WindowControls />` unchanged.

- [ ] **Step 3: Add the capability**

Insert `"core:window:allow-start-dragging"` into `src-tauri/capabilities/default.json` alongside the other integrated-window permissions.

- [ ] **Step 4: Remove obsolete narrow-region CSS**

Delete `.app-topbar__drag-region` rules, move its flexible spacing responsibility to `.brand-lockup`, and keep the actions group at the right:

```css
.brand-lockup {
  min-width: 140px;
  flex: 1 1 auto;
  user-select: none;
}

.app-topbar__actions {
  min-width: 0;
  display: flex;
  flex: 0 1 auto;
  align-self: stretch;
  align-items: center;
  gap: 12px;
}
```

At `max-width: 600px`, set `.brand-lockup { min-width: 72px; }` instead of styling the removed drag wrapper.

- [ ] **Step 5: Run the focused drag tests and verify GREEN**

Run:

```powershell
pnpm test -- src/app/App.test.tsx
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
```

Expected: all App tests pass and the desktop source contract reports `Desktop UX source contracts passed.`

### Task 3: Simplify the sidebar safety status

**Files:**
- Modify: `src/app/AppShell.tsx`
- Modify: `src/styles/theme.css`
- Test: `src/app/AppShell.test.tsx`

- [ ] **Step 1: Replace the technical two-line copy**

Use one status element in `src/app/AppShell.tsx`:

```tsx
<div className="side-navigation__security">
  <span className="status-dot" aria-hidden="true" />
  <strong>本地安全保护已开启</strong>
</div>
```

- [ ] **Step 2: Keep the compact row readable**

Update the status CSS:

```css
.side-navigation__security {
  display: flex;
  align-items: center;
  gap: 9px;
  margin-top: auto;
  padding: 13px 9px 4px;
  border-top: 1px solid rgb(120 136 158 / 28%);
  white-space: nowrap;
}

.side-navigation__security strong {
  min-width: 0;
  overflow: hidden;
  color: #1f3554;
  font-size: 0.78rem;
  text-overflow: ellipsis;
}
```

Preserve the existing green status dot and the existing responsive rule that hides this row when navigation changes to the compact horizontal layout.

- [ ] **Step 3: Run the focused copy test and verify GREEN**

Run:

```powershell
pnpm test -- src/app/AppShell.test.tsx
```

Expected: all AppShell tests pass; the new single-line text is present and `WebView` is absent.

### Task 4: Verify and create the r6 local test build

**Files:**
- Verify: all files modified by Tasks 1–3
- Create locally: `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r6/`
- Create locally: `artifacts/local-test/QingzhouSSH-v0.1.5-local.20260804-r6-windows-x86_64-portable.zip`

- [ ] **Step 1: Run the complete frontend suite and production build**

Run:

```powershell
pnpm test
pnpm build
```

Expected: every Vitest test passes and the Vite production build exits with code 0.

- [ ] **Step 2: Run desktop and data-location contracts**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\desktop-ux.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\dev-env.tests.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\tests\local-build.tests.ps1
```

Expected: all scripts exit with code 0 and confirm build/test data remains inside the D-drive project.

- [ ] **Step 3: Run Rust checks**

Run:

```powershell
cargo fmt --check --manifest-path .\src-tauri\Cargo.toml
cargo test --locked --manifest-path .\src-tauri\Cargo.toml --all-targets
```

Expected: formatting is clean and all non-live Rust tests pass; fixture-dependent live tests remain ignored by design.

- [ ] **Step 4: Build the r6 package without overwriting r5**

Run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-local-test.ps1 -PackageVersion '0.1.5-local.20260804-r6'
```

Expected: the expanded package and ZIP are created only below `D:\Codex Project\轻量化SSH快捷工具\artifacts\local-test`.

- [ ] **Step 5: Inspect the executable and produce hashes**

Confirm the EXE exists, its sibling `data` directory is inside the r6 folder, and calculate SHA-256 for the EXE and ZIP with `Get-FileHash`. Do not publish or upload either artifact.

## Self-review

- Spec coverage: complete topbar drag, control exclusion, simplified copy, local-only r6 packaging, and D-drive containment are each covered by a task.
- Placeholder scan: no deferred implementation markers are present.
- Type consistency: `windowControls.startDragging` is defined once and used by both production code and the mocked test API.
