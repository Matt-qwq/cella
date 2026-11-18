# DESIGN.md — "Precision Blueprint"

The official-site candidate for Cella, direction: **Precision Blueprint** — a dark
technical-engineering aesthetic. The page reads like the engineering drawing of the machine:
an instrument panel on near-black blue ink, hairline grids, dimension-annotated statistics,
monospaced micro-labels, and one signal accent.

## Concept

A spreadsheet is a grid, and so is the page. The whole site behaves like a **sheet of graph
paper inside a drawing frame**: a fixed 1px frame around the viewport with crosshair "+" marks
at its corners, column letters (A, B, C…) ruling the top edge, row numbers (1, 2, 3…) climbing
the left edge as you scroll, and a subtle cyan crosshair with a hollow-cell marker that tracks
the pointer — the **active cell**. The Name Box is real: a live `CELL K23` readout in the nav
bar. Under `prefers-reduced-motion` or on coarse (touch) pointers the crosshair and readout
disappear; the rulers and frame remain as static drawing furniture.

The hero is anchored by a **technical title block** — the bordered strip a drawing carries:
NAME / REV 0.13.0 / STAGE ALPHA / MATERIAL RUST / SHEET 1 / LICENCE — above an animated cell
field: a small illustrative workbook whose cells ripple in a diagonal sweep every 9 s, with the
active cell F6 outlined exactly the way a real grid outlines it.

Statistics are drawn as **engineering callouts**: big tabular-numeral values over a dimension
line with end ticks and a centred small-caps label ("LIVE HEAP", "ONE EDIT → 1M RECALC"),
each annotated with the conditions of the measurement. The comparison chart hatches the
reference applications and keeps Cella solid; the status meters extend that vocabulary into a
legend — **solid = measured, hatched = estimated** — so honesty about data is itself a visual
device, not a footnote.

## Design tokens

Defined once in `:root` (`assets/style.css` §1), used nowhere else as raw values.

| Group | Tokens |
| --- | --- |
| Ink canvas | `--ink-0 #0A0F16` (page), `--ink-1 #0D141D` (panels), `--ink-2 #111A26` (hover), `--ink-3 #0B1119` (console) |
| Paper text | `--paper #EAF0F7`, `--paper-2 #C2CEDC` (body), `--paper-3 #92A6BB` (micro-labels) — all ≥ 7:1 on ink |
| Hairlines | `--line` at 15% and `--line-strong` at 32% of a blue-ink tint `rgba(148,182,221,…)` |
| Signal accent | **Electric cyan** `--cyan #4FD5F2` — the single committed accent; interactive states, focus rings, key stats, the active cell |
| Spreadsheet motif | `--green #49C47E` — **only** for cell values inside the sheet motifs (hero sheet, console spreadsheet strings) |
| Spacing | `--s1…--s9` = 4 / 8 / 12 / 16 / 24 / 32 / 48 / 64 / 96 px |
| Motion | `--ease cubic-bezier(.22,.34,0,1)` (a precision ease, no bounce), `--t1 150ms / --t2 200ms / --t3 250ms` |
| Geometry | `--frame 10px` (drawing-frame inset; 8px ≤ 759px), `--rowh 48px` (ruler pitch = minor grid pitch), `--wrap 1200px` |
| Type | Sans body via `system-ui`; everything structural (nav, headings, labels, stats, code, tables) in `ui-monospace` stacks; radius is deliberately 0 everywhere — corners are square, precision is the beauty |

Backgrounds are pure CSS: a minor 48px grid (matching the ruler pitch) and a major 240px grid
via four `linear-gradient` layers on `body::before`, with a radial vignette on `body::after`.
No binary images exist in the candidate at all — every graphic, the favicon (a data-URI SVG)
and the logo glyph (a 24×24 cell grid with one active cell) are drawn in code.

## Signature moment

Scroll the page and the sheet moves with you: row numbers in the left ruler climb like a real
worksheet's gutter, the crosshair's cell reference updates in the nav (`CELL K23`), and the
column letter under the pointer lights in the top ruler. It is scroll-linked but never loud:
1px hairlines, one small hollow square, 150–250 ms eases, all of it `aria-hidden` decoration
that vanishes for reduced-motion users and touch devices.

## What makes it distinct from a generic template

1. **Joined hairline grids instead of cards.** Panels are laid out in `.gridlines` containers —
   `display: grid; gap: 1px` over a hairline background — so panels share gridlines the way
   cells do. No rounded corners, no drop shadows, no gradients-as-decoration.
2. **Figure and section numbering** — "FIG. 00 — SAMPLE EXCERPT", "SECTION C — MEASURED
   PERFORMANCE", "TABLE T-01", "C-01…C-05" callout indices — the page documents itself like a
   drawing set.
3. **Honest data as a visual system.** The comparison table includes the rows Excel wins (idle
   memory, single recalc); the meters hatch their estimates and legend the difference; the open
   time chart hatches the reference apps instead of styling them as villains.
4. **System fonts, zero dependencies, zero external requests.** The aesthetic comes from hairlines,
   mono micro-labels and rhythm, not from a webfont or a component library.
5. **Micro-labels with real content.** Every "FIG." caption states a fact ("VERBATIM FROM
   README.MD", "DEV MACHINE · RELEASE BUILD"), never filler.

## Motion inventory (all gated by `prefers-reduced-motion`)

- Cell-field ripple: 9 s diagonal sweep, one 3% window per cell, staggered by `--i * 0.45s`.
- Scroll reveal: opacity + 10px translate, 250 ms, staggered ≤ 70 ms between siblings.
- Meter fills: `scaleX` from 0, 550 ms, once in view.
- Stat count-up: 700 ms ease-out-cubic to the exact printed value (HTML holds the final value;
  JS only ticks toward it — no-JS users see the number immediately).
- Hover/focus: border-colour and text-colour transitions at 150 ms; the copy button confirms
  in green; nav source arrow and panel "+" corner marks brighten on hover.

## Accessibility notes

- Semantic landmarks (`header`, `nav` ×2, `main`, `footer`), one `h1`, sections labelled by
  their `h2`; skip link; `lang="en"`".
- The hero sheet and the chart are `role="img"` with descriptive `aria-label`s; all fixed
  chrome (`frame`, rulers, crosshair) is `aria-hidden`.
- Focus: visible 2px cyan outlines with 3px offset on interactive elements.
- Contrast: body text ≥ 10:1, micro-labels ≥ 7:1, cyan ≥ 11:1 on the ink canvas.
- The comparison table scrolls horizontally on narrow screens inside a labelled
  `role="region"` with `tabindex="0"`".
- With JavaScript disabled: every fact, list, table, figure caption and command line is fully
  readable; rulers show static coordinates; nothing depends on a fetch.

## Files

| File | Role |
| --- | --- |
| `index.html` | The complete site, hand-authored |
| `assets/style.css` | The token system and all styling |
| `assets/app.js` | Chrome, reveals, count-ups, scrollspy, copy buttons |
| `DESIGN.md` | This document |
| `DEPLOY.md` | How to publish on GitHub Pages |

Open `index.html` directly from disk — no build step, no server, no network.
