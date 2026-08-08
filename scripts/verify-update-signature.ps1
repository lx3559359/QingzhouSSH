param(
  [Parameter(Mandatory = $true)] [string]$UpdaterPublicKey,
  [Parameter(Mandatory = $true)] [string]$SignaturePath,
  [Parameter(Mandatory = $true)] [string]$ArtifactPath
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$projectPrefix = $projectRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$pathComparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
$resolvedInputs = foreach ($candidate in @($SignaturePath, $ArtifactPath)) {
  $resolved = [IO.Path]::GetFullPath($candidate)
  if (-not $resolved.StartsWith($projectPrefix, $pathComparison) -or -not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
    throw "Signature verification input must be a project file: $resolved"
  }
  $resolved
}
$resolvedSignature = $resolvedInputs[0]
$resolvedArtifact = $resolvedInputs[1]
$oldTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = Join-Path (Join-Path $projectRoot 'target') 'release-signature-verifier'
try {
  $oldErrorActionPreference = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    $output = & cargo run --quiet --locked `
      --manifest-path (Join-Path (Join-Path $PSScriptRoot 'release-signature-verifier') 'Cargo.toml') `
      -- $UpdaterPublicKey $resolvedSignature $resolvedArtifact 2>&1
    $cargoExitCode = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $oldErrorActionPreference
  }
  if ($cargoExitCode -ne 0) { throw "Updater signature cryptographic verification failed: $($output -join [Environment]::NewLine)" }
} finally {
  if ($null -eq $oldTarget) { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue } else { $env:CARGO_TARGET_DIR = $oldTarget }
}
