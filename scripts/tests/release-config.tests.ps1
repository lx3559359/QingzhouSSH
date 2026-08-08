$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$tauri = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
$cargoText = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\Cargo.toml')
$release = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'release\release-config.json') | ConvertFrom-Json

$cargoVersion = [regex]::Match($cargoText, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $cargoVersion) { throw 'Cargo package version was not found' }
if ($package.version -ne $cargoVersion -or $package.version -ne $tauri.version) {
  throw "Version mismatch: package=$($package.version), cargo=$cargoVersion, tauri=$($tauri.version)"
}
if ($package.version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') {
  throw "Version is not SemVer: $($package.version)"
}
if ($release.projectName -ne 'QingzhouSSH') { throw 'Unexpected public project name' }
if ($release.github.repository -ne 'lx3559359/QingzhouSSH') { throw 'Unexpected GitHub repository' }
if ($release.modelscope.repository -ne 'QingzhouSSH') { throw 'Unexpected ModelScope repository' }
if ($release.modelscope.repositoryType -ne 'model') { throw 'ModelScope release mirror must support public single-file downloads' }
if ($release.modelscope.namespace -ne 'lx3559359') { throw 'Unexpected ModelScope namespace' }
if ($release.modelscope.namespaceEnvironment -ne 'QINGZHOU_MODELSCOPE_NAMESPACE') {
  throw 'ModelScope namespace must be supplied by a build environment variable'
}
$expectedPlatforms = [ordered]@{
  'windows-x86_64-nsis' = @('windows-2025', 'x86_64-pc-windows-msvc', 'nsis')
  'windows-aarch64-nsis' = @('windows-11-arm', 'aarch64-pc-windows-msvc', 'nsis')
  'macos-x86_64-dmg' = @('macos-15-intel', 'x86_64-apple-darwin', 'dmg')
  'macos-aarch64-dmg' = @('macos-15', 'aarch64-apple-darwin', 'dmg')
  'linux-x86_64-appimage' = @('ubuntu-22.04', 'x86_64-unknown-linux-gnu', 'appimage')
  'linux-aarch64-appimage' = @('ubuntu-22.04-arm', 'aarch64-unknown-linux-gnu', 'appimage')
}
foreach ($entry in $expectedPlatforms.GetEnumerator()) {
  $platform = $release.platforms.($entry.Key)
  if ($null -eq $platform) { throw "Release platform is missing: $($entry.Key)" }
  if ($platform.runner -ne $entry.Value[0] -or $platform.rustTarget -ne $entry.Value[1] -or $platform.bundle -ne $entry.Value[2]) {
    throw "Release platform contract differs: $($entry.Key)"
  }
}
if (@($release.platforms.PSObject.Properties).Count -ne $expectedPlatforms.Count) {
  throw 'Release platform contract contains undeclared targets'
}

$updateService = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\src\services\update_service.rs')
if ($updateService -notmatch 'option_env!\("QINGZHOU_MODELSCOPE_NAMESPACE"\)\.unwrap_or\("lx3559359"\)') {
  throw 'Local builds must retain the trusted public ModelScope namespace'
}

Write-Output "PASS: release contract is consistent for version $($package.version)"
