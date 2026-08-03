param(
  [Parameter(Mandatory = $true)]
  [string]$ReleaseDirectory
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$resolvedRelease = [IO.Path]::GetFullPath($ReleaseDirectory)
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if (-not $resolvedRelease.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Release directory must remain inside the project folder: $resolvedRelease"
}
if (-not (Test-Path -LiteralPath $resolvedRelease -PathType Container)) { throw 'Release directory was not found' }

function Get-LowerHash([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Read-Json([string]$RelativePath) {
  $path = Join-Path $resolvedRelease ($RelativePath.Replace('/', [IO.Path]::DirectorySeparatorChar))
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing release file: $RelativePath" }
  return Get-Content -Raw -Encoding utf8 $path | ConvertFrom-Json
}

$metadata = Read-Json 'release-metadata.json'
if ($metadata.schemaVersion -ne 1 -or $metadata.project -ne 'QingzhouSSH' -or $metadata.platform -ne 'windows-x86_64') {
  throw 'Release metadata contract is invalid'
}
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
if ($metadata.version -ne $package.version) { throw 'Release version does not match package.json' }
if ($metadata.buildId -notmatch "^build-$([regex]::Escape($metadata.version))-[0-9a-f]{12}$") { throw 'Release build identifier is invalid' }

$declaredNames = @()
foreach ($record in @($metadata.files)) {
  $name = [string]$record.name
  if ([string]::IsNullOrWhiteSpace($name) -or (Split-Path $name -Leaf) -ne $name) { throw 'Release metadata contains an unsafe file name' }
  if ($declaredNames -contains $name) { throw "Duplicate release file: $name" }
  $declaredNames += $name
  $path = Join-Path $resolvedRelease $name
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing release artifact: $name" }
  if ((Get-Item -LiteralPath $path).Length -ne [int64]$record.size) { throw "Release size mismatch: $name" }
  if ((Get-LowerHash $path) -ne [string]$record.sha256) { throw "Release hash mismatch: $name" }
}
foreach ($requiredRole in @('installer', 'updater', 'updater-signature', 'portable')) {
  if (@($metadata.files | Where-Object role -eq $requiredRole).Count -ne 1) { throw "Release role is missing or duplicated: $requiredRole" }
}

$signaturePath = Join-Path $resolvedRelease ([string]$metadata.signatureFile)
$signature = (Get-Content -Raw -Encoding utf8 $signaturePath).Trim()
try { [void][Convert]::FromBase64String($signature) } catch { throw 'Updater signature is not valid Base64' }
$updaterPath = Join-Path $resolvedRelease ([string]$metadata.updaterFile)
$updaterHash = Get-LowerHash $updaterPath
$updaterSize = (Get-Item -LiteralPath $updaterPath).Length

$github = Read-Json ([string]$metadata.githubManifest)
$modelscope = Read-Json ([string]$metadata.modelscopeManifest)
$githubPlatform = $github.platforms.'windows-x86_64'
$modelscopePlatform = $modelscope.platforms.'windows-x86_64'
foreach ($manifest in @($github, $modelscope)) {
  if ($manifest.version -ne $metadata.version -or $manifest.pub_date -ne $metadata.publishedAt) { throw 'Manifest release identity differs from metadata' }
}
foreach ($platform in @($githubPlatform, $modelscopePlatform)) {
  if ($platform.signature -ne $signature) { throw 'Manifest signature differs from the signed updater' }
  if ($platform.sha256 -ne $updaterHash -or [int64]$platform.size -ne $updaterSize) { throw 'Manifest integrity values differ from the updater' }
  if ($platform.build_id -ne $metadata.buildId) { throw 'Manifest build identifier differs from metadata' }
}
$expectedGithubPrefix = "https://github.com/lx3559359/QingzhouSSH/releases/download/v$($metadata.version)/"
if (-not ([string]$githubPlatform.url).StartsWith($expectedGithubPrefix, [StringComparison]::Ordinal)) { throw 'GitHub manifest URL is outside the trusted release' }
if ([string]$githubPlatform.url -notlike "*/$($metadata.updaterFile)") { throw 'GitHub manifest references the wrong updater file' }
$modelscopeUri = [Uri]([string]$modelscopePlatform.url)
if ($modelscopeUri.Scheme -ne 'https' -or $modelscopeUri.Host -notin @('modelscope.cn', 'www.modelscope.cn')) { throw 'ModelScope manifest URL is not trusted HTTPS' }
$query = [Uri]::UnescapeDataString($modelscopeUri.Query)
if ($query -notlike "*Revision=master*" -or $query -notlike "*FilePath=releases/v$($metadata.version)/$($metadata.updaterFile)*") {
  throw 'ModelScope manifest references the wrong updater file'
}

$sbom = Read-Json 'SBOM.spdx.json'
if ($sbom.spdxVersion -ne 'SPDX-2.3' -or $sbom.dataLicense -ne 'CC0-1.0') { throw 'SPDX SBOM contract is invalid' }
foreach ($record in @($metadata.files)) {
  $entry = @($sbom.files | Where-Object fileName -eq "./$($record.name)")
  if ($entry.Count -ne 1 -or @($entry[0].checksums | Where-Object { $_.algorithm -eq 'SHA256' -and $_.checksumValue -eq $record.sha256 }).Count -ne 1) {
    throw "SBOM checksum is missing for $($record.name)"
  }
}

$sumsPath = Join-Path $resolvedRelease 'SHA256SUMS'
$sumRecords = @{}
foreach ($line in Get-Content -Encoding utf8 $sumsPath) {
  if ($line -notmatch '^([0-9a-f]{64})  ([0-9A-Za-z._/-]+)$') { throw "Invalid SHA256SUMS line: $line" }
  if ($sumRecords.ContainsKey($Matches[2])) { throw "Duplicate SHA256SUMS entry: $($Matches[2])" }
  $sumRecords[$Matches[2]] = $Matches[1]
}
$expectedFiles = @($declaredNames + [string]$metadata.githubManifest + [string]$metadata.modelscopeManifest + 'release-metadata.json' + 'SBOM.spdx.json') | Sort-Object -Unique
if ($sumRecords.Count -ne $expectedFiles.Count) { throw 'SHA256SUMS file set is incomplete or contains extras' }
foreach ($relative in $expectedFiles) {
  if (-not $sumRecords.ContainsKey($relative)) { throw "SHA256SUMS is missing $relative" }
  $path = Join-Path $resolvedRelease ($relative.Replace('/', [IO.Path]::DirectorySeparatorChar))
  if ((Get-LowerHash $path) -ne $sumRecords[$relative]) { throw "SHA256SUMS mismatch: $relative" }
}

$actualFiles = @(
  Get-ChildItem -LiteralPath $resolvedRelease -File -Recurse | ForEach-Object {
    $_.FullName.Substring($resolvedRelease.Length + 1).Replace('\', '/')
  }
) | Sort-Object
$expectedActual = @($expectedFiles + 'SHA256SUMS') | Sort-Object
if (($actualFiles -join "`n") -ne ($expectedActual -join "`n")) { throw 'Release directory contains an undeclared or missing file' }

Write-Output "PASS: verified release $($metadata.version) ($($metadata.buildId))"
