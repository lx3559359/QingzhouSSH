$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$themePath = Join-Path $projectRoot 'src\styles\theme.css'
$workflowPath = Join-Path $projectRoot 'src\features\workflows\workflow.css'
$theme = Get-Content -LiteralPath $themePath -Raw
$workflow = Get-Content -LiteralPath $workflowPath -Raw

function Assert-Contains {
  param([string]$Text, [string]$Needle, [string]$Message)

  if (-not $Text.Contains($Needle)) {
    throw $Message
  }
}

function Assert-NotMatches {
  param([string]$Text, [string]$Pattern, [string]$Message)

  if ($Text -match $Pattern) {
    throw $Message
  }
}

function Assert-Matches {
  param([string]$Text, [string]$Pattern, [string]$Message)

  if ($Text -notmatch $Pattern) {
    throw $Message
  }
}

Assert-Contains $theme '--density-page-padding:' 'Shared page-padding density token is missing.'
Assert-Contains $theme '--density-shell-gap:' 'Shared shell-gap density token is missing.'
Assert-Contains $theme '--density-card-gap:' 'Shared card-gap density token is missing.'
Assert-Contains $theme '--density-card-padding:' 'Shared card-padding density token is missing.'
Assert-Contains $theme '--density-sidebar-width:' 'Shared sidebar-width density token is missing.'
Assert-Contains $theme '--density-nav-height:' 'Shared navigation-height density token is missing.'
Assert-Contains $theme '--density-control-height:' 'Shared control-height density token is missing.'
Assert-Contains $theme '@media (min-width: 1050px) and (max-width: 1359px)' 'Compact viewport mode is missing.'
Assert-Contains $theme '@media (max-width: 1049px)' 'Minimum viewport mode must include the 958px inner width of a 960px native window.'

$compactTheme = (($theme -split [regex]::Escape('@media (min-width: 1050px) and (max-width: 1359px)'), 2)[1] -split [regex]::Escape('@media (max-width: 1049px)'), 2)[0]
$minimumTheme = (($theme -split [regex]::Escape('@media (max-width: 1049px)'), 2)[1] -split [regex]::Escape('@container transfer-page'), 2)[0]

Assert-Matches $compactTheme '\.side-navigation\s*\{' 'Compact mode must reduce the persistent sidebar.'
Assert-Matches $minimumTheme '(?s)\.workspace-shell\s*\{[^}]*grid-template-columns:\s*1fr' 'Minimum mode must place navigation above page content.'
Assert-Matches $minimumTheme '(?s)\.side-navigation__items\s*\{[^}]*overflow-x:\s*auto' 'Minimum navigation needs its own horizontal scrolling boundary.'
Assert-Matches $minimumTheme '(?s)\.side-navigation__items button\s*\{[^}]*white-space:\s*nowrap' 'Minimum navigation labels must stay readable on one line.'
Assert-Matches $compactTheme '(?s)\.data-root-badge\s*\{[^}]*max-width:\s*34vw' 'The compact data-root badge must truncate before crowding window controls.'
Assert-NotMatches $theme '(?s)html,\s*body,\s*#root\s*\{[^}]*overflow-x:\s*auto' 'Global horizontal scrolling is forbidden.'
Assert-Matches $compactTheme '(?s)\.task-card-grid\s*\{[^}]*grid-template-columns:\s*repeat\(\s*auto-fit,[^}]*max\(210px,[^}]*\/\s*3\)' 'Task cards need a compact auto-fit grid capped at three columns.'
Assert-Matches $minimumTheme '(?s)\.history-filters\s*\{[^}]*grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)' 'Minimum history filters need three usable columns.'
Assert-Matches $minimumTheme '(?s)\.settings-main-grid,[^{]*\.history-layout,[^{]*\.advanced-execution-grid\s*\{[^}]*grid-template-columns:\s*1fr' 'Detail-heavy pages must stack in minimum mode.'
Assert-Matches $minimumTheme '(?s)\.dashboard-grid,[^{]*\.server-grid,[^{]*\.task-card-grid,[^{]*\.download-file-grid\s*\{[^}]*grid-template-columns:\s*repeat\(2,\s*minmax\(0,\s*1fr\)\)' 'Primary card grids must use two columns at the minimum desktop width.'
Assert-NotMatches $compactTheme 'min-height:\s*(?:2[0-9]|3[0-5])px' 'Compact interactive controls cannot be shorter than 36px.'
Assert-NotMatches $minimumTheme 'min-height:\s*(?:2[0-9]|3[0-5])px' 'Minimum interactive controls cannot be shorter than 36px.'
Assert-Matches $compactTheme '(?s)\.log-search-layout\s*\{[^}]*grid-template-columns:\s*minmax\(250px,\s*0\.7fr\)\s*minmax\(0,\s*1\.5fr\)' 'Compact log search must retain proportional side-by-side columns.'
Assert-Matches $minimumTheme '(?s)\.log-search-layout\s*\{[^}]*grid-template-columns:\s*1fr' 'Minimum log search must place the search form above results.'
Assert-Matches $theme '(?s)\.log-table-wrap\s*\{[^}]*overflow:\s*auto' 'Log results need a dedicated table scroll boundary.'
Assert-Matches $theme '(?s)\.log-results-table\s*\{[^}]*min-width:\s*660px' 'Log columns need a stable minimum width inside their scroll boundary.'
Assert-Contains $theme '/* Minimum SFTP stack */' 'The minimum SFTP override must follow its container queries.'
$minimumSftpTheme = ($theme -split [regex]::Escape('/* Minimum SFTP stack */'), 2)[1]
Assert-Matches $minimumSftpTheme '(?s)\.sftp-workspace\s*\{[^}]*grid-template-columns:\s*1fr[^}]*grid-template-areas:\s*"local"\s*"actions"\s*"remote"' 'Minimum SFTP mode must stack local, actions, and remote regions in that order.'
Assert-Matches $minimumSftpTheme '(?s)\.sftp-actions\s*\{[^}]*grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)' 'Minimum SFTP actions must remain visible in one compact row.'
Assert-NotMatches $workflow 'min-width:\s*850px' 'Workflow must not force an 850px page width.'
Assert-NotMatches $workflow '(?s)\.workflow-page\s*\{[^}]*overflow-x:\s*auto' 'Workflow page must not own horizontal scrolling.'
Assert-Contains $workflow 'container-name: workflow-page;' 'Workflow needs a page-internal container query boundary.'
Assert-Matches $workflow '(?s)\.workflow-builder\s*\{[^}]*grid-template-areas:\s*"library canvas inspector"' 'Workflow editor regions need explicit grid areas.'
Assert-Matches $workflow '(?s)\.workflow-canvas__viewport\s*\{[^}]*overflow:\s*auto' 'Workflow canvas must retain its own pan boundary.'
Assert-Matches $workflow '(?s)@media \(max-width: 1049px\).*?\.workflow-builder\s*\{[^}]*grid-template-areas:\s*"library library"\s*"canvas inspector"' 'Minimum workflow mode must put the library above the canvas and inspector.'
Assert-Matches $workflow '(?s)@media \(min-width: 1050px\) and \(max-width: 1359px\).*?\.workflow-zoom button\s*\{[^}]*height:\s*36px' 'Compact workflow zoom controls must meet the 36px control floor.'
Assert-Matches $workflow '(?s)@media \(max-width: 1049px\).*?\.workflow-zoom button\s*\{[^}]*height:\s*36px' 'Minimum workflow zoom controls must meet the 36px control floor.'

Write-Host 'Responsive layout source contracts passed.'
