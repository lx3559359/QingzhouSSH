$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$productionCheck = Join-Path $projectRoot 'scripts\assert-tauri-production-build.ps1'
$localBuilder = Join-Path $projectRoot 'scripts\build-local-test.ps1'

if (-not (Test-Path -LiteralPath $productionCheck -PathType Leaf)) {
  throw 'Production-build verifier is missing'
}
if (-not (Test-Path -LiteralPath $localBuilder -PathType Leaf)) {
  throw 'Local-test builder is missing'
}

$builderText = Get-Content -Raw -Encoding utf8 $localBuilder
if ($builderText -notmatch 'pnpm\s+tauri\s+build\s+--no-bundle') {
  throw 'Local test packages must be built through Tauri with embedded frontend assets'
}
if ($builderText -match 'cargo\s+build') {
  throw 'Local test builder must not call cargo build directly'
}
if ($builderText -notmatch 'assert-tauri-production-build\.ps1') {
  throw 'Local test builder must verify the Tauri production build before packaging'
}

$testRoot = Join-Path $projectRoot '.local\local-build-test'
$targetRoot = Join-Path $testRoot 'target'
$releaseRoot = Join-Path $targetRoot 'release'
$executable = Join-Path $releaseRoot 'qingzhou-ssh.exe'
$dependencyFile = Join-Path $releaseRoot 'qingzhou-ssh.d'
$buildId = 'qingzhou-ssh-deadbeef'
$buildRoot = Join-Path $releaseRoot "build\$buildId"
$buildOutput = Join-Path $buildRoot 'output'
$assetRoot = Join-Path $buildRoot 'out\tauri-codegen-assets'

New-Item -ItemType Directory -Force -Path $releaseRoot, $buildRoot | Out-Null
[IO.File]::WriteAllBytes($executable, [byte[]](77, 90, 1, 2, 3, 4))
[IO.File]::WriteAllText(
  $dependencyFile,
  "target\release\qingzhou-ssh.exe: target\release\build\$buildId\out\asset-marker`n",
  [Text.UTF8Encoding]::new($false)
)

try {
  [IO.File]::WriteAllText(
    $buildOutput,
    "cargo:rustc-check-cfg=cfg(dev)`ncargo:rustc-cfg=dev`n",
    [Text.UTF8Encoding]::new($false)
  )

  $rejected = $false
  try {
    & $productionCheck -ExecutablePath $executable -TargetDirectory $targetRoot | Out-Null
  } catch {
    $rejected = $_.Exception.Message -match 'development'
  }
  if (-not $rejected) { throw 'Development-mode Tauri executable was not rejected' }

  New-Item -ItemType Directory -Force -Path $assetRoot | Out-Null
  [IO.File]::WriteAllText(
    (Join-Path $assetRoot 'index.html'),
    '<!doctype html>',
    [Text.UTF8Encoding]::new($false)
  )
  [IO.File]::WriteAllText(
    $buildOutput,
    "cargo:rustc-check-cfg=cfg(dev)`n",
    [Text.UTF8Encoding]::new($false)
  )

  & $productionCheck -ExecutablePath $executable -TargetDirectory $targetRoot | Out-Null
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    $projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedTestRoot.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
      throw 'Refusing to clean local-build test data outside the project'
    }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
  }
}

Write-Output 'PASS: local packages require an embedded Tauri production frontend'
