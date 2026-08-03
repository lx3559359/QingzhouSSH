param(
  [Parameter(Mandatory = $true)] [string]$ReleaseDirectory,
  [Parameter(Mandatory = $true)] [string]$GitHubDirectory,
  [Parameter(Mandatory = $true)] [string]$ModelScopeDirectory,
  [string]$UpdaterPublicKey
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar

function Resolve-ProjectDirectory([string]$Label, [string]$Path) {
  $resolved = [IO.Path]::GetFullPath($Path)
  if (-not $resolved.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$Label must remain inside the project folder: $resolved"
  }
  if (-not (Test-Path -LiteralPath $resolved -PathType Container)) { throw "$Label was not found: $resolved" }
  return $resolved
}

function Get-LowerHash([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Read-Manifest([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing source manifest: $Path" }
  return Get-Content -Raw -Encoding utf8 $Path | ConvertFrom-Json
}

$releaseRoot = Resolve-ProjectDirectory 'Release directory' $ReleaseDirectory
$githubRoot = Resolve-ProjectDirectory 'GitHub readback directory' $GitHubDirectory
$modelscopeRoot = Resolve-ProjectDirectory 'ModelScope readback directory' $ModelScopeDirectory
$verifyArguments = @{ ReleaseDirectory = $releaseRoot }
if (-not [string]::IsNullOrWhiteSpace($UpdaterPublicKey)) {
  $verifyArguments.UpdaterPublicKey = $UpdaterPublicKey
}
& (Join-Path $PSScriptRoot 'verify-release.ps1') @verifyArguments | Out-Null

$metadata = Get-Content -Raw -Encoding utf8 (Join-Path $releaseRoot 'release-metadata.json') | ConvertFrom-Json
$commonFiles = @($metadata.files | ForEach-Object name) + @('SHA256SUMS', 'SBOM.spdx.json', 'release-metadata.json')
foreach ($name in $commonFiles) {
  $local = Join-Path $releaseRoot $name
  $github = Join-Path $githubRoot $name
  $modelscope = Join-Path $modelscopeRoot $name
  foreach ($candidate in @($local, $github, $modelscope)) {
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { throw "Mirrored release file is missing: $candidate" }
  }
  $expected = Get-LowerHash $local
  if ((Get-LowerHash $github) -ne $expected -or (Get-LowerHash $modelscope) -ne $expected) {
    throw "Dual-source byte comparison failed: $name"
  }
}

$githubManifestLocal = Join-Path $releaseRoot ([string]$metadata.githubManifest).Replace('/', [IO.Path]::DirectorySeparatorChar)
$modelscopeManifestLocal = Join-Path $releaseRoot ([string]$metadata.modelscopeManifest).Replace('/', [IO.Path]::DirectorySeparatorChar)
$githubManifestRemote = Join-Path $githubRoot 'latest.json'
$modelscopeManifestRemote = Join-Path $modelscopeRoot 'latest.json'
if ((Get-LowerHash $githubManifestLocal) -ne (Get-LowerHash $githubManifestRemote)) { throw 'GitHub manifest readback differs from the published manifest' }
if ((Get-LowerHash $modelscopeManifestLocal) -ne (Get-LowerHash $modelscopeManifestRemote)) { throw 'ModelScope manifest readback differs from the published manifest' }

$githubManifest = Read-Manifest $githubManifestRemote
$modelscopeManifest = Read-Manifest $modelscopeManifestRemote
$githubPlatform = $githubManifest.platforms.'windows-x86_64'
$modelscopePlatform = $modelscopeManifest.platforms.'windows-x86_64'
if ($githubManifest.version -ne $modelscopeManifest.version -or $githubManifest.version -ne $metadata.version) { throw 'Dual-source versions differ' }
if ($githubPlatform.build_id -ne $modelscopePlatform.build_id -or $githubPlatform.build_id -ne $metadata.buildId) { throw 'Dual-source build identifiers differ' }
foreach ($property in @('signature', 'sha256', 'size')) {
  if ($githubPlatform.$property -ne $modelscopePlatform.$property) { throw "Dual-source updater property differs: $property" }
}

$expectedRemoteFiles = @($commonFiles + 'latest.json') | Sort-Object -Unique
foreach ($source in @(@('GitHub', $githubRoot), @('ModelScope', $modelscopeRoot))) {
  $actual = @(Get-ChildItem -LiteralPath $source[1] -File -Recurse | ForEach-Object {
    $_.FullName.Substring($source[1].Length + 1).Replace('\', '/')
  }) | Sort-Object
  if (($actual -join "`n") -ne ($expectedRemoteFiles -join "`n")) { throw "$($source[0]) readback file set differs from the release contract" }
}

Write-Output "PASS: GitHub and ModelScope release $($metadata.version) are byte-identical"
