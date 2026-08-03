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

if (($env:Path -split ';') -notcontains $env:PNPM_HOME) {
  $env:Path = "$env:PNPM_HOME;$env:Path"
}

if (-not $Quiet) {
  Write-Host "QingzhouSSH development environment: $projectRoot"
  Write-Host "Cargo home: $env:CARGO_HOME"
  Write-Host "Data root:  $env:QINGZHOU_DATA_ROOT"
}
