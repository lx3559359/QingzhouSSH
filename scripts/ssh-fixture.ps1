param(
  [ValidateSet('Start', 'Stop', 'Status', 'Capabilities')]
  [string]$Action = 'Status',
  [switch]$SkipPythonDependencyInstall
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$localRoot = Join-Path $projectRoot '.local'
$pythonPackages = Join-Path $localRoot 'python-packages'
$pipCache = Join-Path $localRoot 'pip-cache'
$pythonCache = Join-Path $localRoot 'pycache'
$keyRoot = Join-Path $localRoot 'test-keys'
$runtimeRoot = Join-Path $localRoot 'ssh-fixture'
$remoteRoot = Join-Path $runtimeRoot 'remote-root'
$pidFile = Join-Path $runtimeRoot 'fixture.pid'
$stdoutLog = Join-Path $runtimeRoot 'fixture.stdout.log'
$stderrLog = Join-Path $runtimeRoot 'fixture.stderr.log'
$fixtureRoot = Join-Path $projectRoot 'tests\fixtures\sshd'
$requirements = Join-Path $fixtureRoot 'requirements.txt'
$fixtureScript = Join-Path $fixtureRoot 'server.py'
$hostKey = Join-Path $keyRoot 'ssh_host_rsa_key'
$clientKey = Join-Path $keyRoot 'id_ed25519'
$authorizedKeys = Join-Path $keyRoot 'authorized_keys'

function Assert-ProjectPath([string]$Path) {
  $fullPath = [IO.Path]::GetFullPath($Path)
  if (-not $fullPath.StartsWith($projectPrefix, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Fixture path escapes the project root: $fullPath"
  }
  return $fullPath
}

function Get-FixtureProcess {
  if (-not (Test-Path -LiteralPath $pidFile)) { return $null }
  $fixturePid = 0
  if (-not [int]::TryParse((Get-Content -Raw -LiteralPath $pidFile).Trim(), [ref]$fixturePid)) { return $null }
  return Get-Process -Id $fixturePid -ErrorAction SilentlyContinue
}

function Stop-Fixture {
  $fixtureProcess = Get-FixtureProcess
  if ($fixtureProcess) {
    Stop-Process -Id $fixtureProcess.Id -Force
    Wait-Process -Id $fixtureProcess.Id -ErrorAction SilentlyContinue
  }
  if (Test-Path -LiteralPath $pidFile) { Remove-Item -LiteralPath $pidFile -Force }
  $safeRemoteRoot = Assert-ProjectPath $remoteRoot
  if (Test-Path -LiteralPath $safeRemoteRoot) { Remove-Item -LiteralPath $safeRemoteRoot -Recurse -Force }
}

if ($Action -eq 'Stop') {
  Stop-Fixture
  Write-Output 'QingzhouSSH fixture stopped.'
  exit 0
}

if ($Action -eq 'Status') {
  $fixtureProcess = Get-FixtureProcess
  if ($fixtureProcess) { Write-Output "running:$($fixtureProcess.Id)" } else { Write-Output 'stopped' }
  exit 0
}

if ($Action -eq 'Capabilities') {
  [pscustomobject]@{
    networkShape = 'unavailable'
    reason = 'The project-local direct fixture does not mutate host network settings.'
    supportsLargePayload = $true
    metricsPrefix = 'QZ_SFTP_METRICS='
  } | ConvertTo-Json -Compress
  exit 0
}

$existing = Get-FixtureProcess
if ($existing) {
  Write-Output "QingzhouSSH fixture already running with PID $($existing.Id)."
  exit 0
}
Stop-Fixture

foreach ($path in @($pythonPackages, $pipCache, $pythonCache, $keyRoot, $runtimeRoot, $remoteRoot)) {
  $safePath = Assert-ProjectPath $path
  New-Item -ItemType Directory -Force -Path $safePath | Out-Null
}

$env:PIP_CACHE_DIR = $pipCache
$env:PYTHONPYCACHEPREFIX = $pythonCache
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:PIP_DISABLE_PIP_VERSION_CHECK = '1'
$env:PYTHONPATH = $pythonPackages
$python = (Get-Command python -ErrorAction Stop).Source

if (-not (Test-Path -LiteralPath (Join-Path $pythonPackages 'asyncssh'))) {
  if ($SkipPythonDependencyInstall) { throw "AsyncSSH is missing from $pythonPackages" }
  & $python -B -m pip install --target $pythonPackages --cache-dir $pipCache --no-warn-script-location -r $requirements
  if ($LASTEXITCODE -ne 0) { throw 'Project-local AsyncSSH installation failed' }
}

if (-not (Test-Path -LiteralPath $hostKey)) {
  & ssh-keygen -q -t rsa -b 3072 -N '""' -C 'qingzhou-host-fixture-only' -f $hostKey
  if ($LASTEXITCODE -ne 0) { throw 'Fixture host key generation failed' }
}
if (-not (Test-Path -LiteralPath $clientKey)) {
  & ssh-keygen -q -t ed25519 -N 'fixture-passphrase' -C 'qingzhou-fixture-only' -f $clientKey
  if ($LASTEXITCODE -ne 0) { throw 'Fixture client key generation failed' }
}
Copy-Item -LiteralPath "$clientKey.pub" -Destination $authorizedKeys -Force

$startInfo = @{
  FilePath = $python
  ArgumentList = @('-B', '-S', "`"$fixtureScript`"", '--host-key', "`"$hostKey`"", '--authorized-keys', "`"$authorizedKeys`"", '--remote-root', "`"$remoteRoot`"")
  WorkingDirectory = $projectRoot
  WindowStyle = 'Hidden'
  RedirectStandardOutput = $stdoutLog
  RedirectStandardError = $stderrLog
  PassThru = $true
}
$fixtureProcess = Start-Process @startInfo
Set-Content -LiteralPath $pidFile -Value $fixtureProcess.Id -Encoding ascii

$deadline = [DateTime]::UtcNow.AddSeconds(15)
$ready = $false
while ([DateTime]::UtcNow -lt $deadline -and -not $fixtureProcess.HasExited) {
  $client = [System.Net.Sockets.TcpClient]::new()
  try {
    $connect = $client.ConnectAsync('127.0.0.1', 2222)
    if ($connect.Wait(250) -and $client.Connected) { $ready = $true; break }
  } catch {
    # The fixture may still be importing dependencies and binding the port.
  } finally {
    $client.Dispose()
  }
  Start-Sleep -Milliseconds 200
}

if (-not $ready) {
  $details = if (Test-Path -LiteralPath $stderrLog) { Get-Content -Raw -LiteralPath $stderrLog } else { 'No fixture error log was produced.' }
  Stop-Fixture
  throw "Project-local SSH fixture did not start.`n$details"
}

Write-Output "QingzhouSSH fixture started with PID $($fixtureProcess.Id)."
