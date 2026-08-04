function Find-InstalledExecutable([string]$InstallLocation) {
  return Get-ChildItem -LiteralPath $InstallLocation -Filter '*.exe' -Recurse | Where-Object {
    -not $_.PSIsContainer -and $_.Name -notmatch 'uninstall'
  } | Select-Object -First 1
}
