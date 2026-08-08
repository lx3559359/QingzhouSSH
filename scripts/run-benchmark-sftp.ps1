$ErrorActionPreference = 'Stop'

$forward = @($args)
if ($forward.Count -gt 0 -and $forward[0] -eq '--') {
  $forward = if ($forward.Count -eq 1) { @() } else { @($forward[1..($forward.Count - 1)]) }
}
& (Join-Path $PSScriptRoot 'benchmark-sftp.ps1') @forward
