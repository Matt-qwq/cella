//! The golden number-format corpus: 152 format codes and 555 values (707 recorded cases).
//!
//! Output over a large grid (format codes x values: `format_value` and `text_function` in both date
//! systems, then General at widths 1..=30 and a few larger ones) is compared with golden digests in
//! `tests/golden/*.txt`, one FNV-1a digest per format code / per value, so the files stay small. A
//! failure names the row that differs; set `NUMFMT_DUMP_DIR` to write the full text of the current
//! output for diffing. To re-record after an intended behaviour change run with `NUMFMT_BLESS=1`.
//!
//! Adopted from upstream (the grid below and the golden files are upstream's, byte for byte): every
//! one of the 707 rows matched this fork's renderer when the corpus landed, so nothing is excluded
//! and no row is listed as a deliberate difference. `DELIBERATE_DIFFERENCES` says how to record a
//! divergence we keep on purpose.

#![allow(clippy::approx_constant, clippy::excessive_precision)] // values are test data, not constants

use std::fmt::Write as _;

use cella_core::{DateSystem, Value};
use cella_numfmt::{NumberFormat, format_general_fit, format_value, text_function};

/// Rows this fork renders differently from the recorded golden **on purpose**, with the reason.
///
/// The corpus records what Excel shows; a row listed here is one where this fork deliberately does
/// not follow it (a feature we do not model, a documented policy). Listing a row keeps the gate
/// meaningful: every other row still has to match, and an entry that stops differing is reported as
/// stale rather than silently kept. The key is the row's label as the golden file prints it: the
/// format code in `codes.txt` (Rust debug form, `"0.00"`) or the value in `general.txt` (`1e-5`).
/// Nothing is listed today.
const DELIBERATE_DIFFERENCES: &[(&str, &str)] = &[];

fn difference_reason(key: &str) -> Option<&'static str> {
    DELIBERATE_DIFFERENCES.iter().find(|(k, _)| *k == key).map(|(_, why)| *why)
}

// Placeholders far beyond the 15 significant digits.
const LONG_DECIMALS: &str = "0.0000000000000000000000000";
#[rustfmt::skip]
const CODES: &[&str] = &[
    LONG_DECIMALS, "#,###,###,###,###,###,###,###,##0.00", "0.0000000000000000000000000E+00", "# ?????/?????",
    // numbers
    "General", "0", "0.00", "#,##0", "#,##0.00", "#.##", "0.0##", "00000", "000.000", "#,###", "#,##0,", "#,##0,,", "0.0,,\"M\"", "?.??", "##0.0", "# ", "#", ".00",
    ",0", "0,0", "0.00,", "\"$\"#,##0.00", "\"$\"#,##0_);\\(\"$\"#,##0\\)", "#,##0.00;[Red](#,##0.00)", "#,##0 ;(#,##0)", "0;-0;;@", "0.00;;", ";;;",
    "0_);(0)", "0\" units\"", "\\A0", "(0)", "00-00", "000-00-0000", "[<=9999999]###-####;(###) ###-####", "[$€-407]#,##0.00",
    // currency/accounting with fill
    "_(* #,##0_);_(* \\(#,##0\\);_(* \"-\"_);_(@_)", "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)", "*-0", "0*x", "*0 0.0", "@*_",
    // percent
    "0%", "0.00%", "#,##0%", "0.0%%", "%0", "0%;[Red]-0%",
    // scientific
    "0.00E+00", "##0.0E+0", "0E+0", "0.0e-0", "0.00E+000", "E+0", "0.00E+", "#.##E+00", "0.0E+00 \"x\"",
    // fractions
    "# ?/?", "# ??/??", "# ???/???", "?/?", "# ?/8", "# ??/16", "0/0", "# #/#", "#/#", "?/100", "# ?/?;-# ?/?", "0 0/0",
    // dates/times
    "m/d/yyyy", "d-mmm-yy", "d-mmm", "mmm-yy", "h:mm AM/PM", "h:mm:ss AM/PM", "h:mm", "h:mm:ss", "m/d/yyyy h:mm", "mm:ss", "[h]:mm:ss", "[h]:mm", "[m]:ss", "[s]", "mmss.0",
    "yyyy-mm-dd", "dddd, mmmm d, yyyy", "ddd mmm dd yy", "mmmmm", "yy", "h:mm:ss.000", "h:mm:ss.0000", "hh:mm:ss A/P", "[hh]:mm:ss", "d", "dd", "dddd", "m", "mm", "s.00",
    "yyyy-mm-dd hh:mm:ss.00", "h AM/PM", "[$-409]d-mmm-yy", "yyyy\"年\"m\"月\"", "[h]\" hrs\"", "mmmm yyyy;@",
    // text sections, conditions, colours
    "@", "\"x\"@", "0;\"neg\"0;\"zero\";\"txt: \"@", "General;General;General;@", "[Red]0.0;[Blue]-0.0", "[Green]0;[Magenta]0;[Cyan]0;[Yellow]@", "[Color3]0.00",
    "[>100]\"big\"0;[<-100]\"small\"0;0.0", "[=0]\"zero\";0.0", "[>=1000]#,##0,\"K\";0", "[<1]0.00%;0.0", "[Red][>5]0;0.00", "General\" kg\"", "\"v\"General",
    "[Red]General", "0.0;[Red]General",
    // invalid / odd
    "[", "0.0.0", "0\"", "\\", "[h", "[$", "yyyy-mm-dd;", ";", "0;0;0;0;0", "\"abc", "[>x]0", "ggg", "#?#0", "0E", "E0", "/", "0/", "/0", "*", "_", "_0", "[Red", "0.00.00%", "mm:ss.0.0",
    "[ABC]0", "[=1]0;[=2]0;0", "0 ??/??;0",
];

fn values() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        0.25,
        1.005,
        2.675,
        999.9995,
        0.9995,
        9.995,
        99.5,
        0.05,
        0.005,
        0.0005,
        1e-5,
        1e-10,
        1e-300,
        5e-324,
        123456789.0,
        1234.5678,
        12345678901.0,
        1e11,
        1e15,
        123456789012345.0,
        1234567890123455.0,
        1e20,
        1e100,
        1.7976931348623157e308,
        -1234.5,
        -0.004,
        -1e-10,
        1.0 / 3.0,
        2.0 / 3.0,
        0.1 + 0.2,
        100.0,
        1000.0,
        59.0,
        60.0,
        61.0,
        36526.0,
        36526.75,
        43831.5,
        45000.123456,
        2958465.0,
        2958465.999999,
        2958466.0,
        -5.0,
        0.99999999,
        0.000999,
        41.99999999999,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.041666666666666664,
        0.9999999999,
        1.0000000001,
        12.3456789,
        3.14159265358979,
        65535.0,
        1e5,
        123456.0,
        4.5,
        0.375,
        1e300,
        1.5e-8,
        999999999999999.0,
        99999999999999.99,
        0.00001234,
        123456789012345678.0,
        1e22,
        9.999999999999999e22,
        0.1234567890123456,
    ];
    v.push(61.2 / 86400.0);
    v
}

fn fmt_formatted(f: &cella_numfmt::Formatted) -> String {
    format!("{:?}|{:?}|{:?}|{}", f.text, f.color, f.fill, f.numeric)
}

fn dump_codes() -> Vec<(String, String)> {
    assert!(CODES.len() >= 80 && values().len() >= 40);
    let mut rows = Vec::new();
    let others = [Value::Text("abc".into()), Value::Text("12.5".into()), Value::Bool(true), Value::Empty];
    for code in CODES {
        let mut out = String::new();
        let nf = NumberFormat::parse(code);
        let systems: &[(DateSystem, &str)] =
            if nf.is_date() { &[(DateSystem::D1900, "1900"), (DateSystem::D1904, "1904")] } else { &[(DateSystem::D1900, "1900")] };
        for (sys, name) in systems {
            for n in values() {
                let v = Value::Number(n);
                let _ =
                    writeln!(out, "{code:?} {name} {n:e} => {} || {:?}", fmt_formatted(&format_value(&v, &nf, *sys)), text_function(&v, code, *sys));
            }
        }
        for v in &others {
            let _ = writeln!(
                out,
                "{code:?} {v:?} => {} || {:?}",
                fmt_formatted(&format_value(v, &nf, DateSystem::D1900)),
                text_function(v, code, DateSystem::D1900)
            );
        }
        rows.push((format!("{code:?}"), out));
    }
    rows
}

fn dump_general() -> Vec<(String, String)> {
    let mut vals = values();
    let mut x = 0.7f64;
    // Deterministic spread over many magnitudes and digit patterns (no RNG dependency).
    for i in 0..240 {
        x = (x * 997.0 + 0.123_456_789).fract() + 1e-3;
        let e = (i % 41) - 20;
        let m = x * 10f64.powi(e);
        vals.push(if i % 3 == 0 { -m } else { m });
        vals.push((m * 1000.0).round() / 1000.0);
    }
    assert!(vals.len() >= 300);
    let mut rows = Vec::new();
    for n in vals {
        let mut out = String::new();
        let _ = write!(out, "{n:e} =>");
        for w in (1..=30usize).chain([31, 40, 100, 399, 400, 401, 500, 1000]) {
            let _ = write!(out, " {:?}", format_general_fit(n, w));
        }
        out.push('\n');
        rows.push((format!("{n:e}"), out));
    }
    rows
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// How many differing rows to print in full before summarising the rest.
const REPORTED: usize = 8;

/// The golden line a row is recorded as: its digest, its line count and its label.
fn golden_row(key: &str, body: &str) -> String {
    format!("{:016x} {} {key}", fnv1a(body), body.lines().count())
}

fn check(name: &str, rows: &[(String, String)], golden: &str, path: &str) {
    if let Some(dir) = std::env::var_os("NUMFMT_DUMP_DIR") {
        let all: String = rows.iter().map(|(_, b)| b.as_str()).collect();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.txt")), all).expect("write dump");
    }
    let recorded: Vec<&str> = golden.lines().collect();
    if std::env::var_os("NUMFMT_BLESS").is_some() {
        let actual: String = rows.iter().map(|(k, b)| golden_row(k, b) + "\n").collect();
        std::fs::write(path, &actual).expect("write golden");
        return;
    }
    // A listed row must be a real row of this corpus, so a typo cannot exempt nothing.
    for (key, _) in DELIBERATE_DIFFERENCES {
        assert!(rows.iter().any(|(k, _)| k == key), "{name}: deliberate difference {key} is not a row of the corpus");
    }
    assert_eq!(rows.len(), recorded.len(), "{name}: the corpus has {} rows, the golden file {}", rows.len(), recorded.len());
    let mut differing = Vec::new();
    for (i, (key, body)) in rows.iter().enumerate() {
        let actual = golden_row(key, body);
        let recorded_row = recorded.get(i).copied();
        if difference_reason(key).is_some() {
            // A row that starts matching again belongs back in the gate, not in the list.
            assert!(recorded_row != Some(actual.as_str()), "{name}: {key} matches the golden again; remove it from DELIBERATE_DIFFERENCES");
            continue;
        }
        if recorded_row != Some(actual.as_str()) {
            differing.push((key, body, recorded_row.unwrap_or("<the golden file is short>")));
        }
    }
    let shown: Vec<String> = differing
        .iter()
        .take(REPORTED)
        .map(|(key, body, old)| {
            format!(
                "  row {key}\n  new: {}\n  old: {old}\n  current output of that row (first 12 lines):\n{}",
                golden_row(key, body),
                body.lines().take(12).collect::<Vec<_>>().join("\n")
            )
        })
        .collect();
    assert!(
        differing.is_empty(),
        "{name}: {} of {} rows differ from the golden corpus ({} more not shown):\n{}",
        differing.len(),
        rows.len(),
        differing.len().saturating_sub(REPORTED),
        shown.join("\n")
    );
}

#[test]
fn format_value_and_text_function_match_golden() {
    check("codes", &dump_codes(), include_str!("golden/codes.txt"), concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/codes.txt"));
}

#[test]
fn general_fit_matches_golden() {
    check("general", &dump_general(), include_str!("golden/general.txt"), concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/general.txt"));
}
