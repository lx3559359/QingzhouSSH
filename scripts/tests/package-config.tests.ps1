$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$tauri = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
$vite = Get-Content -Raw -Encoding utf8 (Join-Path $projectRoot 'vite.config.ts')

if ($tauri.bundle.targets -ne 'nsis') { throw 'Only the per-user NSIS target may be enabled' }
if ($tauri.bundle.createUpdaterArtifacts -ne $true) { throw 'Signed updater artifacts must be enabled' }
if ($tauri.bundle.useLocalToolsDir -ne $true) { throw 'NSIS tools must be cached in project target/.tauri instead of user AppData' }
if ($tauri.bundle.windows.nsis.installMode -ne 'currentUser') { throw 'NSIS must not require administrator rights' }
foreach ($ignoredDirectory in @('**/.local/**', '**/target/**', '**/artifacts/**', '**/.worktrees/**')) {
  if (-not $vite.Contains($ignoredDirectory)) { throw "Vite must ignore project-local generated directory: $ignoredDirectory" }
}
if ($vite -notmatch 'testTimeout:\s*15_?000') { throw 'Vitest needs a Windows CI-safe interaction timeout' }
if ($tauri.bundle.windows.nsis.displayLanguageSelector -ne $true) { throw 'Installer language selection must be explicit' }
$languages = @($tauri.bundle.windows.nsis.languages)
if ($languages -notcontains 'SimpChinese' -or $languages -notcontains 'English') {
  throw 'NSIS must include Simplified Chinese and English'
}
$pubkey = [string]$tauri.plugins.updater.pubkey
if ($pubkey.Length -lt 80 -or $pubkey -match 'placeholder|replace|todo') {
  throw 'A real compile-time updater public key is required'
}

$portableFlag = Join-Path $projectRoot 'release\portable\portable.flag'
$portableReadme = Join-Path $projectRoot 'release\portable\README-portable.txt'
if (-not (Test-Path -LiteralPath $portableFlag -PathType Leaf)) { throw 'portable.flag template is missing' }
if (-not (Test-Path -LiteralPath $portableReadme -PathType Leaf)) { throw 'Portable instructions are missing' }
if ((Get-Item -LiteralPath $portableFlag).Length -ne 0) { throw 'portable.flag must be empty' }

$testRoot = Join-Path $projectRoot '.local\package-config-test'
$fakeExecutable = Join-Path $testRoot 'qingzhou-ssh.exe'
$outputDirectory = Join-Path $testRoot 'artifacts'
New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
[IO.File]::WriteAllBytes($fakeExecutable, [byte[]](1, 2, 3, 4))

try {
  & (Join-Path $projectRoot 'scripts\package-portable.ps1') `
    -ExecutablePath $fakeExecutable `
    -Version '9.8.7-test.1' `
    -OutputDirectory $outputDirectory | Out-Null

  $archive = Join-Path $outputDirectory 'QingzhouSSH-v9.8.7-test.1-windows-x86_64-portable.zip'
  if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw 'Portable ZIP was not produced' }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $zip = [IO.Compression.ZipFile]::OpenRead($archive)
  try {
    $entries = @($zip.Entries | ForEach-Object FullName)
    foreach ($required in @('QingzhouSSH.exe', 'portable.flag', 'README-portable.txt', 'LICENSE')) {
      if ($entries -notcontains $required) { throw "Portable ZIP is missing $required" }
    }
  } finally {
    $zip.Dispose()
  }
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolvedTestRoot.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
      throw 'Refusing to clean a package test path outside the project'
    }
    Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
  }
}

Write-Output 'PASS: installer and portable package contract is safe'
