param([string]$Exe = "target\release\cella.exe", [int]$Runs = 5, [int]$FileRuns = 3)
$ErrorActionPreference = "Stop"
Add-Type -Path (Join-Path $PSScriptRoot "HiddenDesk.cs")
function MB($b) { [math]::Round($b / 1MB, 1) }
$exePath = (Resolve-Path $Exe).ProviderPath

function New-App([string[]]$AppArgs, [int]$Port) {
  $desk = [HiddenDesk]::CreateDesktop("cella-start", [IntPtr]::Zero, [IntPtr]::Zero, 0, 0x01FF, [IntPtr]::Zero)
  if ($desk -eq [IntPtr]::Zero) { throw "no desktop" }
  $si = New-Object HiddenDesk+STARTUPINFO
  $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
  $si.lpDesktop = "cella-start"
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

# case A: no document. Returns "accept_ms inspect_ms reply_chars ws_mb".
function Case-Empty() {
  $app = New-App @() (Get-Random -Minimum 21000 -Maximum 39000)
  try {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $client = Wait-Port $app 20000
    if ($null -eq $client) { return "no-port" }
    $accept = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $stream = $client.GetStream()
    $writer = New-Object System.IO.StreamWriter($stream); $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($stream)
    $writer.WriteLine('{"id":1,"method":"ui.inspect","params":{}}')
    $line = $reader.ReadLine()
    $inspect = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $ws = 0
    try { $ws = MB (Get-Process -Id $app.Pi.dwProcessId -ErrorAction Stop).WorkingSet64 } catch {}
    $client.Close()
    return "accept_ms=$accept inspect_ms=$inspect reply_chars=$($line.Length) ws_mb=$ws"
  } finally { Stop-App $app }
}

# case B: --sample sales.
function Case-Sample() {
  $app = New-App @("--sample","sales") (Get-Random -Minimum 21000 -Maximum 39000)
  try {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $client = Wait-Port $app 20000
    if ($null -eq $client) { return "no-port" }
    $accept = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $stream = $client.GetStream()
    $writer = New-Object System.IO.StreamWriter($stream); $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($stream)
    $writer.WriteLine('{"id":1,"method":"ui.inspect","params":{}}')
    $line = $reader.ReadLine()
    $inspect = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $ws = 0
    try { $ws = MB (Get-Process -Id $app.Pi.dwProcessId -ErrorAction Stop).WorkingSet64 } catch {}
    $client.Close()
    return "accept_ms=$accept inspect_ms=$inspect reply_chars=$($line.Length) ws_mb=$ws"
  } finally { Stop-App $app }
}

# case C: empty start, open the 70 MB reference file, then inspect.
function Case-File([string]$Path) {
  $app = New-App @() (Get-Random -Minimum 21000 -Maximum 39000)
  try {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $client = Wait-Port $app 20000
    if ($null -eq $client) { return "no-port" }
    $accept = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $stream = $client.GetStream()
    $writer = New-Object System.IO.StreamWriter($stream); $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($stream)
    $jsonPath = $Path -replace '\\','\\'
    $writer.WriteLine('{"id":1,"method":"engine.execute","params":{"command":"file.open","params":{"path":"' + $jsonPath + '"}}}')
    $null = $reader.ReadLine()
    $open = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $writer.WriteLine('{"id":2,"method":"ui.inspect","params":{}}')
    $line = $reader.ReadLine()
    $inspect = [math]::Round($sw.Elapsed.TotalMilliseconds)
    $ws = 0
    try { $ws = MB (Get-Process -Id $app.Pi.dwProcessId -ErrorAction Stop).WorkingSet64 } catch {}
    $client.Close()
    return "accept_ms=$accept open_ms=$open inspect_ms=$inspect reply_chars=$($line.Length) ws_mb=$ws"
  } finally { Stop-App $app }
}

Write-Output "prime: $(Case-Empty)"
for ($i = 1; $i -le $Runs; $i++) { Write-Output "empty-$i`: $(Case-Empty)" }
for ($i = 1; $i -le $Runs; $i++) { Write-Output "sample-$i`: $(Case-Sample)" }
$ref = "D:\pycharm_projects\cella\plan\bench\ref-1m.xlsx"
for ($i = 1; $i -le $FileRuns; $i++) { Write-Output "file-$i`: $(Case-File $ref)" }
