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
  $record = if ($metadata.schemaVersion -eq 2) {
    @($metadata.files | Where-Object { $_.role -eq $Role -and $_.platform -eq 'windows-x86_64-nsis' })
  } else {
    @($metadata.files | Where-Object role -eq $Role)
  }
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
function Stop-ReleaseProcessTree([int]$RootProcessId, [string]$ExpectedDataRoot) {
  $knownIds = [Collections.Generic.HashSet[int]]::new()
  [void]$knownIds.Add($RootProcessId)
  for ($attempt = 1; $attempt -le 20; $attempt++) {
    $processes = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine)
    $expanded = $true
    while ($expanded) {
      $expanded = $false
      foreach ($candidate in $processes) {
        if ($knownIds.Contains([int]$candidate.ParentProcessId) -and $knownIds.Add([int]$candidate.ProcessId)) {
          $expanded = $true
        }
      }
    }
    $targets = @($processes | Where-Object {
      $knownIds.Contains([int]$_.ProcessId) -or
      ($_.Name -eq 'msedgewebview2.exe' -and
        -not [string]::IsNullOrWhiteSpace([string]$_.CommandLine) -and
        ([string]$_.CommandLine).IndexOf($ExpectedDataRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0)
    })
    if ($targets.Count -eq 0) { return }

    # Stop the app first so it cannot create another WebView child while cleanup is running.
    foreach ($target in @($targets | Sort-Object { if ([int]$_.ProcessId -eq $RootProcessId) { 0 } else { 1 } })) {
      Stop-Process -Id ([int]$target.ProcessId) -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 250
  }
  throw "Application processes did not release the smoke profile: $ExpectedDataRoot"
}
function Remove-SmokeDirectoryWithRetry([string]$Path, [int]$Attempts = 20) {
  for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    try {
      Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
      return
    } catch {
      if ($attempt -eq $Attempts) { throw }
      Start-Sleep -Milliseconds 250
    }
  }
}
function Start-And-Check([string]$Executable, [string]$ExpectedDataRoot) {
  $process = Start-Process -FilePath $Executable -PassThru
  Start-Sleep -Seconds $StartupSeconds
  if ($process.HasExited) { throw "Application exited during smoke launch: $Executable" }
  if (-not (Test-Path -LiteralPath $ExpectedDataRoot -PathType Container)) { throw "Application did not initialize its expected data root: $ExpectedDataRoot" }
  Stop-ReleaseProcessTree -RootProcessId $process.Id -ExpectedDataRoot $ExpectedDataRoot
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
  Remove-SmokeDirectoryWithRetry -Path $smokeRoot
}

Write-Output "PASS: portable, install, update replacement and uninstall smoke completed for $($metadata.version)"
