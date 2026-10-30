<#
.SYNOPSIS
  Measures Folio against the performance budgets in docs/PRODUCT.md.

.DESCRIPTION
  1. Creates fixture folders (10k and 50k items) under -FixtureRoot.
  2. Builds a release binary (skip with -NoBuild).
  3. Cold start: launches the app -ColdRuns times and records time to first paint.
  4. Idle memory: Folio + its WebView2 processes, 10 s after first paint.
  5. Benchmark: opens each fixture in grid and list view, scrolls and jumps.

.EXAMPLE
  pwsh scripts/perf/run.ps1
#>
param(
  [string]$FixtureRoot = (Join-Path $env:TEMP "folio-perf"),
  [int]$ColdRuns = 3,
  [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$exe = Join-Path $repo "src-tauri\target\release\folio.exe"

function Wait-ForLine([string]$log, [string]$pattern, [int]$timeoutSec) {
  $deadline = (Get-Date).AddSeconds($timeoutSec)
  while ((Get-Date) -lt $deadline) {
    if ((Test-Path $log) -and (Select-String -Path $log -Pattern $pattern -Quiet)) { return $true }
    Start-Sleep -Milliseconds 200
  }
  return $false
}

function Get-FolioMemoryMB([int]$folioPid) {
  # Only this instance: its WebView2 browser process and that browser's children.
  $procs = @(Get-CimInstance Win32_Process -Filter "ProcessId = $folioPid")
  $webviews = @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'")
  $browsers = @($webviews | Where-Object { $_.ParentProcessId -eq $folioPid })
  $browserIds = @($browsers | ForEach-Object { $_.ProcessId })
  $procs += $browsers
  $procs += $webviews | Where-Object { $browserIds -contains $_.ParentProcessId }
  $ws = 0; $priv = 0
  foreach ($p in $procs) {
    $gp = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
    if ($gp) { $ws += $gp.WorkingSet64; $priv += $gp.PrivateMemorySize64 }
  }
  [pscustomobject]@{
    Processes = $procs.Count
    WorkingSetMB = [math]::Round($ws / 1MB, 1)
    PrivateMB = [math]::Round($priv / 1MB, 1)
    HostWorkingSetMB = [math]::Round((Get-Process -Id $folioPid).WorkingSet64 / 1MB, 1)
  }
}

function Start-Folio([string]$log, [string]$benchDirs) {
  Remove-Item $log -ErrorAction SilentlyContinue
  $env:FOLIO_PERF_LOG = $log
  $env:FOLIO_BENCH_DIRS = $benchDirs
  try { return Start-Process -FilePath $exe -PassThru }
  finally { Remove-Item Env:FOLIO_PERF_LOG, Env:FOLIO_BENCH_DIRS -ErrorAction SilentlyContinue }
}

Write-Host "== Fixtures" -ForegroundColor Cyan
node (Join-Path $PSScriptRoot "make-fixtures.mjs") $FixtureRoot 10000 50000
if ($LASTEXITCODE) { throw "Fixture generation failed ($LASTEXITCODE)" }
$dirs = @((Join-Path $FixtureRoot "items-10000"), (Join-Path $FixtureRoot "items-50000"))

if (-not $NoBuild) {
  Write-Host "== Release build" -ForegroundColor Cyan
  Push-Location $repo
  try {
    npm run tauri build -- --no-bundle | Out-Host
    if ($LASTEXITCODE) { throw "Release build failed ($LASTEXITCODE)" }
  } finally { Pop-Location }
}
if (-not (Test-Path $exe)) { throw "Release binary not found: $exe" }

$log = Join-Path $FixtureRoot "perf.log"
# Don't touch a Folio the user is running; it would also skew the memory numbers.
if (Get-Process folio -ErrorAction SilentlyContinue) {
  throw "Folio is already running. Close it and run the benchmark again."
}

Write-Host "== Cold start x$ColdRuns + idle memory" -ForegroundColor Cyan
$cold = @(); $webview = @(); $memory = $null
for ($i = 1; $i -le $ColdRuns; $i++) {
  $proc = Start-Folio $log ""
  if (-not (Wait-ForLine $log "first-paint" 30)) { $proc | Stop-Process -Force; throw "No first paint within 30 s" }
  $line = (Get-Content $log | Select-String "first-paint" | Select-Object -First 1).Line.Split("`t")
  $cold += [double]$line[0]
  $webview += [double]($line[2] -replace "webviewMs=", "")
  if ($i -eq $ColdRuns) {
    Start-Sleep -Seconds 10
    $memory = Get-FolioMemoryMB $proc.Id
  }
  $proc | Stop-Process -Force
  Start-Sleep -Seconds 2
}

Write-Host "== Folder benchmarks" -ForegroundColor Cyan
$proc = Start-Folio $log ($dirs -join ";")
if (-not (Wait-ForLine $log "bench-done" 600)) { $proc | Stop-Process -Force; throw "Benchmark did not finish" }
Start-Sleep -Seconds 5
$benchMemory = Get-FolioMemoryMB $proc.Id
$proc | Stop-Process -Force

$bench = Get-Content $log | Where-Object { $_ -match "`tbench`t" } | ForEach-Object {
  $row = [ordered]@{}
  foreach ($kv in ($_.Split("`t") | Select-Object -Skip 2)) { $k, $v = $kv.Split("=", 2); $row[$k] = $v }
  [pscustomobject]$row
}

$coldMedian = ($cold | Sort-Object)[[math]::Floor($cold.Count / 2)]
Write-Host ""
Write-Host "== Results" -ForegroundColor Green
"Cold start to first paint: median {0} ms (runs: {1}); of which webview page load to paint: {2}" -f `
  $coldMedian, ($cold -join ", "), ($webview -join ", ")
"Idle memory: {0} MB working set, {1} MB private across {2} processes (host {3} MB)" -f `
  $memory.WorkingSetMB, $memory.PrivateMB, $memory.Processes, $memory.HostWorkingSetMB
"Memory after benchmarks: {0} MB working set, {1} MB private" -f $benchMemory.WorkingSetMB, $benchMemory.PrivateMB
$bench | Format-Table -AutoSize | Out-String | Write-Host
"Raw log: $log"
