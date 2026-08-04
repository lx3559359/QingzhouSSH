$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$workflowPath = Join-Path $projectRoot '.github\workflows\release.yml'
$smokePath = Join-Path $projectRoot 'scripts\smoke-release.ps1'
$modelscopePath = Join-Path $projectRoot 'scripts\modelscope-release.py'
$comparePath = Join-Path $projectRoot 'scripts\compare-release-sources.ps1'
foreach ($required in @($workflowPath, $smokePath, $modelscopePath, $comparePath)) {
  if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing release pipeline file: $required" }
}

$workflow = Get-Content -Raw -Encoding utf8 $workflowPath
foreach ($requiredText in @(
  'actions/checkout@v6',
  'pnpm/action-setup@v6',
  'actions/setup-node@v6',
  'actions/upload-artifact@v7',
  'windows-2025',
  'pnpm install --frozen-lockfile',
  'pnpm test',
  'cargo clippy --locked',
  'cargo test --locked',
  'scripts\smoke-release.ps1',
  'scripts\modelscope-release.py upload',
  'scripts\modelscope-release.py download',
  'scripts\compare-release-sources.ps1',
  'TAURI_SIGNING_PRIVATE_KEY',
  'TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  'MODELSCOPE_API_TOKEN',
  'MODELSCOPE_HOME',
  'MODELSCOPE_CACHE',
  'gh release',
  'refs/tags/v'
)) {
  if (-not $workflow.Contains($requiredText)) { throw "Release workflow is missing: $requiredText" }
}
if ([regex]::Matches($workflow, 'pnpm tauri build --bundles nsis').Count -ne 1) {
  throw 'Release workflow must build the signed NSIS bundle exactly once'
}
$preflightStart = $workflow.IndexOf('name: Preflight release credentials', [StringComparison]::Ordinal)
$signedBuildStart = $workflow.IndexOf('name: Build signed NSIS bundle once', [StringComparison]::Ordinal)
if ($preflightStart -lt 0 -or $signedBuildStart -lt 0 -or $preflightStart -gt $signedBuildStart) {
  throw 'Tagged releases must validate every credential before signed build or publication'
}
$artifactUploadStart = $workflow.IndexOf('name: Upload workflow artifact', [StringComparison]::Ordinal)
$releaseSmokeStart = $workflow.IndexOf('name: Install, update and portable smoke', [StringComparison]::Ordinal)
if ($artifactUploadStart -lt 0 -or $releaseSmokeStart -lt 0 -or $artifactUploadStart -gt $releaseSmokeStart) {
  throw 'Assembled signed artifacts must be retained before release smoke begins'
}
$preflightBlock = $workflow.Substring($preflightStart, $signedBuildStart - $preflightStart)
foreach ($requiredCredential in @(
  'secrets.TAURI_SIGNING_PRIVATE_KEY',
  'secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD',
  'secrets.MODELSCOPE_API_TOKEN',
  'vars.QINGZHOU_MODELSCOPE_NAMESPACE'
)) {
  if (-not $preflightBlock.Contains($requiredCredential)) { throw "Release preflight is missing: $requiredCredential" }
}
foreach ($requiredValidation in @('[char]0xFEFF', 'leading or trailing whitespace')) {
  if (-not $preflightBlock.Contains($requiredValidation)) { throw "Release preflight format validation is missing: $requiredValidation" }
}
if ($workflow -match 'shell:\s*powershell') { throw 'Release workflow must use PowerShell 7 native error propagation' }
if ([regex]::Matches($workflow, '\$PSNativeCommandUseErrorActionPreference\s*=\s*\$true').Count -lt 10) {
  throw 'Every command-bearing release step must fail on a non-zero native exit code'
}
if ($workflow -match '(?i)(token|private.key|password)\s*:\s*["''][^$][^"'']+["'']') {
  throw 'Release workflow appears to contain a hard-coded secret'
}
if ($workflow -notmatch '(?m)^\s*contents:\s*write\s*$') { throw 'Release publication requires explicit contents: write permission' }
if ($workflow -notmatch '\$\{\{\s*github\.workspace\s*\}\}\\\.local') { throw 'Release caches must be rooted inside the checkout' }
if ($workflow -notmatch '(?s)env:\s+.*?QINGZHOU_MODELSCOPE_NAMESPACE:\s*\$\{\{\s*vars\.QINGZHOU_MODELSCOPE_NAMESPACE\s*\}\}.*?steps:') {
  throw 'ModelScope namespace must be present before the signed compilation step'
}

$smoke = Get-Content -Raw -Encoding utf8 $smokePath
foreach ($requiredText in @('/S', 'portable.flag', '.exe.sig', 'update replacement', 'uninstall')) {
  if (-not $smoke.Contains($requiredText)) { throw "Release smoke test is missing: $requiredText" }
}
if ($smoke -notmatch 'QINGZHOU_DATA_ROOT') { throw 'Installed smoke launch must use an explicit data root' }

$python = Get-Command python -ErrorAction SilentlyContinue
if (-not $python) { throw 'Python is required to validate the ModelScope release helper' }
& $python.Source -B -c "import ast, pathlib; ast.parse(pathlib.Path(r'$modelscopePath').read_text(encoding='utf-8'))"
if ($LASTEXITCODE -ne 0) { throw 'ModelScope release helper is not valid Python' }
$modelscope = Get-Content -Raw -Encoding utf8 $modelscopePath
foreach ($requiredText in @('from modelscope_hub import HubApi', 'repo_type = "studio"', 'releases/latest.json', 'upload_file', 'download_file')) {
  if (-not $modelscope.Contains($requiredText)) { throw "ModelScope helper is missing: $requiredText" }
}

$testRoot = Join-Path $projectRoot '.local\release-pipeline-test'
$inputRoot = Join-Path $testRoot 'input'
$releaseRoot = Join-Path $testRoot 'release'
$githubRoot = Join-Path $testRoot 'readback\github'
$modelscopeRoot = Join-Path $testRoot 'readback\modelscope'
New-Item -ItemType Directory -Force -Path $inputRoot, $githubRoot, $modelscopeRoot | Out-Null
$installer = Join-Path $inputRoot 'QingzhouSSH_0.1.0_x64-setup.exe'
$signature = "$installer.sig"
$portable = Join-Path $inputRoot 'QingzhouSSH-v0.1.0-windows-x86_64-portable.zip'
$fixturePublicKey = 'RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3'
$fixtureSignature = "untrusted comment: signature from minisign secret key`nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=`ntrusted comment: timestamp:1633700835`tfile:test`tprehashed`nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ=="
[IO.File]::WriteAllBytes($installer, [Text.UTF8Encoding]::new($false).GetBytes('test'))
[IO.File]::WriteAllText($signature, [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($fixtureSignature)), [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllBytes($portable, [byte[]](65..96))

try {
  & (Join-Path $projectRoot 'scripts\build-release.ps1') `
    -InstallerPath $installer `
    -UpdaterSignaturePath $signature `
    -PortableArchivePath $portable `
    -OutputDirectory $releaseRoot `
    -ModelScopeNamespace 'lx3559359' `
    -PublishedAt '2026-08-04T10:00:00Z' | Out-Null

  $metadata = Get-Content -Raw -Encoding utf8 (Join-Path $releaseRoot 'release-metadata.json') | ConvertFrom-Json
  $commonFiles = @($metadata.files | ForEach-Object name) + @('SHA256SUMS', 'SBOM.spdx.json', 'release-metadata.json')
  foreach ($name in $commonFiles) {
    Copy-Item -LiteralPath (Join-Path $releaseRoot $name) -Destination (Join-Path $githubRoot $name)
    Copy-Item -LiteralPath (Join-Path $releaseRoot $name) -Destination (Join-Path $modelscopeRoot $name)
  }
  Copy-Item -LiteralPath (Join-Path $releaseRoot 'github\latest.json') -Destination (Join-Path $githubRoot 'latest.json')
  Copy-Item -LiteralPath (Join-Path $releaseRoot 'modelscope\latest.json') -Destination (Join-Path $modelscopeRoot 'latest.json')

  & $comparePath -ReleaseDirectory $releaseRoot -GitHubDirectory $githubRoot -ModelScopeDirectory $modelscopeRoot -UpdaterPublicKey $fixturePublicKey | Out-Null

  [IO.File]::WriteAllBytes((Join-Path $modelscopeRoot $metadata.updaterFile), [byte[]](9, 9, 9))
  $tamperRejected = $false
  try {
    & $comparePath -ReleaseDirectory $releaseRoot -GitHubDirectory $githubRoot -ModelScopeDirectory $modelscopeRoot -UpdaterPublicKey $fixturePublicKey | Out-Null
  } catch {
    $tamperRejected = $true
  }
  if (-not $tamperRejected) { throw 'Dual-source comparison must reject a changed mirrored artifact' }
} finally {
  if (Test-Path -LiteralPath $testRoot) {
    $resolved = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolved.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
      throw 'Refusing to clean a pipeline test path outside the project'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
}

Write-Output 'PASS: release pipeline builds once, smokes, mirrors and compares both sources'
