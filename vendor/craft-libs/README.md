# Vendored code: craft-libs

Two of the app's platform pieces were written upstream as shared libraries for the Craft family.
They are **vendored here** (copied, unmodified except for the manifest fields listed below) so this
repository builds without reaching github.com/storytold/craft-libs at build time.

| directory | package | what it does | used by |
|---|---|---|---|
| `print/` | `craft-print` | the system print spooler: CUPS (`lpstat`/`lp`) on Linux and macOS, `Win32_Printer` + `Windows.Data.Pdf` through PowerShell on Windows | `apps/cella/src/print.rs` |
| `ui/` | `craft-ui` | reusable egui widgets; this repository uses only `craft_ui::printing` (printer options and job feedback types) | `crates/ui-egui/src/printing.rs` |

**Source:** `https://github.com/storytold/craft-libs`, revision
`d27646a5f7c8b4fc85d9dd3213b8c88b58e0088d` (the last commit the pinned git dependency resolved to),
directories `crates/print` and `crates/ui`.

**Licence:** MIT OR Apache-2.0 — Copyright (c) 2026 ArtCraft Team and the craft-libs contributors.
The licence texts and the upstream NOTICE are in this directory (`LICENSE-MIT`, `LICENSE-APACHE`,
`NOTICE`), and this repository's `NOTICE` names the two vendored crates.

**What was changed on import (and nothing else):**

- `Cargo.toml`: the fields that were `*.workspace = true` in craft-libs became literals
  (`version = "0.1.0"`, `edition = "2024"`, `license`, `repository`, `rust-version = "1.90"`), and
  `egui = { workspace = true }` became `egui = "0.36"`. The source files are byte-identical.
- `ui/src/printing/tests.rs`, two call sites (2026-11-18): epaint 0.36 asserts in `Drop` that a
  frame's `TexturesDelta` was applied, and a headless test has no renderer to apply it to, so the
  two tests that render text clear the deltas before dropping the output. Without this the crate's
  tests panic under `cargo test --workspace`; nothing outside tests changed.

**Updating:** copy the two directories from a newer `craft-libs` revision, re-apply those manifest
edits, and run `cargo check --workspace` plus `cargo xtask layers`. If a newer revision adds crates
or dependencies, add them to the workspace `members` and to `xtask/src/layers.rs`.

**Why vendored rather than rewritten:** the spooler is platform glue (CUPS, PowerShell), and the
widget types are simple data. Vendoring keeps `cargo build` offline-capable and the build
reproducible; replacing them with our own implementation is a separate decision.
