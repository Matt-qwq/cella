# bench — performance measurement infrastructure

The tools that decide whether a change to Cella's hot paths is a real gain. Numbers and
methodology live in [docs/performance.md](../docs/performance.md); the scripts here produce
them. Run everything from the repository root on an idle machine.

## The A/B gate (how a gain is certified)

`ab_gate.ps1` and `ab_gate_recalc.ps1` run two release binaries as **8 adjacent pairs**
(odd pairs baseline-first, even pairs candidate-first, so warm-up bias lands on both sides),
discard one prime run, and feed the **paired deltas** to a one-sided **sign test** whose
verdict rule is fixed before the run:

- `GAIN(significant)` — sign p ≤ 0.05 (7/8 ⇒ p=0.035, 8/8 ⇒ 0.004) and the median delta is negative.
- Anything else is `INCONCLUSIVE` — do not argue with it; add pairs only when t ≤ −1.9, or
  move the measurement to a quieter window / a targeted micro-benchmark.

An **A/A run** (the same binary on both sides) calibrates the noise floor: it shows what
"no change" looks like on this machine right now. Re-run it when the machine state changes.
A quiet machine resolves ~1–3% effects; an interfered one cannot resolve 20% (paired σ
435 ms vs 4.9 ms for the same comparison on the same day).

```text
# gate scenarios (perf harness): baseline vs candidate, 8 pairs
bench/ab_gate.ps1 -Baseline target/release/examples/perf-old.exe `
                  -Candidate target/release/examples/perf.exe -Pairs 8 -Label mine

# recalc_compare harness: an aggregate edit or forced-open comparison
bench/ab_gate_recalc.ps1 -Baseline <old.exe> -Candidate <new.exe> `
                         -Agg sumifs -Extract 'reopened/sumifs' -Pairs 8 -Label mine

# noise floor: same binary both sides
bench/ab_gate.ps1 -Baseline <exe> -Candidate <exe> -Pairs 8 -Label aa
```

Raw samples land in `plan/ab-*.csv` — `plan/` is gitignored, so results stay local.

## Single-number harnesses

| Script | Measures | Notes |
| --- | --- | --- |
| `startup_r14.ps1` | process start → control port → first `ui.inspect`, empty/sample/file cases | runs the app on a hidden desktop (`HiddenDesk.cs`), random port per run |
| `input_r16.ps1` | synthetic input → model/UI state: channel round trip, light and heavy edit commit | waits 200 ms after `ui.cell.click` — the click queues and lands on a later frame |
| `cargo run --release -p cella-engine --example mem_drift` | long-run memory drift (edit/undo/redo soak with sorts and saves) | steady state should be flat within ~0.1% |
| `cargo run --release -p cella-engine --example recalc_compare` | chained-edit A/B in one binary (`--aggregates=…`, `--full-on-open`) | also self-checks results against a full recalculation |
| `cargo run --release -p cella-engine --example sort_roundtrip` | sort/undo/redo over values, embedded and outside formulas | validates stable ties, holes, formula addresses outside the timers |

Names carry the session-round ids that created them (`r14`, `r16`) — kept so the recorded
measurements in `docs/performance.md` stay resolvable.

## Discipline

- Fix the verdict rule before running; never re-interpret after seeing the numbers.
- The machine must be idle for the whole run: no builds, no scans, no sync clients. If
  `sd(d)` in the verdict table is large, the window was noisy — retry later, don't escalate.
- Gate scenarios resolve ~4–8% at 8 pairs (per-scenario floors from A/A runs); for smaller
  expected effects write a targeted micro-benchmark instead of adding pairs.
