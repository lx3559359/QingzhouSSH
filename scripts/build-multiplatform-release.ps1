param(
  [Parameter(Mandatory = $true)] [string]$NativeArtifactDirectory,
  [Parameter(Mandatory = $true)] [string]$OutputDirectory,
  [Parameter(Mandatory = $true)] [string]$ModelScopeNamespace,
  [Parameter(Mandatory = $true)] [string]$PublishedAt,
  [string]$ReleaseNotes
)

$ErrorActionPreference = 'Stop'
$utf8NoBom = [Text.UTF8Encoding]::new($false)
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$pathComparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }

function Resolve-ProjectPath([string]$Label, [string]$Path, [bool]$MustExist) {
  $resolved = [IO.Path]::GetFullPath($Path)
  if (-not $resolved.StartsWith($projectPrefix, $pathComparison)) {
    throw "$Label must remain inside the project folder: $resolved"
  }
  if ($MustExist -and -not (Test-Path -LiteralPath $resolved)) { throw "$Label was not found: $resolved" }
  return $resolved
}

function Get-LowerHash([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-StringHash([string]$Value) {
  $algorithm = [Security.Cryptography.SHA256]::Create()
  try {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Value)
    return ([BitConverter]::ToString($algorithm.ComputeHash($bytes)) -replace '-', '').ToLowerInvariant()
  } finally {
    $algorithm.Dispose()
  }
}

function Write-Json([string]$Path, $Value) {
  $json = (($Value | ConvertTo-Json -Depth 32) -replace "`r`n?", "`n")
  [IO.File]::WriteAllText($Path, $json + "`n", $utf8NoBom)
}

function New-FileRecord([string]$Path, [string]$Role, [string]$Platform) {
  return [ordered]@{
    name = Split-Path $Path -Leaf
    role = $Role
    platform = $Platform
    sha256 = Get-LowerHash $Path
    size = (Get-Item -LiteralPath $Path).Length
  }
}

& (Join-Path $PSScriptRoot 'tests\release-config.tests.ps1') | Out-Null
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$releaseConfig = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'release\release-config.json') | ConvertFrom-Json
$version = [string]$package.version
if ($ModelScopeNamespace -notmatch '^[0-9A-Za-z_-]{1,64}$' -or $ModelScopeNamespace -ne [string]$releaseConfig.modelscope.namespace) {
  throw 'ModelScope namespace differs from the trusted release configuration'
}
if ($PublishedAt -notmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$') { throw 'PublishedAt must be RFC 3339 UTC' }
try { [void][DateTimeOffset]::Parse($PublishedAt, [Globalization.CultureInfo]::InvariantCulture) } catch { throw 'PublishedAt is invalid' }
if ([string]::IsNullOrWhiteSpace($ReleaseNotes)) { $ReleaseNotes = "QingzhouSSH v$version" }

$nativeRoot = Resolve-ProjectPath 'Native artifact directory' $NativeArtifactDirectory $true
if (-not (Test-Path -LiteralPath $nativeRoot -PathType Container)) { throw 'Native artifact input must be a directory' }
$descriptorPaths = @(Get-ChildItem -LiteralPath $nativeRoot -Filter 'platform-artifact.json' -File -Recurse)
$configuredPlatforms = @($releaseConfig.platforms.PSObject.Properties.Name)
if ($descriptorPaths.Count -ne $configuredPlatforms.Count) { throw 'Native artifact directory must contain one descriptor for every configured platform' }

$descriptors = [ordered]@{}
$publicNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
[void]$publicNames.Add('LICENSE')
foreach ($descriptorPath in $descriptorPaths) {
  $descriptor = Get-Content -Raw -Encoding utf8 $descriptorPath.FullName | ConvertFrom-Json
  $platform = [string]$descriptor.platform
  if ($descriptor.schemaVersion -ne 1 -or $descriptor.project -ne $releaseConfig.projectName -or $descriptor.version -ne $version) {
    throw "Native descriptor identity is invalid: $($descriptorPath.FullName)"
  }
  if ($configuredPlatforms -notcontains $platform -or $descriptors.Contains($platform)) { throw "Native descriptor platform is invalid or duplicated: $platform" }
  $root = $descriptorPath.Directory.FullName
  $declaredNames = @($descriptor.files | ForEach-Object { [string]$_.name })
  foreach ($requiredName in @([string]$descriptor.installerFile, [string]$descriptor.updaterFile, [string]$descriptor.signatureFile)) {
    if ($requiredName -notmatch '^[0-9A-Za-z._-]+$' -or $declaredNames -notcontains $requiredName) {
      throw "Native descriptor references an unsafe or undeclared artifact: $requiredName"
    }
  }
  foreach ($file in @($descriptor.files)) {
    $name = [string]$file.name
    $role = [string]$file.role
    if ($name -notmatch '^[0-9A-Za-z._-]+$' -or [string]::IsNullOrWhiteSpace($role)) { throw "Native artifact declaration is unsafe: $name" }
    if (-not $publicNames.Add($name)) { throw "Public native artifact name is duplicated: $name" }
    $source = Join-Path $root $name
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Native artifact is missing: $source" }
  }
  $signature = (Get-Content -Raw -Encoding utf8 (Join-Path $root ([string]$descriptor.signatureFile))).Trim()
  try { [void][Convert]::FromBase64String($signature) } catch { throw "Updater signature is not Base64 for $platform" }
  $descriptors[$platform] = [pscustomobject]@{ contract = $descriptor; root = $root; signature = $signature }
}
if (@($descriptors.Keys | Where-Object { $configuredPlatforms -notcontains $_ }).Count -ne 0 -or $descriptors.Count -ne $configuredPlatforms.Count) {
  throw 'Native descriptor platform set differs from release configuration'
}

$resolvedOutput = Resolve-ProjectPath 'Release output directory' $OutputDirectory $false
$outputParent = Split-Path $resolvedOutput -Parent
New-Item -ItemType Directory -Force -Path $outputParent | Out-Null
$staging = Resolve-ProjectPath 'Release staging directory' (Join-Path $outputParent ('.release-staging-' + [Guid]::NewGuid().ToString('N'))) $false
New-Item -ItemType Directory -Path $staging | Out-Null

try {
  New-Item -ItemType Directory -Path (Join-Path $staging 'github'), (Join-Path $staging 'modelscope') | Out-Null
  $fileRecords = @()
  $platformRecords = @()
  $githubPlatforms = [ordered]@{}
  $modelscopePlatforms = [ordered]@{}
  foreach ($platform in $configuredPlatforms) {
    $item = $descriptors[$platform]
    $descriptor = $item.contract
    foreach ($file in @($descriptor.files)) {
      $source = Join-Path $item.root ([string]$file.name)
      $destination = Join-Path $staging ([string]$file.name)
      Copy-Item -LiteralPath $source -Destination $destination
      $fileRecords += New-FileRecord $destination ([string]$file.role) $platform
    }
    $updaterName = [string]$descriptor.updaterFile
    $updaterPath = Join-Path $staging $updaterName
    $updaterHash = Get-LowerHash $updaterPath
    $updaterSize = (Get-Item -LiteralPath $updaterPath).Length
    if ($updaterSize -le 0 -or $updaterSize -gt 536870912) { throw "Updater size is invalid for $platform" }
    $platformBuildId = "build-$version-$platform-$($updaterHash.Substring(0, 12))"
    $githubUrl = "https://github.com/$($releaseConfig.github.repository)/releases/download/v$version/$([Uri]::EscapeDataString($updaterName))"
    $modelscopeFile = [Uri]::EscapeDataString("releases/v$version/$updaterName")
    $modelscopeUrl = "https://modelscope.cn/api/v1/models/$ModelScopeNamespace/$($releaseConfig.modelscope.repository)/repo?Revision=master&FilePath=$modelscopeFile"
    $manifestBase = [ordered]@{
      signature = $item.signature
      sha256 = $updaterHash
      size = $updaterSize
      build_id = $platformBuildId
    }
    $githubPlatforms[$platform] = [ordered]@{ url = $githubUrl } + $manifestBase
    $modelscopePlatforms[$platform] = [ordered]@{ url = $modelscopeUrl } + $manifestBase
    $platformRecords += [ordered]@{
      platform = $platform
      installerFile = [string]$descriptor.installerFile
      updaterFile = $updaterName
      signatureFile = [string]$descriptor.signatureFile
      buildId = $platformBuildId
    }
  }
  $licenseSource = Join-Path $projectRoot 'LICENSE'
  if (-not (Test-Path -LiteralPath $licenseSource -PathType Leaf)) { throw 'Apache-2.0 license file is missing' }
  $licenseDestination = Join-Path $staging 'LICENSE'
  $licenseText = [IO.File]::ReadAllText($licenseSource)
  $canonicalLicenseText = $licenseText.Replace("`r`n", "`n").Replace("`r", "`n")
  [IO.File]::WriteAllText($licenseDestination, $canonicalLicenseText, [Text.UTF8Encoding]::new($false))
  $fileRecords += New-FileRecord $licenseDestination 'license' 'all'

  function New-Manifest($Platforms) {
    return [ordered]@{ version = $version; notes = $ReleaseNotes; pub_date = $PublishedAt; platforms = $Platforms }
  }
  Write-Json (Join-Path $staging 'github\latest.json') (New-Manifest $githubPlatforms)
  Write-Json (Join-Path $staging 'modelscope\latest.json') (New-Manifest $modelscopePlatforms)

  $identity = ($platformRecords | ForEach-Object { "$($_.platform):$($_.buildId)" }) -join "`n"
  $buildId = "build-$version-multi-$((Get-StringHash $identity).Substring(0, 12))"
  $metadata = [ordered]@{
    schemaVersion = 2
    project = $releaseConfig.projectName
    version = $version
    platform = 'multi'
    publishedAt = $PublishedAt
    buildId = $buildId
    platforms = $platformRecords
    githubManifest = 'github/latest.json'
    modelscopeManifest = 'modelscope/latest.json'
    files = $fileRecords
  }
  Write-Json (Join-Path $staging 'release-metadata.json') $metadata

  $npmPackage = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
  $packages = @()
  foreach ($sectionName in @('dependencies', 'devDependencies')) {
    $section = $npmPackage.$sectionName
    if ($null -eq $section) { continue }
    foreach ($property in $section.PSObject.Properties) {
      $packages += [pscustomobject]@{ ecosystem = 'npm'; name = $property.Name; version = [string]$property.Value }
    }
  }
  $cargoLock = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\Cargo.lock')
  foreach ($match in [regex]::Matches($cargoLock, '(?ms)^\[\[package\]\]\s*(.*?)(?=^\[\[package\]\]|\z)')) {
    $block = $match.Groups[1].Value
    $nameMatch = [regex]::Match($block, '(?m)^name\s*=\s*"([^"]+)"')
    $versionMatch = [regex]::Match($block, '(?m)^version\s*=\s*"([^"]+)"')
    if ($nameMatch.Success -and $versionMatch.Success) {
      $packages += [pscustomobject]@{ ecosystem = 'cargo'; name = $nameMatch.Groups[1].Value; version = $versionMatch.Groups[1].Value }
    }
  }
  $packages = @($packages | Sort-Object ecosystem, name, version -Unique)
  $spdxPackages = @()
  $packageIndex = 0
  foreach ($dependency in $packages) {
    $packageIndex++
    $safeName = ($dependency.name -replace '[^0-9A-Za-z.-]', '-')
    $spdxPackages += [ordered]@{
      SPDXID = "SPDXRef-Package-$($dependency.ecosystem)-$safeName-$packageIndex"
      name = $dependency.name
      versionInfo = $dependency.version
      downloadLocation = 'NOASSERTION'
      filesAnalyzed = $false
      licenseConcluded = 'NOASSERTION'
      licenseDeclared = 'NOASSERTION'
      supplier = 'NOASSERTION'
      externalRefs = @([ordered]@{
        referenceCategory = 'PACKAGE-MANAGER'
        referenceType = 'purl'
        referenceLocator = "pkg:$($dependency.ecosystem)/$([Uri]::EscapeDataString($dependency.name))@$([Uri]::EscapeDataString($dependency.version.TrimStart('^', '~')))"
      })
    }
  }
  $spdxFiles = @()
  $fileIndex = 0
  foreach ($record in $fileRecords) {
    $fileIndex++
    $spdxFiles += [ordered]@{
      SPDXID = "SPDXRef-File-$fileIndex"
      fileName = "./$($record.name)"
      checksums = @([ordered]@{ algorithm = 'SHA256'; checksumValue = $record.sha256 })
      licenseConcluded = 'NOASSERTION'
      copyrightText = 'NOASSERTION'
    }
  }
  $sbom = [ordered]@{
    spdxVersion = 'SPDX-2.3'
    dataLicense = 'CC0-1.0'
    SPDXID = 'SPDXRef-DOCUMENT'
    name = "QingzhouSSH-$version"
    documentNamespace = "https://github.com/$($releaseConfig.github.repository)/releases/tag/v$version/sbom/$buildId"
    creationInfo = [ordered]@{ created = $PublishedAt; creators = @('Tool: QingzhouSSH multiplatform release builder') }
    packages = $spdxPackages
    files = $spdxFiles
  }
  Write-Json (Join-Path $staging 'SBOM.spdx.json') $sbom

  $sumPaths = @($fileRecords | ForEach-Object name) + @('github/latest.json', 'modelscope/latest.json', 'release-metadata.json', 'SBOM.spdx.json')
  $sumLines = foreach ($relative in ($sumPaths | Sort-Object -Unique)) {
    $nativeRelative = $relative.Replace('/', [IO.Path]::DirectorySeparatorChar)
    "$(Get-LowerHash (Join-Path $staging $nativeRelative))  $relative"
  }
  [IO.File]::WriteAllText((Join-Path $staging 'SHA256SUMS'), (($sumLines -join "`n") + "`n"), $utf8NoBom)

  if (Test-Path -LiteralPath $resolvedOutput) {
    $validatedOutput = Resolve-ProjectPath 'Release output cleanup' $resolvedOutput $true
    Remove-Item -LiteralPath $validatedOutput -Recurse -Force
  }
  Move-Item -LiteralPath $staging -Destination $resolvedOutput
  $staging = $null
} finally {
  if ($null -ne $staging -and (Test-Path -LiteralPath $staging)) {
    $validatedStaging = Resolve-ProjectPath 'Release staging cleanup' $staging $true
    if ((Split-Path $validatedStaging -Leaf) -notlike '.release-staging-*') { throw 'Refusing unexpected release staging cleanup' }
    Remove-Item -LiteralPath $validatedStaging -Recurse -Force
  }
}

Write-Output $resolvedOutput
