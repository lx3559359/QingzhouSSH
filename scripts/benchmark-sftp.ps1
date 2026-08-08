param(
  [double]$RttMs = 0,
  [double]$BandwidthMbps = 1000,
  [long]$PayloadBytes = 16777216,
  [int]$Iterations = 3,
  [string]$OutputPath = 'artifacts\benchmarks\local-sftp.json',
  [switch]$SkipPythonDependencyInstall
)

$ErrorActionPreference = 'Stop'

if ($RttMs -lt 0) { throw 'RttMs must be zero or greater.' }
if ($BandwidthMbps -le 0) { throw 'BandwidthMbps must be greater than zero.' }
if ($PayloadBytes -le 0) { throw 'PayloadBytes must be greater than zero.' }
if ($PayloadBytes -gt 512MB) { throw 'PayloadBytes must not exceed 512 MiB.' }
if ($Iterations -le 0) { throw 'Iterations must be greater than zero.' }
if ([string]::IsNullOrWhiteSpace($OutputPath)) { throw 'OutputPath is required.' }

$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$benchmarkRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'artifacts\benchmarks'))
$benchmarkPrefix = $benchmarkRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$resolvedOutput = if ([IO.Path]::IsPathRooted($OutputPath)) {
  [IO.Path]::GetFullPath($OutputPath)
} else {
  [IO.Path]::GetFullPath((Join-Path $projectRoot $OutputPath))
}
if (-not $resolvedOutput.StartsWith($benchmarkPrefix, [StringComparison]::OrdinalIgnoreCase)) {
  throw 'OutputPath must be a file under artifacts/benchmarks.'
}
if ([IO.Path]::GetExtension($resolvedOutput) -ne '.json') {
  throw 'OutputPath must use the .json extension.'
}

$fixture = Join-Path $projectRoot 'scripts\ssh-fixture.ps1'
$manifest = Join-Path $projectRoot 'src-tauri\Cargo.toml'
$runRoot = Join-Path $projectRoot '.local\sftp-benchmark'
$previousPayload = $env:QZ_SFTP_LARGE_TEST_BYTES
$fixtureStarted = $false
$networkShape = 'unavailable'
$networkShapeReason = 'No project-scoped traffic shaper is available on this host; results are loopback diagnostics.'

function Get-Median([double[]]$Values) {
  if (-not $Values -or $Values.Count -eq 0) { return $null }
  $ordered = @($Values | Sort-Object)
  $middle = [Math]::Floor($ordered.Count / 2)
  if ($ordered.Count % 2 -eq 1) { return [double]$ordered[$middle] }
  return ([double]$ordered[$middle - 1] + [double]$ordered[$middle]) / 2
}

function Stop-NetworkShape {
  # Deliberately a no-op until a project-scoped Linux CI fixture exposes tc/netem.
}

function Initialize-Cargo {
  . (Join-Path $projectRoot 'scripts\dev-env.ps1') -Quiet
  $cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
  if ($cargoCommand) { return $cargoCommand.Source }

  $candidateRoots = @(
    $projectRoot,
    [IO.Path]::GetFullPath((Join-Path $projectRoot '..\..'))
  ) | Select-Object -Unique
  foreach ($candidateRoot in $candidateRoots) {
    $candidateCargoHome = Join-Path $candidateRoot '.local\cargo-home'
    $candidateRustupHome = Join-Path $candidateRoot '.local\rustup-home'
    $candidateCargo = Join-Path $candidateCargoHome 'bin\cargo.exe'
    if (Test-Path -LiteralPath $candidateCargo -PathType Leaf) {
      $env:CARGO_HOME = $candidateCargoHome
      $env:RUSTUP_HOME = $candidateRustupHome
      $env:Path = "$(Join-Path $candidateCargoHome 'bin');$env:Path"
      return $candidateCargo
    }
  }
  throw 'cargo is unavailable. Run scripts/install-dev-toolchain.ps1 first.'
}

function Invoke-SftpMeasurement(
  [string]$CargoPath,
  [int]$Iteration,
  [bool]$Warmup
) {
  $label = if ($Warmup) { 'warmup' } else { "iteration-$Iteration" }
  $stdoutPath = Join-Path $runRoot "$label.stdout.log"
  $stderrPath = Join-Path $runRoot "$label.stderr.log"
  Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
  $arguments = @(
    'test', '--manifest-path', "`"$manifest`"", '--test', 'sftp_live', '--',
    '--ignored', '--nocapture', '--test-threads=1'
  )
  $startInfo = @{
    FilePath = $CargoPath
    ArgumentList = $arguments
    WorkingDirectory = $projectRoot
    RedirectStandardOutput = $stdoutPath
    RedirectStandardError = $stderrPath
    PassThru = $true
  }
  if ([Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([Runtime.InteropServices.OSPlatform]::Windows)) {
    $startInfo.WindowStyle = 'Hidden'
  }
  $process = Start-Process @startInfo
  $peakMemoryBytes = 0L
  $cpuSeconds = 0.0
  while (-not $process.HasExited) {
    Start-Sleep -Milliseconds 50
    try {
      $process.Refresh()
      $peakMemoryBytes = [Math]::Max($peakMemoryBytes, [long]$process.PeakWorkingSet64)
      $cpuSeconds = [Math]::Max($cpuSeconds, [double]$process.TotalProcessorTime.TotalSeconds)
    } catch {
      # The process can exit between HasExited and Refresh.
    }
  }
  $process.WaitForExit()
  $cpuSeconds = [Math]::Round($cpuSeconds, 6)
  $combined = ''
  if (Test-Path -LiteralPath $stdoutPath) { $combined += Get-Content -Raw -LiteralPath $stdoutPath }
  if (Test-Path -LiteralPath $stderrPath) { $combined += "`n" + (Get-Content -Raw -LiteralPath $stderrPath) }
  $matches = [regex]::Matches($combined, '(?m)^QZ_SFTP_METRICS=(\{.*\})\s*$')
  $metrics = if ($matches.Count -gt 0) {
    $matches[$matches.Count - 1].Groups[1].Value | ConvertFrom-Json
  } else {
    $null
  }
  $exitCode = if ($combined -match 'test result:\s+ok\.') { 0 } else { 1 }
  $success = $exitCode -eq 0 -and $null -ne $metrics
  $uploadMbps = if ($success -and [double]$metrics.uploadMs -gt 0) {
    [Math]::Round(($PayloadBytes * 8.0) / ([double]$metrics.uploadMs * 1000.0), 3)
  } else { $null }
  $downloadMbps = if ($success -and [double]$metrics.downloadMs -gt 0) {
    [Math]::Round(($PayloadBytes * 8.0) / ([double]$metrics.downloadMs * 1000.0), 3)
  } else { $null }

  [pscustomobject]@{
    iteration = $Iteration
    warmup = $Warmup
    success = $success
    exitCode = $exitCode
    directoryMs = if ($metrics) { [long]$metrics.directoryMs } else { $null }
    uploadMs = if ($metrics) { [long]$metrics.uploadMs } else { $null }
    downloadMs = if ($metrics) { [long]$metrics.downloadMs } else { $null }
    uploadMbps = $uploadMbps
    downloadMbps = $downloadMbps
    verificationPolicy = if ($metrics) { [string]$metrics.verificationPolicy } else { $null }
    progressEventCount = if ($metrics) { [long]$metrics.progressEventCount } else { $null }
    cancellationLatencyMs = if ($metrics) { [long]$metrics.cancellationLatencyMs } else { $null }
    pipelineMaxInFlight = if ($metrics) { [int]$metrics.pipelineMaxInFlight } else { $null }
    pipelineMaxBufferedBytes = if ($metrics) { [long]$metrics.pipelineMaxBufferedBytes } else { $null }
    cpuSeconds = $cpuSeconds
    peakMemoryBytes = $peakMemoryBytes
    error = if ($success) { $null } else { (($combined -split "`r?`n") | Select-Object -Last 1) }
  }
}

$samples = @()
try {
  New-Item -ItemType Directory -Force -Path $benchmarkRoot, $runRoot | Out-Null
  $cargo = Initialize-Cargo
  $env:QZ_SFTP_LARGE_TEST_BYTES = [string]$PayloadBytes
  & $fixture -Action Start -SkipPythonDependencyInstall:$SkipPythonDependencyInstall | Out-Null
  $fixtureStarted = $true

  $warmup = Invoke-SftpMeasurement -CargoPath $cargo -Iteration 0 -Warmup $true
  if (-not $warmup.success) { throw 'SFTP benchmark warm-up failed correctness checks.' }

  $measuredIterations = [Math]::Max(3, $Iterations)
  foreach ($iteration in 1..$measuredIterations) {
    $samples += Invoke-SftpMeasurement -CargoPath $cargo -Iteration $iteration -Warmup $false
  }

  $successful = @($samples | Where-Object success)
  $package = Get-Content -Raw -Encoding utf8 -LiteralPath (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
  $commit = (& git -C $projectRoot rev-parse --short HEAD 2>$null)
  if (-not $commit) { $commit = 'unknown' }
  $report = [ordered]@{
    schemaVersion = 1
    version = [string]$package.version
    commit = [string]$commit
    generatedAt = [DateTime]::UtcNow.ToString('o')
    platform = [Runtime.InteropServices.RuntimeInformation]::OSDescription
    architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    rttMs = $RttMs
    bandwidthMbps = $BandwidthMbps
    networkShape = $networkShape
    networkShapeReason = $networkShapeReason
    payloadBytes = $PayloadBytes
    requestedIterations = $Iterations
    measuredIterations = $samples.Count
    verificationPolicy = 'balanced'
    samples = @($samples)
    medianDirectoryMs = Get-Median @($successful | ForEach-Object { [double]$_.directoryMs })
    medianUploadMbps = Get-Median @($successful | ForEach-Object { [double]$_.uploadMbps })
    medianDownloadMbps = Get-Median @($successful | ForEach-Object { [double]$_.downloadMbps })
    cpuSeconds = [Math]::Round(($samples | Measure-Object -Property cpuSeconds -Sum).Sum, 6)
    peakMemoryBytes = ($samples | Measure-Object -Property peakMemoryBytes -Maximum).Maximum
    progressEventCount = ($samples | Measure-Object -Property progressEventCount -Sum).Sum
    cancellationLatencyMs = Get-Median @($successful | ForEach-Object { [double]$_.cancellationLatencyMs })
  }
  $json = $report | ConvertTo-Json -Depth 8
  $outputDirectory = Split-Path -Parent $resolvedOutput
  New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
  [IO.File]::WriteAllText($resolvedOutput, $json, [Text.UTF8Encoding]::new($false))

  if ($successful.Count -ne $samples.Count) {
    throw "One or more benchmark samples failed correctness checks. Report: $resolvedOutput"
  }
  Write-Output $resolvedOutput
} finally {
  Stop-NetworkShape
  if ($fixtureStarted -or (Test-Path -LiteralPath $fixture)) {
    & $fixture -Action Stop | Out-Null
  }
  if ($null -eq $previousPayload) {
    Remove-Item Env:QZ_SFTP_LARGE_TEST_BYTES -ErrorAction SilentlyContinue
  } else {
    $env:QZ_SFTP_LARGE_TEST_BYTES = $previousPayload
  }
}
