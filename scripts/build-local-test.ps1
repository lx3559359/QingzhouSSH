param(
  [Parameter(Mandatory = $true)]
  [string]$PackageVersion,

  [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$sourceVersion = [string]$package.version
if ($PackageVersion -notmatch ('^' + [regex]::Escape($sourceVersion) + '(?:-|$)')) {
  throw "Package version must preserve source version $sourceVersion"
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $projectRoot 'artifacts\local-test'
}
$resolvedOutput = [IO.Path]::GetFullPath($OutputDirectory)
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
if (-not $resolvedOutput.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
  throw "Local-test output must remain inside the project folder: $resolvedOutput"
}

$expandedDirectory = Join-Path $resolvedOutput "QingzhouSSH-v$PackageVersion"
$archivePath = Join-Path $resolvedOutput "QingzhouSSH-v$PackageVersion-windows-x86_64-portable.zip"
foreach ($path in @($expandedDirectory, $archivePath)) {
  if (Test-Path -LiteralPath $path) {
    throw "Refusing to overwrite an existing local-test artifact: $path"
  }
}

. (Join-Path $PSScriptRoot 'dev-env.ps1') -Quiet

Push-Location $projectRoot
try {
  & pnpm tauri build --no-bundle
  if ($LASTEXITCODE -ne 0) { throw "Tauri build failed with exit code $LASTEXITCODE" }
} finally {
  Pop-Location
}

$executable = Join-Path $env:CARGO_TARGET_DIR 'release\qingzhou-ssh.exe'
& (Join-Path $PSScriptRoot 'assert-tauri-production-build.ps1') `
  -ExecutablePath $executable `
  -TargetDirectory $env:CARGO_TARGET_DIR | Out-Null

$producedArchive = & (Join-Path $PSScriptRoot 'package-portable.ps1') `
  -ExecutablePath $executable `
  -Version $PackageVersion `
  -OutputDirectory $resolvedOutput

New-Item -ItemType Directory -Path $expandedDirectory | Out-Null
try {
  Expand-Archive -LiteralPath $producedArchive -DestinationPath $expandedDirectory
} catch {
  if (Test-Path -LiteralPath $expandedDirectory) {
    Remove-Item -LiteralPath $expandedDirectory -Recurse -Force
  }
  throw
}

[pscustomobject]@{
  Executable = Join-Path $expandedDirectory 'QingzhouSSH.exe'
  Directory = $expandedDirectory
  Archive = [IO.Path]::GetFullPath([string]$producedArchive)
  SourceVersion = $sourceVersion
}
