# Release playbook

This tree has no release pipeline. The upstream playbook (a shared document in the craftrules
repository, reached through `storytold/craft-common-actions`) drove a signed multi-platform release
from a push to the `release` branch; that workflow and its signing secrets were ArtCraft's and were
removed with the rest of the upstream identity surface.

What remains here:

- `packaging/` — the scripts that build local installers (macOS `Info.plist`, DMG, Linux
  nfpm/Flatpak, Windows WiX, FreeBSD, web). They carry Cella's own identity (app id
  `io.github.matt-qwq.cella`, publisher `Matt-qwq`, WiX UpgradeCode `5410AC94-…`); see
  [releasing.md](releasing.md).
- `cargo xtask version` — bumps the workspace version and keeps the packaging files in step.

The identity decisions are made (product `Cella`, publisher `Matt-qwq`, reverse-DNS
`io.github.matt-qwq`, UpgradeCode Cella's own since 2026-11-18); signing material belongs to the
distributor. The step-by-step checklist — version, gates, performance report, packaging,
resource verification, checksums, tag and `gh release` — is [releasing.md](releasing.md).
