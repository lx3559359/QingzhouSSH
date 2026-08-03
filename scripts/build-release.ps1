param(
  [Parameter(Mandatory = $true)] [string]$InstallerPath,
  [Parameter(Mandatory = $true)] [string]$UpdaterArchivePath,
  [Parameter(Mandatory = $true)] [string]$UpdaterSignaturePath,
  [Parameter(Mandatory = $true)] [string]$PortableArchivePath,
  [Parameter(Mandatory = $true)] [string]$OutputDirectory,
  [Parameter(Mandatory = $true)] [string]$ModelScopeNamespace,
  [Parameter(Mandatory = $true)] [string]$PublishedAt,
  [string]$ReleaseNotes
)

$ErrorActionPreference = 'Stop'
$utf8NoBom = [Text.UTF8Encoding]::new($false)
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Assert-UnderProject([string]$Label, [string]$Path) {
  $resolved = [IO.Path]::GetFullPath($Path)
  $prefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$Label must remain inside the project folder: $resolved"
  }
  return $resolved
}

function Resolve-ProjectFile([string]$Label, [string]$Path) {
  $resolved = Assert-UnderProject $Label $Path
  if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) { throw "$Label was not found: $resolved" }
  return $resolved
}

function Get-LowerHash([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Write-Json([string]$Path, $Value) {
  $json = $Value | ConvertTo-Json -Depth 24
  [IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, $utf8NoBom)
}

function New-FileRecord([string]$Path, [string]$Role) {
  return [ordered]@{
    name = Split-Path $Path -Leaf
    role = $Role
    sha256 = Get-LowerHash $Path
    size = (Get-Item -LiteralPath $Path).Length
  }
}

& (Join-Path $PSScriptRoot 'tests\release-config.tests.ps1') | Out-Null
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$releaseConfig = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'release\release-config.json') | ConvertFrom-Json
$version = [string]$package.version

if ($ModelScopeNamespace -notmatch '^[0-9A-Za-z_-]{1,64}$') { throw 'ModelScope namespace is invalid' }
if ($ModelScopeNamespace -ne [string]$releaseConfig.modelscope.namespace) { throw 'ModelScope namespace differs from the trusted release configuration' }
if ($PublishedAt -notmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$') { throw 'PublishedAt must be an RFC 3339 UTC timestamp' }
try { [void][DateTimeOffset]::Parse($PublishedAt, [Globalization.CultureInfo]::InvariantCulture) } catch { throw 'PublishedAt is invalid' }
if ([string]::IsNullOrWhiteSpace($ReleaseNotes)) { $ReleaseNotes = "QingzhouSSH v$version" }

$installer = Resolve-ProjectFile 'Installer' $InstallerPath
$updater = Resolve-ProjectFile 'Updater archive' $UpdaterArchivePath
$signatureFile = Resolve-ProjectFile 'Updater signature' $UpdaterSignaturePath
$portable = Resolve-ProjectFile 'Portable archive' $PortableArchivePath
if ((Split-Path $signatureFile -Leaf) -ne ((Split-Path $updater -Leaf) + '.sig')) {
  throw 'Updater signature file must be named after the updater archive'
}
$names = @($installer, $updater, $signatureFile, $portable | ForEach-Object { Split-Path $_ -Leaf })
if (@($names | Select-Object -Unique).Count -ne 4) { throw 'Release artifact names must be unique' }

$signature = (Get-Content -Raw -Encoding utf8 $signatureFile).Trim()
if ($signature.Length -lt 64 -or $signature.Length -gt 16384 -or $signature -match 'placeholder|replace|todo') {
  throw 'Updater signature is missing or invalid'
}
try { [void][Convert]::FromBase64String($signature) } catch { throw 'Updater signature is not valid Base64' }

$updaterSize = (Get-Item -LiteralPath $updater).Length
if ($updaterSize -le 0 -or $updaterSize -gt 536870912) { throw 'Updater archive size is invalid' }
$updaterHash = Get-LowerHash $updater
$buildId = "build-$version-$($updaterHash.Substring(0, 12))"
$resolvedOutput = Assert-UnderProject 'Release output directory' $OutputDirectory
$outputParent = Split-Path $resolvedOutput -Parent
New-Item -ItemType Directory -Force -Path $outputParent | Out-Null
$staging = Assert-UnderProject 'Release staging directory' (Join-Path $outputParent ('.release-staging-' + [Guid]::NewGuid().ToString('N')))

New-Item -ItemType Directory -Path $staging | Out-Null
try {
  New-Item -ItemType Directory -Path (Join-Path $staging 'github'), (Join-Path $staging 'modelscope') | Out-Null
  foreach ($source in @($installer, $updater, $signatureFile, $portable)) {
    Copy-Item -LiteralPath $source -Destination $staging
  }

  $fileRecords = @(
    (New-FileRecord (Join-Path $staging (Split-Path $installer -Leaf)) 'installer'),
    (New-FileRecord (Join-Path $staging (Split-Path $updater -Leaf)) 'updater'),
    (New-FileRecord (Join-Path $staging (Split-Path $signatureFile -Leaf)) 'updater-signature'),
    (New-FileRecord (Join-Path $staging (Split-Path $portable -Leaf)) 'portable')
  )

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
    creationInfo = [ordered]@{ created = $PublishedAt; creators = @('Tool: QingzhouSSH release builder') }
    packages = $spdxPackages
    files = $spdxFiles
  }
  Write-Json (Join-Path $staging 'SBOM.spdx.json') $sbom

  $updaterName = Split-Path $updater -Leaf
  $githubUrl = "https://github.com/$($releaseConfig.github.repository)/releases/download/v$version/$([Uri]::EscapeDataString($updaterName))"
  $modelscopeFile = [Uri]::EscapeDataString("releases/v$version/$updaterName")
  $modelscopeUrl = "https://modelscope.cn/api/v1/studios/$ModelScopeNamespace/$($releaseConfig.modelscope.repository)/repo?Revision=master&FilePath=$modelscopeFile"
  function New-Manifest([string]$Url) {
    return [ordered]@{
      version = $version
      notes = $ReleaseNotes
      pub_date = $PublishedAt
      platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
          url = $Url
          signature = $signature
          sha256 = $updaterHash
          size = $updaterSize
          build_id = $buildId
        }
      }
    }
  }
  Write-Json (Join-Path $staging 'github\latest.json') (New-Manifest $githubUrl)
  Write-Json (Join-Path $staging 'modelscope\latest.json') (New-Manifest $modelscopeUrl)

  $metadata = [ordered]@{
    schemaVersion = 1
    project = $releaseConfig.projectName
    version = $version
    platform = $releaseConfig.platform
    publishedAt = $PublishedAt
    buildId = $buildId
    updaterFile = $updaterName
    signatureFile = Split-Path $signatureFile -Leaf
    githubManifest = 'github/latest.json'
    modelscopeManifest = 'modelscope/latest.json'
    files = $fileRecords
  }
  Write-Json (Join-Path $staging 'release-metadata.json') $metadata

  $sumPaths = @()
  foreach ($record in $fileRecords) { $sumPaths += $record.name }
  $sumPaths += @('github/latest.json', 'modelscope/latest.json', 'release-metadata.json', 'SBOM.spdx.json')
  $sumLines = foreach ($relative in ($sumPaths | Sort-Object -Unique)) {
    $nativeRelative = $relative.Replace('/', [IO.Path]::DirectorySeparatorChar)
    "$(Get-LowerHash (Join-Path $staging $nativeRelative))  $relative"
  }
  [IO.File]::WriteAllText((Join-Path $staging 'SHA256SUMS'), (($sumLines -join "`n") + "`n"), $utf8NoBom)

  if (Test-Path -LiteralPath $resolvedOutput) {
    $validatedOutput = Assert-UnderProject 'Release output cleanup' $resolvedOutput
    Remove-Item -LiteralPath $validatedOutput -Recurse -Force
  }
  Move-Item -LiteralPath $staging -Destination $resolvedOutput
  $staging = $null
} finally {
  if ($null -ne $staging -and (Test-Path -LiteralPath $staging)) {
    $validatedStaging = Assert-UnderProject 'Release staging cleanup' $staging
    if ((Split-Path $validatedStaging -Leaf) -notlike '.release-staging-*') { throw 'Refusing unexpected release staging cleanup' }
    Remove-Item -LiteralPath $validatedStaging -Recurse -Force
  }
}

Write-Output $resolvedOutput
