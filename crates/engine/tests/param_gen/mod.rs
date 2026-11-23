//! Params for a command, generated from its `params` doc string.
//!
//! One generator, two users, so the two never drift: the command-sequence fuzzer
//! (`tests/command_sequence_fuzz.rs`) wants params a command accepts so the run reaches the
//! code inside it, and the per-command timing catalogue (`examples/bench_commands.rs`) wants the
//! same command pointed at the region a user would give it. Everything except the size of that
//! region is identical, including which keys are refused (`path`, `password`, `html`: a command
//! that reads or writes a file has no business in either).
//!
//! A params doc is the `params` string every [`cella_engine::cmd::CommandSpec`] carries, for
//! example `{range?, shift: right|down|row|column, count?}`. [`doc_keys`] reads its top-level
//! keys, and [`doc_params`] picks a value for each one: by the key's name first (`range` is a
//! range, `cell` a cell, `sheet` a sheet), then by the shape of the hint (`asc|desc` is one of
//! those, `bool` a boolean, `1..5` an integer in that range).
//!
//! Each test target that includes this module uses the part it needs (the fuzzer never asks for
//! [`Scale::Whole`]), so the rest is unused there.

#![allow(dead_code)]

use cella_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, col_to_letters};
use cella_engine::model::{Table, Workbook};
use serde_json::{Value as Json, json};

// ---------------------------------------------------------------- deterministic randomness

/// SplitMix64: tiny, deterministic and good enough to pick params.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    /// Inclusive range.
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % ((hi - lo + 1).max(1) as u64)) as i64
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

// ---------------------------------------------------------------- coordinates and ranges

/// How big the region-shaped params are.
///
/// The fuzzer uses [`Scale::Default`]: every address is small and near the top-left, so a run
/// of commands stays fast and a formula's references stay cheap to recalculate. The catalogue
/// uses [`Scale::Whole`]: a region-naming key gets the used range of the sheet, which is what
/// "select the column and format it" means for a person, and the only way a command that reads
/// its `range` parameter can show that its cost scales with the data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scale {
    #[default]
    Small,
    /// `A1:` the bottom-right corner of `rows` × `cols`.
    Whole { rows: u32, cols: u32 },
}

impl Scale {
    /// A region-shaped parameter under this scale.
    fn range(&self, rng: &mut Rng) -> String {
        // `range` draws from the generator, so it is drawn either way: a command's *other* params
        // then stay the same at both scales for one seed, and a catalogue row differs from the
        // fuzzer's params only in where the region points.
        let small = range(rng);
        match *self {
            Scale::Small => small,
            Scale::Whole { rows, cols } => whole_range(rows, cols),
        }
    }
}

pub fn letter(c: u32) -> String {
    col_to_letters(c)
}

/// A row/column index: mostly near the data, sometimes further out, rarely at the sheet's edge.
fn coord(rng: &mut Rng, near: i64, far: i64, max: u32) -> u32 {
    match rng.below(20) {
        0 => max - 1 - rng.int(0, 2) as u32,
        1 | 2 => rng.int(near, far) as u32,
        _ => rng.int(0, near) as u32,
    }
}

pub fn cell(rng: &mut Rng) -> CellRef {
    CellRef::new(coord(rng, 24, 120, MAX_ROWS), coord(rng, 11, 40, MAX_COLS))
}

/// A cell near the data (formula references stay small).
pub fn near_cell(rng: &mut Rng) -> CellRef {
    CellRef::new(rng.int(0, 22) as u32, rng.int(0, 10) as u32)
}

pub fn near_range(rng: &mut Rng) -> RangeRef {
    let a = near_cell(rng);
    let b = CellRef::new(a.row + rng.int(0, 7) as u32, a.col + rng.int(0, 4) as u32);
    RangeRef::new(a, b)
}

pub fn range(rng: &mut Rng) -> String {
    match rng.below(20) {
        0 | 1 => {
            let c = rng.int(0, 10) as u32;
            format!("{}:{}", letter(c), letter(c + rng.int(0, 2) as u32))
        }
        2 | 3 => {
            let r = rng.int(1, 25);
            format!("{r}:{}", r + rng.int(0, 2))
        }
        4 => cell(rng).a1(),
        5 => {
            let a = cell(rng);
            RangeRef::new(a, a.offset_clamped(rng.int(0, 3), rng.int(0, 3))).a1()
        }
        _ => near_range(rng).a1(),
    }
}

/// `A1:` its bottom-right corner, for `rows` × `cols` from the top-left.
pub fn whole_range(rows: u32, cols: u32) -> String {
    let rows = rows.clamp(1, MAX_ROWS);
    let cols = cols.clamp(1, MAX_COLS);
    format!("A1:{}{rows}", letter(cols - 1))
}

pub fn rows_spec(rng: &mut Rng) -> String {
    let r = coord(rng, 24, 120, MAX_ROWS) as i64 + 1;
    format!("{r}:{}", (r + rng.int(0, 2)).min(MAX_ROWS as i64))
}

pub fn cols_spec(rng: &mut Rng) -> String {
    let c = coord(rng, 11, 40, MAX_COLS);
    format!("{}:{}", letter(c), letter((c + rng.int(0, 1) as u32).min(MAX_COLS - 1)))
}

pub fn quoted(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

pub fn spill_anchors(wb: &Workbook) -> Vec<(usize, CellRef)> {
    wb.sheets.iter().enumerate().flat_map(|(i, sh)| sh.spill_ranges.keys().map(move |c| (i, *c))).collect()
}

pub fn tables(wb: &Workbook) -> Vec<(usize, &Table)> {
    wb.sheets.iter().enumerate().flat_map(|(i, sh)| sh.tables.iter().map(move |t| (i, t))).collect()
}

pub fn other_sheet(rng: &mut Rng, wb: &Workbook) -> String {
    quoted(&wb.sheets[rng.below(wb.sheets.len())].name)
}

pub fn formula(rng: &mut Rng, wb: &Workbook) -> String {
    let c = near_cell(rng).a1();
    let c2 = near_cell(rng).a1();
    let r = near_range(rng).a1();
    let n = rng.int(1, 6);
    let col = letter(rng.int(0, 9) as u32);
    let tabs = tables(wb);
    match rng.below(30) {
        0 => format!("={c}+{n}"),
        1 => format!("=SUM({r})"),
        2 => format!("={c}*{c2}"),
        3 => format!("={}!{c}*2", other_sheet(rng, wb)),
        4 => format!("=SEQUENCE({n},{})", rng.int(1, 3)),
        5 | 6 if !tabs.is_empty() => {
            let (_, t) = rng.pick(&tabs);
            let column = t.columns.get(rng.below(t.columns.len())).map(|c| c.name.clone()).unwrap_or_default();
            match rng.below(4) {
                0 => format!("=SUM({}[{column}])", t.name),
                1 => format!("=ROWS({})", t.name),
                2 => format!("={}[@{column}]", t.name),
                _ => format!("=COUNTA({}[#All])", t.name),
            }
        }
        7 => format!("=INDIRECT(\"{c}\")"),
        8 => format!("=OFFSET({c},1,1)"),
        9 => format!("=IF({c}>{n},\"big\",{c})"),
        10 => {
            let anchors = spill_anchors(wb);
            match anchors.get(rng.below(anchors.len().max(1))) {
                Some((si, a)) => format!("=SUM({}!{a}#)", quoted(&wb.sheets[*si].name)),
                None => format!("={c}#"),
            }
        }
        11 => format!("=SORT({r})"),
        12 => format!("=UNIQUE({r})"),
        13 => format!("=TRANSPOSE({r})"),
        14 => format!("=COUNTIF({col}:{col},\">0\")"),
        15 => format!("=SUM({}:{}!{c})", other_sheet(rng, wb), other_sheet(rng, wb)),
        16 => format!("=LET(x,{c},x*2)"),
        17 => format!("=XLOOKUP({c},{col}1:{col}20,{col}1:{col}20)"),
        18 => format!("=FILTER({col}1:{col}20,{col}1:{col}20>{n})"),
        19 => match wb.names.get(rng.below(wb.names.len().max(1))) {
            Some(nm) => format!("=SUM({})", nm.name),
            None => format!("={c}"),
        },
        20 => "=ROW()+COLUMN()".into(),
        21 => format!("={c}&\"-\"&{c2}"),
        22 => format!("=AVERAGE({r})*{n}"),
        23 => format!("=MAX({r})-MIN({r})"),
        24 => rng.pick(&["=1/0", "=NA()", "=#REF!+1", "=NOSUCHFN(1)", "=SUM(", "=\"text\""]).to_string(),
        25 => format!("={r}"),
        26 => format!("=INDEX({r},{n},1)"),
        27 => format!("=VLOOKUP({c},{r},2,FALSE)"),
        28 => format!("=SEQUENCE({n})*{c}"),
        _ => format!("=TEXT({c},\"0.00\")"),
    }
}

pub fn literal(rng: &mut Rng) -> String {
    match rng.below(14) {
        0..=3 => rng.int(-50, 500).to_string(),
        4 => format!("{}.{}", rng.int(-9, 99), rng.int(0, 99)),
        5 => rng.pick(&["North", "South", "East", "West", "abc", "Total", "x y"]).to_string(),
        6 => rng.pick(&["TRUE", "FALSE"]).to_string(),
        7 => format!("{}%", rng.int(0, 100)),
        8 => format!("2026-{:02}-{:02}", rng.int(1, 12), rng.int(1, 28)),
        9 => String::new(),
        10 => "'007".into(),
        11 => "$1,234.50".into(),
        12 => "#N/A".into(),
        _ => "1e3".into(),
    }
}

pub fn input(rng: &mut Rng, wb: &Workbook) -> String {
    if rng.chance(55) { formula(rng, wb) } else { literal(rng) }
}

pub fn json_input(rng: &mut Rng, wb: &Workbook) -> Json {
    match rng.below(10) {
        0..=1 => json!(rng.int(-20, 200)),
        2 => Json::Null,
        3 => json!(rng.chance(50)),
        _ => json!(input(rng, wb)),
    }
}

pub fn values_2d(rng: &mut Rng, wb: &Workbook) -> Json {
    let (h, w) = (rng.int(1, 4), rng.int(1, 3));
    Json::Array((0..h).map(|_| Json::Array((0..w).map(|_| json_input(rng, wb)).collect())).collect())
}

pub fn sheet_param(rng: &mut Rng, wb: &Workbook) -> Json {
    match rng.below(10) {
        0 => json!(wb.sheets.len() + rng.below(2)),
        1..=4 => json!(wb.sheets[rng.below(wb.sheets.len())].name),
        _ => json!(rng.below(wb.sheets.len())),
    }
}

pub fn new_name(rng: &mut Rng) -> String {
    rng.pick(&["Data", "Q4 2026", "Summary", "Sheet9", "x", "Sales", "Long name of a sheet 31 chars!", "it's", "Grades"]).to_string()
}

pub fn color(rng: &mut Rng) -> String {
    rng.pick(&["#FF0000", "#00B050", "#FFFF00", "#1F4E79", "none"]).to_string()
}

pub fn table_param(rng: &mut Rng, wb: &Workbook) -> Json {
    let tabs = tables(wb);
    match tabs.get(rng.below(tabs.len() + 1)) {
        Some((_, t)) => json!(t.name),
        None => json!("NoSuchTable"),
    }
}

pub fn chart_ids(wb: &Workbook) -> Vec<u32> {
    wb.sheets.iter().flat_map(|s| s.charts.iter().map(|c| c.id)).collect()
}

// ---------------------------------------------------------------- params docs

/// Top-level keys of a params doc (`{range?, shift: right|down, count?}`): (key, optional, hint).
pub fn doc_keys(doc: &str) -> Vec<(String, bool, String)> {
    let Some(start) = doc.find('{') else { return vec![] };
    let mut parts = Vec::new();
    let (mut depth, mut in_str, mut cur) = (0i32, false, String::new());
    for ch in doc[start + 1..].chars() {
        match ch {
            '"' => {
                in_str = !in_str;
                cur.push(ch);
            }
            _ if in_str => cur.push(ch),
            '{' | '[' | '(' => {
                depth += 1;
                cur.push(ch);
            }
            '}' | ']' | ')' if depth == 0 => break,
            '}' | ']' | ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    let mut out = Vec::new();
    for part in parts {
        let part = part.trim();
        let key: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if key.is_empty() {
            continue;
        }
        let rest = &part[key.len()..];
        let optional = rest.starts_with('?');
        let hint = rest.trim_start_matches('?').trim_start();
        let hint = hint.strip_prefix(':').unwrap_or(hint).trim().to_string();
        out.push((key, optional, hint));
    }
    out
}

/// Bare-word alternatives in a hint (`asc|desc`, `"Good"|"Bad"`), if it is an enumeration.
pub fn alternatives(hint: &str) -> Option<Vec<String>> {
    if !hint.contains('|') {
        return None;
    }
    let alts: Vec<String> = hint
        .split('|')
        .map(|a| a.split(" (").next().unwrap_or(a).trim().trim_matches('"').trim_end_matches('…').to_string())
        .filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || " .".contains(c)))
        .collect();
    (!alts.is_empty()).then_some(alts)
}

/// Params from a command's params doc: values chosen by key name, then by the hint's shape.
pub fn doc_params(rng: &mut Rng, wb: &Workbook, doc: &str, scale: Scale) -> Json {
    let mut p = serde_json::Map::new();
    for (key, optional, hint) in doc_keys(doc) {
        if optional && rng.chance(40) {
            continue;
        }
        if let Some(v) = doc_value(rng, wb, &key, &hint, scale) {
            p.insert(key, v);
        }
    }
    Json::Object(p)
}

fn doc_value(rng: &mut Rng, wb: &Workbook, key: &str, hint: &str, scale: Scale) -> Option<Json> {
    let lower = key.to_ascii_lowercase();
    let v = match lower.as_str() {
        "path" | "base64" | "password" | "html" => return None,
        "range" | "source" | "target" | "destination" | "criteria" | "copyto" | "printarea" | "location" | "resultcells" | "changing"
        | "rowinput" | "columninput" | "add" | "remove" | "apply" => json!(scale.range(rng)),
        "refersto" => json!(format!("={}!$A$1:$B$4", other_sheet(rng, wb))),
        "cell" | "at" | "active" | "fromcell" | "set" | "reference" => json!(cell(rng).a1()),
        "rows" | "titlerows" if !hint.starts_with('[') => json!(rows_spec(rng)),
        "cols" | "titlecols" if !hint.starts_with('[') => json!(cols_spec(rng)),
        "sheet" => sheet_param(rng, wb),
        "input" | "formula" | "formula1" | "formula2" | "value" | "value2" | "with" => json!(input(rng, wb)),
        "table" => table_param(rng, wb),
        "chart" | "id" => {
            let ids = chart_ids(wb);
            json!(ids.get(rng.below(ids.len() + 1)).copied().unwrap_or(rng.int(0, 5) as u32))
        }
        "ids" => json!(chart_ids(wb)),
        "color" => json!(color(rng)),
        "values" if hint.starts_with("[[") => values_2d(rng, wb),
        "keys" => json!([{"column": letter(rng.int(0, 9) as u32), "order": rng.pick(&["asc", "desc"])}]),
        "column" | "groupby" => json!(letter(rng.int(0, 9) as u32)),
        "columns" | "sources" if !hint.contains('|') => json!([letter(rng.int(0, 9) as u32)]),
        "points" => json!([[10, 10], [40, 25], [70, 5]]),
        "commands" => json!([{"command": "cell.set", "params": {"cell": near_cell(rng).a1(), "input": input(rng, wb)}}]),
        "char" => json!("€"),
        "name" | "oldname" if alternatives(hint).is_none() => json!(new_name(rng)),
        "text" | "title" | "comment" | "author" | "word" | "header" | "footer" | "what" if alternatives(hint).is_none() => {
            json!(rng.pick(&["North", "note", "&CPage &P", "x"]))
        }
        _ => return Some(hint_value(rng, wb, hint)),
    };
    Some(v)
}

fn hint_value(rng: &mut Rng, wb: &Workbook, hint: &str) -> Json {
    if let Some(alts) = alternatives(hint) {
        return json!(rng.pick(&alts));
    }
    if hint.contains("bool") || hint == "true" || hint == "false" {
        return json!(rng.chance(50));
    }
    if let Some((a, b)) = hint.split_once("..")
        && let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().split(' ').next().unwrap_or("").parse::<i64>())
    {
        return json!(rng.int(a, b));
    }
    if let Ok(n) = hint.split(' ').next().unwrap_or("").parse::<f64>() {
        return json!((n * rng.int(0, 3) as f64).round());
    }
    if hint.starts_with('[') {
        let heads: Vec<String> = tables(wb).iter().flat_map(|(_, t)| t.columns.iter().map(|c| c.name.clone())).collect();
        return match heads.get(rng.below(heads.len() + 1)) {
            Some(h) => json!([h]),
            None => json!([]),
        };
    }
    if let Some(ex) = hint.strip_prefix('"').and_then(|h| h.split('"').next())
        && rng.chance(60)
    {
        return json!(ex);
    }
    match rng.below(4) {
        0 => json!(rng.chance(50)),
        1 => json!(rng.int(0, 5)),
        2 => json!(near_range(rng).a1()),
        _ => json!(literal(rng)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_parse_into_keys() {
        let keys = doc_keys("{range?, shift: right|down|row|column, input: \"text, number or =formula\"}");
        let names: Vec<_> = keys.iter().map(|(k, o, _)| (k.as_str(), *o)).collect();
        assert_eq!(names, [("range", true), ("shift", false), ("input", false)]);
        assert_eq!(alternatives(&keys[1].2), Some(vec!["right".into(), "down".into(), "row".into(), "column".into()]));
    }

    /// The catalogue's one difference: a region-naming key covers the whole used area.
    #[test]
    fn whole_scale_points_a_range_at_the_used_area() {
        let wb = Workbook::default();
        let doc = "{range: A1:F, header?}";
        let small = doc_params(&mut Rng::new(1), &wb, doc, Scale::Small);
        let whole = doc_params(&mut Rng::new(1), &wb, doc, Scale::Whole { rows: 1_000_000, cols: 6 });
        assert_eq!(whole["range"], json!("A1:F1000000"));
        // The same seed gives the same non-region keys, so a row of the catalogue differs from the
        // fuzzer's params only in where it points.
        assert_eq!(small.get("header").is_some(), whole.get("header").is_some());
        assert_ne!(small["range"], whole["range"]);
        assert_eq!(whole_range(100_000, 6), "A1:F100000");
        // Hostile sizes are clamped to the sheet, never 0 or past its edge.
        assert_eq!(whole_range(0, 0), "A1:A1");
    }
}
