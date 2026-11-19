# Cell Lab

**Purpose:** a Pages homepage for people deciding whether Cella fits their workbooks or their scripts. The first thing to try is one edit, not a screenshot: the print-run worksheet leads into formulas, compatible files, and automation. It ends with releases and a source quick start, not a promise of full Excel parity or a hosted demo.

## Visual system

- **Plum glass** (`#1d171e` ground, `#2b202a` bench) with bone lettering (`#f5ede5`) and fine sheet rules. Amber (`#ffc47a`) marks the selected cell, its result, and primary entry points. The one light sheet is the quick-start section.
- Use local system type: large, tightly set sans headings; monospace only for formulas, addresses, and code. Rows and dividers carry the visual language down the page. On narrow screens the lab reads as input → worksheet → result, in the same order as the HTML.

## Worksheet behavior

Native radios change B1. The formula bar shows B4, `=SUM(B2,B1*B3)`; the side readout shows B5, `=B4/B1`. B2 is a $60.00 setup charge and B3 is $0.20 per copy in both presets.

| B1 · copies | B4 · total cost | B5 · cost per copy |
| ---: | ---: | ---: |
| 100 | $80.00 | $0.80 |
| 250 | $110.00 | $0.44 |

The values are precomputed and labeled as such. CSS `:has()` exposes only the selected preset's values. Each radio is also described by its total and cost per copy, and the values can be read in the worksheet table. No live-region timing or client code is required. Unsupported browsers see a fixed 100-copy example without the radios. The only state animation is a brief opacity change on the readout, disabled under reduced motion.

For Pages previews, keep `site-candidates/cell-lab/` at this depth relative to `assets/` and `docs/`; the icon and render URLs also resolve under a `/cella/` prefix.

## Credits

`../../assets/app-icon/cella-small.svg` and `../../docs/images/dark-insert.png` are by Cella contributors (MIT OR Apache-2.0), as recorded in the repository's [attribution ledger](../../ATTRIBUTION.md). The small inline SVG arrows and both teaching examples are original to this page. All type is supplied by the browser or OS. See [NOTICE](../../NOTICE) for upstream code attribution.
