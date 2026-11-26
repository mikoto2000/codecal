param(
  [Parameter(Mandatory=$true)][int]$ParentProcessId,
  [Parameter(Mandatory=$true)][string]$ExpectedExecutable,
  [Parameter(Mandatory=$true)][string]$ReadyFile,
  [Parameter(Mandatory=$true)][string]$StopFile,
  [Parameter(Mandatory=$true)][string]$ResultFile
)
$ErrorActionPreference='Stop'
$taskRoot=Get-Process -Id $ParentProcessId
if ($taskRoot.Path -ne (Resolve-Path -LiteralPath $ExpectedExecutable).Path) { throw 'Unexpected native test process' }
# Inventory only IDs/parents; never read other processes' command lines or content.
$taskInfo=Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId
$taskIds=[Collections.Generic.List[int]]::new()
$taskIds.Add($ParentProcessId)
for ($taskIndex=0; $taskIndex -lt $taskIds.Count; $taskIndex++) {
  foreach ($taskChild in $taskInfo) {
    if ($taskChild.ParentProcessId -eq $taskIds[$taskIndex] -and -not $taskIds.Contains([int]$taskChild.ProcessId)) { $taskIds.Add([int]$taskChild.ProcessId) }
  }
}
$taskMembers=@(foreach ($taskMemberId in $taskIds) {
  $taskMember=Get-Process -Id $taskMemberId -ErrorAction SilentlyContinue
  if ($taskMember -and $taskMember.StartTime -ge $taskRoot.StartTime) { @{Process=$taskMember;Started=$taskMember.StartTime} }
})
if (@($taskMembers | Where-Object { $_.Process.ProcessName -like '*webview*' }).Count -eq 0) { throw 'No owned WebView2 processes found' }
[long]$taskBaseline=0
foreach ($taskMember in $taskMembers) { $taskMember.Process.Refresh(); $taskBaseline+=$taskMember.Process.WorkingSet64 }
[long]$taskPeak=$taskBaseline
[long]$taskSamples=0
$taskWatch=[Diagnostics.Stopwatch]::StartNew()
[IO.File]::WriteAllText($ReadyFile,'ready')
while (-not (Test-Path -LiteralPath $StopFile) -and -not $taskRoot.HasExited) {
  [long]$taskCurrent=0
  foreach ($taskMember in $taskMembers) {
    $taskMember.Process.Refresh()
    if (-not $taskMember.Process.HasExited -and $taskMember.Process.StartTime -eq $taskMember.Started) { $taskCurrent+=$taskMember.Process.WorkingSet64 }
  }
  $taskPeak=[Math]::Max($taskPeak,$taskCurrent)
  $taskSamples++
  Start-Sleep -Milliseconds 20
}
$taskWatch.Stop()
@{
  baseline_tree_rss_bytes=$taskBaseline
  observed_peak_tree_rss_bytes=$taskPeak
  observed_tree_rss_increment_bytes=($taskPeak-$taskBaseline)
  samples=$taskSamples
  duration_ms=$taskWatch.Elapsed.TotalMilliseconds
  requested_sample_interval_ms=20
  processes=$taskMembers.Count
  scope='Tauri plus its initial live descendants including WebView2; newly created or exited transient subprocesses are not independently tracked'
} | ConvertTo-Json | Set-Content -LiteralPath $ResultFile -Encoding utf8
