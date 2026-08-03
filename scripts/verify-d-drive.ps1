$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$rootPrefix = $repoRoot.TrimEnd('\') + '\'
$repoDrive = Split-Path -Qualifier $repoRoot

$forbidden = @(
  (Join-Path $env:APPDATA 'QingzhouSSH'),
  (Join-Path $env:LOCALAPPDATA 'QingzhouSSH')
)
$before = @{}
foreach ($path in $forbidden) {
  $before[$path] = Test-Path -LiteralPath $path
}

. (Join-Path $PSScriptRoot 'dev-env.ps1') -Quiet

function Assert-UnderRepo([string]$name, [string]$path) {
  if ([string]::IsNullOrWhiteSpace($path)) {
    throw "$name is empty"
  }
  $full = [IO.Path]::GetFullPath($path)
  if ($full -ne $repoRoot -and -not $full.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$name escaped repository: $full"
  }
  if ((Split-Path -Qualifier $full) -ne $repoDrive) {
    throw "$name escaped the project drive: $full"
  }
}

function Assert-TargetJunction([string]$path) {
  $full = [IO.Path]::GetFullPath($path)
  if ((Split-Path -Qualifier $full) -ne $repoDrive) {
    throw "Cargo target alias escaped the project drive: $full"
  }
  $item = Get-Item -LiteralPath $full
  if (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw "Cargo target alias is not a junction: $full"
  }
  Assert-UnderRepo 'Cargo target junction destination' ([string]$item.Target)
}

$metadata = cargo metadata --manifest-path (Join-Path $repoRoot 'src-tauri\Cargo.toml') --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) {
  throw 'cargo metadata failed'
}
$pnpmStore = (pnpm store path).Trim()
if ($LASTEXITCODE -ne 0) {
  throw 'pnpm store path failed'
}

Assert-TargetJunction $metadata.target_directory
Assert-UnderRepo 'Cargo home' $env:CARGO_HOME
Assert-UnderRepo 'Rustup home' $env:RUSTUP_HOME
Assert-UnderRepo 'npm cache' $env:NPM_CONFIG_CACHE
Assert-UnderRepo 'Corepack home' $env:COREPACK_HOME
Assert-UnderRepo 'pnpm home' $env:PNPM_HOME
Assert-UnderRepo 'pnpm store' $pnpmStore
Assert-UnderRepo 'temporary directory' $env:TEMP
Assert-UnderRepo 'development data root' $env:QINGZHOU_DATA_ROOT
Assert-UnderRepo 'artifacts directory' $env:QINGZHOU_ARTIFACTS_DIR

$tauriConfig = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
if (@($tauriConfig.app.windows).Count -ne 0) {
  throw 'Static Tauri windows can create WebView2 data before root resolution'
}
$windowSource = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src-tauri\src\window.rs')
if ($windowSource -notmatch '\.data_directory\(' -or $windowSource -notmatch '\.incognito\(true\)') {
  throw 'Window builder must route persistent WebView2 data to the selected root and use incognito first-run mode'
}

foreach ($path in $forbidden) {
  if (-not $before[$path] -and (Test-Path -LiteralPath $path)) {
    throw "Development audit created forbidden AppData path: $path"
  }
  if ($before[$path]) {
    Write-Warning "Pre-existing path was not created by this audit: $path"
  }
}

Write-Host "PASS: controllable development and application paths remain on $repoDrive under the project"
