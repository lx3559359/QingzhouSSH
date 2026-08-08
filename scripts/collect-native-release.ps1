param(
  [Parameter(Mandatory = $true)]
  [ValidateSet(
    'windows-x86_64-nsis',
    'windows-aarch64-nsis',
    'macos-x86_64-dmg',
    'macos-aarch64-dmg',
    'linux-x86_64-appimage',
    'linux-aarch64-appimage'
  )]
  [string]$Platform,

  [Parameter(Mandatory = $true)]
  [string]$BundleRoot,

  [string]$BinaryPath,

  [Parameter(Mandatory = $true)]
  [string]$OutputDirectory
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
  if ($MustExist -and -not (Test-Path -LiteralPath $resolved)) {
    throw "$Label was not found: $resolved"
  }
  return $resolved
}

function Find-OneFile([string]$Label, [string]$Directory, [string]$Filter) {
  $matches = @(Get-ChildItem -LiteralPath $Directory -Filter $Filter -File)
  if ($matches.Count -ne 1) {
    throw "$Label must contain exactly one $Filter file: $Directory"
  }
  return $matches[0].FullName
}

function Copy-PublicFile([string]$Source, [string]$Name, [string]$Role) {
  if ($Name -notmatch '^[0-9A-Za-z._-]+$') { throw "Public artifact name is unsafe: $Name" }
  $destination = Join-Path $staging $Name
  Copy-Item -LiteralPath $Source -Destination $destination
  return [ordered]@{ name = $Name; role = $Role }
}

$bundle = Resolve-ProjectPath 'Bundle root' $BundleRoot $true
if (-not (Test-Path -LiteralPath $bundle -PathType Container)) { throw 'Bundle root must be a directory' }
$output = Resolve-ProjectPath 'Native artifact output' $OutputDirectory $false
$outputParent = Split-Path $output -Parent
New-Item -ItemType Directory -Force -Path $outputParent | Out-Null
$staging = Resolve-ProjectPath 'Native artifact staging' (Join-Path $outputParent ('.native-release-' + [Guid]::NewGuid().ToString('N'))) $false
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$version = [string]$package.version
$architecture = if ($Platform -like '*aarch64*') { 'aarch64' } else { 'x86_64' }

New-Item -ItemType Directory -Path $staging | Out-Null
try {
  $files = @()
  if ($Platform -like 'windows-*') {
    if ([string]::IsNullOrWhiteSpace($BinaryPath)) { throw 'Windows native artifacts require BinaryPath' }
    $binary = Resolve-ProjectPath 'Windows executable' $BinaryPath $true
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Windows executable must be a file' }
    $installer = Find-OneFile 'NSIS bundle' (Join-Path $bundle 'nsis') '*-setup.exe'
    $signature = "$installer.sig"
    if (-not (Test-Path -LiteralPath $signature -PathType Leaf)) { throw 'NSIS updater signature is missing' }
    $installerName = "QingzhouSSH_${version}_windows_${architecture}-setup.exe"
    $signatureName = "$installerName.sig"
    $files += Copy-PublicFile $installer $installerName 'installer-updater'
    $files += Copy-PublicFile $signature $signatureName 'updater-signature'
    $portable = & (Join-Path $PSScriptRoot 'package-portable.ps1') `
      -ExecutablePath $binary `
      -Version $version `
      -Architecture $architecture `
      -OutputDirectory $staging
    $portableName = Split-Path $portable -Leaf
    $files += [ordered]@{ name = $portableName; role = 'portable' }
    $updaterName = $installerName
  } elseif ($Platform -like 'macos-*') {
    $installer = Find-OneFile 'DMG bundle' (Join-Path $bundle 'dmg') '*.dmg'
    $updater = Find-OneFile 'macOS updater bundle' (Join-Path $bundle 'macos') '*.app.tar.gz'
    $signature = "$updater.sig"
    if (-not (Test-Path -LiteralPath $signature -PathType Leaf)) { throw 'macOS updater signature is missing' }
    $installerName = "QingzhouSSH_${version}_macos_${architecture}.dmg"
    $updaterName = "QingzhouSSH_${version}_macos_${architecture}.app.tar.gz"
    $signatureName = "$updaterName.sig"
    $files += Copy-PublicFile $installer $installerName 'installer'
    $files += Copy-PublicFile $updater $updaterName 'updater'
    $files += Copy-PublicFile $signature $signatureName 'updater-signature'
  } else {
    $installer = Find-OneFile 'AppImage bundle' (Join-Path $bundle 'appimage') '*.AppImage'
    $signature = "$installer.sig"
    if (-not (Test-Path -LiteralPath $signature -PathType Leaf)) { throw 'AppImage updater signature is missing' }
    $installerName = "QingzhouSSH_${version}_linux_${architecture}.AppImage"
    $signatureName = "$installerName.sig"
    $files += Copy-PublicFile $installer $installerName 'installer-updater'
    $files += Copy-PublicFile $signature $signatureName 'updater-signature'
    $updaterName = $installerName
  }

  $descriptor = [ordered]@{
    schemaVersion = 1
    project = 'QingzhouSSH'
    version = $version
    platform = $Platform
    installerFile = $installerName
    updaterFile = $updaterName
    signatureFile = $signatureName
    files = $files
  }
  $json = (($descriptor | ConvertTo-Json -Depth 12) -replace "`r`n?", "`n") + "`n"
  [IO.File]::WriteAllText((Join-Path $staging 'platform-artifact.json'), $json, $utf8NoBom)

  if (Test-Path -LiteralPath $output) {
    $validatedOutput = Resolve-ProjectPath 'Native artifact cleanup' $output $true
    Remove-Item -LiteralPath $validatedOutput -Recurse -Force
  }
  Move-Item -LiteralPath $staging -Destination $output
  $staging = $null
} finally {
  if ($null -ne $staging -and (Test-Path -LiteralPath $staging)) {
    $validatedStaging = Resolve-ProjectPath 'Native artifact staging cleanup' $staging $true
    if ((Split-Path $validatedStaging -Leaf) -notlike '.native-release-*') {
      throw 'Refusing unexpected native artifact staging cleanup'
    }
    Remove-Item -LiteralPath $validatedStaging -Recurse -Force
  }
}

Write-Output $output
