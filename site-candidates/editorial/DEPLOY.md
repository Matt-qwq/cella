# DEPLOY — publishing this candidate on GitHub Pages

The site is a static directory: `index.html` plus `assets/`. Every URL inside
is relative, so it works at any base path — including the project Pages URL
`https://matt-qwq.github.io/cella/` — and straight from `file://`.

## The constraint that shaped it

GitHub Pages will serve the site under **`/cella/`**, not `/`. Absolute paths
like `/assets/style.css` would 404. This candidate therefore contains **no
leading-slash URLs at all**: styles, script and favicon are relative, anchors
are in-page. Check with:

```bash
grep -n 'href="/' index.html ; grep -n 'src="/' index.html   # no output = good
```

## Publishing (gh-pages branch)

From a clean checkout of `main`, at the repository root:

```bash
# 1. Keep a copy of the site outside the tree.
/bin/cp -rf site-candidates/editorial ../cella-site-candidate

# 2. Create the empty branch (this removes the tracked files from the
#    working tree and index; untracked files stay — check `git status`).
git switch --orphan gh-pages

# 3. Bring the site in as the branch root. Verify that the directory now
#    holds only what should be public: index.html, assets/, and optionally
#    DESIGN.md / DEPLOY.md (delete those two here if you want it lean).
/bin/cp -rf ../cella-site-candidate/. .
ls -la                       # expect index.html, assets/ (+ the two .md files)

git add -A
git commit -m "site: publish the Editorial Paper candidate"
git push -u origin gh-pages
```

Then on GitHub: **Settings → Pages → Build and deployment → Source: Deploy from
a branch → Branch: `gh-pages`, folder `/(root)` → Save.** The site appears at
`https://matt-qwq.github.io/cella/` within a few minutes.

## Notes

- **No `og:image` yet, on purpose.** The brief for this candidate forbade new
  binary files, so the Open Graph tags are title/description/type/url only.
  To add one later: commit a PNG (e.g. `assets/og.png`, 1200×630) to
  `gh-pages` and add `<meta property="og:image" content="…/assets/og.png">`.
- **DESIGN.md / DEPLOY.md** ride along in the directory; they are harmless on
  the published branch (GitHub renders them), and step 3 shows where to drop
  them if you would rather not ship them.
- **Custom domain?** Works unchanged: the relative URLs adapt to whatever base
  the site is served from.
- To keep `main` as the single source (branch-free alternative), a small
  Actions workflow that copies `site-candidates/editorial/` to the Pages
  artifact would do the same job; the branch route above needs no CI secrets.
