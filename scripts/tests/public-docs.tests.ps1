$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$requiredFiles = @(
  'LICENSE',
  'SECURITY.md',
  'README.md',
  'docs\user-guide.md',
  'docs\support-matrix.md',
  'docs\data-and-updates.md',
  'docs\security.md'
)
foreach ($relative in $requiredFiles) {
  if (-not (Test-Path -LiteralPath (Join-Path $projectRoot $relative) -PathType Leaf)) { throw "Public documentation is missing: $relative" }
}

$license = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'LICENSE')
if ($license -notmatch 'Apache License\s+Version 2\.0') { throw 'Apache-2.0 license text is incomplete' }

$readme = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'README.md')
foreach ($required in @('No SSH Terminal', 'Log search', 'Workflow', 'currentUser', 'portable.flag', 'Online update', 'github.com/lx3559359/QingzhouSSH', 'modelscope.cn', 'Data root')) {
  if (-not $readme.Contains($required)) { throw "README is missing public product information: $required" }
}
if ($readme -match 'TODO|TBD|placeholder|Milestone 3') { throw 'README still describes an incomplete milestone or placeholder' }

$support = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'docs\support-matrix.md')
foreach ($required in @('Windows 10', 'Windows 11', 'x86_64', 'Ubuntu', 'Debian', 'openEuler', 'UOS', 'Kylin', 'Anolis', 'Rocky', 'Auto detection')) {
  if (-not $support.Contains($required)) { throw "Support matrix is missing: $required" }
}

$guide = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'docs\user-guide.md')
foreach ($required in @('Host key', 'Log search and download', 'Quick tasks', 'SFTP', 'Workflow', 'Dangerous actions', 'PTY')) {
  if (-not $guide.Contains($required)) { throw "User guide is missing: $required" }
}

$data = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'docs\data-and-updates.md')
foreach ($required in @('currentUser', 'portable.flag', '<data-root>\updates', 'GitHub', 'ModelScope', 'Tauri updater signature', 'SHA-256', 'Rollback', 'Uninstall', 'D:\Codex Project\')) {
  if (-not $data.Contains($required)) { throw "Data/update guide is missing: $required" }
}

$security = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'SECURITY.md')
foreach ($required in @('GitHub Security Advisory', 'Do not', 'Updater signature', 'Authenticode', 'Host key')) {
  if (-not $security.Contains($required)) { throw "Security policy is missing: $required" }
}
$technicalSecurity = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'docs\security.md')
if ($technicalSecurity -match 'Milestone 4|M4.*unfinished') { throw 'Technical security documentation still claims M4 is unfinished' }

Write-Output 'PASS: public documentation covers product, support, data, updates and security'
