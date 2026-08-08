$ErrorActionPreference = 'Stop'
$pathComparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$collector = Join-Path $projectRoot 'scripts\collect-native-release.ps1'
if (-not (Test-Path -LiteralPath $collector -PathType Leaf)) {
  throw 'Native release collector is missing'
}

$testRoot = Join-Path $projectRoot '.local\native-release-test'
$version = [string](Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'package.json') | ConvertFrom-Json).version
$cases = @(
  @{ platform = 'windows-x86_64-nsis'; bundle = 'nsis'; installer = 'fixture-setup.exe'; updater = 'fixture-setup.exe'; signature = 'fixture-setup.exe.sig'; binary = 'qingzhou-ssh.exe' },
  @{ platform = 'windows-aarch64-nsis'; bundle = 'nsis'; installer = 'fixture-setup.exe'; updater = 'fixture-setup.exe'; signature = 'fixture-setup.exe.sig'; binary = 'qingzhou-ssh.exe' },
  @{ platform = 'macos-x86_64-dmg'; bundle = 'dmg'; installer = 'fixture.dmg'; updater = 'fixture.app.tar.gz'; signature = 'fixture.app.tar.gz.sig'; binary = $null },
  @{ platform = 'macos-aarch64-dmg'; bundle = 'dmg'; installer = 'fixture.dmg'; updater = 'fixture.app.tar.gz'; signature = 'fixture.app.tar.gz.sig'; binary = $null },
  @{ platform = 'linux-x86_64-appimage'; bundle = 'appimage'; installer = 'fixture.AppImage'; updater = 'fixture.AppImage'; signature = 'fixture.AppImage.sig'; binary = $null },
  @{ platform = 'linux-aarch64-appimage'; bundle = 'appimage'; installer = 'fixture.AppImage'; updater = 'fixture.AppImage'; signature = 'fixture.AppImage.sig'; binary = $null }
)

try {
  foreach ($case in $cases) {
    $caseRoot = Join-Path $testRoot $case.platform
    $bundleRoot = Join-Path $caseRoot 'bundle'
    $outputRoot = Join-Path $caseRoot 'output'
    New-Item -ItemType Directory -Force -Path (Join-Path $bundleRoot $case.bundle) | Out-Null
    if ($case.platform -like 'macos-*') {
      New-Item -ItemType Directory -Force -Path (Join-Path $bundleRoot 'macos') | Out-Null
    }
    [IO.File]::WriteAllBytes((Join-Path $bundleRoot "$($case.bundle)\$($case.installer)"), [byte[]](1, 2, 3, 4))
    if ($case.updater -ne $case.installer) {
      [IO.File]::WriteAllBytes((Join-Path $bundleRoot "macos\$($case.updater)"), [byte[]](5, 6, 7, 8))
    }
    $signatureDirectory = if ($case.platform -like 'macos-*') { 'macos' } else { $case.bundle }
    [IO.File]::WriteAllText(
      (Join-Path $bundleRoot "$signatureDirectory\$($case.signature)"),
      [Convert]::ToBase64String([byte[]](1..96)),
      [Text.UTF8Encoding]::new($false)
    )
    $arguments = @{
      Platform = $case.platform
      BundleRoot = $bundleRoot
      OutputDirectory = $outputRoot
    }
    if ($case.binary) {
      $binary = Join-Path $caseRoot $case.binary
      [IO.File]::WriteAllBytes($binary, [byte[]](9, 8, 7, 6))
      $arguments.BinaryPath = $binary
    }
    & $collector @arguments | Out-Null

    $descriptorPath = Join-Path $outputRoot 'platform-artifact.json'
    if (-not (Test-Path -LiteralPath $descriptorPath -PathType Leaf)) {
      throw "Native descriptor was not produced for $($case.platform)"
    }
    $descriptor = Get-Content -Raw -Encoding utf8 $descriptorPath | ConvertFrom-Json
    if ($descriptor.schemaVersion -ne 1 -or $descriptor.version -ne $version -or $descriptor.platform -ne $case.platform) {
      throw "Native descriptor identity differs for $($case.platform)"
    }
    foreach ($property in @('installerFile', 'updaterFile', 'signatureFile')) {
      $name = [string]$descriptor.$property
      if ($name -notmatch '^[0-9A-Za-z._-]+$' -or -not (Test-Path -LiteralPath (Join-Path $outputRoot $name) -PathType Leaf)) {
        throw "Native descriptor has an invalid $property for $($case.platform)"
      }
    }
    if ($case.platform -like 'windows-*') {
      $portable = @($descriptor.files | Where-Object role -eq 'portable')
      if ($portable.Count -ne 1 -or -not (Test-Path -LiteralPath (Join-Path $outputRoot $portable[0].name) -PathType Leaf)) {
        throw "Windows portable package is missing for $($case.platform)"
      }
    }
    if ($case.platform -like 'macos-*' -and $descriptor.updaterFile -notlike '*.app.tar.gz') {
      throw 'macOS updater must use the signed app archive instead of the DMG'
    }
  }
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolved.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, $pathComparison)) {
      throw 'Refusing to clean native release fixtures outside the project'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
}

Write-Output 'PASS: six native release artifact contracts are complete'
