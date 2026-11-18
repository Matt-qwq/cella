# Kinetic Gradient — design direction

Cella's launch page as a **major-version product film**: a deep dark stage, one
violet→cyan→spring-green gradient system doing all the colour work, glass cards, and
scroll-driven motion that behaves like a recalculation — upstream first, dependents
next, one pass, then it rests.

## Concept

The metaphor is the spreadsheet's own physics: a value lands in one cell and a wave of
dependents updates in order. Every motion on the page — reveals, counters, the chart
draw, the terminal session — follows the same rhythm as Cella's recalculation engine:
something is edited, the dependency chain flashes in order, a status chip confirms,
everything settles. Nothing oscillates forever; nothing moves without a reason.

## Design tokens (defined once, in `assets/style.css` `:root`)

| Token group | Values |
| --- | --- |
| Base | `--bg-0: #07070f` (near-black indigo), hairlines `--line: rgba(255,255,255,.09)` |
| Ink | `--ink-0: #eef0ff` (headings), `--ink-1: #c0c5e6` (body), `--ink-2: #9298c0` (captions; ≈6:1 on base — WCAG-AA) |
| Gradient system | violet `#8a7bff` → cyan `#3fd8ff` → spring `#4df0a6`; the *only* accent source — text clip, buttons, hairlines, dots, icons and the mesh all derive from it |
| Type | system stack only (no webfonts); display `clamp(2.5rem, 1.1rem + 4.7vw, 5.4rem)`, h2 `clamp(1.7rem, 1rem + 2.4vw, 2.7rem)`; all stats `font-variant-numeric: tabular-nums` |
| Spacing | 8px scale `--sp-1…--sp-7`; section padding `clamp(76px, 9.5vw, 124px)` keeps one rhythm from phone to ultrawide |
| Radii | `--r-sm: 8` `--r-md: 14` `--r-lg: 22` `--r-xl: 30` |
| Easing | `--ease-out: cubic-bezier(.22,1,.36,1)` (everything entering), `--ease-io: cubic-bezier(.65,.05,.35,.95)` (ambient only); durations .25/.45/.8 s |
| Glass | `rgba(255,255,255,.045)` + `backdrop-filter: blur(14px) saturate(1.3)`, with a solid `@supports` fallback |

## The signature moment

`#performance` opens with a spreadsheet window mockup — title bar, formula bar, a real
`<table>` with `tabular-nums`, and an SVG revenue chart beside it. JavaScript stages a
before-state, then plays: **B4 is edited (cyan ring) → the value lands → dependents flash
spring-green in dependency order** (row revenue, the units and revenue totals, then every
share, staggered 100 ms) — the formula bar tracks the active cell's formula and a chip
turns to *recalculated · 8 cells*. The SVG line chart draws itself on scroll
(`stroke-dashoffset`, one 1.6 s pass, dots popping after), its Q3 spike annotated
+$1,350. A film-cut dip resets the loop; it only runs while the section is on screen.

**Fallbacks:** with JavaScript off, the HTML already carries the fully recalculated
state — the story is still complete, just static. With
`prefers-reduced-motion: reduce`, the JS keeps the finished state and animates nothing.

## Motion rules

- Motion is gated twice: CSS `@media (prefers-reduced-motion: reduce)` kills every
  animation/transition, and the JS checks `matchMedia` before arming anything.
- Two ambient loops only, both slow: the hero gradient-mesh drift (32 s alternate) and
  the capabilities marquee (52 s linear, pauses on hover, scrolls horizontally instead
  under reduced motion).
- Reveals are IntersectionObserver-driven (`data-reveal` + per-card `--rv` delays);
  unrevealed states exist **only** under `html.js`, so no-JS visitors never see a
  hidden page.
- One easing family; hover states lift 2–5 px with `translate` (kept separate from the
  `transform` used by reveals, so the two never fight).

## Distinct from a generic template

1. **One gradient system** — a single violet→cyan→spring axis is the entire palette;
   no random accent colours, no glow spam (glow appears twice: the terminal's live dot
   and the edited cell's ring).
2. **The motion is domain-specific** — reveals, counters and the chart all move like a
   recalculation wave, not like generic "fade up on scroll".
3. **Real instrument panel** — a formula bar, a status chip, tabular numerals, a
   fuzzed-JSON terminal: furniture borrowed from the product, not decoration.
4. **Every number is sourced** — measured on the dev machine against
   Excel 16.0.20527 and WPS 12.1.0.25222, with the link to `docs/performance.md`.
5. **Spreadsheet grain** — a faint 44 px cell grid fades behind the hero, and an SVG
   noise layer (`feTurbulence`, inline data-URI) sits over the whole page at 5%.
6. **Original art only** — logomark, icons, mockup, chart and OG card are all
   hand-authored inline SVG; system fonts; no third-party asset anywhere.

## File map

- `index.html` — the whole site, semantic landmarks, one `<h1>`.
- `assets/style.css` — tokens + all styling (`@supports` and reduced-motion blocks included).
- `assets/app.js` — enhancement layer only (reveals, counters, wave, chart, terminal, header depth).
- `assets/og.svg` — 1200×630 Open-Graph card, same tokens, no external references.
