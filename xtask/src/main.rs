//! Workspace tooling: `cargo xtask <command>` (or `cargo run -p xtask -- <command>`).
//!
//! Pure Rust (std + serde_json, plus the engine for `parity`). External tools (`cargo`, `git`)
//! are invoked through `std::process::Command`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod assets;
#[cfg(feature = "engine")]
mod bench;
#[cfg(feature = "engine")]
mod corpus;
mod eol;
mod i18n;
mod ico;
mod keytips;
mod layers;
#[cfg(feature = "engine")]
mod oracle;
#[cfg_attr(not(feature = "engine"), allow(dead_code))]
mod parity;
mod perf;
mod perfreport;
mod stats;
mod version;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci              fmt --check, clippy -D warnings, test, assets, layers, wasm, parity --check,
                  keytips --check, i18n --check, bench-coverage --check (stops at the first
                  failure; the parity check runs only if docs/parity-checklist.md exists)
  assets          check that every icon/image/font/sample file is attributed in ATTRIBUTION.md
  layers          enforce the crate dependency layering (AGENTS.md \"Layering\")
  wasm            cargo check --target wasm32-unknown-unknown for the L0-L5 crates (+ cella-web)
  parity [--check] [--out PATH]
                  write docs/parity-checklist.md from cella_engine::catalog (implemented vs missing ids)
  oracle [--fixture=NAME] [--no-app] [--keep]
                  read the sample workbooks back through foreign implementations (openpyxl; an
                  independent application through COM on Windows) — see docs/oracle.md
  corpus <dir> [--recursive] [--all] [--no-openpyxl]
                  open every .xlsx in a directory, report warnings, save/reopen a sample and
                  read it back with openpyxl — never writes into <dir> (see docs/oracle.md)
  keytips [--check]
                  regenerate crates/ui-egui/src/keytips/generated.rs from keytips.json (--check: fail if stale)
  i18n [--check] [--write]
                  list the user-visible literals in crates/ui-egui/src that skip the translation
                  lookup, with a ratchet baseline in xtask/i18n-baseline.txt (--write: refresh it,
                  --check: fail on a new literal)
  perf [--update] [--rows=N,N] [--scenario=ID,...] [--repeat=N] [--budgets=PATH] [--scale=F]
  bench-commands [--rows=N,N] [--repeat=N] [--only=ID,...] [--out PATH]
                  time every registered command once, at 100k and 1M rows, with its params near the
                  cursor and over the whole used area; write docs/performance-commands.md
                  (release, minutes — NOT part of `ci`; see docs/performance.md)
  bench-coverage [--check] [--write]
                  the ledger xtask/bench-coverage.txt: where each command's cost is observed
                  (--check fails on an unclassified command; a step of `ci`)
  bench-ui [--rows=N] [--frames=N] [--out PATH]
                  the frame-time ruler: one repaint of the whole window at 1M rows, offscreen,
                  no GPU; writes docs/performance-ui.md and fails over budget
                  (release, minutes the first time — NOT part of `ci`)
  perf-report     write docs/performance-report.md from the scenario gate and the live-heap example
                  the performance gate: run crates/engine/examples/perf.rs in release at 100k and
                  1M rows and compare it with xtask/perf-budgets.txt, plus the mem_drift soak and
                  the save.repeat micro-benchmark as their own release subprocesses
                  (--update records the current
                  numbers; CELLA_PERF_SCALE slackens every budget for a slower machine).
                  NOT part of `ci` — run it before a release (see docs/performance.md)
  version [set X.Y.Z[-pre]]
                  print or set the workspace version ([workspace.package] in Cargo.toml)
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("layers") => cmd_layers(),
        Some("wasm") => cmd_wasm(),
        Some("ci") => cmd_ci(),
        Some("stats") => stats::run(&root(), rest.contains(&"--exact")),
        Some("parity") => parity::run(&root(), &rest),
        #[cfg(feature = "engine")]
        Some("oracle") => oracle::run(&root(), &rest),
        #[cfg(feature = "engine")]
        Some("corpus") => corpus::run(&rest),
        #[cfg(not(feature = "engine"))]
        Some("corpus") => Err("`corpus` needs the engine feature (on by default)".to_string()),
        #[cfg(not(feature = "engine"))]
        Some("oracle") => Err("`oracle` needs the engine feature (on by default)".to_string()),
        Some("keytips") => keytips::run(&root(), &rest),
        Some("i18n") => i18n::run(&root(), &rest),
        Some("perf") => perf::run(&root(), &rest),
        #[cfg(feature = "engine")]
        Some("bench-commands") => bench::run_commands(&root(), &rest),
        #[cfg(not(feature = "engine"))]
        Some("bench-commands") => Err("`bench-commands` needs the engine feature (on by default)".to_string()),
        #[cfg(feature = "engine")]
        Some("bench-coverage") => bench::run_coverage(&root(), &rest),
        #[cfg(not(feature = "engine"))]
        Some("bench-coverage") => Err("`bench-coverage` needs the engine feature (on by default)".to_string()),
        #[cfg(feature = "engine")]
        Some("bench-ui") => bench::run_ui(&root(), &rest),
        #[cfg(not(feature = "engine"))]
        Some("bench-ui") => Err("`bench-ui` needs the engine feature (on by default)".to_string()),
        Some("perf-report") => perfreport::run(&root()),
        Some("ico") => ico::run(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (AGENTS.md \"Layering\")\n");
    println!("{:<28} {:<10} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<10} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Workspace packages that must build for wasm32: every L0–L5 crate, plus the web app once it
/// exists (`apps/cella-web`).
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 5 || c.name == "cella-web",
            _ => false,
        })
        .map(|c| c.name)
        .collect())
}

fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    if set.is_empty() {
        return Err("no wasm crates found in the workspace".into());
    }
    let mut c = cargo();
    c.args(["check", "--target", "wasm32-unknown-unknown"]);
    for p in &set {
        c.args(["-p", p]);
    }
    let r = run(c, &format!("cargo check --target wasm32-unknown-unknown -p {}", set.join(" -p ")));
    println!("\nwasm32-unknown-unknown check ({}): {}", set.join(", "), if r.is_ok() { "ok" } else { "FAIL" });
    if r.is_err() {
        println!("(if the target is missing: rustup target add wasm32-unknown-unknown)");
    }
    r
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        (
            "test",
            Box::new(|| {
                let mut c = cargo();
                c.args(["test", "--workspace"]);
                run(c, "cargo test --workspace")?;
                // The connectors are off by default (the browser build has no filesystem and no
                // sockets), so their tests need the feature named or they would never run — the
                // one place in the gate that asks for a feature on purpose.
                let mut c = cargo();
                c.args(["test", "-p", "cella-engine", "--features", "connectors", "--test", "connectors"]);
                run(c, "cargo test -p cella-engine --features connectors --test connectors")
            }),
        ),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
        (
            "parity",
            Box::new(|| {
                if root().join("docs/parity-checklist.md").exists() {
                    parity::run(&root(), &["--check"])
                } else {
                    println!("parity: docs/parity-checklist.md not generated yet; skipped (run `cargo xtask parity`)");
                    Ok(())
                }
            }),
        ),
        ("keytips", Box::new(|| keytips::run(&root(), &["--check"]))),
        ("i18n", Box::new(|| i18n::run(&root(), &["--check"]))),
        (
            "bench-coverage",
            Box::new(|| {
                #[cfg(feature = "engine")]
                {
                    bench::run_coverage(&root(), &["--check"])
                }
                #[cfg(not(feature = "engine"))]
                {
                    println!("bench-coverage: xtask was built without the engine feature; skipped");
                    Ok(())
                }
            }),
        ),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}
