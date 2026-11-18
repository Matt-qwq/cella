# DEPLOY.md — publishing this candidate on GitHub Pages

The site lives in `site-candidates/blueprint/` and is fully self-contained:
`index.html` plus `assets/style.css` and `assets/app.js`. All asset URLs are
**relative** (`assets/style.css`, `assets/app.js`, favicon as an inline data
URI), because the site will be served under `https://<user>.github.io/cella/` —
a subdirectory, not a domain root. There are no absolute paths starting with `/`
anywhere, and the page also works opened directly from disk (`file://`).

## Option A — a gh-pages branch (no workflow file needed)

From a clean checkout of `main` (commands for the maintainer; do not run them from
a worktree):

```bash
# 1. an orphan branch with no history (old files stay on disk, untracked)
git switch --orphan gh-pages

# 2. empty the worktree - a site branch carries only the site
git clean -xfd

# 3. bring the candidate in and move its files to the branch root
git checkout main -- site-candidates/blueprint
mv site-candidates/blueprint/* .
rmdir site-candidates/blueprint site-candidates

git add index.html assets/ DESIGN.md DEPLOY.md
git commit -m "site: deploy blueprint candidate"
git push origin gh-pages
```

Then in the repository: **Settings ▸ Pages ▸ Source: Deploy from a branch** —
branch `gh-pages`, folder /(root). The site appears at
`https://matt-qwq.github.io/cella/` within a few minutes.

## Option B — a GitHub Actions workflow (recommended long-term)

Add `.github/workflows/pages.yml` on `main` (outside this candidate's scope, so
it is not committed here) that copies the folder and publishes the artifact:

```yaml
name: Pages
on: { push: { branches: [main] } }
permissions: { contents: read, pages: write, id-token: write }
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: mkdir -p _site && cp -r site-candidates/blueprint/. _site/
      - uses: actions/upload-pages-artifact@v3
        with: { path: _site }
  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment: github-pages
    steps:
      - uses: actions/deploy-pages@v4
```

Then **Settings ▸ Pages ▸ Source: GitHub Actions**. Option B keeps the site in
review on `main` and needs no orphan branch.

## Constraints and checks

- **Relative paths only.** The site must work under `/cella/`; never introduce a
  leading `/` in `href` or `src`, and never point at a CDN — the page is
  offline-capable by design (system font stacks, inline SVG, no analytics).
- **Verify before publishing:**

  cd site-candidates/blueprint
  python -m http.server 8000    # open http://localhost:8000/

  Check: 320 px viewport, JavaScript disabled, keyboard tab order,
  `prefers-reduced-motion` emulation, and the Open Graph tags.
- **HTTPS**: enable "Enforce HTTPS" in the Pages settings.
- The candidate commits only its own files; the deploy step is the maintainer's
  separate, explicit action.
