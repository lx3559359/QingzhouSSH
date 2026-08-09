$ErrorActionPreference = 'Stop'
$pathComparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$builder = Join-Path $projectRoot 'scripts\build-multiplatform-release.ps1'
if (-not (Test-Path -LiteralPath $builder -PathType Leaf)) {
  throw 'Multiplatform release builder is missing'
}
$testRoot = Join-Path $projectRoot '.local\multiplatform-release-test'
$nativeRoot = Join-Path $testRoot 'native'
$releaseRoot = Join-Path $testRoot 'release'
$version = [string](Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json).version
$fixturePublicKey = 'RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3'
$fixtureSignature = "untrusted comment: signature from minisign secret key`nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=`ntrusted comment: timestamp:1633700835`tfile:test`tprehashed`nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ=="
$encodedSignature = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($fixtureSignature))
$platforms = @(
  'windows-x86_64-nsis',
  'windows-aarch64-nsis',
  'macos-x86_64-dmg',
  'macos-aarch64-dmg',
  'linux-x86_64-appimage',
  'linux-aarch64-appimage'
)

try {
  foreach ($platform in $platforms) {
    $root = Join-Path $nativeRoot $platform
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $installerExtension = if ($platform -like 'windows-*') { '.exe' } elseif ($platform -like 'macos-*') { '.dmg' } else { '.AppImage' }
    $installerName = "QingzhouSSH_${version}_${platform}$installerExtension"
    $updaterName = if ($platform -like 'macos-*') { "QingzhouSSH_${version}_${platform}.app.tar.gz" } else { $installerName }
    $signatureName = "$updaterName.sig"
    [IO.File]::WriteAllBytes((Join-Path $root $installerName), [Text.UTF8Encoding]::new($false).GetBytes('test'))
    if ($updaterName -ne $installerName) {
      [IO.File]::WriteAllBytes((Join-Path $root $updaterName), [Text.UTF8Encoding]::new($false).GetBytes('test'))
    }
    [IO.File]::WriteAllText((Join-Path $root $signatureName), $encodedSignature, [Text.UTF8Encoding]::new($false))
    $files = @(
      [ordered]@{ name = $installerName; role = if ($updaterName -eq $installerName) { 'installer-updater' } else { 'installer' } },
      [ordered]@{ name = $signatureName; role = 'updater-signature' }
    )
    if ($updaterName -ne $installerName) { $files += [ordered]@{ name = $updaterName; role = 'updater' } }
    if ($platform -like 'windows-*') {
      $portableName = "QingzhouSSH-v${version}-${platform}-portable.zip"
      [IO.File]::WriteAllBytes((Join-Path $root $portableName), [byte[]](1..32))
      $files += [ordered]@{ name = $portableName; role = 'portable' }
    }
    $descriptor = [ordered]@{
      schemaVersion = 1
      project = 'QingzhouSSH'
      version = $version
      platform = $platform
      installerFile = $installerName
      updaterFile = $updaterName
      signatureFile = $signatureName
      files = $files
    }
    [IO.File]::WriteAllText(
      (Join-Path $root 'platform-artifact.json'),
      (($descriptor | ConvertTo-Json -Depth 10) + "`n"),
      [Text.UTF8Encoding]::new($false)
    )
  }

  & $builder `
    -NativeArtifactDirectory $nativeRoot `
    -OutputDirectory $releaseRoot `
    -ModelScopeNamespace 'lx3559359' `
    -PublishedAt '2026-08-08T10:00:00Z' `
    -ReleaseNotes 'Six-platform fixture' | Out-Null

  $metadata = Get-Content -Raw -Encoding utf8 (Join-Path $releaseRoot 'release-metadata.json') | ConvertFrom-Json
  if ($metadata.schemaVersion -ne 2 -or $metadata.platform -ne 'multi' -or @($metadata.platforms).Count -ne 6) {
    throw 'Multiplatform metadata contract is incomplete'
  }
  $license = @($metadata.files | Where-Object { $_.platform -eq 'all' -and $_.role -eq 'license' -and $_.name -eq 'LICENSE' })
  if ($license.Count -ne 1 -or -not (Test-Path -LiteralPath (Join-Path $releaseRoot 'LICENSE') -PathType Leaf)) {
    throw 'Multiplatform release must include the Apache-2.0 license'
  }
  $licenseBytes = [IO.File]::ReadAllBytes((Join-Path $releaseRoot 'LICENSE'))
  if ($licenseBytes -contains 13) {
    throw 'Published LICENSE must use canonical LF line endings for byte-identical release mirrors'
  }
  if ($licenseBytes.Length -ge 3 -and $licenseBytes[0] -eq 0xEF -and $licenseBytes[1] -eq 0xBB -and $licenseBytes[2] -eq 0xBF) {
    throw 'Published LICENSE must use UTF-8 without a BOM'
  }
  $github = Get-Content -Raw -Encoding utf8 (Join-Path $releaseRoot 'github\latest.json') | ConvertFrom-Json
  $modelscope = Get-Content -Raw -Encoding utf8 (Join-Path $releaseRoot 'modelscope\latest.json') | ConvertFrom-Json
  foreach ($platform in $platforms) {
    foreach ($manifest in @($github, $modelscope)) {
      $entry = $manifest.platforms.$platform
      if ($null -eq $entry -or $entry.sha256 -notmatch '^[0-9a-f]{64}$' -or [int64]$entry.size -ne 4) {
        throw "Manifest entry is invalid for $platform"
      }
    }
  }
  if ($github.platforms.'macos-aarch64-dmg'.url -notlike '*.app.tar.gz') {
    throw 'macOS manifest must reference the signed app updater archive'
  }
  & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $releaseRoot -UpdaterPublicKey $fixturePublicKey | Out-Null

  $githubReadback = Join-Path $testRoot 'readback\github'
  $modelscopeReadback = Join-Path $testRoot 'readback\modelscope'
  New-Item -ItemType Directory -Force -Path $githubReadback, $modelscopeReadback | Out-Null
  $commonFiles = @($metadata.files | ForEach-Object name) + @('SHA256SUMS', 'SBOM.spdx.json', 'release-metadata.json')
  foreach ($name in $commonFiles) {
    Copy-Item -LiteralPath (Join-Path $releaseRoot $name) -Destination (Join-Path $githubReadback $name)
    Copy-Item -LiteralPath (Join-Path $releaseRoot $name) -Destination (Join-Path $modelscopeReadback $name)
  }
  Copy-Item -LiteralPath (Join-Path $releaseRoot 'github\latest.json') -Destination (Join-Path $githubReadback 'latest.json')
  Copy-Item -LiteralPath (Join-Path $releaseRoot 'modelscope\latest.json') -Destination (Join-Path $modelscopeReadback 'latest.json')
  & (Join-Path $projectRoot 'scripts\compare-release-sources.ps1') `
    -ReleaseDirectory $releaseRoot `
    -GitHubDirectory $githubReadback `
    -ModelScopeDirectory $modelscopeReadback `
    -UpdaterPublicKey $fixturePublicKey | Out-Null

  $linuxUpdater = [string](@($metadata.platforms | Where-Object platform -eq 'linux-aarch64-appimage')[0].updaterFile)
  [IO.File]::WriteAllBytes((Join-Path $releaseRoot $linuxUpdater), [byte[]](9, 9, 9))
  $tamperRejected = $false
  try {
    & (Join-Path $projectRoot 'scripts\verify-release.ps1') -ReleaseDirectory $releaseRoot -UpdaterPublicKey $fixturePublicKey | Out-Null
  } catch {
    $tamperRejected = $true
  }
  if (-not $tamperRejected) { throw 'Tampered non-Windows updater must be rejected' }
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolved.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, $pathComparison)) {
      throw 'Refusing to clean multiplatform fixtures outside the project'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
}

Write-Output 'PASS: multiplatform manifests and every signed updater are verified'
