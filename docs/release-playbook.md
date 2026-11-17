# Release playbook

This tree has no release pipeline. The upstream playbook (a shared document in the craftrules
repository, reached through `storytold/craft-common-actions`) drove a signed multi-platform release
from a push to the `release` branch; that workflow and its signing secrets were ArtCraft's and were
removed with the rest of the upstream identity surface.

What remains here:

- `packaging/` — the scripts that build local installers (macOS `Info.plist`, DMG, Linux
  nfpm/Flatpak, Windows WiX, FreeBSD, web). They still carry upstream's app id and are being
  updated; see [releasing.md](releasing.md).
- `cargo xtask version` — bumps the workspace version and keeps the packaging files in step.

Releasing as an independent product needs its own decisions first: the product name, the
organisation/domain and the reverse-DNS id, and how builds are signed. Those are recorded in
[`roadmap.md`](roadmap.md) § Product position.
