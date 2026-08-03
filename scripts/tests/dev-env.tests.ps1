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
