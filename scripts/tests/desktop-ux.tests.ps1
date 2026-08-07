$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$theme = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'src\styles\theme.css')
$workflowTheme = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'src\features\workflows\workflow.css')
$entrypoint = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'src-tauri\src\main.rs')
$windowSource = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'src-tauri\src\window.rs')
$appShellSource = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'src\app\AppShell.tsx')
$capabilities = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'src-tauri\capabilities\default.json') | ConvertFrom-Json

if ($theme -notmatch '(?s)\.app-window\s*\{.*?width:\s*100%') {
  throw 'Desktop app window must grow with the resized native window.'
}
if ($theme -match '(?s)\.app-window\s*\{.*?width:\s*min\(1220px') {
  throw 'Desktop app window must not retain the legacy 1220px width cap.'
}
if ($theme -notmatch '(?s)\.dashboard-page,.*?\.history-page\s*\{.*?width:\s*100%') {
  throw 'Feature pages must use the available workspace width.'
}
if ($workflowTheme -notmatch '(?s)\.workflow-page\s*\{.*?width:\s*100%') {
  throw 'Workflow page must use the available workspace width.'
}
if ($entrypoint -notmatch 'windows_subsystem\s*=\s*"windows"') {
  throw 'Windows release entrypoint must use the GUI subsystem.'
}
if ($windowSource -notmatch '\.decorations\(false\)') {
  throw 'The main window must not render the native title bar.'
}
if ($windowSource -notmatch '\.resizable\(true\)') {
  throw 'The frameless main window must remain resizable.'
}
foreach ($permission in @(
  'core:window:allow-start-dragging',
  'core:window:allow-minimize',
  'core:window:allow-toggle-maximize',
  'core:window:allow-close'
)) {
  if ($capabilities.permissions -notcontains $permission) {
    throw "Missing integrated title-bar permission: $permission"
  }
}
if ($theme -notmatch '(?s)html,\s*body,\s*#root\s*\{.*?height:\s*100%.*?overflow:\s*hidden') {
  throw 'The document root must fill the WebView without creating an outer page scrollbar.'
}
if ($theme -notmatch '(?s)\.app-shell\s*\{.*?height:\s*100dvh.*?padding:\s*0') {
  throw 'The WebView shell must be flush with the native client area.'
}
if ($theme -notmatch '(?s)\.app-window\s*\{.*?border:\s*0.*?border-radius:\s*0.*?box-shadow:\s*none') {
  throw 'The WebView must not draw a second rounded desktop window inside the native frame.'
}
if ($theme -notmatch '(?s)\.workspace-content\s*\{.*?overflow-y:\s*auto') {
  throw 'Feature content must scroll inside the fixed application shell.'
}
if ($theme -notmatch '(?s)\.transfer-page\s*\{.*?container-type:\s*inline-size') {
  throw 'The SFTP page must respond to its actual content width after navigation is accounted for.'
}
if ($theme -notmatch '(?s)@container\s+transfer-page\s*\(max-width:\s*920px\).*?\.sftp-workspace\s*\{.*?grid-template-areas:\s*"local remote"\s*"actions actions"') {
  throw 'A reduced content area must keep both SFTP panes readable and move actions into their own row.'
}
if ($theme -notmatch '(?s)@container\s+transfer-page\s*\(max-width:\s*620px\).*?\.sftp-workspace\s*\{.*?grid-template-areas:\s*"local"\s*"actions"\s*"remote"') {
  throw 'Very compact content areas must stack the complete SFTP workflow.'
}
if ($appShellSource -notmatch "lazy\(\(\)\s*=>\s*import\('") {
  throw 'Feature pages must be loaded on demand instead of being bundled into the initial desktop view.'
}
if ($appShellSource -notmatch '<Suspense\s+fallback=') {
  throw 'Lazy feature pages must show an explicit loading state.'
}
foreach ($eagerFeatureImport in @(
  'ServerListPage',
  'TaskPage',
  'LogSearchPage',
  'FileTransferPage',
  'DownloadsPage',
  'ExecutionHistoryPage',
  'WorkflowPage',
  'SettingsPage'
)) {
  if ($appShellSource -match "import\s+\{\s*$eagerFeatureImport\s*\}\s+from") {
    throw "Feature page remains eagerly imported: $eagerFeatureImport"
  }
}

Write-Host 'Desktop UX source contracts passed.'
