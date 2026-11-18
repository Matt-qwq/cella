# Publishing this candidate to GitHub Pages

The site is one self-contained folder: `index.html` plus `assets/`, no build step, no
external resource of any kind. It works from `file://` and under any path prefix.

## The relative-path constraint (already respected)

GitHub Pages serves a project site under **`/cella/`**, so every asset URL in the
candidate is relative (`assets/style.css`, `assets/app.js`, `assets/og.svg`, a
data-URI favicon). There are **no URLs starting with `/`** and no CDN or webfont
references — copy the folder anywhere and it still renders. Keep it that way: if you add
files, reference them relatively.

The branch must contain the site at its **root**, so the published URL is
**https://matt-qwq.github.io/cella/**.

## gh-pages branch workflow

The recipe below works in a throwaway worktree, so it never disturbs your main checkout
(or its ignored `target/`, `plan/` and friends).

**First publish** (from the repo root, on `main`, with `site-candidates/kinetic` committed):

```bash
git worktree add --detach ../cella-publish main
cd ../cella-publish
git switch --orphan gh-pages   # the branch does not exist yet
git clean -fdx                 # safe here: this worktree only ever held tracked files
git checkout main -- site-candidates/kinetic
cp -r site-candidates/kinetic/. .
rm -rf site-candidates
git add -A
git commit -m "site: publish the Kinetic Gradient candidate"
git push origin gh-pages
cd -
git worktree remove --force ../cella-publish
```

**Later updates:**

```bash
git worktree add --detach ../cella-publish main
cd ../cella-publish
git switch gh-pages             # if it only exists on the remote:
                                #   git switch -c gh-pages --track origin/gh-pages
git clean -fdx
git checkout main -- site-candidates/kinetic
cp -r site-candidates/kinetic/. .
rm -rf site-candidates
git add -A
git commit -m "site: update"
git push origin gh-pages
cd -
git worktree remove --force ../cella-publish
```

Then on GitHub once: **Settings → Pages → Build and deployment → Source: Deploy from a
branch**, branch `gh-pages`, folder `(root)`.

On this Windows shell `cp` is aliased interactively — use `/bin/cp -f` or a file tool.

## Notes and honest caveats

- **og:image is relative and an SVG.** Crawlers resolve it against the page, but most
  social-card consumers expect an absolute PNG/JPEG URL and will not rasterise SVG.
  Before sharing widely, render `assets/og.svg` → `assets/og.png` (1200×630) locally,
  commit the PNG, and point `og:image` at the absolute
  `https://matt-qwq.github.io/cella/assets/og.png`.
- **Custom domain:** drop a `CNAME` file (the domain, one line) next to `index.html` on
  the branch; relative paths keep working.
- **Offline / file://:** unchanged — the site is fully self-contained.
- **Sanity check before pushing:** open `index.html` from disk with JavaScript disabled
  once; everything must still be readable, with the mockup showing its recalculated state.
