//! The Cella JSON workbook: a versioned envelope around a [`Workbook`].
//!
//! ```json
//! {"formatVersion": 1, "workbook": {"sheets": [ … ]}}
//! ```
//!
//! * `formatVersion` names the schema of the object beside it. A file *without* `formatVersion`
//!   is version 0: the bare workbook JSON every build before this one wrote, and still read
//!   (an old file is never rejected, it is simply upgraded on its next save).
//! * `workbook` holds the model. A file written by a newer Cella (or by a build that kept a
//!   field this one dropped) may carry fields we do not model, at the envelope, in the workbook
//!   object and in a sheet object. They are kept as they were read — see
//!   [`cella_model::JsonKept`] — and written back on save, so an older build never destroys
//!   a newer file's data.
//! * The reader caps the whole file and every string in it, so a damaged or hostile file is
//!   refused with a message instead of exhausting memory. The whole-file cap can be raised with
//!   `CELLA_MAX_JSON_MB` for a workbook you trust; the writer does not enforce it (refusing to
//!   save would lose the user's work), so a file may be saved larger than this build would read
//!   back without that variable.
//!
//! This is our own format, not a copy of anyone else's: the version number counts *our* changes
//! to the model, and a change that only renames a field is a version bump (the old name survives
//! through the kept fields rather than disappearing).

use std::sync::OnceLock;

use cella_model::Workbook;
use serde_json::{Map, Value};

use crate::{EngineError, Result};

/// The version this build writes. A file without `formatVersion` is [`BARE_VERSION`].
pub const FORMAT_VERSION: u64 = 1;
/// The version of a file that has no envelope: the bare workbook JSON written before versioning.
pub const BARE_VERSION: u64 = 0;
/// The envelope key holding the schema version.
pub const VERSION_KEY: &str = "formatVersion";
/// The envelope key holding the workbook.
pub const WORKBOOK_KEY: &str = "workbook";
/// The envelope keys this build writes itself; a kept field with one of these names is dropped
/// rather than allowed to overwrite the model.
const RESERVED: &[&str] = &[VERSION_KEY, WORKBOOK_KEY];

/// The largest file read by default: 512 MiB. A JSON workbook is the model written out, so it is
/// several times the size of the same data as XLSX.
pub const DEFAULT_MAX_BYTES: u64 = 512 << 20;
/// The largest `CELLA_MAX_JSON_MB` can ask for: a typo cannot turn the guard off.
pub const MAX_MAX_BYTES: u64 = 4096 << 20;
/// The longest single string read by default: 32 MiB. It covers a VBA project (base64 in JSON) and
/// every cell text there is (Excel's own limit is 32,767 characters); anything longer is a guard
/// tripping, not a workbook.
pub const DEFAULT_MAX_STRING: u64 = 32 << 20;

/// What a read refuses: the whole file's size, and the length of any one string in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: u64,
    pub max_string: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_bytes: env_max_bytes(), max_string: DEFAULT_MAX_STRING }
    }
}

/// `CELLA_MAX_JSON_MB` in bytes, or [`DEFAULT_MAX_BYTES`]. Read once: the knob is a
/// diagnostic (and a test hook), not something every open should re-parse.
fn env_max_bytes() -> u64 {
    static LIMIT: OnceLock<u64> = OnceLock::new();
    *LIMIT.get_or_init(|| match std::env::var("CELLA_MAX_JSON_MB").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
        // 0 (or a non-number) keeps the default: there is no way to turn the guard off.
        Some(mb) if mb > 0 => mb.saturating_mul(1 << 20).min(MAX_MAX_BYTES),
        _ => DEFAULT_MAX_BYTES,
    })
}

/// Reads a JSON workbook: an enveloped (version ≥ 1) file or the bare JSON of version 0. The
/// warnings say what the reader had to work around (a newer format version, say).
pub fn read(bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    read_with(bytes, &Limits::default())
}

/// [`read`] with explicit caps, which is what a test uses to prove an oversized file is refused
/// without building a 512 MiB fixture.
pub fn read_with(bytes: &[u8], limits: &Limits) -> Result<(Workbook, Vec<String>)> {
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > limits.max_bytes {
        return Err(too_large(size, limits.max_bytes));
    }
    // The parse is also what turns a truncated file into a message: serde_json says where it ran
    // out, and its own recursion limit refuses a deeply nested one before the stack does.
    let doc: Value = serde_json::from_slice(bytes).map_err(|e| refused(format!("{e}. The file may be damaged, or only partly downloaded")))?;
    check_strings(&doc, limits.max_string)?;
    let Value::Object(mut doc) = doc else { return Err(refused(format!("a JSON {} is not a workbook, which is an object", kind(&doc)))) };
    // The envelope is the `formatVersion` key, not the number under it: a file without the key is
    // the bare workbook every build before this one wrote, and is read as version 0.
    let Some(version) = doc.remove(VERSION_KEY).map(|v| version_of(&v)).transpose()? else {
        let wb = from_value(Value::Object(doc))?;
        return Ok((wb, Vec::new()));
    };
    let Some(book) = doc.remove(WORKBOOK_KEY) else {
        return Err(refused(format!("format version {version} has no \"{WORKBOOK_KEY}\"")));
    };
    let mut wb = from_value(book)?;
    // What is left of the envelope: a newer version's keys, kept for the save that writes them back.
    wb.json_envelope.keep_object(doc, RESERVED);
    let warnings = match version.cmp(&FORMAT_VERSION) {
        std::cmp::Ordering::Greater => vec![format!(
            "This workbook was written in JSON format {version}; this build knows format {FORMAT_VERSION}. \
             Fields it does not know are kept when you save."
        )],
        _ => Vec::new(),
    };
    Ok((wb, warnings))
}

/// The error for a file of `size` bytes over the `max` a read takes: the wording a read gives,
/// so a caller that can see a file's size refuse it before reading it says the same thing.
pub fn too_large(size: u64, max: u64) -> EngineError {
    refused(format!(
        "it is {}, over the {} one file may be (a guard against damaged or malicious files). \
         For a file you trust, raise the limit with the CELLA_MAX_JSON_MB environment variable.",
        megabytes(size),
        megabytes(max)
    ))
}

/// Writes `wb` as the current envelope. Every field the model does not hold but the file it came
/// from did (see [`cella_model::JsonKept`]) is written back where it was.
pub fn write(wb: &Workbook) -> Result<Vec<u8>> {
    let mut doc = Map::new();
    for (key, value) in wb.json_envelope.fields() {
        if !RESERVED.contains(&key.as_str()) {
            doc.insert(key.clone(), value.clone());
        }
    }
    doc.insert(VERSION_KEY.to_string(), Value::from(FORMAT_VERSION));
    let book = serde_json::to_value(wb).map_err(|e| EngineError::Other(format!("this workbook cannot be written as JSON: {e}")))?;
    doc.insert(WORKBOOK_KEY.to_string(), book);
    // Pretty, and stable: serde_json's map is ordered, so the same workbook always gives the same
    // bytes (there is a golden test on it).
    serde_json::to_vec_pretty(&Value::Object(doc)).map_err(|e| EngineError::Other(format!("this workbook cannot be written as JSON: {e}")))
}

/// The workbook of `value`, with the error a reader can act on.
fn from_value(value: Value) -> Result<Workbook> {
    serde_json::from_value(value).map_err(|e| refused(format!("{e}")))
}

fn refused(why: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("not a Cella JSON workbook: {why}"))
}

/// `formatVersion`'s number: a whole number that fits in 64 bits. `1.0` is accepted (some JSON
/// writers emit every number as a float) and anything else is refused by name, because guessing
/// would be worse than saying so.
fn version_of(value: &Value) -> Result<u64> {
    match value.as_u64() {
        Some(v) => Ok(v),
        None => match value.as_f64() {
            Some(f) if f >= 0.0 && f.fract() == 0.0 && f <= 9.007_199_254_740_992e15 => Ok(f as u64),
            _ => Err(refused(format!("formatVersion must be a whole number, not {value}"))),
        },
    }
}

/// Refuses a file holding a string longer than `max` bytes. Walked with an explicit stack: the
/// depth is the parser's business (it has its own limit), not this function's stack's.
fn check_strings(value: &Value, max: u64) -> Result<()> {
    let too_long = |len: u64| {
        refused(format!(
            "one of its strings is {}, over the {} a single string may be (a guard against damaged or malicious files)",
            megabytes(len),
            megabytes(max)
        ))
    };
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::String(s) => {
                let len = u64::try_from(s.len()).unwrap_or(u64::MAX);
                if len > max {
                    return Err(too_long(len));
                }
            }
            Value::Array(items) => stack.extend(items.iter()),
            Value::Object(fields) => {
                for (key, value) in fields {
                    let len = u64::try_from(key.len()).unwrap_or(u64::MAX);
                    if len > max {
                        return Err(too_long(len));
                    }
                    stack.push(value);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A size for a message: whole megabytes when it has them, else the exact byte count under one.
fn megabytes(bytes: u64) -> String {
    let mb = bytes >> 20;
    if mb > 0 { format!("{mb} MB") } else { format!("{} bytes", bytes) }
}

/// What a JSON value is, for a message that says what the file held instead of a workbook.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true/false",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits { max_bytes: 1 << 20, max_string: 1024 }
    }

    #[test]
    fn a_file_over_the_cap_is_refused_with_its_size() {
        let bytes = vec![b' '; 2048];
        let limits = Limits { max_bytes: 1024, max_string: 1024 };
        let err = read_with(&bytes, &limits).unwrap_err().to_string();
        assert!(err.contains("over the 1024 bytes one file may be"), "{err}");
    }

    #[test]
    fn a_string_over_the_cap_is_refused() {
        let book = format!(r#"{{"formatVersion":1,"workbook":{{"sheets":[{{"name":"{}"}}]}}}}"#, "x".repeat(2048));
        let err = read_with(book.as_bytes(), &limits()).unwrap_err().to_string();
        assert!(err.contains("over the 1024 bytes a single string may be"), "{err}");
    }

    #[test]
    fn a_number_is_not_a_workbook() {
        let err = read_with(b"7", &limits()).unwrap_err().to_string();
        assert!(err.contains("a JSON number is not a workbook"), "{err}");
    }

    #[test]
    fn a_bad_version_says_so() {
        let err = read_with(br#"{"formatVersion":"one","workbook":{}}"#, &limits()).unwrap_err().to_string();
        assert!(err.contains("formatVersion must be a whole number"), "{err}");
        let err = read_with(br#"{"formatVersion":1}"#, &limits()).unwrap_err().to_string();
        assert!(err.contains("has no \"workbook\""), "{err}");
    }

    /// A legacy (Ctrl+Shift+Enter) array formula keeps its range through the envelope, and the
    /// version the writer stamps does not move (S6 of `plan/SHAPE-SHARING.md`: the range is
    /// boxed for its size, which serde does not see).
    #[test]
    fn an_array_formula_round_trips_without_a_version_bump() {
        use cella_core::{CellRef, RangeRef};
        use cella_model::{Cell, Formula};
        let mut wb = Workbook::new();
        let mut f = Formula::new("=A1:A2*2");
        f.array = RangeRef::parse("C1:C2").map(Box::new);
        f.dynamic = true;
        wb.sheet_mut(0).expect("sheet").cells.set(CellRef::new(0, 2), Cell::formula(f));
        let bytes = write(&wb).expect("writes");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(doc.get(VERSION_KEY), Some(&Value::from(FORMAT_VERSION)), "no formatVersion bump");
        // The boxed range is written as the object the inline range always was.
        assert!(
            doc["workbook"]["sheets"][0].to_string().contains(r#""array":{"end":{"col":2,"row":1},"start":{"col":2,"row":0}}"#),
            "the array range keeps its spelling: {}",
            doc["workbook"]["sheets"][0]
        );
        let (back, warnings) = read(&bytes).expect("reads");
        assert!(warnings.is_empty(), "{warnings:?}");
        let cell = back.sheet(0).and_then(|s| s.cell(CellRef::new(0, 2))).expect("the cell is back");
        let f = cell.formula.as_ref().expect("the formula is back");
        assert_eq!(f.text, "A1:A2*2");
        assert_eq!(f.array.as_deref().copied(), RangeRef::parse("C1:C2"));
        assert!(f.dynamic);
    }

    #[test]
    fn a_newer_version_is_read_with_a_warning() {
        let book = serde_json::to_string(&Workbook::new()).unwrap();
        let book = format!(r#"{{"formatVersion":9,"workbook":{book},"future":{{"a":1}}}}"#);
        let (wb, warnings) = read_with(book.as_bytes(), &limits()).unwrap();
        assert_eq!(wb.sheets.first().map(|s| s.name.as_str()), Some("Sheet1"));
        assert_eq!(wb.json_envelope.get("future"), Some(&serde_json::json!({"a": 1})));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
