# Releasing Cella

A release is a **manual, local run**. The repository publishes nothing by itself: the upstream
workflow (`storytold/craft-common-actions` plus the ArtCraft signing secrets) was removed when this
tree stopped being a fork kept to propose changes upstream. `.github/workflows/` still holds the
packaging *checks* and the FreeBSD VM job; none of them build or upload a release.

Everything below is Cella's own identity. Change these values together if the product moves again.

| App-specific value | Where |
|---|---|
| Display name `Cella`, binaries `cella` + `cella-cli` | every script and workflow |
| Bundle / app id `io.github.matt-qwq.cella` | `packaging/macos/Info.plist.in`, `packaging/linux/*`, flatpak manifest |
| Publisher `Matt-qwq` | WiX `Manufacturer` (`packaging/windows/cella.wxs`), `CompanyName` in `apps/cella/build.rs`, AppStream `<developer>`, nfpm `vendor` and `maintainer` |
| WiX UpgradeCode `5410AC94-288D-44C6-BE25-D154F47331B6` (Cella's own, set 2026-11-18) | `packaging/windows/cella.wxs` |
| File types `.xlsx` `.xlsm` `.xlsb` `.ods` `.csv` `.tsv` | Info.plist (`CFBundleDocumentTypes`), WiX (ProgIds/OpenWithProgids), `.desktop` `MimeType=`, `io.github.matt-qwq.cella.mime.xml`, metainfo |
| Web app dir `apps/cella-web` | `packaging/web/package.sh` |
| App colour `#1f9d55` (ink `#147a40`), **proposed** | `assets/app-icon/README.md`, metainfo `<branding>` |

## Cutting a release

The checklist below is the process that produced 0.13.0, in order. Steps 2–4 are the gates; skip
none of them "just this once".

1. **Bump the version:** `cargo xtask version set X.Y.Z` (updates `Cargo.toml`,
   `Cargo.lock` and `xtask/version`). One commit, message `X.Y.Z: release`.
2. **Gate:** `cargo xtask ci` — fmt, `clippy --all-targets -D warnings`, the tests, `assets`
   (every asset file has an `ATTRIBUTION.md` row), `layers`, `wasm`. It takes minutes; a green
   run prints `9/9`.
3. **Performance gate:** `cargo xtask perf` against `xtask/perf-budgets.txt` (release build,
   100k and 1M rows). It is deliberately outside `ci`; a budget is 2× a quiet pass, so a failure
   means a real regression, not noise.
4. **Performance report:** `cargo xtask perf-report` rewrites `docs/performance-report.md` and
   `docs/performance-report.zh-CN.md` (it embeds `docs/external-benchmark.md`), then commit them.
   The docs link these pages, so a release with stale numbers ships a wrong comparison.
5. **Build:** `cargo build --release -p cella -p cella-cli`.
6. **Package:** the platform script — `packaging/windows/package.ps1 -Arch x64`,
   `packaging/macos/package.sh [--arch aarch64]`, `packaging/linux/package.sh`. Output lands in
   `dist/release/`. Signing is optional and belongs to the distributor; unsigned artifacts are
   still checksummable and releasable.
7. **Verify the Windows resources** before publishing anything (this shipped wrong once): the
   packager sets `CELLA_REQUIRE_WINRES=1`, and the check is what the *built* exe carries —
   `(Get-Item cella.exe).VersionInfo` must show `FileVersion X.Y.Z` and `CompanyName Matt-qwq`,
   and the MSI must embed the icon (`CellaIcon`, reachable through `ARPPRODUCTICON`). An older
   tree read a different variable name, so the guard passed while the exe came out iconless.
8. **Checksums:** `sha256sum dist/release/*` (or `Get-FileHash`); keep sizes and hashes for the
   release notes.
9. **Tag, then publish:**
   ```sh
   git tag -a vX.Y.Z -m "Cella X.Y.Z" && git push origin vX.Y.Z
   gh release create vX.Y.Z --repo Matt-qwq/cella dist/release/* --title "Cella X.Y.Z" \
       --notes-file RELEASE-NOTES.md
   ```
   `--repo` is not optional: `origin` is `Matt-qwq/cella`, but a checkout with an upstream
   remote configured resolves `gh` to `storytold/gridcraft`.
10. **Verify the published page:** artifact names, sizes and hashes match step 8, and the notes
    state what was **not** run (macOS/Linux gates, the pinned corpus, signing).

### Writing the notes

`RELEASE-NOTES.md` is what `gh release create --notes-file` publishes. Keep it under one screen:

- **A first paragraph a user can act on** — what this release is, and the one number that matters.
- **At most five bullets**, each one line, each a change they can see or feel. No changelog dump,
  no per-commit list; the history is in `git log`.
- **One table** for numbers, if numbers are the story. Everything else links to `docs/`.
- **`## Installing`** — artifact names and any upgrade step (an UpgradeCode change is one).
- **`## Not verified in this build`** — the honest list. A release that says what it did not test
  reads as trustworthy; one that implies it tested everything does not.
- Say "unsupported"/"unsigned" where it applies rather than implying a support promise.

### Traps met so far

- **A tag can already exist upstream.** `v0.13.0` was pushed to the upstream repository first and
  the local tag pointed at that old commit. Delete the local tag, re-tag the release commit and
  force-push the tag, otherwise the GitHub release points at upstream's tree.
- **`gh` defaults to the other repository** when both remotes exist — every `gh` call in this
  document carries `--repo Matt-qwq/cella`.
- **A new UpgradeCode does not upgrade the old install.** Cella changed its UpgradeCode on
  2026-11-18 (it had inherited GridCraft's `5511327B-…`); installations made with the old code
  stay side by side, so the release notes say to uninstall them first.
- **Release notes age fast.** The numbers in them (memory, timings) are that build's; when a later
  round changes them, correct that release's notes rather than writing new numbers into an old
  release.

## What gets built

| Platform | Artifacts | Built on |
|---|---|---|
| macOS 11+ (universal: Apple silicon + Intel) | `cella-<v>-macos-universal.dmg`, `cella-cli-<v>-macos-universal.zip` | `macos-15` |
| Windows 10+ x64 | `cella-<v>-windows-x64.msi`, `cella-<v>-windows-x64-portable.zip` | `windows-latest` |
| Windows 10+ x86 (32-bit) | `cella-<v>-windows-x86.msi`, `cella-<v>-windows-x86-portable.zip` | `windows-latest` |
| Windows 11 ARM64 | `cella-<v>-windows-arm64.msi`, `…-portable.zip` (cross-compiled, signed like x64) | `windows-latest`; installed and run on `windows-11-arm` by `windows-arm64.yml` |
| Linux x86_64 | `cella-<v>-linux-x86_64.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` | `ubuntu-22.04` |
| Linux aarch64 | `cella-<v>-linux-aarch64.{AppImage,AppImage.zsync,deb,rpm,tar.gz}` | `ubuntu-22.04-arm` |
| Linux riscv64 | `cella-<v>-linux-riscv64.tar.gz` (cross-compiled, glibc >= 2.39; CLI smoke-tested under QEMU) | `ubuntu-24.04` |
| Flatpak x86_64, aarch64 | `cella-<v>-linux-<arch>.flatpak` (repackages the Linux tarball) | `ubuntu-24.04`, `ubuntu-24.04-arm` |
| FreeBSD 14 x86_64 | `cella-<v>-freebsd-x86_64.tar.gz` | FreeBSD 14.3 VM (`freebsd.yml`, called by `release.yml`) |
| Web | `cella-web-<v>.zip` (static site; see [`packaging/web/README.md`](../packaging/web/README.md)) | `ubuntu-latest` |

### macOS

`packaging/macos/package.sh` builds `aarch64-apple-darwin` and `x86_64-apple-darwin` with
`MACOSX_DEPLOYMENT_TARGET=11.0`, joins them with `lipo`, and assembles `Cella.app`:

- `Info.plist` comes from `Info.plist.in`: bundle id `io.github.matt-qwq.cella`,
  `LSMinimumSystemVersion` 11.0, `NSHighResolutionCapable`, category productivity, and
  `CFBundleDocumentTypes` for `.xlsx`, `.xlsm`, `.xlsb`, `.ods`, `.csv` and `.tsv` with rank
  **Alternate**, so Cella is offered in *Open With* without taking over Numbers' (or another
  app's) defaults. `.xlsm` and `.xlsb` get imported UTIs in case macOS doesn't know them.
- **Signing** goes inside-out with the hardened runtime and a secure timestamp; no `--deep` on
  the final signature; `entitlements.plist` is deliberately empty.
- **Notarization:** the app is zipped and sent with `xcrun notarytool submit --wait`, then
  stapled. The DMG (with an `Applications` link) is signed, notarized and stapled too, and
  checked with `codesign --verify --strict`, `stapler validate` and `spctl -a -vvv`.
  Its Finder window (background, icon size and positions) comes from
  [`packaging/macos/dmg/`](../packaging/macos/dmg/README.md), and its volume is named `Cella`
  without the version, which the window's background needs; the DMG file name keeps the version.
- **CLI:** the universal `cella-cli` is signed, zipped and notarized.

Locally, without certificates, the script signs ad-hoc (`codesign -s -`) and skips notarization:

```sh
packaging/macos/package.sh                    # universal; needs both rustup targets
packaging/macos/package.sh --arch aarch64     # quicker, host-only
open dist/release/cella-*-macos-*.dmg
```

### Windows

`packaging/windows/package.ps1 -Arch x64|x86|arm64` builds with `-C target-feature=+crt-static`
(no VC++ redistributable needed), checks both PE headers (machine matches `-Arch`; `cella.exe`
is GUI, `cella-cli.exe` console), signs both executables, builds the MSI with WiX v5, signs
the MSI, and zips a portable build.

- `cella.wxs`: per-machine install into Program Files, advertised Start Menu shortcut, App
  Paths (Win+R `cella`). File types: ProgIds `Cella.Workbook` (`.xlsx`, `.xlsm`) and
  `Cella.TextData` (`.csv`, `.tsv`), listed under each extension's `OpenWithProgids` and
  in `Capabilities` + `RegisteredApplications`, so Cella appears in *Open with* and
  *Settings › Default apps* but never silently becomes the default. The MSI version is the
  numeric `X.Y.Z`; same-version upgrades let release candidates replace each other.
- **Desktop shortcut:** full installer UI offers an unchecked checkbox on a new installation.
  Unattended installs can use `msiexec /i cella.msi /qn DESKTOPSHORTCUT=1` to opt in
  (or `DESKTOPSHORTCUT=0` to opt out). The choice is stored per machine and reused on upgrades;
  repair retains the installed component choice, and uninstall removes the managed shortcut.
  The Start Menu shortcut is always installed. Run
  the Packaging lint workflow’s Windows job to build small fixture installers and check
  installation, repair, upgrade, opt-out and cleanup, including removal by SYSTEM after
  installation by the runner account. The shortcut marker uses `HKMU`, which resolves to
  `HKLM` for this per-machine package; it leaves no installing-user registry marker. Unlike
  a literal `HKLM` key path, MSI registry root -1 also meets ICE43's shortcut rule without
  suppressing validation ([WiX registry roots](https://docs.firegiant.com/wix/schema/wxs/registryvalue/),
  [ICE43](https://learn.microsoft.com/windows/win32/msi/ice43)). Its
  `packaging/windows/test-desktop-shortcut.ps1` script requires a GitHub-hosted Windows runner.
- **Installer review images:** the same Windows job renders the compiled MSI's dialogs through
  Windows Installer's native preview API and uploads PNGs plus install logs in
  `desktop-shortcut-msi-evidence`, on success or failure. These are native dialog previews;
  the separate silent lifecycle test verifies installation behavior. Normal full UI presents
  the shortcut choice and then completion; cancel/failure and files-in-use dialogs are support
  paths. To review on a maintainer branch, run **Packaging lint** with that branch selected
  (or `gh workflow run packaging-lint.yml --repo Matt-qwq/cella --ref BRANCH`). No release
  environment, signing credentials or Rust build is required. Interactive checkbox behavior
  still needs a Windows desktop check; previews alone do not establish that behavior.
- **Signing:** `packaging/windows/sign.ps1` uses `signtool` with SHA-256 and an RFC 3161
  timestamp, from a `.pfx` (`WINDOWS_CERTIFICATE*`) if present, otherwise **Azure Trusted
  Signing** (`AZURE_*`, if the distributor has such a subscription). With neither it warns and leaves files unsigned.

Locally on Windows: `dotnet tool install -g wix --version 5.0.2`, then
`pwsh packaging/windows/package.ps1 -Arch x64`.

### Linux

`packaging/linux/package.sh` stages one FHS tree (both binaries,
`io.github.matt-qwq.cella.desktop` with `MimeType=` for the four types, hicolor icons 16–512 px
+ scalable SVG, AppStream metainfo, shared-mime-info) and builds the AppImage (appimagetool),
`.deb` and `.rpm` (nfpm, `nfpm.yaml`) and `.tar.gz` from it. Built on Ubuntu 22.04, so the
binaries need glibc ≥ 2.35. X11/Wayland/xkbcommon/Vulkan/EGL are loaded at runtime; the
packages declare them (see `nfpm.yaml`).

Each AppImage embeds update information
(`gh-releases-zsync|Matt-qwq|cella|latest|cella-*-linux-<arch>.AppImage.zsync`), and the
matching `.zsync` is published beside it, so AppImageUpdate and AppImageLauncher fetch only the
changed blocks of the newest published release. appimagetool writes the `.zsync` when
`zsyncmake` (the `zsync` package) is installed; without it the script warns and skips it.

Flatpak: `packaging/linux/flatpak/io.github.matt-qwq.cella.yml` builds from source and is ready
for a Flathub submission (freedesktop 25.08; Wayland + X11 fallback, `dri`, IPC; Documents and
Downloads; everything else through portals); build it by hand with the commands in its header.
The single-file `.flatpak` on each release comes from `packaging/linux/flatpak-bundle.sh` and
`io.github.matt-qwq.cella.bundle.yml`, which repackage the release's Linux tarball (no Rust
build) and smoke-test the result in the sandbox. packaging-lint checks that both manifests
agree on runtime and finish-args.

Locally (on Linux): install [nfpm](https://nfpm.goreleaser.com/install/), then
`packaging/linux/package.sh` (or `--formats "deb tar"`).

### FreeBSD

GitHub has no FreeBSD runners. `.github/workflows/freebsd.yml` boots a FreeBSD 14.3 VM
(`vmactions/freebsd-vm`). Run by hand from `release`, it builds and tests the workspace; called
from `release.yml` with `package: true` it runs `packaging/freebsd/package.sh`, which builds the
release binaries and writes `cella-<v>-freebsd-x86_64.tar.gz` (`bin/` + `share/`, extract
under `/usr/local`). The target dir lives outside the synced checkout so only `dist/release`
comes back from the VM. No signing (checksums are in `SHA256SUMS.txt`).

### Web

`packaging/web/package.sh` runs `trunk build --release --dist dist/web --public-url ./` in
`apps/cella-web` and zips the site with sample `_headers` / `.htaccess` and the hosting guide
([`packaging/web/README.md`](../packaging/web/README.md)). Relative URLs only, so it works under
any path and in an iframe; the script fails on root-absolute URLs.

## Signing material

The repository holds no secrets. Notarization (macOS) and Authenticode (Windows) use the
distributor's own certificates, outside `packaging/`; the scripts take their paths from the
environment (`packaging/env.sh` lists what they read).

## Icons

`assets/app-icon/cella.svg` is a **placeholder** (an original ledger-page drawing on
Cella green) until the owner makes Cella's engraved creature portrait (craftrules
`standards/icon-design.md`). `packaging/icons.sh` regenerates the 1024 px PNG, the macOS 512 px
PNG, the `.icns` (iconutil), the `.ico` (`cargo xtask ico`) and the hicolor PNGs from it; it
needs `resvg`. The outputs are committed, so packaging never needs those tools. Every file has a
row in `ATTRIBUTION.md` (`cargo xtask assets`).

## Checks

- `.github/workflows/packaging-lint.yml` (seconds, on changes to `packaging/`, the workflows or
  the icons): actionlint, shellcheck, a PowerShell parse, xmllint, a WiX icon-id check,
  `desktop-file-validate`, `appstreamcli validate`, and a Flatpak manifest check.
- `.github/workflows/windows-arm64.yml` (on `release` pushes): packages ARM64 on x64, then
  installs, runs and uninstalls the MSI on a Windows 11 ARM64 runner.
- `.github/workflows/freebsd.yml`: FreeBSD build + tests (manual run from `release`).

**Only the `release` branch builds.** No build workflow runs on `main` or pull requests; every
build job checks `github.ref == 'refs/heads/release'`, so a manual run from another branch is
skipped. Only `packaging-lint.yml` (no build, no secrets) runs on `main` and PRs.
- Locally: `cargo xtask ci` (fmt, clippy, tests, assets, layers, wasm, parity check).
- Before tagging: `cargo xtask perf` — the performance gate against `xtask/perf-budgets.txt`
  (release build at 100k and 1M rows; it is not part of `ci`; see [performance.md](performance.md)).
