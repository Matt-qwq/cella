# Interleaved paired A/B gate for the recalc_compare harness (aggregates / full-on-open).
#
#   ab_gate_recalc.ps1 -Baseline <exe> -Candidate <exe> -Agg 'sumifs' -Extract 'reopened/sumifs' [-Pairs 8]
#   ab_gate_recalc.ps1 -Baseline <exe> -Candidate <exe> -FullOnOpen -Extract 'forced' [-Pairs 8]
#
# Same statistics as ab_gate.ps1: adjacent pairs, odd/even alternating first side, a discarded
# prime, paired deltas, one-sided sign test (7/8 => p=0.035, 8/8 => p=0.004), verdict decided by
# the rule printed with the numbers. `recalc_compare` prints "<case>: median X ms" per case on
# stderr; --repeat=1 keeps one measured round per invocation so every pair is comparable.
param(
  [Parameter(Mandatory = $true)][string]$Baseline,
  [Parameter(Mandatory = $true)][string]$Candidate,
  [string]$Agg = '',
  [switch]$FullOnOpen,
  [int]$Rows = 1000000,
  [int]$Pairs = 8,
  [Parameter(Mandatory = $true)][string]$Extract,
  [string]$Label = 'recert'
)
$ErrorActionPreference = 'Stop'
$args_ = if ($FullOnOpen) { @("--rows=$Rows", '--repeat=1', '--full-on-open') } else { @("--rows=$Rows", "--aggregates=$Agg", '--repeat=1') }
$pattern = [regex]::Escape($Extract) + ': median ([0-9.]+) ms'

function Run-Once([string]$exe) {
  $err = "$env:TEMP\ab_recalc_run.err.txt"
  Remove-Item -LiteralPath $err -ErrorAction SilentlyContinue
  # A native command's stderr lines become ErrorRecords under 'Stop' the moment `2>` wraps
  # them, which kills the run; the lead's session jobs all redirect under 'Continue'.
  $ErrorActionPreference = 'Continue'
  $null = & $exe @args_ 1> $null 2> $err
  $ErrorActionPreference = 'Stop'
  if ($LASTEXITCODE -ne 0) { throw "run failed: $exe exit=$LASTEXITCODE" }
  $m = (Select-String -LiteralPath $err -Pattern $pattern | Select-Object -Last 1).Matches[0]
  if ($null -eq $m) { throw "no '$Extract' median in output of $exe" }
  return [double]$m.Groups[1].Value
}

function Median([double[]]$a) {
  $s = $a | Sort-Object
  $n = $s.Count
  if ($n % 2 -eq 1) { return [double]$s[[int](($n - 1) / 2)] }
  return ([double]$s[[int]($n / 2 - 1)] + [double]$s[[int]($n / 2)]) / 2.0
}

function Sd([double[]]$a) {
  $mean = ($a | Measure-Object -Average).Average
  $ss = 0.0
  foreach ($x in $a) { $ss += ($x - $mean) * ($x - $mean) }
  if ($a.Count -lt 2) { return 0.0 }
  return [math]::Sqrt($ss / ($a.Count - 1))
}

function SignP([int]$k, [int]$n) {
  if ($k -le 0) { return 1.0 }
  $sum = 0.0
  for ($j = $k; $j -le $n; $j++) {
    $c = 1.0
    for ($m = 0; $m -lt $j; $m++) { $c = $c * ($n - $m) / ($m + 1) }
    $sum += $c
  }
  return $sum / [math]::Pow(2.0, $n)
}

Write-Host "prime run..."
$null = Run-Once $Baseline

$b = [System.Collections.Generic.List[double]]::new()
$c = [System.Collections.Generic.List[double]]::new()
$csv = @("pair,side,ms")

for ($p = 1; $p -le $Pairs; $p++) {
  $baseFirst = ($p % 2 -eq 1)
  $m1 = if ($baseFirst) { Run-Once $Baseline } else { Run-Once $Candidate }
  $m2 = if ($baseFirst) { Run-Once $Candidate } else { Run-Once $Baseline }
  if ($baseFirst) {
    $b.Add($m1); $c.Add($m2)
    $csv += "$p,base,$m1"; $csv += "$p,cand,$m2"
  } else {
    $c.Add($m1); $b.Add($m2)
    $csv += "$p,cand,$m1"; $csv += "$p,base,$m2"
  }
  Write-Host "pair $p/$Pairs done"
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$csvPath = "plan/ab-$Label-$stamp.csv"
$csv | Set-Content -LiteralPath $csvPath

$ba = $b.ToArray(); $ca = $c.ToArray()
$d = for ($i = 0; $i -lt $Pairs; $i++) { $ca[$i] - $ba[$i] }
$d = [double[]]$d
$mb = Median $ba; $mc = Median $ca
$deltaPct = if ($mb -ne 0) { [math]::Round(($mc - $mb) / $mb * 100.0, 2) } else { 0 }
$fav = @($d | Where-Object { $_ -lt 0 }).Count
$p = SignP $fav $Pairs
$sd = Sd $d
$mean = ($d | Measure-Object -Average).Average
$t = if ($sd -gt 0) { [math]::Round($mean / ($sd / [math]::Sqrt($Pairs)), 2) } else { 0 }
$verdict = if ($p -le 0.05 -and $deltaPct -lt 0) { 'GAIN(significant)' }
elseif ($p -le 0.05 -and $deltaPct -gt 0) { 'LOSS(significant)' }
else { 'INCONCLUSIVE' }

Write-Host ""
Write-Host "label=$Label extract=$Extract baseline=$Baseline candidate=$Candidate rows=$Rows pairs=$Pairs"
Write-Host "rule: GAIN/LOSS(significant) requires sign p<=0.05 with matching median direction; else INCONCLUSIVE"
Write-Host ""
"{0,-22} {1,9} {2,9} {3,8} {4,6} {5,8} {6,9} {7,8} {8,-18}" -f 'case', 'base-med', 'cand-med', 'delta%', 'fav', 'signP', 't(pair)', 'sd(d)', 'verdict' | Write-Host
"{0,-22} {1,9} {2,9} {3,8} {4,6} {5,8} {6,9} {7,8} {8,-18}" -f $Extract, [math]::Round($mb), [math]::Round($mc), $deltaPct, "$fav/$Pairs", [math]::Round($p, 4), $t, [math]::Round($sd, 1), $verdict | Write-Host
Write-Host ""
Write-Host "raw samples: $csvPath"
