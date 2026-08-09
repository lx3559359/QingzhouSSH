$ErrorActionPreference = 'Stop'
$pathComparison = if ([IO.Path]::DirectorySeparatorChar -eq '\') { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$workflowPath = Join-Path $projectRoot '.github\workflows\release.yml'
$smokePath = Join-Path $projectRoot 'scripts\smoke-release.ps1'
$modelscopePath = Join-Path $projectRoot 'scripts\modelscope-release.py'
$comparePath = Join-Path $projectRoot 'scripts\compare-release-sources.ps1'
$verifyPath = Join-Path $projectRoot 'scripts\verify-release.ps1'
foreach ($required in @($workflowPath, $smokePath, $modelscopePath, $comparePath)) {
  if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing release pipeline file: $required" }
}
$verifyScript = Get-Content -Raw -Encoding utf8 $verifyPath
if ($verifyScript -notmatch 'Get-Command\s+cargo' -or $verifyScript -notmatch 'dev-env\.ps1') {
  throw 'Release verification must preserve a configured Cargo toolchain and bootstrap only when Cargo is unavailable'
}

$workflow = Get-Content -Raw -Encoding utf8 $workflowPath
foreach ($requiredText in @(
  'actions/checkout@v6',
  'pnpm/action-setup@v6',
  'actions/setup-node@v6',
  'actions/upload-artifact@v7',
  'actions/download-artifact@v8',
  'windows-2025',
  'windows-11-arm',
  'macos-15-intel',
  'macos-15',
  'ubuntu-22.04',
  'ubuntu-22.04-arm',
  'x86_64-pc-windows-msvc',
  'aarch64-pc-windows-msvc',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
  'libwebkit2gtk-4.1-dev',
  'pnpm tauri build --debug --no-bundle --target',
  'pnpm tauri build --bundles ${{ matrix.bundle }} --target',
  'scripts/collect-native-release.ps1',
  'scripts\build-multiplatform-release.ps1',
  'pnpm install --frozen-lockfile',
  'pnpm test',
  'cargo clippy --locked',
  'cargo test --locked',
  'scripts\smoke-release.ps1',
  'scripts\modelscope-release.py upload',
  'scripts\modelscope-release.py download',
  'scripts\modelscope-release.py prepare',
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
$nativeBundles = @(
  @{ platform = 'windows-x86_64-nsis'; bundle = 'nsis' },
  @{ platform = 'windows-aarch64-nsis'; bundle = 'nsis' },
  @{ platform = 'macos-x86_64-dmg'; bundle = 'dmg' },
  @{ platform = 'macos-aarch64-dmg'; bundle = 'dmg' },
  @{ platform = 'linux-x86_64-appimage'; bundle = 'appimage' },
  @{ platform = 'linux-aarch64-appimage'; bundle = 'appimage' }
)
foreach ($nativeBundle in $nativeBundles) {
  $escapedPlatform = [regex]::Escape($nativeBundle.platform)
  $escapedBundle = [regex]::Escape($nativeBundle.bundle)
  $matrixEntry = [regex]::Match(
    $workflow,
    "(?ms)^\s*- platform:\s*$escapedPlatform\s*(?<block>.*?)(?=^\s*- platform:|^\s*env:)"
  )
  if (-not $matrixEntry.Success -or $matrixEntry.Groups['block'].Value -notmatch "(?m)^\s*bundle:\s*$escapedBundle\s*$") {
    throw "Release matrix bundle is missing or incorrect for $($nativeBundle.platform)"
  }
}
$preflightStart = $workflow.IndexOf('name: Validate tagged release credentials before native builds', [StringComparison]::Ordinal)
$signedBuildStart = $workflow.IndexOf('name: Build signed native bundle', [StringComparison]::Ordinal)
if ($preflightStart -lt 0 -or $signedBuildStart -lt 0 -or $preflightStart -gt $signedBuildStart) {
  throw 'Tagged releases must validate every credential before six-platform signed builds'
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
  'secrets.APPLE_CERTIFICATE',
  'secrets.APPLE_CERTIFICATE_PASSWORD',
  'secrets.APPLE_SIGNING_IDENTITY',
  'secrets.APPLE_ID',
  'secrets.APPLE_PASSWORD',
  'secrets.APPLE_TEAM_ID',
  'secrets.MODELSCOPE_API_TOKEN',
  'vars.QINGZHOU_MODELSCOPE_NAMESPACE'
)) {
  if (-not $preflightBlock.Contains($requiredCredential)) { throw "Release preflight is missing: $requiredCredential" }
}
foreach ($applePolicy in @(
  'Apple signing credentials must be either fully configured or fully omitted',
  "APPLE_SIGNING_IDENTITY = '-'",
  'macOS candidate uses ad-hoc signing'
)) {
  if (-not $workflow.Contains($applePolicy)) { throw "Release workflow is missing Apple fallback policy: $applePolicy" }
}
$adHocSigningStart = $workflow.IndexOf("APPLE_SIGNING_IDENTITY = '-'", [StringComparison]::Ordinal)
$nativeBuildCommandStart = $workflow.IndexOf('pnpm tauri build --bundles', [StringComparison]::Ordinal)
if ($adHocSigningStart -lt 0 -or $nativeBuildCommandStart -lt 0 -or $adHocSigningStart -gt $nativeBuildCommandStart) {
  throw 'macOS ad-hoc signing must be configured before the native Tauri build starts'
}

if (-not $workflow.Contains('prepare_modelscope_mirror')) {
  throw 'Manual ModelScope mirror bootstrap input is missing'
}
if (-not $workflow.Contains('releases%2Fhealthcheck.bin')) {
  throw 'ModelScope bootstrap must verify byte-exact binary public readback'
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
foreach ($requiredText in @('from modelscope_hub import HubApi', 'repo_type = "model"', 'releases/latest.json', 'upload_file', 'repo_exists', 'create_repo', 'urlopen', '/api/v1/models/')) {
  if (-not $modelscope.Contains($requiredText)) { throw "ModelScope helper is missing: $requiredText" }
}
if ($modelscope.Contains('/api/v1/studios/') -or $modelscope.Contains('git clone')) {
  throw 'Updater readback must exercise the public single-file model repository API'
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
    if (-not $resolved.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, $pathComparison)) {
      throw 'Refusing to clean a pipeline test path outside the project'
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
}

Write-Output 'PASS: release pipeline builds six native targets, smokes, mirrors and compares both sources'
