# DEPLOY — publishing this candidate on GitHub Pages

The site is a fully self-contained, hand-authored bundle:

    index.html
    assets/style.css
    assets/app.js
    assets/og-card.svg

No build step, no dependencies, no network fetches. Open `index.html`
directly from disk (file://) or serve the folder to preview:

    cd site-candidates/retro
    python -m http.server 8000

## The path constraint

GitHub Pages will serve this at **https://matt-qwq.github.io/cella/** — a
subdirectory. Every asset URL in the bundle is therefore **relative**
(`assets/style.css`, not `/assets/style.css`), which also keeps the page
working when opened from disk. Anchor links (`#why` …) are unaffected.

After publishing you may absolutize `og:image` to
`https://matt-qwq.github.io/cella/assets/og-card.svg` for social scrapers
(it ships relative so the constraint holds everywhere; some scrapers also
prefer a raster card — a future, optional step).

## Publishing (gh-pages branch)

The candidate lives at `site-candidates/retro` on `main`; the gh-pages branch
needs those files at its **root**. From a clone of
https://github.com/Matt-qwq/cella :

    git clone https://github.com/Matt-qwq/cella cella-publish
    cd cella-publish
    git switch --orphan gh-pages        # empty orphan branch, no history
    git checkout main -- site-candidates/retro
    git mv site-candidates/retro/index.html index.html
    git mv site-candidates/retro/assets assets
    git mv site-candidates/retro/DESIGN.md DESIGN.md   # optional on the site
    # DEPLOY.md is a process doc — keep it out of the published root if you like
    rmdir site-candidates/retro site-candidates 2>/dev/null || true
    git commit -m "site: publish Terminal Ledger candidate"
    git push -u origin gh-pages

Then: repo **Settings → Pages → Deploy from a branch → gh-pages / (root)**.
The site appears at https://matt-qwq.github.io/cella/ within a minute or two.

## CI alternative (no extra files committed here)

If you prefer a workflow, add `.github/workflows/pages.yml` on main (this
candidate deliberately ships no workflow file):

    name: pages
    on: { push: { branches: [main] } }
    permissions: { contents: read, pages: write, id-token: write }
    jobs:
      deploy:
        runs-on: ubuntu-latest
        steps:
          - uses: actions/checkout@v4
          - run: mkdir -p public && cp -r site-candidates/retro/. public/
          - uses: actions/upload-pages-artifact@v3
            with: { path: public }
          - uses: actions/deploy-pages@v4

(Verify action versions before use; pins are the maintainer's call.)

## Checklist before flipping it live

- Serve under a subdirectory and click every nav anchor.
- Disable JavaScript: boot log, sheet values, and bars all render.
- Emulate prefers-reduced-motion: no typing, blink, marquee or pulses.
- 320px viewport: no horizontal scroll outside the code blocks and wordmark.
- Confirm `grep -r "http" index.html` only matches `<a href>` links.
