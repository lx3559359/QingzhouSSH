param(
  [Parameter(Mandatory = $true)] [string]$ReleaseDirectory,
  [switch]$AllowLocalMachineMutation,
  [int]$StartupSeconds = 5
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$releaseRoot = [IO.Path]::GetFullPath($ReleaseDirectory)
if (-not $releaseRoot.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Release smoke input must remain inside the project' }
if (-not $AllowLocalMachineMutation -and $env:CI -ne 'true') { throw 'Installer smoke changes HKCU and requires explicit -AllowLocalMachineMutation outside CI' }
. (Join-Path $PSScriptRoot 'lib\release-smoke.ps1')
& (Join-Path $PSScriptRoot 'verify-release.ps1') -ReleaseDirectory $releaseRoot | Out-Null

$metadata = Get-Content -Raw -Encoding utf8 (Join-Path $releaseRoot 'release-metadata.json') | ConvertFrom-Json
function File-ForRole([string]$Role) {
  $record = @($metadata.files | Where-Object role -eq $Role)
  if ($record.Count -ne 1) { throw "Release role is missing: $Role" }
  return Join-Path $releaseRoot ([string]$record[0].name)
}
function Installed-Product {
  $root = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
  return Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue | ForEach-Object {
    Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction SilentlyContinue
  } | Where-Object { $_.DisplayVersion -eq $metadata.version -and $_.DisplayName -match 'SSH' } | Select-Object -First 1
}
function Command-Path([string]$Value) {
  if ($Value -match '^"([^"]+)"') { return $Matches[1] }
  return ($Value -split '\s+')[0]
}
function Start-And-Check([string]$Executable, [string]$ExpectedDataRoot) {
  $process = Start-Process -FilePath $Executable -PassThru
  Start-Sleep -Seconds $StartupSeconds
  if ($process.HasExited) { throw "Application exited during smoke launch: $Executable" }
  if (-not (Test-Path -LiteralPath $ExpectedDataRoot -PathType Container)) { throw "Application did not initialize its expected data root: $ExpectedDataRoot" }
  Stop-Process -Id $process.Id -Force
  $process.WaitForExit()
}

$installer = File-ForRole 'installer-updater'
$portableArchive = File-ForRole 'portable'
# Tauri signs the NSIS installer directly as setup.exe.sig; the same EXE is the updater payload.
$updaterSignature = File-ForRole 'updater-signature' # .exe.sig
$smokeRoot = Join-Path $projectRoot ('.local\release-smoke-' + [Guid]::NewGuid().ToString('N'))
$portableRoot = Join-Path $smokeRoot 'portable'
$installedData = Join-Path $smokeRoot 'installed-data'
$oldDataRoot = $env:QINGZHOU_DATA_ROOT
$uninstaller = $null

New-Item -ItemType Directory -Force -Path $portableRoot, $installedData | Out-Null
try {
  if (-not (Test-Path -LiteralPath $updaterSignature -PathType Leaf)) { throw 'Signed NSIS installer has no .exe.sig sidecar' }
  Expand-Archive -LiteralPath $portableArchive -DestinationPath $portableRoot
  $portableExe = Join-Path $portableRoot 'QingzhouSSH.exe'
  if (-not (Test-Path -LiteralPath (Join-Path $portableRoot 'portable.flag') -PathType Leaf)) { throw 'Portable smoke package has no portable.flag' }
  Remove-Item Env:QINGZHOU_DATA_ROOT -ErrorAction SilentlyContinue
  Start-And-Check $portableExe (Join-Path $portableRoot 'data')

  $install = Start-Process -FilePath $installer -ArgumentList '/S' -Wait -PassThru
  if ($install.ExitCode -ne 0) { throw "Silent current-user installation failed: $($install.ExitCode)" }
  $product = Installed-Product
  if ($null -eq $product) { throw 'Current-user uninstall registry entry was not created' }
  $uninstaller = Command-Path ([string]$product.UninstallString)
  $installLocation = [string]$product.InstallLocation
  if ([string]::IsNullOrWhiteSpace($installLocation)) { $installLocation = Split-Path $uninstaller -Parent }
  $installedExe = Find-InstalledExecutable $installLocation
  if ($null -eq $installedExe) { throw 'Installed executable was not found' }
  $env:QINGZHOU_DATA_ROOT = $installedData
  Start-And-Check $installedExe.FullName $installedData

  $updateInstaller = Get-Item -LiteralPath $installer
  $installedHash = (Get-FileHash -LiteralPath $installedExe.FullName -Algorithm SHA256).Hash
  # update replacement: corrupt the stopped app, then require the signed updater installer to restore it.
  [IO.File]::WriteAllBytes($installedExe.FullName, [byte[]](1, 2, 3, 4))
  $update = Start-Process -FilePath $updateInstaller.FullName -ArgumentList '/S' -Wait -PassThru
  if ($update.ExitCode -ne 0 -or (Get-FileHash -LiteralPath $installedExe.FullName -Algorithm SHA256).Hash -ne $installedHash) {
    throw 'Signed updater did not replace the installed executable'
  }

  # uninstall smoke must remove the installed application without elevation.
  $remove = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
  if ($remove.ExitCode -ne 0) { throw "Silent uninstall failed: $($remove.ExitCode)" }
  Start-Sleep -Seconds 2
  if (Test-Path -LiteralPath $installedExe.FullName) { throw 'Uninstall left the application executable behind' }
  $uninstaller = $null
} catch {
  $failure = $_
  $source = if ([string]::IsNullOrWhiteSpace($failure.InvocationInfo.ScriptName)) {
    'smoke-release.ps1'
  } else {
    Split-Path $failure.InvocationInfo.ScriptName -Leaf
  }
  throw "Release smoke failed at ${source}:$($failure.InvocationInfo.ScriptLineNumber): $($failure.Exception.Message)"
} finally {
  if ($null -ne $uninstaller -and (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
    Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait | Out-Null
  }
  if ($null -eq $oldDataRoot) { Remove-Item Env:QINGZHOU_DATA_ROOT -ErrorAction SilentlyContinue } else { $env:QINGZHOU_DATA_ROOT = $oldDataRoot }
  if (Test-Path -LiteralPath $smokeRoot) { Remove-Item -LiteralPath $smokeRoot -Recurse -Force }
}

Write-Output "PASS: portable, install, update replacement and uninstall smoke completed for $($metadata.version)"
