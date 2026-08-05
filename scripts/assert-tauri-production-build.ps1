param(
  [Parameter(Mandatory = $true)]
  [string]$ExecutablePath,

  [string]$TargetDirectory
)

$ErrorActionPreference = 'Stop'

function Assert-UnderProject {
  param(
    [Parameter(Mandatory = $true)] [string]$Label,
    [Parameter(Mandatory = $true)] [string]$Path,
    [Parameter(Mandatory = $true)] [string]$ProjectRoot
  )

  $resolved = [IO.Path]::GetFullPath($Path)
  $prefix = $ProjectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$Label must remain inside the project folder: $resolved"
  }
  return $resolved
}

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$resolvedExecutable = Assert-UnderProject -Label 'Executable' -Path $ExecutablePath -ProjectRoot $projectRoot
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
  throw "Executable was not found: $resolvedExecutable"
}

if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
  $TargetDirectory = Split-Path (Split-Path $resolvedExecutable -Parent) -Parent
}
$resolvedTarget = Assert-UnderProject -Label 'Cargo target directory' -Path $TargetDirectory -ProjectRoot $projectRoot

$dependencyFile = [IO.Path]::ChangeExtension($resolvedExecutable, '.d')
if (-not (Test-Path -LiteralPath $dependencyFile -PathType Leaf)) {
  throw "Tauri build provenance was not found: $dependencyFile"
}

$dependencyText = Get-Content -Raw -Encoding utf8 $dependencyFile
$matches = [regex]::Matches(
  $dependencyText,
  'target[\\/]release[\\/]build[\\/](qingzhou-ssh-[0-9a-f]+)[\\/]out[\\/]'
)
$buildIds = @($matches | ForEach-Object { $_.Groups[1].Value } | Select-Object -Unique)
if ($buildIds.Count -ne 1) {
  throw 'Tauri build provenance does not identify exactly one application build'
}

$buildRoot = Join-Path $resolvedTarget ("release\build\" + $buildIds[0])
$buildOutput = Join-Path $buildRoot 'output'
if (-not (Test-Path -LiteralPath $buildOutput -PathType Leaf)) {
  throw "Tauri build output was not found: $buildOutput"
}

$buildOutputText = Get-Content -Raw -Encoding utf8 $buildOutput
if ($buildOutputText -match '(?m)^cargo:rustc-cfg=dev\r?$') {
  throw 'Refusing to package a Tauri development-mode executable'
}
if ($buildOutputText -notmatch '(?m)^cargo:rustc-check-cfg=cfg\(dev\)\r?$') {
  throw 'Tauri production marker is missing from build provenance'
}

$assetRoot = Join-Path $buildRoot 'out\tauri-codegen-assets'
$assets = @(Get-ChildItem -LiteralPath $assetRoot -File -ErrorAction SilentlyContinue)
if ($assets.Count -eq 0) {
  throw 'Embedded frontend assets are missing from the Tauri production build'
}

Write-Output "PASS: production Tauri assets are embedded in $resolvedExecutable"
