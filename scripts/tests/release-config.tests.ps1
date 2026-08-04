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
if ($release.platform -ne 'windows-x86_64') { throw 'Only the declared Windows x64 updater target is supported' }
if ($release.installer -ne 'nsis') { throw 'The public updater must use the NSIS installer' }

$updateService = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\src\services\update_service.rs')
if ($updateService -notmatch 'option_env!\("QINGZHOU_MODELSCOPE_NAMESPACE"\)\.unwrap_or\("lx3559359"\)') {
  throw 'Local builds must retain the trusted public ModelScope namespace'
}

Write-Output "PASS: release contract is consistent for version $($package.version)"
