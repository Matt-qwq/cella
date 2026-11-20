param([string]$Exe = "target\release\cella.exe", [int]$SimpleRuns = 5, [int]$FileRuns = 3)
$ErrorActionPreference = "Stop"
Add-Type -Path (Join-Path $PSScriptRoot "HiddenDesk.cs")
$exePath = (Resolve-Path $Exe).ProviderPath

function New-App([string[]]$AppArgs, [int]$Port) {
  $desk = [HiddenDesk]::CreateDesktop("cella-input", [IntPtr]::Zero, [IntPtr]::Zero, 0, 0x01FF, [IntPtr]::Zero)
  if ($desk -eq [IntPtr]::Zero) { throw "no desktop" }
  $si = New-Object HiddenDesk+STARTUPINFO
  $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
  $si.lpDesktop = "cella-input"
  $pi = New-Object HiddenDesk+PROCESS_INFORMATION
  $argline = ('"{0}"' -f $exePath) + " " + ($AppArgs -join " ") + " --control $Port"
  $ok = [HiddenDesk]::CreateProcess($exePath, $argline, [IntPtr]::Zero, [IntPtr]::Zero, $false, 0, [IntPtr]::Zero, (Split-Path $exePath), [ref]$si, [ref]$pi)
  if (-not $ok) { [HiddenDesk]::CloseDesktop($desk) | Out-Null; throw "CreateProcess failed" }
  @{ Desk = $desk; Pi = $pi; Port = $Port }
}

function Stop-App($app) {
  [HiddenDesk]::TerminateProcess($app.Pi.hProcess, 0) | Out-Null
  [HiddenDesk]::CloseHandle($app.Pi.hProcess) | Out-Null
  [HiddenDesk]::CloseHandle($app.Pi.hThread) | Out-Null
  [HiddenDesk]::CloseDesktop($app.Desk) | Out-Null
}

function Wait-Port($app, [double]$TimeoutMs) {
  $deadline = [Diagnostics.Stopwatch]::StartNew()
  while ($deadline.Elapsed.TotalMilliseconds -lt $TimeoutMs) {
    try { return (New-Object System.Net.Sockets.TcpClient("127.0.0.1", $app.Port)) } catch { Start-Sleep -Milliseconds 15 }
  }
  return $null
}

function Ask($writer, $reader, [string]$json) {
  $writer.WriteLine($json)
  return $reader.ReadLine()
}

# Synthetic input -> model/UI state observed. `commit_ms` covers the engine work inside the
# commit acknowledgement; `observe_ms` polls ui.inspect at ~10 ms until the editor is null, so
# it carries a polling upper bound, not a paint timestamp.
function Edit-Case([string[]]$AppArgs, [string]$Cell, [string]$Value, [string]$OpenPath) {
  $app = New-App $AppArgs (Get-Random -Minimum 21000 -Maximum 39000)
  try {
    $client = Wait-Port $app 20000
    if ($null -eq $client) { return "no-port" }
    $stream = $client.GetStream()
    $writer = New-Object System.IO.StreamWriter($stream); $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($stream)
    $null = Ask $writer $reader '{"id":1,"method":"ui.inspect","params":{}}'
    if ($OpenPath) {
      $sw = [Diagnostics.Stopwatch]::StartNew()
      $jsonPath = $OpenPath -replace '\\','\\'
      $null = Ask $writer $reader ('{"id":2,"method":"engine.execute","params":{"command":"file.open","params":{"path":"' + $jsonPath + '"}}}')
      $openMs = [math]::Round($sw.Elapsed.TotalMilliseconds)
    } else { $openMs = 0 }
    Start-Sleep -Milliseconds 300
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $null = Ask $writer $reader ('{"id":3,"method":"ui.cell.click","params":{"cell":"' + $Cell + '"}}')
    # The click only queues pointer events; the selection lands on a later frame.
    Start-Sleep -Milliseconds 200
    $null = Ask $writer $reader ('{"id":7,"method":"engine.execute","params":{"command":"cell.get","params":{"cell":"' + $Cell + '"}}}')
    $getMs = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $sw.Restart()
    $null = Ask $writer $reader ('{"id":4,"method":"ui.edit.begin","params":{"text":"' + $Value + '"}}')
    $beginMs = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $reply = Ask $writer $reader '{"id":5,"method":"ui.edit.commit","params":{"move":"none"}}'
    $commitMs = [math]::Round($sw.Elapsed.TotalMilliseconds)
    if ($reply -notmatch '"result"') { return "commit-failed: $reply" }
    $polls = 0
    $sw.Restart()
    while ($sw.Elapsed.TotalMilliseconds -lt 5000) {
      $line = Ask $writer $reader '{"id":6,"method":"ui.inspect","params":{}}'
      $polls++
      if ($line -match '"editor":null') { break }
      Start-Sleep -Milliseconds 10
    }
    $observeMs = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $client.Close()
    return "open_ms=$openMs get_ms=$getMs begin_ms=$beginMs commit_ms=$commitMs observe_ms=$observeMs polls=$polls"
  } finally { Stop-App $app }
}

for ($i = 1; $i -le $SimpleRuns; $i++) { Write-Output "sample-$i`: $(Edit-Case @('--sample','sales') 'C5' '5' $null)" }
# The reference workbook is a local-only 70 MB file (never committed); build one with
# `cargo run --release -p cella-engine --example perf -- 1000000` output or point elsewhere.
$ref = "plan\bench\ref-1m.xlsx"
for ($i = 1; $i -le $FileRuns; $i++) { Write-Output "ref-1m-$i`: $(Edit-Case @() 'B1' '3' $ref)" }
