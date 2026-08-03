$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

. (Join-Path $repoRoot 'scripts\dev-env.ps1') -Quiet

$paths = @(
  $env:CARGO_HOME,
  $env:RUSTUP_HOME,
  $env:QINGZHOU_PERL_HOME,
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

if ($env:CARGO_TARGET_DIR -match '[^\x00-\x7F]') {
  throw "Cargo target alias must be ASCII-only for vendored OpenSSL: $env:CARGO_TARGET_DIR"
}
$asciiProjectAncestor = $repoRoot
while ($asciiProjectAncestor -match '[^\x00-\x7F]') {
  $asciiProjectAncestor = Split-Path -Parent $asciiProjectAncestor
}
$expectedAliasRoot = Join-Path $asciiProjectAncestor '.qingzhou-ssh-build'
if (-not $env:CARGO_TARGET_DIR.StartsWith("$expectedAliasRoot\", [StringComparison]::OrdinalIgnoreCase)) {
  throw "Cargo target alias escaped the D-drive project folder: $env:CARGO_TARGET_DIR"
}
$targetAlias = Get-Item -LiteralPath $env:CARGO_TARGET_DIR
$expectedTarget = Join-Path $repoRoot 'target'
if (-not ($targetAlias.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
  throw "Cargo target path is not a junction: $env:CARGO_TARGET_DIR"
}
$linkedTarget = [IO.Path]::GetFullPath([string]$targetAlias.Target)
if ($linkedTarget -ne [IO.Path]::GetFullPath($expectedTarget)) {
  throw "Cargo target junction escaped repository: $linkedTarget"
}

$cargoBin = Join-Path $env:CARGO_HOME 'bin'
if (($env:Path -split ';') -notcontains $cargoBin) {
  throw "Cargo bin is missing from PATH: $cargoBin"
}

if (-not (Get-Command perl -ErrorAction SilentlyContinue)) {
  throw 'Perl is unavailable for the vendored OpenSSL build'
}
perl -MLocale::Maketext::Simple -e 1
if ($LASTEXITCODE -ne 0) {
  throw 'Perl lacks modules required by the vendored OpenSSL build'
}
if (-not (Get-Command nasm -ErrorAction SilentlyContinue)) {
  throw 'NASM is unavailable for the vendored OpenSSL build'
}

Write-Host 'PASS: all controllable development paths are project-local'
