param(
  [Parameter(Mandatory = $true)]
  [string]$ExecutablePath,

  [Parameter(Mandatory = $true)]
  [string]$Version,

  [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'

function Assert-UnderProject {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Label,

    [Parameter(Mandatory = $true)]
    [string]$Path,

    [Parameter(Mandatory = $true)]
    [string]$ProjectRoot
  )

  $resolvedPath = [IO.Path]::GetFullPath($Path)
  $projectPrefix = $ProjectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
  if (-not $resolvedPath.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "$Label must remain inside the project folder: $resolvedPath"
  }

  return $resolvedPath
}

if ($Version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$') {
  throw "Version must be valid SemVer: $Version"
}

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
  throw "Executable was not found: $ExecutablePath"
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $projectRoot 'artifacts\release'
}
$resolvedOutput = Assert-UnderProject -Label 'Portable output directory' -Path $OutputDirectory -ProjectRoot $projectRoot
New-Item -ItemType Directory -Force -Path $resolvedOutput | Out-Null

$stagingName = '.portable-staging-' + [Guid]::NewGuid().ToString('N')
$stagingDirectory = Assert-UnderProject -Label 'Portable staging directory' -Path (Join-Path $resolvedOutput $stagingName) -ProjectRoot $projectRoot
$archiveName = "QingzhouSSH-v$Version-windows-x86_64-portable.zip"
$archivePath = Assert-UnderProject -Label 'Portable archive' -Path (Join-Path $resolvedOutput $archiveName) -ProjectRoot $projectRoot

New-Item -ItemType Directory -Path $stagingDirectory | Out-Null
try {
  Copy-Item -LiteralPath $resolvedExecutable -Destination (Join-Path $stagingDirectory 'QingzhouSSH.exe')
  Copy-Item -LiteralPath (Join-Path $projectRoot 'release\portable\portable.flag') -Destination $stagingDirectory
  Copy-Item -LiteralPath (Join-Path $projectRoot 'release\portable\README-portable.txt') -Destination $stagingDirectory
  Copy-Item -LiteralPath (Join-Path $projectRoot 'LICENSE') -Destination $stagingDirectory

  if (Test-Path -LiteralPath $archivePath) {
    Remove-Item -LiteralPath $archivePath -Force
  }
  Compress-Archive -Path (Join-Path $stagingDirectory '*') -DestinationPath $archivePath -CompressionLevel Optimal
} finally {
  if (Test-Path -LiteralPath $stagingDirectory) {
    $validatedStaging = Assert-UnderProject -Label 'Portable staging cleanup' -Path $stagingDirectory -ProjectRoot $projectRoot
    if ((Split-Path $validatedStaging -Leaf) -notlike '.portable-staging-*') {
      throw "Refusing to clean an unexpected staging directory: $validatedStaging"
    }
    Remove-Item -LiteralPath $validatedStaging -Recurse -Force
  }
}

Write-Output $archivePath
