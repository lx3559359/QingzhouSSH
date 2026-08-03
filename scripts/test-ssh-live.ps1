param([switch]$SkipPythonDependencyInstall)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$fixture = Join-Path $PSScriptRoot 'ssh-fixture.ps1'
. (Join-Path $PSScriptRoot 'dev-env.ps1') -Quiet

try {
  & $fixture -Action Start -SkipPythonDependencyInstall:$SkipPythonDependencyInstall
  foreach ($testName in @('ssh_live', 'sftp_live', 'm2_live')) {
    cargo test --locked --manifest-path (Join-Path $projectRoot 'src-tauri\Cargo.toml') --test $testName -- --ignored --nocapture --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw "Live test failed: $testName" }
  }
} finally {
  & $fixture -Action Stop
}
