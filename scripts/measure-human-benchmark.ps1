param(
  [Parameter(Mandatory = $true)][string]$BenchmarkExe,
  [ValidateSet('sequential', 'uuid')][string]$IdKind = 'sequential',
  [ValidateRange(1, 20)][int]$Rounds = 1,
  [ValidateRange(1, 60000)][int]$CadenceMs = 60000,
  [int[]]$Sizes = @(10000, 100000, 1000000),
  [ValidatePattern('^[a-zA-Z0-9_-]+$')][string]$Label = 'run'
)
$ErrorActionPreference = 'Stop'
if ($Sizes | Where-Object { $_ -lt 1 -or $_ -gt 1000000 }) { throw 'Fixture sizes must be within 1..1000000' }
$taskOutput = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../.local/human-performance'))
$taskTemp = Join-Path $taskOutput 'tmp'
New-Item -ItemType Directory -Path $taskOutput, $taskTemp -Force | Out-Null
$taskName = "$Label-$IdKind-$CadenceMs-$([DateTime]::UtcNow.Ticks)"
$taskStdout = Join-Path $taskOutput "$taskName.jsonl"
$taskStderr = Join-Path $taskOutput "$taskName.stderr.log"
$taskEnvironment = @{}
foreach ($taskVariable in @('CODECAL_BENCH_IDS', 'CODECAL_BENCH_ROUNDS', 'CODECAL_BENCH_CADENCE_MS', 'TMP', 'TEMP')) {
  $taskEnvironment[$taskVariable] = [Environment]::GetEnvironmentVariable($taskVariable, 'Process')
}
try {
  $env:CODECAL_BENCH_IDS = $IdKind
  $env:CODECAL_BENCH_ROUNDS = [string]$Rounds
  $env:CODECAL_BENCH_CADENCE_MS = [string]$CadenceMs
  $env:TMP = $taskTemp
  $env:TEMP = $taskTemp
  $taskBegan = [Diagnostics.Stopwatch]::StartNew()
  $taskProcess = Start-Process -FilePath (Resolve-Path -LiteralPath $BenchmarkExe).Path -ArgumentList $Sizes -WindowStyle Hidden -PassThru -RedirectStandardOutput $taskStdout -RedirectStandardError $taskStderr
  [long]$taskPeakRss = 0
  [long]$taskPeakPrivate = 0
  [long]$taskSamples = 0
  while (-not $taskProcess.HasExited) {
    try {
      $taskProcess.Refresh()
      $taskPeakRss = [Math]::Max($taskPeakRss, $taskProcess.PeakWorkingSet64)
      $taskPeakPrivate = [Math]::Max($taskPeakPrivate, $taskProcess.PrivateMemorySize64)
      $taskSamples++
    } catch {
      if (-not $taskProcess.HasExited) { throw }
    }
    Start-Sleep -Milliseconds 20
  }
  $taskProcess.WaitForExit()
  $taskBegan.Stop()
  $taskMetadata = [ordered]@{
    exit_code = $taskProcess.ExitCode
    duration_ms = $taskBegan.Elapsed.TotalMilliseconds
    observed_peak_working_set_bytes = $taskPeakRss
    sampled_peak_private_bytes = $taskPeakPrivate
    sample_interval_ms = 20
    samples = $taskSamples
    os_cache_flushed = $false
    fixture_path_accepted = $false
  }
  $taskMetadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $taskOutput "$taskName.metadata.json") -Encoding utf8
  Write-Output $taskStdout
  Write-Output ($taskMetadata | ConvertTo-Json -Compress)
  if ($taskProcess.ExitCode -ne 0) { throw 'Synthetic benchmark process failed; see its isolated diagnostics' }
  $taskResults = Get-Content -LiteralPath $taskStdout | ForEach-Object { $_ | ConvertFrom-Json }
  if ($taskResults | Where-Object { $_.PSObject.Properties.Name -contains 'initial_error' }) { throw 'A synthetic synchronization failed; the failure is recorded in JSON' }
} finally {
  foreach ($taskVariable in $taskEnvironment.Keys) {
    [Environment]::SetEnvironmentVariable($taskVariable, $taskEnvironment[$taskVariable], 'Process')
  }
}
