function Find-InstalledExecutable([string]$InstallLocation) {
  $normalizedInstallLocation = $InstallLocation.Trim().Trim('"')
  if ([string]::IsNullOrWhiteSpace($normalizedInstallLocation)) { return $null }
  return Get-ChildItem -LiteralPath $normalizedInstallLocation -Filter '*.exe' -Recurse | Where-Object {
    -not $_.PSIsContainer -and $_.Name -notmatch 'uninstall'
  } | Select-Object -First 1
}
