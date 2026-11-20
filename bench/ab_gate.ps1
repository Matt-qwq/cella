# Interleaved paired A/B gate with a sign test.
#
#   ab_gate.ps1 -Baseline <exe> -Candidate <exe> [-Pairs 8] [-Rows 1000000] [-Label name]
#
# Runs the two executables as adjacent pairs (odd pairs baseline-first, even pairs
# candidate-first, so warm-up bias lands on both sides). For every watched scenario the
# per-pair deltas feed a one-sided sign test: "at least how many of n pairs favour the
# candidate, if a pair is a coin flip?" 7/8 gives p=0.035, 8/8 gives p=0.004.
# Verdicts are decided by the rule printed with the numbers -- never after looking at them:
#   GAIN(significant)    sign p <= 0.05 and the median delta is negative
#   LOSS(significant)    sign p <= 0.05 and the median delta is positive
#   INCONCLUSIVE         anything else (keep sampling or reject)
# An A/A run (same exe on both sides) calibrates the noise floor per scenario.
param(
  [Parameter(Mandatory = $true)][string]$Baseline,
  [Parameter(Mandatory = $true)][string]$Candidate,
  [int]$Pairs = 8,
  [int]$Rows = 1000000,
  [int]$Repeat = 1,
  [string]$ScenarioList = 'fill.values,fill.formulas,fill.chained,aggregate,edit.recalc,edit.undo,save.xlsx,open.xlsx,sort.desc',
  [string[]]$Watch = @('fill.values', 'save.xlsx', 'open.xlsx', 'edit.recalc', 'sort.desc'),
  [string]$Label = 'ab'
)
$ErrorActionPreference = 'Stop'
$sc = "--scenario=$ScenarioList"

function Run-Once([string]$exe) {
  $lines = & $exe $Rows $sc "--repeat=$Repeat" '--json'
  if ($LASTEXITCODE -ne 0) { throw "run failed: $exe" }
  return (($lines -join "`n") | ConvertFrom-Json)
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

# One-sided binomial tail P(X >= k) for X ~ Bin(n, 1/2).
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

# A discarded prime run: first invocation pays cold caches for both sides alike.
Write-Host "prime run..."
$null = Run-Once $Baseline

$base = @{}
$cand = @{}
foreach ($w in $Watch) { $base[$w] = [System.Collections.Generic.List[double]]::new(); $cand[$w] = [System.Collections.Generic.List[double]]::new() }
$csv = @("pair,scenario,side,ms")

for ($p = 1; $p -le $Pairs; $p++) {
  $baseFirst = ($p % 2 -eq 1)
  $first = if ($baseFirst) { Run-Once $Baseline } else { Run-Once $Candidate }
  $second = if ($baseFirst) { Run-Once $Candidate } else { Run-Once $Baseline }
  foreach ($w in $Watch) {
    $m1 = [double](($first.results | Where-Object scenario -eq $w).ms)
    $m2 = [double](($second.results | Where-Object scenario -eq $w).ms)
    if ($baseFirst) {
      $base[$w].Add($m1); $cand[$w].Add($m2)
      $csv += "$p,$w,base,$m1"; $csv += "$p,$w,cand,$m2"
    } else {
      $cand[$w].Add($m1); $base[$w].Add($m2)
      $csv += "$p,$w,cand,$m1"; $csv += "$p,$w,base,$m2"
    }
  }
  Write-Host "pair $p/$Pairs done"
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$csvPath = "plan/ab-$Label-$stamp.csv"
$csv | Set-Content -LiteralPath $csvPath

Write-Host ""
Write-Host "label=$Label baseline=$Baseline candidate=$Candidate rows=$Rows pairs=$Pairs"
Write-Host "rule: GAIN/LOSS(significant) requires sign p<=0.05 with matching median direction; else INCONCLUSIVE"
Write-Host ""
"{0,-14} {1,9} {2,9} {3,8} {4,6} {5,8} {6,9} {7,8} {8,-18}" -f 'scenario', 'base-med', 'cand-med', 'delta%', 'fav', 'signP', 't(pair)', 'sd(d)', 'verdict' | Write-Host
foreach ($w in $Watch) {
  $b = $base[$w].ToArray(); $c = $cand[$w].ToArray()
  $d = for ($i = 0; $i -lt $Pairs; $i++) { $c[$i] - $b[$i] }
  $d = [double[]]$d
  $mb = Median $b; $mc = Median $c
  $deltaPct = if ($mb -ne 0) { [math]::Round(($mc - $mb) / $mb * 100.0, 2) } else { 0 }
  $fav = @($d | Where-Object { $_ -lt 0 }).Count
  $p = SignP $fav $Pairs
  $sd = Sd $d
  $mean = ($d | Measure-Object -Average).Average
  $t = if ($sd -gt 0) { [math]::Round($mean / ($sd / [math]::Sqrt($Pairs)), 2) } else { 0 }
  $verdict = if ($p -le 0.05 -and $deltaPct -lt 0) { 'GAIN(significant)' }
  elseif ($p -le 0.05 -and $deltaPct -gt 0) { 'LOSS(significant)' }
  else { 'INCONCLUSIVE' }
  "{0,-14} {1,9} {2,9} {3,8} {4,6} {5,8} {6,9} {7,8} {8,-18}" -f $w, [math]::Round($mb), [math]::Round($mc), $deltaPct, "$fav/$Pairs", [math]::Round($p, 4), $t, [math]::Round($sd, 1), $verdict | Write-Host
}
Write-Host ""
Write-Host "raw samples: $csvPath"
