param(
  [switch]$SkipPythonDependencyInstall
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$localRoot = Join-Path $projectRoot '.local'
$pythonPackages = Join-Path $localRoot 'python-packages'
$pipCache = Join-Path $localRoot 'pip-cache'
$pythonCache = Join-Path $localRoot 'pycache'
$keyRoot = Join-Path $localRoot 'test-keys'
$fixtureRoot = Join-Path $projectRoot 'tests\fixtures\sshd'
$requirements = Join-Path $fixtureRoot 'requirements.txt'
$fixtureScript = Join-Path $fixtureRoot 'server.py'
$hostKey = Join-Path $keyRoot 'ssh_host_rsa_key'
$clientKey = Join-Path $keyRoot 'id_ed25519'
$authorizedKeys = Join-Path $keyRoot 'authorized_keys'
$stdoutLog = Join-Path $keyRoot 'fixture.stdout.log'
$stderrLog = Join-Path $keyRoot 'fixture.stderr.log'

New-Item -ItemType Directory -Force -Path $pythonPackages, $pipCache, $pythonCache, $keyRoot | Out-Null

$env:PIP_CACHE_DIR = $pipCache
$env:PYTHONPYCACHEPREFIX = $pythonCache
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:PIP_DISABLE_PIP_VERSION_CHECK = '1'
$env:PYTHONPATH = $pythonPackages

$python = (Get-Command python -ErrorAction Stop).Source
if (-not (Test-Path -LiteralPath (Join-Path $pythonPackages 'asyncssh'))) {
  if ($SkipPythonDependencyInstall) {
    throw "AsyncSSH is missing from $pythonPackages"
  }
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

. (Join-Path $PSScriptRoot 'dev-env.ps1') -Quiet

$fixture = $null
try {
  $startInfo = @{
    FilePath = $python
    ArgumentList = @(
      '-B',
      '-S',
      "`"$fixtureScript`"",
      '--host-key',
      "`"$hostKey`"",
      '--authorized-keys',
      "`"$authorizedKeys`""
    )
    WorkingDirectory = $projectRoot
    WindowStyle = 'Hidden'
    RedirectStandardOutput = $stdoutLog
    RedirectStandardError = $stderrLog
    PassThru = $true
  }
  $fixture = Start-Process @startInfo

  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  $ready = $false
  while ([DateTime]::UtcNow -lt $deadline -and -not $fixture.HasExited) {
    $client = [System.Net.Sockets.TcpClient]::new()
    try {
      $connect = $client.ConnectAsync('127.0.0.1', 2222)
      if ($connect.Wait(250) -and $client.Connected) {
        $ready = $true
        break
      }
    } catch {
      # The fixture may still be importing dependencies and binding the port.
    } finally {
      $client.Dispose()
    }
    Start-Sleep -Milliseconds 200
  }

  if (-not $ready) {
    $details = if (Test-Path -LiteralPath $stderrLog) {
      Get-Content -Raw -LiteralPath $stderrLog
    } else {
      'No fixture error log was produced.'
    }
    throw "Project-local SSH fixture did not start.`n$details"
  }

  cargo test --manifest-path (Join-Path $projectRoot 'src-tauri\Cargo.toml') --test ssh_live -- --ignored --test-threads=1
  if ($LASTEXITCODE -ne 0) { throw 'Live SSH tests failed' }
} finally {
  if ($fixture -and -not $fixture.HasExited) {
    Stop-Process -Id $fixture.Id -Force
    Wait-Process -Id $fixture.Id -ErrorAction SilentlyContinue
  }
}
