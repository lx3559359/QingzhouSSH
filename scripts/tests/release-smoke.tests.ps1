$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$helperPath = Join-Path $projectRoot 'scripts\lib\release-smoke.ps1'
$helper = Get-Content -Raw -Encoding utf8 $helperPath
$smokePath = Join-Path $projectRoot 'scripts\smoke-release.ps1'
$smoke = Get-Content -Raw -Encoding utf8 $smokePath

if ($helper -match '(?m)Get-ChildItem[^\r\n]*\s-File(?:\s|$)') {
  throw 'Release smoke must not depend on the FileSystem-only Get-ChildItem -File dynamic parameter'
}
if ($helper -notmatch '(?s)Get-ChildItem[^\r\n]+-Recurse\s*\|\s*Where-Object\s*\{.{0,200}?-not\s+\$_\.PSIsContainer') {
  throw 'Release smoke must filter installed executables with PSIsContainer'
}
if ($smoke -notmatch 'Stop-ReleaseProcessTree\s+-RootProcessId\s+\$process\.Id') {
  throw 'Release smoke must stop the application and its WebView child process tree'
}
if ($smoke -notmatch 'Stop-ReleaseProcessTree\s+-RootProcessId\s+\$process\.Id\s+-ExpectedDataRoot\s+\$ExpectedDataRoot') {
  throw 'Release smoke must identify detached WebView processes through the smoke data root'
}
if ($smoke -match 'Stop-Process\s+-Id\s+\$process\.Id') {
  throw 'Release smoke must not stop only the parent application process'
}
if ($smoke -match 'Select-Object\s+-Reverse') {
  throw 'Release smoke must use PowerShell 5.1-compatible process ordering'
}
if ($smoke -notmatch "(?s)Name\s+-eq\s+'msedgewebview2\.exe'.{0,500}?CommandLine") {
  throw 'Release smoke must stop WebView processes that still reference the isolated smoke profile'
}
if ($smoke -notmatch 'Remove-SmokeDirectoryWithRetry\s+-Path\s+\$smokeRoot') {
  throw 'Release smoke cleanup must retry while WebView releases its profile locks'
}

. $helperPath
$testRoot = Join-Path $projectRoot ('.local\release-smoke-helper-test-' + [Guid]::NewGuid().ToString('N'))
$nestedRoot = Join-Path $testRoot 'nested'
New-Item -ItemType Directory -Force -Path $nestedRoot | Out-Null
try {
  [IO.File]::WriteAllBytes((Join-Path $testRoot 'uninstall.exe'), [byte[]](1))
  $expected = Join-Path $nestedRoot 'QingzhouSSH.exe'
  [IO.File]::WriteAllBytes($expected, [byte[]](2))
  $actual = Find-InstalledExecutable ('"' + $testRoot + '"')
  if ($null -eq $actual -or $actual.FullName -ne $expected) {
    throw 'Installed executable discovery did not ignore the uninstaller and find the nested application'
  }
} finally {
  if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
}

Write-Output 'PASS: release smoke uses provider-compatible installed-file discovery'
