$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$testRoot = Join-Path $projectRoot '.local\release-artifacts-test'
$inputRoot = Join-Path $testRoot 'input'
$outputRoot = Join-Path $testRoot 'output'
New-Item -ItemType Directory -Force -Path $inputRoot | Out-Null

$installer = Join-Path $inputRoot 'QingzhouSSH_0.1.0_x64-setup.exe'
$updater = Join-Path $inputRoot 'QingzhouSSH_0.1.0_x64-setup.nsis.zip'
$signature = "$updater.sig"
$portable = Join-Path $inputRoot 'QingzhouSSH-v0.1.0-windows-x86_64-portable.zip'
[IO.File]::WriteAllBytes($installer, [byte[]](1..32))
[IO.File]::WriteAllBytes($updater, [byte[]](33..96))
[IO.File]::WriteAllText($signature, [Convert]::ToBase64String([byte[]](1..96)), [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllBytes($portable, [byte[]](97..128))

try {
  $outsideRejected = $false
  try {
    & (Join-Path $projectRoot 'scripts\build-release.ps1') `
      -InstallerPath $installer `
      -UpdaterArchivePath $updater `
      -UpdaterSignaturePath $signature `
      -PortableArchivePath $portable `
      -OutputDirectory 'D:\qingzhou-release-outside-test' `
      -ModelScopeNamespace 'domestic-user' `
      -PublishedAt '2026-08-04T10:00:00Z' | Out-Null
  } catch {
    $outsideRejected = $true
  }
  if (-not $outsideRejected) { throw 'Release output outside the project must be rejected' }
  if (Test-Path -LiteralPath 'D:\qingzhou-release-outside-test') { throw 'Outside release path was created' }

  & (Join-Path $projectRoot 'scripts\build-release.ps1') `
    -InstallerPath $installer `
    -UpdaterArchivePath $updater `
    -UpdaterSignaturePath $signature `
    -PortableArchivePath $portable `
    -OutputDirectory $outputRoot `
    -ModelScopeNamespace 'domestic-user' `
    -PublishedAt '2026-08-04T10:00:00Z' `
    -ReleaseNotes 'Release contract fixture' | Out-Null

  $githubManifestPath = Join-Path $outputRoot 'github\latest.json'
  $modelscopeManifestPath = Join-Path $outputRoot 'modelscope\latest.json'
  $metadataPath = Join-Path $outputRoot 'release-metadata.json'
  $sumsPath = Join-Path $outputRoot 'SHA256SUMS'
  $sbomPath = Join-Path $outputRoot 'SBOM.spdx.json'
  foreach ($required in @($githubManifestPath, $modelscopeManifestPath, $metadataPath, $sumsPath, $sbomPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing release file: $required" }
  }

  $github = Get-Content -Raw -Encoding utf8 $githubManifestPath | ConvertFrom-Json
  $modelscope = Get-Content -Raw -Encoding utf8 $modelscopeManifestPath | ConvertFrom-Json
  $githubPlatform = $github.platforms.'windows-x86_64'
  $modelscopePlatform = $modelscope.platforms.'windows-x86_64'
  $expectedUpdaterHash = (Get-FileHash -LiteralPath $updater -Algorithm SHA256).Hash.ToLowerInvariant()
  $expectedSignature = (Get-Content -Raw -Encoding utf8 $signature).Trim()

  if ($github.version -ne '0.1.0' -or $modelscope.version -ne '0.1.0') { throw 'Manifest version is not synchronized' }
  if ($github.pub_date -ne '2026-08-04T10:00:00Z') { throw 'Published time was not preserved' }
  if ($githubPlatform.url -ne 'https://github.com/lx3559359/QingzhouSSH/releases/download/v0.1.0/QingzhouSSH_0.1.0_x64-setup.nsis.zip') {
    throw 'GitHub updater URL is not pinned to the public release'
  }
  if ($modelscopePlatform.url -ne 'https://modelscope.cn/api/v1/studios/domestic-user/QingzhouSSH/repo?Revision=master&FilePath=releases%2Fv0.1.0%2FQingzhouSSH_0.1.0_x64-setup.nsis.zip') {
    throw 'ModelScope updater URL is not pinned to the mirrored release'
  }
  foreach ($platform in @($githubPlatform, $modelscopePlatform)) {
    if ($platform.signature -ne $expectedSignature) { throw 'Updater signature was not copied exactly' }
    if ($platform.sha256 -ne $expectedUpdaterHash) { throw 'Updater SHA-256 is incorrect' }
    if ([int64]$platform.size -ne (Get-Item -LiteralPath $updater).Length) { throw 'Updater size is incorrect' }
  }
  if ($githubPlatform.build_id -ne $modelscopePlatform.build_id -or $githubPlatform.build_id -notmatch '^build-0\.1\.0-[0-9a-f]{12}$') {
    throw 'Dual-source build identifiers differ or are not reproducible'
  }

  $metadata = Get-Content -Raw -Encoding utf8 $metadataPath | ConvertFrom-Json
  $payloadNames = @($metadata.files | ForEach-Object name)
  foreach ($requiredName in @(
    (Split-Path $installer -Leaf),
    (Split-Path $updater -Leaf),
    (Split-Path $signature -Leaf),
    (Split-Path $portable -Leaf)
  )) {
    if ($payloadNames -notcontains $requiredName) { throw "Release metadata is missing $requiredName" }
  }

  $sums = Get-Content -Encoding utf8 $sumsPath
  foreach ($name in @($payloadNames + 'github/latest.json' + 'modelscope/latest.json' + 'release-metadata.json' + 'SBOM.spdx.json')) {
    if (-not ($sums -match "^[0-9a-f]{64}  $([regex]::Escape($name))$")) { throw "SHA256SUMS is missing $name" }
  }

  $sbom = Get-Content -Raw -Encoding utf8 $sbomPath | ConvertFrom-Json
  if ($sbom.spdxVersion -ne 'SPDX-2.3' -or $sbom.dataLicense -ne 'CC0-1.0') { throw 'SBOM is not SPDX 2.3 JSON' }
  if (@($sbom.packages | Where-Object name -eq 'react').Count -ne 1) { throw 'SBOM is missing npm dependencies' }
  if (@($sbom.packages | Where-Object name -eq 'tauri').Count -ne 1) { throw 'SBOM is missing Cargo dependencies' }
  if (@($sbom.files).Count -lt 4) { throw 'SBOM is missing release files' }

  & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $outputRoot | Out-Null

  $firstBuildHashes = @{}
  Get-ChildItem -LiteralPath $outputRoot -File -Recurse | ForEach-Object {
    $relative = $_.FullName.Substring($outputRoot.Length + 1).Replace('\', '/')
    $firstBuildHashes[$relative] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
  }
  & (Join-Path $projectRoot 'scripts\build-release.ps1') `
    -InstallerPath $installer `
    -UpdaterArchivePath $updater `
    -UpdaterSignaturePath $signature `
    -PortableArchivePath $portable `
    -OutputDirectory $outputRoot `
    -ModelScopeNamespace 'domestic-user' `
    -PublishedAt '2026-08-04T10:00:00Z' `
    -ReleaseNotes 'Release contract fixture' | Out-Null
  $secondFiles = @(Get-ChildItem -LiteralPath $outputRoot -File -Recurse)
  if ($secondFiles.Count -ne $firstBuildHashes.Count) { throw 'Repeated release build changed the file set' }
  foreach ($file in $secondFiles) {
    $relative = $file.FullName.Substring($outputRoot.Length + 1).Replace('\', '/')
    if (-not $firstBuildHashes.ContainsKey($relative) -or $firstBuildHashes[$relative] -ne (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash) {
      throw "Repeated release build is not reproducible: $relative"
    }
  }

  $updaterOutput = Join-Path $outputRoot (Split-Path $updater -Leaf)
  [IO.File]::WriteAllBytes($updaterOutput, [byte[]](9, 9, 9))
  $tamperRejected = $false
  try {
    & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $outputRoot | Out-Null
  } catch {
    $tamperRejected = $true
  }
  if (-not $tamperRejected) { throw 'Tampered updater archive must be rejected' }

  Copy-Item -LiteralPath $updater -Destination $updaterOutput -Force
  & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $outputRoot | Out-Null
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolvedTestRoot.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
      throw 'Refusing to clean a release test path outside the project'
    }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
  }
}

Write-Output 'PASS: release artifacts, manifests, hashes, signatures and SBOM are reproducible'
