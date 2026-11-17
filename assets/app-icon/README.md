# Cella app icon

**The mark:** the letter **C**, with one **cell** in its opening. Cella is named for the Latin
*cella* (a cell, a small chamber) — the word *cell* comes from it — so the mark is the name itself:
a C holding one cell. No grid page, no folded corner, no vendor likeness.

Original geometry, hand-written for this repository by Cella contributors. No third-party material.
Licensed like the rest of Cella (MIT OR Apache-2.0, see `LICENSE.txt`).

## Palette

Exactly three colours:

| Colour | Hex | Used for |
|---|---|---|
| Tile | `#17181c` | The full-bleed rounded field |
| Paper | `#efe9dc` | The C |
| Cell (accent) | `#f7d98a` → `#e8a83a` | The one cell, a vertical gradient |

The accent is warm on purpose: green reads as Excel (and as the Craft suite this product left), and
a warm cell on near-black keeps its edge on both light and dark desktops.

## Geometry

A 512 × 512 viewBox, clipped to a rounded square with `rx=112`. The C is one stroke from
`(344,156)` round through the left side to `(344,356)`, width 52, rounded terminals; the cell is a
64 px square with `rx=16` centred in the opening (`300,226`).

Windows and Linux use the full tile. The macOS icons (`.icns`, `cella-macos-512.png`) sit on
Apple's grid: an 824/1024 body with a transparent margin (`packaging/icons.sh` widens the viewBox to
`-62.136 -62.136 636.272 636.272`).

## Files

| File | Use |
|---|---|
| `cella.svg` | Canonical artwork; also `hicolor/scalable` |
| `cella-1024.png` | 1024 px render of the full tile |
| `cella-macos-512.png` | Runtime Dock icon on macOS (for `apps/cella` to embed) |
| `cella.icns` | macOS bundle icon (`CFBundleIconFile`), 11 sizes |
| `cella.ico` | Windows executable/installer icon, 9 sizes |
| `hicolor/<size>/apps/io.github.matt-qwq.cella.png` | Linux icon theme, 16–512 px |

Regenerate everything with `packaging/icons.sh` (needs `resvg`; `iconutil` on macOS for the .icns —
elsewhere the `.icns` is packed from the same PNGs and must be rebuilt on a Mac before a signed
release). The outputs are committed, so builds and packaging never need these tools.

## Provenance

First version authored 2026-11-18 (owner's pick from a rendered comparison sheet). Replacing the art
means: edit `cella.svg`, run `packaging/icons.sh`, refresh the rows in [`ATTRIBUTION.md`](../../ATTRIBUTION.md),
and update this page's palette and geometry sections.
