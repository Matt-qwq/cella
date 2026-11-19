# Cella website candidates

[Workbook Studio](studio/index.html) is the selected Cella homepage and the default
for the manual GitHub Pages workflow. Its [Simplified Chinese](studio/index.html)
page is the default, with a complete [English version](studio/en.html). The
[comparison page](index.html) still opens
all four proposals. The older `blueprint/`, `editorial/`, `kinetic/` and `retro/` directories
are preserved for reference and are not part of the deploy workflow.

| Design | Lead idea |
| --- | --- |
| [Workbook Studio](studio/index.html) | The real Cella interface and a sales dashboard come first. |
| [Cell Lab](cell-lab/index.html) | An interactive, precomputed worksheet shows what changes when one input changes. |
| [Open Print](open-print/index.html) | An open-source manifesto set as a large-format print poster. |
| [Casebook](casebook/index.html) | A gallery of actual workbooks, organized by what they do. |

## Preview locally

Open `site-candidates/index.html` in a browser from the source checkout. You can also serve
from the **repository root** so the shared Cella icon and original screenshots resolve:

```sh
python -m http.server 8000 --bind 127.0.0.1
```

Then open [Workbook Studio](http://127.0.0.1:8000/site-candidates/studio/) or
[the comparison page](http://127.0.0.1:8000/site-candidates/).
No build, package manager, or network-loaded fonts are needed. The website uses
only its native HTML controls and CSS; the app and its Rust/WASM build are separate.

## Deploy

The site is live at <https://matt-qwq.github.io/cella/> (English:
<https://matt-qwq.github.io/cella/en.html>). The repository's **Settings → Pages →
Source** is set to **GitHub Actions** and the repository homepage points at the site.

1. In **Actions → GitHub Pages → Run workflow**, select **main** and leave the
   design set to `studio`. Use `compare` only if you want to publish the review gallery
   instead. The workflow is manual: a push does not publish or replace the site.
2. Check the deployment URL reported by the job (for the default project domain,
   `https://matt-qwq.github.io/cella/`). Rerun the workflow with a different choice
   to replace it. The Pages artifact includes only the website, its assets and
   licence notices, not the source tree.
