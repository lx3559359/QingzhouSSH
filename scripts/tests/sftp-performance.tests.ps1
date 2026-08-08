$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$benchmark = Join-Path $projectRoot 'scripts\benchmark-sftp.ps1'
$wrapper = Join-Path $projectRoot 'scripts\run-benchmark-sftp.ps1'
if (-not (Test-Path -LiteralPath $benchmark -PathType Leaf)) {
  throw 'SFTP benchmark runner is missing.'
}
if (-not (Test-Path -LiteralPath $wrapper -PathType Leaf)) {
  throw 'SFTP benchmark package-manager wrapper is missing.'
}

$source = Get-Content -Raw -Encoding utf8 -LiteralPath $benchmark
foreach ($parameter in @('RttMs', 'BandwidthMbps', 'PayloadBytes', 'Iterations', 'OutputPath')) {
  if ($source -notmatch ('\$' + [regex]::Escape($parameter) + '\b')) {
    throw "Benchmark parameter is missing: $parameter"
  }
}
foreach ($field in @(
  'version', 'commit', 'platform', 'architecture', 'rttMs', 'bandwidthMbps', 'payloadBytes',
  'samples', 'medianDirectoryMs', 'medianUploadMbps', 'medianDownloadMbps',
  'verificationPolicy', 'cpuSeconds', 'peakMemoryBytes', 'progressEventCount',
  'cancellationLatencyMs', 'networkShape'
)) {
  if ($source -notmatch [regex]::Escape($field)) { throw "Benchmark JSON field is missing: $field" }
}
if ($source -notmatch 'finally' -or $source -notmatch "-Action\s+Stop") {
  throw 'Benchmark must stop the fixture in a finally block.'
}
if ($source -notmatch 'artifacts[\\/]benchmarks' -or $source -notmatch 'GetFullPath') {
  throw 'Benchmark output must be confined to artifacts/benchmarks.'
}
if ($source -match 'ApplicationData|LocalApplicationData|APPDATA') {
  throw 'Benchmark must not write to AppData.'
}
$wrapperSource = Get-Content -Raw -Encoding utf8 -LiteralPath $wrapper
if ($wrapperSource -notmatch "'-{2}'" -or $wrapperSource -notmatch 'benchmark-sftp\.ps1') {
  throw 'Benchmark wrapper must remove the package-manager separator and forward arguments.'
}
$package = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
if (-not $package.scripts.'test:sftp-performance-contract' -or -not $package.scripts.'benchmark:sftp') {
  throw 'SFTP performance package scripts are missing.'
}

function Assert-Rejected([object[]]$Arguments, [string]$Case) {
  $rejected = $false
  try {
    & $benchmark @Arguments | Out-Null
  } catch {
    $rejected = $true
  }
  if (-not $rejected) { throw "Benchmark accepted invalid input: $Case" }
}

Assert-Rejected @('-RttMs', -1) 'negative RTT'
Assert-Rejected @('-BandwidthMbps', 0) 'zero bandwidth'
Assert-Rejected @('-PayloadBytes', 0) 'zero payload'
Assert-Rejected @('-Iterations', 0) 'zero iterations'
Assert-Rejected @('-OutputPath', '.local\outside.json') 'output outside artifacts/benchmarks'

Write-Output 'SFTP performance benchmark contract passed.'
