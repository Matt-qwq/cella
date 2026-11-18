# DESIGN — "Terminal Ledger"

Cella's site as a phosphor-green CRT that boots into a 1980s spreadsheet soul
running on a 2026 engine. Terse system-log lines meet modern clarity: the page
itself is a spreadsheet program — row-numbered ledgers, a formula bar, a
function-key legend, a boot sequence — never a generic landing page.

## Concept

A spreadsheet is a *ledger*; a terminal is where engineers already live; Cella's
positioning is "for data sets, for agents" — so the site speaks in the visual
language of both: boot logs, dotted-leader status lines, prompt headers
(A:\> why cella), monospace everywhere, and measurement copy that reads like a
benchmark report. The retro is a material, not a costume: contrast is held at
WCAG-AA+, motion is restrained and skippable, and every number on the page is
real and attributed.

## Design tokens

One system, defined once in `assets/style.css` under `:root`:

| family | tokens |
|---|---|
| colour | --bg #050b07 (green-tinted near-black) · --ink #b7f5c6 (14.3:1) · --ink-dim #7fc79b (9.6:1) · --phos #46ff8c (13.2:1) · --phos-soft #66c68f (9.5:1) · --err #ff9a6b · decorative line/border greens (contrast exempt) |
| amber | `[data-phos="amber"]` swaps the whole phosphor family to #ffb454 on #0b0703 — same geometry, warm tube |
| type | ui-monospace stack; --fs-xs .72rem / --fs-s .83 / --fs-m .95 / --fs-l 1.15 / --fs-xl clamp(1.35–1.6) / --fs-stat clamp(1.9–2.9) |
| space | 4px rhythm: --s-1 .25 → --s-7 6rem; sections --s-5, hero --s-6 |
| radii | --r-1 2px, --r-2 6px — CRT-sharp, never bubbly |
| motion | --ease cubic-bezier(.22,.9,.28,1), --ease-out cubic-bezier(.16,1,.3,1); --dur-1 120ms / --dur-2 240ms / --dur-3 560ms |

## The signature moment

**The sheet.** A working in-page spreadsheet: clickable cells, a name box, a
formula bar, and a ~120-line bounded vanilla-JS evaluator (arithmetic, cell
refs, SUM/AVERAGE with ranges — no eval(), 120-char cap, depth 24, step budget
4000). Edit C2, press RECALC, and dependents cascade down the RUNNING column
with per-depth animation delays (depth × 70 ms) — a visible miniature of the
real engine's 1M-deep chain at ~400 ms. The status log reports the demo's own
measured recompute time. With JS off, the same table ships pre-computed:
D2 =B2*C2 … D6 =SUM(D2:D5), values baked in, plus a noscript note.

## Motifs

- **Boot sequence** — hero terminal types five dotted-leader status lines
  (~1.2 s), skippable via SKIP ▸, click, or keypress; ships complete in the
  markup for JS-off and prefers-reduced-motion.
- **Blinking block cursor** — `█` steps(1) blink on the boot prompt; static
  under reduced motion.
- **Prompt headers** — `A:\> why cella` … `H:\> status`: drive letters that
  double as spreadsheet column letters.
- **Row-numbered ledgers** — WHY / CAPABILITIES / NEVER-CRASH lists carry
  `01 02 03` row badges in a spreadsheet gutter.
- **Function ticker** — real function names (SUM, LET, LAMBDA, XLOOKUP,
  SEQUENCE…) scroll in a marquee under the nav; CSS-only, pauses on hover,
  static under reduced motion.
- **Function-key legend** — footer strip of original labels; F1 is a real docs
  link, F2/F3/F8/F9 actually edit/recalc/switch phosphor.
- **CRT furniture** — fixed scanline overlay (1px/3px, 26% black) + radial
  vignette + faint 48px grid on the body; glow only on accents, never body text.
- **System-log voice** — "READY · 8 FORMULA CELLS", "RECORD ▸ OpenGL is the
  Windows default", dotted leaders, ALL-CAPS micro-labels.

## Accessibility & fallbacks

- AA+ contrast for all reading text (computed above); dim greens are reserved
  for borders and rules.
- Full no-JS path: static boot log, pre-computed sheet, static ticker, bars at
  final width.
- prefers-reduced-motion: typing, blinking, marquee, pulses, smooth scroll and
  bar growth all disabled; layout unchanged.
- Semantic HTML throughout: h1/h2/h3 ladder, table with scope, aria-live status
  log, skip link, visible :focus-visible rings, lang="en".
- Responsive 320px → ultrawide: hero stacks, nav scrolls horizontally, bars and
  readouts go single-column, sheet font steps down.

## What makes it distinct from a template

No hero image, no stock gradient, no card grid sameness: the page is *made of
spreadsheet parts* — ledgers with row numbers, a live formula bar, a boot log,
function keys. Copy is terse log-speak grounded in measured facts; the only
"decoration" is the CRT itself, drawn in pure CSS. Everything on the page is
either true, functional, or both.

## Polish flaws found in self-review (and fixed)

Found by re-reading the finished HTML/CSS/JS end to end:

1. **D4 shipped `data-f="B4*C4"`** (missing `=`) — the demo would have
   shown #NAME? on load; fixed to `data-f="=B4*C4"` (verified: all nine
   static values now match the evaluator's output).
2. **Boot MEMORY line had 17 leader dots instead of 18** — the value column
   was off by one against the other four lines; rebuilt every line so the
   value column sits at exactly char 31 (verified programmatically).
3. **Sheet numeric alignment**: `td:nth-child(n+3):not([data-f])` excluded
   formula cells from right-align and counted the row-header `th`; replaced
   with explicit per-column rules (B–E right, A left) and dropped the dead
   `.num-cell` selector.
4. **`.topbar` used `color-mix()` with no fallback** — an older browser would
   have rendered the sticky bar fully transparent; plain `var(--bg)` line
   added first.
5. **SKIP button was visible but dead** with JavaScript off or
   prefers-reduced-motion; now ships `hidden` and is un-hidden only when
   typing actually starts, re-hidden on finish.
6. **"CAPS" nav label** read like caps-lock; renamed to FEATURES.
7. **Amber theme flashed green on return visits** — localStorage was only
   read by the deferred script; a pre-paint inline script in `<head>` now
   applies the saved phosphor before first render.
8. **noscript note** did not explain the inert RECALC/COMMIT buttons; copy
   now says the formula bar and RECALC need JavaScript.
