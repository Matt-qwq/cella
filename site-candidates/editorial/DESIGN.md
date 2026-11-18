# DESIGN — Editorial Paper

**Candidate:** `site-candidates/editorial/` — the official-site candidate for Cella.
**Direction:** Editorial Paper — a light typographic journal. A long-form magazine
feature about a serious tool: warm paper, near-black ink, one oxblood accent,
serif display type, numbered chapters, margin notes, journal-grade data tables.

## The concept

The page is set the way a well-made journal sets a feature article: a masthead with
an issue line, a standfirst, a plate, a contents list, six numbered chapters, an
appendix and a colophon. Nothing on the page is a card, a gradient, a shadow or a
chip. Hierarchy is carried entirely by type size, small-caps eyebrows, hairline
and heavy rules, and whitespace. The argument for the product is made with the
product's own evidence: every performance claim sits in a hairline data table
with a footnote citing the repo file it came from, and the comparison table
prints the rows Cella loses, with a note saying why.

## Design tokens (defined once, in `assets/style.css` `:root`)

| Group | Values |
|---|---|
| Paper | `#FAF7F0` paper, `#F4EFE5` aged paper (insets, colophon) |
| Ink | `#1F1B16` primary, `#57503F` secondary (6.6:1 on paper) |
| Accent | oxblood `#7A2E26`, hover `#5E211B` — eyebrows, thin rules, drop caps, footnote refs, active cell |
| Rules | hairline `#D9D2C3`; heavy rules and scotch rules are `#1F1B16` |
| Type | serif stack Charter → Bitstream Charter → Iowan Old Style → Palatino → Georgia; mono stack `ui-monospace` → Cascadia → Consolas |
| Scale | 0.6875 / 0.8125 / 1.0625 / 1.3125 / 1.75 / 2.125 / 2.75 / 4.25 rem |
| Spacing | 0.375 → 0.75 → 1.125 → 1.75 → 2.75 → 4.25 → 6.75 rem (a 1.6-ratio rhythm) |
| Motion | `cubic-bezier(0.19, 1, 0.22, 1)`, 180 ms; one 600 ms article-head fade |
| Shape | radius 2 px on the few inset surfaces; everything else is square |

## The signature moment

The margin notes. On wide screens each chapter's reading column is a CSS grid
(`66ch` + a gutter); every citation — `ROADMAP.md`, `docs/performance.md`,
`AGENTS.md` — floats as numbered marginalia in that gutter, exactly like an
academic journal, with a hairline over each note. The same markup becomes an
inline gloss with a left rule on narrow screens and in print, so no content
depends on the layout. It is the element no template has, and it fits the
product: a project that measures everything and cites everything.

Supporting signatures: the masthead with its issue line (`Vol. 0.13 · stage
alpha`), scotch double rules, and the Cella mark — a four-cell grid with the
lower-right cell in oxblood, drawn as inline SVG and repeated as the data-URI
favicon; Fig. 1, a worksheet drawn entirely in SVG type and hairlines; drop
caps opening all six chapters; justified prose with hyphenation and old-style
figures, while every stat and table cell is forced to tabular numerals.

## What makes it distinct from a generic template

1. **Journal furniture, not landing-page furniture** — masthead, contents that
   track reading position, pull quotes with real citations, numbered chapters,
   a colophon. No hero banner, no feature cards, no "trusted by" row.
2. **Candour as design** — Table 2 prints the rows where Excel 16 wins, with a
   note; the status chapter opens with the project's own ~55 % scorecard.
3. **The evidence is linked** — sixteen margin notes, each naming the repo
   file the claim came from.
4. **Print is a feature** — the print stylesheet turns the page into a clean
   article: no backgrounds, notes inline, tables and listings kept whole,
   `@page` margins set.
5. **Zero external anything** — system fonts, inline SVG, data-URI favicon;
   the page is complete offline, from `file://`, and with JavaScript off.

## Accessibility notes

Semantic HTML (`header/main/section/nav/figure/aside/dl`), `lang="en"`, a skip
link, visible focus rings (2 px oxblood, 3 px offset), WCAG-AA contrast for all
body text (ink 14:1, secondary 6.6:1, accent 8.7:1 on paper), footnote refs are
real anchors to focusable (`tabindex="-1"`) notes with a `:target` wash,
`prefers-reduced-motion` removes the one fade and all transitions, the current
contents item also sets `aria-current`. The chip is `aria-hidden` decoration.

## Engineering

Hand-authored HTML/CSS/JS, no build step, no framework. Three files:
`index.html`, `assets/style.css` (tokens, layout, print), `assets/app.js`
(scroll tracking + clipboard buttons, both optional enhancements). All URLs
relative, so the site works under `/cella/` on GitHub Pages and from disk.
