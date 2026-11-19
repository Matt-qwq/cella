# Workbook Studio

Workbook Studio is the selected GitHub Pages homepage for Cella. This is a
product-first page for spreadsheet users deciding whether to try the alpha:
see the actual workbook, check compatibility, then explore what sets Cella apart. The
landing page keeps the product-first headline, blue-gray mount, and graphite
workbook section. The amber hero action starts the guide; downloads live in Get Cella.

## Design decisions

| Role | Tokens / treatment |
| --- | --- |
| Page and content | Porcelain `#f8f9fa` with graphite `#17222d`. Headings use a Song-style serif (Noto Serif SC with local fallbacks) in Chinese and a corresponding local serif in English. Prose, navigation and data stay sans; monospace is reserved for commands and standalone file labels. |
| Product surfaces | The dashboard sits on a cool mount (`#edf2f6` to `#e2eaf1`); the second cut stays in graphite (`#253443` to `#15202b`). Depth belongs to the mounted screenshots, not to every section. |
| Actions | Amber `#f2b958` marks the primary guide and release actions. The top-right repository link spells out GitHub and reuses the original external-link arrow; it does not use a third-party mark. |
| Motion | One Silk curve (`.22, .9, .24, 1`): 780ms opening, 460ms view changes, 980ms command trace, 200ms or less for feedback. Motion has a distinct job in each scene. |

- **Set type by role.** The two-line Chinese headline adds the character of a
  printed report without turning its interface into one. Hero and section
  headings use serif, while step titles, captions and controls stay sans.
  Chinese measure is set in `ic`, not Latin character units. The masthead
  wordmark is shared across locales. No font files are shipped or fetched.
- **Explore the sales workbook.** The four native options move the real screenshot
  inside a fixed window. The selection follows the table, regional chart or pie
  on the same image coordinate system. On narrow layouts the options move above the
  image, keeping the controls and result close together.
- **Switch between real views.** Six workbook screenshots share one frame:
  Insert, PivotTable, formulas, budget, chart formatting and page breaks. The
  selector wraps to keep every choice visible. The screenshot and caption
  change together. Both preview groups switch on mouse hover
  (240ms when motion is enabled) and return to the checked view on exit.
  Touch, click to keep a view, and native keyboard selection still work;
  keyboard focus takes precedence over the mouse. Reduced motion switches
  instantly. The compatibility strip stays immediately below the workbook section.
- **Show what sets Cella apart.** A short feature section points to automation,
  measured performance, and build targets. The CLI example keeps the localized
  sample CSV beside the command. Trace command illustrates read → summarize → write;
  it does not run a command or touch a file.
- **Show measured performance.** A labelled chart compares like-for-like open
  times; the table keeps recalculation and memory visible, including results
  where Cella trails Excel. The public comparison covers Cella, Excel and WPS.
  These are dated snapshots of the repository reports: update both
  localized pages when replacing their measurements. On phones, wide tables
  scroll within their own region, keeping the measurement column in view.
- **Connect the sections.** Native scroll timelines move the image inside the dark
  frame, settle the CLI example, and draw reading progress and the release divider.
  This does not intercept the wheel or touch. Browsers without native scroll
  timelines keep the complete content and local controls.
- **Follow device motion preferences.** Reduced motion keeps all three
  demonstrations usable as immediate state changes. Browsers without CSS `:has()`
  see the static examples; the controls that depend on it are hidden. No loops,
  runtime scripts, third-party fonts, or tracking.

## Preview and publish

[简体中文](index.html) is the default. [English](en.html) is a complete sibling
page. Both share a stylesheet and anchor IDs; each links to its own localized
workbook screenshots and equivalent synthetic sales CSV. Keep the preview images,
CSV headers and CLI arguments in sync with the
page language. The Chinese previews are actual Cella renders from the snapshot
example (`--language zh --fonts --size 2000x1250 --scale 1`), with translated
workbook labels set through commands and a locally installed CJK font applied to
the cells containing Chinese text. Keep the sales dashboard layout aligned with
the English render so the shared crop coordinates still work. Header links switch
language at the top.
The [Pages workflow](../README.md#deploy-after-review) publishes both files.
The site describes the WASM build; it does not host a running app.

The original Cella screenshot renders and app icon are listed in
[the repository asset ledger](../../ATTRIBUTION.md).
The other line icons are original inline SVG, not vendor artwork. Upstream
GridCraft attribution is in [NOTICE](../../NOTICE); the licence is MIT OR Apache-2.0.
