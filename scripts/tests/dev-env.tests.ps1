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

$expectedTarget = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target'))
$actualTarget = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
if ($actualTarget -ne $expectedTarget) {
  throw "Cargo target directory escaped repository: $actualTarget"
}
$targetItem = Get-Item -LiteralPath $actualTarget
if ($targetItem.Attributes -band [IO.FileAttributes]::ReparsePoint) {
  throw "Cargo target directory must be physical project storage: $actualTarget"
}

$cargoBin = Join-Path $env:CARGO_HOME 'bin'
if (($env:Path -split ';') -notcontains $cargoBin) {
  throw "Cargo bin is missing from PATH: $cargoBin"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw 'Cargo is unavailable after loading the project development environment'
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
