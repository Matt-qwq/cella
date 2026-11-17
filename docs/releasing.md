# Releasing Cella

**This tree has no release pipeline.** The upstream workflow that used to run on a push to the
`release` branch called `storytold/craft-common-actions` and the ArtCraft signing secrets; it was
removed when this repository stopped being a fork kept to propose changes upstream (the same change
removed `docs/brand/`, the Sonar project and the store manifest). The packaging scripts under
`packaging/` remain and build local installers; nothing publishes them.

The identity values in the table below are **upstream's** and are being replaced — see
[`roadmap.md`](roadmap.md) § Product position. Until that lands, a build still carries them, so
**do not distribute a build**.

| App-specific value | Where |
|---|---|
| Display name `Cella`, binaries `cella` + `cella-cli` | every script and workflow |
| Bundle / app id `io.github.matt-qwq.cella` | `packaging/macos/Info.plist.in`, `packaging/linux/*`, flatpak manifest |
| WiX UpgradeCode `5511327B-F305-41BE-92D0-EB84944E7C4D` (fixed forever) | `packaging/windows/cella.wxs` |
| File types `.xlsx` `.xlsm` `.xlsb` `.ods` `.csv` `.tsv` | Info.plist (`CFBundleDocumentTypes`), WiX (ProgIds/OpenWithProgids), `.desktop` `MimeType=`, `io.github.matt-qwq.cella.mime.xml`, metainfo |
| Web app dir `apps/cella-web` | `packaging/web/package.sh` |
| App colour `#1f9d55` (ink `#147a40`), **proposed** | `assets/app-icon/README.md`, metainfo `<branding>` |

## Prerequisites still owned by the app crates

The pipeline references crates that don't exist yet; the first release run fails until they do:

- **`apps/cella`** (the GUI binary `cella`) and **`apps/cella-web`** (trunk
  site) must be workspace members. `apps/cella-cli` exists and supports `--version`.
- **Windows resources:** move `packaging/windows/app-build.rs.in` to `apps/cella/build.rs`
  and add `winresource = "0.1"` under `[build-dependencies]` (instructions at the top of the
  file). `package.ps1` sets `CELLA_REQUIRE_WINRES=1`, so a release build without the icon
  fails, and it checks that `cella.exe` is a GUI-subsystem binary, so `main.rs` needs
  `#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]`.
- **Build info:** CI exports `CELLA_BUILD_SHA` and `CELLA_BUILD_DATE`; read them
  with `option_env!` (fallback: "dev build") for `--version` and *Help › About Cella*.
- **Runtime icon:** embed `assets/app-icon/cella-macos-512.png` (macOS Dock) and
  `hicolor/256x256/apps/io.github.matt-qwq.cella.png` (Windows/Linux) as the window icon, and
  use `io.github.matt-qwq.cella` as the Wayland app id / `StartupWMClass`.
- **`cargo xtask` alias:** the docs use `cargo xtask …`, which needs
  `.cargo/config.toml` with `[alias] xtask = "run --package xtask --"`. Until it exists, use
  `cargo run -p xtask -- …`. The workflows already call `cargo run -p xtask` directly.

## Cutting a release

There is no CI pipeline in this tree — the upstream one called ArtCraft's shared actions and
signing secrets and was removed with the rest of the upstream identity surface. A release is a
manual, local run of the packaging scripts:

1. **Bump the version** (`cargo xtask version set 0.2.0` updates `Cargo.toml` and `Cargo.lock`).
2. **Build the binaries**: `cargo build --release -p cella -p cella-cli` (add `--target` for a
   platform you are not on).
3. **Package** with the platform script (`packaging/*/package.sh`, `package.ps1`); they read
   `packaging/env.sh` and write into `dist/release/`.
4. **Sign** outside the repository: the Apple Developer ID and Windows Authenticode material
   belong to whoever distributes the build.
5. **Publish** wherever you publish; nothing here uploads.

The identity values in the table above are Cella's — change them all together if the product
moves again.

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
  (or `gh workflow run packaging-lint.yml --repo storytold/gridcraft --ref BRANCH`). No release
  environment, signing credentials or Rust build is required. Interactive checkbox behavior
  still needs a Windows desktop check; previews alone do not establish that behavior.
- **Signing:** `packaging/windows/sign.ps1` uses `signtool` with SHA-256 and an RFC 3161
  timestamp, from a `.pfx` (`WINDOWS_CERTIFICATE*`) if present, otherwise **Azure Trusted
  Signing** (`AZURE_*`, the storytold setup). With neither it warns and leaves files unsigned.

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
(`gh-releases-zsync|storytold|cella|latest|cella-*-linux-<arch>.AppImage.zsync`), and the
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
