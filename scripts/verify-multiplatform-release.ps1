param(
  [Parameter(Mandatory = $true)] [string]$ReleaseDirectory,
  [string]$UpdaterPublicKey
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$pathComparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
$releaseRoot = [IO.Path]::GetFullPath($ReleaseDirectory)
if (-not $releaseRoot.StartsWith($projectPrefix, $pathComparison)) { throw 'Release directory must remain inside the project folder' }
if (-not (Test-Path -LiteralPath $releaseRoot -PathType Container)) { throw 'Release directory was not found' }

function Get-LowerHash([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Read-Json([string]$RelativePath) {
  if ([IO.Path]::IsPathRooted($RelativePath) -or $RelativePath -match '(^|[\\/])\.\.?(?:[\\/]|$)') {
    throw "Unsafe release JSON path: $RelativePath"
  }
  $path = Join-Path $releaseRoot ($RelativePath.Replace('/', [IO.Path]::DirectorySeparatorChar))
  $resolved = [IO.Path]::GetFullPath($path)
  $releasePrefix = $releaseRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $resolved.StartsWith($releasePrefix, $pathComparison)) { throw "Release JSON path escapes the release folder: $RelativePath" }
  if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) { throw "Missing release file: $RelativePath" }
  return Get-Content -Raw -Encoding utf8 $resolved | ConvertFrom-Json
}

$metadata = Read-Json 'release-metadata.json'
$releaseConfig = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'release\release-config.json') | ConvertFrom-Json
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
if ($metadata.schemaVersion -ne 2 -or $metadata.project -ne $releaseConfig.projectName -or $metadata.platform -ne 'multi') {
  throw 'Multiplatform release metadata contract is invalid'
}
if ($metadata.version -ne $package.version -or $metadata.buildId -notmatch "^build-$([regex]::Escape($metadata.version))-multi-[0-9a-f]{12}$") {
  throw 'Multiplatform release identity is invalid'
}
if ($metadata.githubManifest -ne 'github/latest.json' -or $metadata.modelscopeManifest -ne 'modelscope/latest.json') {
  throw 'Multiplatform release manifest paths are invalid'
}

$configuredPlatforms = @($releaseConfig.platforms.PSObject.Properties.Name)
$platformRecords = @($metadata.platforms)
if ($platformRecords.Count -ne $configuredPlatforms.Count) { throw 'Release metadata platform count is incomplete' }
$actualPlatforms = @($platformRecords | ForEach-Object { [string]$_.platform })
$actualPlatformSet = (($actualPlatforms | Sort-Object) -join "`n")
$configuredPlatformSet = (($configuredPlatforms | Sort-Object) -join "`n")
if ($actualPlatformSet -ne $configuredPlatformSet) {
  throw 'Release metadata platform set differs from configuration'
}

$declaredNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($record in @($metadata.files)) {
  $name = [string]$record.name
  if ($name -notmatch '^[0-9A-Za-z._-]+$' -or -not $declaredNames.Add($name)) { throw "Release metadata contains an unsafe or duplicate file: $name" }
  $filePlatform = [string]$record.platform
  if ($filePlatform -ne 'all' -and $configuredPlatforms -notcontains $filePlatform) { throw "Release file has an invalid platform: $name" }
  $path = Join-Path $releaseRoot $name
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Release artifact is missing: $name" }
  if ((Get-Item -LiteralPath $path).Length -ne [int64]$record.size -or (Get-LowerHash $path) -ne [string]$record.sha256) {
    throw "Release artifact integrity differs: $name"
  }
}
$licenseRecords = @($metadata.files | Where-Object { $_.platform -eq 'all' -and $_.role -eq 'license' -and $_.name -eq 'LICENSE' })
if ($licenseRecords.Count -ne 1 -or @($metadata.files | Where-Object platform -eq 'all').Count -ne 1) {
  throw 'Release must contain exactly one common Apache-2.0 license file'
}

$github = Read-Json ([string]$metadata.githubManifest)
$modelscope = Read-Json ([string]$metadata.modelscopeManifest)
foreach ($manifest in @($github, $modelscope)) {
  if ($manifest.version -ne $metadata.version -or $manifest.pub_date -ne $metadata.publishedAt) { throw 'Manifest release identity differs from metadata' }
  $manifestPlatforms = @($manifest.platforms.PSObject.Properties.Name)
  $manifestPlatformSet = (($manifestPlatforms | Sort-Object) -join "`n")
  if ($manifestPlatformSet -ne $configuredPlatformSet) {
    throw 'Manifest platform set is incomplete or contains extras'
  }
}

if ([string]::IsNullOrWhiteSpace($UpdaterPublicKey)) {
  $tauriConfig = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
  $UpdaterPublicKey = [string]$tauriConfig.plugins.updater.pubkey
}
if ([string]::IsNullOrWhiteSpace($UpdaterPublicKey)) { throw 'Updater public key is not configured' }

foreach ($platformRecord in $platformRecords) {
  $platform = [string]$platformRecord.platform
  $platformFiles = @($metadata.files | Where-Object platform -eq $platform)
  foreach ($property in @('installerFile', 'updaterFile', 'signatureFile')) {
    $name = [string]$platformRecord.$property
    if (@($platformFiles | Where-Object name -eq $name).Count -ne 1) { throw "$platform metadata reference is missing: $property" }
  }
  $requiredRoles = if ($platform -like 'windows-*') {
    @('installer-updater', 'updater-signature', 'portable')
  } elseif ($platform -like 'macos-*') {
    @('installer', 'updater', 'updater-signature')
  } else {
    @('installer-updater', 'updater-signature')
  }
  if ($platformFiles.Count -ne $requiredRoles.Count) { throw "$platform release file count is invalid" }
  foreach ($role in $requiredRoles) {
    if (@($platformFiles | Where-Object role -eq $role).Count -ne 1) { throw "$platform release role is missing or duplicated: $role" }
  }

  $signaturePath = Join-Path $releaseRoot ([string]$platformRecord.signatureFile)
  $updaterPath = Join-Path $releaseRoot ([string]$platformRecord.updaterFile)
  $signature = (Get-Content -Raw -Encoding utf8 $signaturePath).Trim()
  try { [void][Convert]::FromBase64String($signature) } catch { throw "$platform updater signature is not Base64" }
  try {
    & (Join-Path $PSScriptRoot 'verify-update-signature.ps1') `
      -UpdaterPublicKey $UpdaterPublicKey `
      -SignaturePath $signaturePath `
      -ArtifactPath $updaterPath
  } catch {
    throw "$platform updater signature verification failed: $($_.Exception.Message)"
  }

  $updaterHash = Get-LowerHash $updaterPath
  $updaterSize = (Get-Item -LiteralPath $updaterPath).Length
  if ($platformRecord.buildId -ne "build-$($metadata.version)-$platform-$($updaterHash.Substring(0, 12))") { throw "$platform build identifier is invalid" }
  $githubPlatform = $github.platforms.$platform
  $modelscopePlatform = $modelscope.platforms.$platform
  foreach ($manifestPlatform in @($githubPlatform, $modelscopePlatform)) {
    if ($null -eq $manifestPlatform -or $manifestPlatform.signature -ne $signature) { throw "$platform manifest signature differs" }
    if ($manifestPlatform.sha256 -ne $updaterHash -or [int64]$manifestPlatform.size -ne $updaterSize) { throw "$platform manifest integrity differs" }
    if ($manifestPlatform.build_id -ne $platformRecord.buildId) { throw "$platform manifest build identifier differs" }
  }
  $expectedGithubPrefix = "https://github.com/$($releaseConfig.github.repository)/releases/download/v$($metadata.version)/"
  if (-not ([string]$githubPlatform.url).StartsWith($expectedGithubPrefix, [StringComparison]::Ordinal)) { throw "$platform GitHub URL is outside the trusted release" }
  $githubUri = [Uri]([string]$githubPlatform.url)
  if ([Uri]::UnescapeDataString($githubUri.Segments[-1]) -ne [string]$platformRecord.updaterFile) { throw "$platform GitHub URL references the wrong updater" }
  $modelscopeUri = [Uri]([string]$modelscopePlatform.url)
  if ($modelscopeUri.Scheme -ne 'https' -or $modelscopeUri.Host -notin @('modelscope.cn', 'www.modelscope.cn')) { throw "$platform ModelScope URL is not trusted HTTPS" }
  $query = [Uri]::UnescapeDataString($modelscopeUri.Query)
  if ($query -notlike "*Revision=master*" -or $query -notlike "*FilePath=releases/v$($metadata.version)/$($platformRecord.updaterFile)*") {
    throw "$platform ModelScope URL references the wrong updater"
  }
}

$sbom = Read-Json 'SBOM.spdx.json'
if ($sbom.spdxVersion -ne 'SPDX-2.3' -or $sbom.dataLicense -ne 'CC0-1.0') { throw 'SPDX SBOM contract is invalid' }
foreach ($record in @($metadata.files)) {
  $entry = @($sbom.files | Where-Object fileName -eq "./$($record.name)")
  if ($entry.Count -ne 1 -or @($entry[0].checksums | Where-Object { $_.algorithm -eq 'SHA256' -and $_.checksumValue -eq $record.sha256 }).Count -ne 1) {
    throw "SBOM checksum is missing for $($record.name)"
  }
}

$sumRecords = @{}
foreach ($line in Get-Content -Encoding utf8 (Join-Path $releaseRoot 'SHA256SUMS')) {
  if ($line -notmatch '^([0-9a-f]{64})  (.+)$' -or $sumRecords.ContainsKey($Matches[2])) { throw "Invalid or duplicate SHA256SUMS line: $line" }
  $sumRecords[$Matches[2]] = $Matches[1]
}
$expectedFiles = @(@($metadata.files | ForEach-Object name) + [string]$metadata.githubManifest + [string]$metadata.modelscopeManifest + 'release-metadata.json' + 'SBOM.spdx.json') | Sort-Object -Unique
if ($sumRecords.Count -ne $expectedFiles.Count) { throw 'SHA256SUMS file set is incomplete or contains extras' }
foreach ($relative in $expectedFiles) {
  $path = Join-Path $releaseRoot ($relative.Replace('/', [IO.Path]::DirectorySeparatorChar))
  if (-not $sumRecords.ContainsKey($relative) -or (Get-LowerHash $path) -ne $sumRecords[$relative]) { throw "SHA256SUMS differs: $relative" }
}
$actualFiles = @(Get-ChildItem -LiteralPath $releaseRoot -File -Recurse | ForEach-Object { $_.FullName.Substring($releaseRoot.Length + 1).Replace('\', '/') }) | Sort-Object
$expectedActual = @($expectedFiles + 'SHA256SUMS') | Sort-Object
if (($actualFiles -join "`n") -ne ($expectedActual -join "`n")) { throw 'Release directory contains an undeclared or missing file' }

Write-Output "PASS: verified six-platform release $($metadata.version) ($($metadata.buildId))"
