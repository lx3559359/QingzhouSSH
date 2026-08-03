$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$fixtureScript = Join-Path $projectRoot 'scripts\ssh-fixture.ps1'
$remoteRoot = Join-Path $projectRoot '.local\ssh-fixture\remote-root'

try {
  & $fixtureScript -Action Stop
  & $fixtureScript -Action Stop
  & $fixtureScript -Action Start -SkipPythonDependencyInstall
  $status = & $fixtureScript -Action Status
  if ($status -notmatch '^running:\d+$') { throw "Unexpected fixture status: $status" }
  foreach ($relative in @('var\log\qingzhou.log', 'var\log\qingzhou.log.gz', 'tmp', 'opt\qingzhou-app\config.yml', 'run\qingzhou-fixture\service.state')) {
    if (-not (Test-Path -LiteralPath (Join-Path $remoteRoot $relative))) {
      throw "Fixture asset is missing: $relative"
    }
  }
  $liveScript = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'scripts\test-ssh-live.ps1')
  if ($liveScript -notmatch "'workflow_live'") { throw 'Workflow live suite is not wired into test-ssh-live.ps1.' }
} finally {
  & $fixtureScript -Action Stop
  & $fixtureScript -Action Stop
}

if (Test-Path -LiteralPath $remoteRoot) { throw 'Fixture remote root was not cleaned.' }
Write-Output 'SSH fixture lifecycle checks passed.'
