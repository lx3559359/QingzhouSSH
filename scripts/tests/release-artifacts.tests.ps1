$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$testRoot = Join-Path $projectRoot '.local\release-artifacts-test'
$inputRoot = Join-Path $testRoot 'input'
$outputRoot = Join-Path $testRoot 'output'
$version = [string](Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json).version
New-Item -ItemType Directory -Force -Path $inputRoot | Out-Null

$inputInstallerName = "轻舟 SSH_${version}_x64-setup.exe"
$publicInstallerName = "QingzhouSSH_${version}_x64-setup.exe"
$publicSignatureName = "$publicInstallerName.sig"
$installer = Join-Path $inputRoot $inputInstallerName
$signature = "$installer.sig"
$portable = Join-Path $inputRoot "QingzhouSSH-v${version}-windows-x86_64-portable.zip"
$fixturePublicKey = 'RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3'
$fixtureSignature = "untrusted comment: signature from minisign secret key`nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=`ntrusted comment: timestamp:1633700835`tfile:test`tprehashed`nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ=="
[IO.File]::WriteAllBytes($installer, [Text.UTF8Encoding]::new($false).GetBytes('test'))
[IO.File]::WriteAllText($signature, [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($fixtureSignature)), [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllBytes($portable, [byte[]](97..128))

try {
  $outsideRejected = $false
  try {
    & (Join-Path $projectRoot 'scripts\build-release.ps1') `
      -InstallerPath $installer `
      -UpdaterSignaturePath $signature `
      -PortableArchivePath $portable `
      -OutputDirectory 'D:\qingzhou-release-outside-test' `
      -ModelScopeNamespace 'lx3559359' `
      -PublishedAt '2026-08-04T10:00:00Z' | Out-Null
  } catch {
    $outsideRejected = $true
  }
  if (-not $outsideRejected) { throw 'Release output outside the project must be rejected' }
  if (Test-Path -LiteralPath 'D:\qingzhou-release-outside-test') { throw 'Outside release path was created' }

  & (Join-Path $projectRoot 'scripts\build-release.ps1') `
    -InstallerPath $installer `
    -UpdaterSignaturePath $signature `
    -PortableArchivePath $portable `
    -OutputDirectory $outputRoot `
    -ModelScopeNamespace 'lx3559359' `
    -PublishedAt '2026-08-04T10:00:00Z' `
    -ReleaseNotes 'Release contract fixture' | Out-Null

  $githubManifestPath = Join-Path $outputRoot 'github\latest.json'
  $modelscopeManifestPath = Join-Path $outputRoot 'modelscope\latest.json'
  $metadataPath = Join-Path $outputRoot 'release-metadata.json'
  $sumsPath = Join-Path $outputRoot 'SHA256SUMS'
  $sbomPath = Join-Path $outputRoot 'SBOM.spdx.json'
  foreach ($required in @($githubManifestPath, $modelscopeManifestPath, $metadataPath, $sumsPath, $sbomPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing release file: $required" }
    if ([IO.File]::ReadAllBytes($required) -contains 13) { throw "Release text files must use reproducible LF line endings: $required" }
  }

  $github = Get-Content -Raw -Encoding utf8 $githubManifestPath | ConvertFrom-Json
  $modelscope = Get-Content -Raw -Encoding utf8 $modelscopeManifestPath | ConvertFrom-Json
  $githubPlatform = $github.platforms.'windows-x86_64'
  $modelscopePlatform = $modelscope.platforms.'windows-x86_64'
  $expectedUpdaterHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
  $expectedSignature = (Get-Content -Raw -Encoding utf8 $signature).Trim()

  if ($github.version -ne $version -or $modelscope.version -ne $version) { throw 'Manifest version is not synchronized' }
  if ($github.pub_date -ne '2026-08-04T10:00:00Z') { throw 'Published time was not preserved' }
  $encodedInstallerName = [Uri]::EscapeDataString($publicInstallerName)
  if ($githubPlatform.url -ne "https://github.com/lx3559359/QingzhouSSH/releases/download/v$version/$encodedInstallerName") {
    throw 'GitHub updater URL is not pinned to the public release'
  }
  $encodedModelScopePath = [Uri]::EscapeDataString("releases/v$version/$publicInstallerName")
  if ($modelscopePlatform.url -ne "https://modelscope.cn/api/v1/studios/lx3559359/QingzhouSSH/repo?Revision=master&FilePath=$encodedModelScopePath") {
    throw 'ModelScope updater URL is not pinned to the mirrored release'
  }
  foreach ($platform in @($githubPlatform, $modelscopePlatform)) {
    if ($platform.signature -ne $expectedSignature) { throw 'Updater signature was not copied exactly' }
    if ($platform.sha256 -ne $expectedUpdaterHash) { throw 'Updater SHA-256 is incorrect' }
    if ([int64]$platform.size -ne (Get-Item -LiteralPath $installer).Length) { throw 'Updater size is incorrect' }
  }
  if ($githubPlatform.build_id -ne $modelscopePlatform.build_id -or $githubPlatform.build_id -notmatch "^build-$([regex]::Escape($version))-[0-9a-f]{12}$") {
    throw 'Dual-source build identifiers differ or are not reproducible'
  }

  $metadata = Get-Content -Raw -Encoding utf8 $metadataPath | ConvertFrom-Json
  if ($metadata.updaterFile -ne $publicInstallerName -or $metadata.signatureFile -ne $publicSignatureName) {
    throw 'Public installer and signature names must be stable ASCII names'
  }
  $payloadNames = @($metadata.files | ForEach-Object name)
  foreach ($requiredName in @(
    $publicInstallerName,
    $publicSignatureName,
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
  if (@($sbom.files).Count -lt 3) { throw 'SBOM is missing release files' }

  & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $outputRoot -UpdaterPublicKey $fixturePublicKey | Out-Null

  $firstBuildHashes = @{}
  Get-ChildItem -LiteralPath $outputRoot -File -Recurse | ForEach-Object {
    $relative = $_.FullName.Substring($outputRoot.Length + 1).Replace('\', '/')
    $firstBuildHashes[$relative] = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
  }
  & (Join-Path $projectRoot 'scripts\build-release.ps1') `
    -InstallerPath $installer `
    -UpdaterSignaturePath $signature `
    -PortableArchivePath $portable `
    -OutputDirectory $outputRoot `
    -ModelScopeNamespace 'lx3559359' `
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

  $updaterOutput = Join-Path $outputRoot $publicInstallerName
  [IO.File]::WriteAllBytes($updaterOutput, [byte[]](9, 9, 9))
  $tamperRejected = $false
  try {
    & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $outputRoot -UpdaterPublicKey $fixturePublicKey | Out-Null
  } catch {
    $tamperRejected = $true
  }
  if (-not $tamperRejected) { throw 'Tampered signed installer must be rejected' }

  Copy-Item -LiteralPath $installer -Destination $updaterOutput -Force
  & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $outputRoot -UpdaterPublicKey $fixturePublicKey | Out-Null
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
