//! CSV import and export (RFC 4180 with the usual real-world leniency).

use std::borrow::Cow;
use std::sync::Arc;

use gridcraft_core::parse::parse_input;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, StyleTable, Workbook};

use crate::{CsvOptions, IoError};

/// The largest CSV or tab-separated file written (bytes).
pub const MAX_CSV_BYTES: usize = 512 << 20;

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// Windows-1256 code points for bytes 0x80–0xFF (unicode.org `CP1256.TXT`).
pub(crate) const CP1256: [u32; 128] = [
    0x20AC, 0x067E, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0679, 0x2039, 0x0152, 0x0686, 0x0698, 0x0688, 0x06AF, 0x2018,
    0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x06A9, 0x2122, 0x0691, 0x203A, 0x0153, 0x200C, 0x200D, 0x06BA, 0x00A0, 0x060C, 0x00A2, 0x00A3,
    0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x06BE, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5,
    0x00B6, 0x00B7, 0x00B8, 0x00B9, 0x061B, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x061F, 0x06C1, 0x0621, 0x0622, 0x0623, 0x0624, 0x0625, 0x0626, 0x0627,
    0x0628, 0x0629, 0x062A, 0x062B, 0x062C, 0x062D, 0x062E, 0x062F, 0x0630, 0x0631, 0x0632, 0x0633, 0x0634, 0x0635, 0x0636, 0x00D7, 0x0637, 0x0638,
    0x0639, 0x063A, 0x0640, 0x0641, 0x0642, 0x0643, 0x00E0, 0x0644, 0x00E2, 0x0645, 0x0646, 0x0647, 0x0648, 0x00E7, 0x00E8, 0x00E9, 0x00EA, 0x00EB,
    0x0649, 0x064A, 0x00EE, 0x00EF, 0x064B, 0x064C, 0x064D, 0x064E, 0x00F4, 0x064F, 0x0650, 0x00F7, 0x0651, 0x00F9, 0x0652, 0x00FB, 0x00FC, 0x200E,
    0x200F, 0x06D2,
];

/// Evidence that bytes which aren't UTF-8 are Windows-1256 Arabic rather than Windows-1252.
#[derive(Default)]
struct Cp1256Evidence {
    /// High bytes that decode to different characters in the two code pages, apart from invisible
    /// marks and ordinals after a digit.
    differing: usize,
    /// Of those, Arabic letters and Arabic punctuation (، ؛ ؟) in Windows-1256.
    arabic: usize,
    /// Arabic bytes next to an ASCII letter: Western accents sit inside Latin words, Arabic doesn't.
    arabic_touching_latin: usize,
    /// Accented Latin bytes the two code pages share, next to another high byte: Hebrew
    /// Windows-1255 letters land there, Arabic and Western text rarely do.
    shared_letters_in_runs: usize,
}

impl Cp1256Evidence {
    fn of(bytes: &[u8]) -> Self {
        let mut e = Self::default();
        for (i, &b) in bytes.iter().enumerate() {
            if b < 0x80 {
                continue;
            }
            let before = i.checked_sub(1).and_then(|j| bytes.get(j)).copied();
            let after = bytes.get(i + 1).copied();
            let next_to = |pred: fn(u8) -> bool| before.is_some_and(pred) || after.is_some_and(pred);
            if same_in_cp1252_and_cp1256(b) {
                if is_shared_latin_letter(b) && next_to(|x| x >= 0x80) {
                    e.shared_letters_in_runs += 1;
                }
                continue;
            }
            if is_invisible_mark(b) || is_ordinal_after_digit(b, before) {
                continue;
            }
            e.differing += 1;
            if is_cp1256_arabic(b) {
                e.arabic += 1;
                if next_to(|x| x.is_ascii_alphabetic()) {
                    e.arabic_touching_latin += 1;
                }
            }
        }
        e
    }

    /// Thresholds chosen against month, day and language names from 95 glibc locales (Western in
    /// Windows-1252, Arabic, Persian and Urdu in 1256, Hebrew and Yiddish in 1255) plus CSV samples.
    fn is_arabic(&self) -> bool {
        self.arabic >= 2
            && self.arabic * 10 >= self.differing * 9
            && self.arabic_touching_latin * 10 <= self.arabic * 3
            && self.shared_letters_in_runs * 20 <= self.arabic
    }
}

fn same_in_cp1252_and_cp1256(b: u8) -> bool {
    matches!(
        b,
        0x80 | 0x82..=0x89 | 0x8B..=0x8C | 0x91..=0x97 | 0x99 | 0x9B..=0x9C | 0xA0 | 0xA2..=0xA9 | 0xAB..=0xB9 | 0xBB..=0xBE | 0xD7 | 0xE0 | 0xE2 | 0xE7..=0xEB
            | 0xEE..=0xEF | 0xF4 | 0xF7 | 0xF9 | 0xFB..=0xFC
    )
}

/// ZWNJ, ZWJ, LRM and RLM: common inside Persian words and around Arabic cells, but no evidence
/// either way.
fn is_invisible_mark(b: u8) -> bool {
    matches!(b, 0x9D | 0x9E | 0xFD | 0xFE)
}

/// `º` and `ª` after a digit (`1º`, `2ª`) are Western ordinals, though Windows-1256 reads the bytes
/// as `؛` and `ھ`; either way they are no evidence.
fn is_ordinal_after_digit(b: u8, before: Option<u8>) -> bool {
    matches!(b, 0xAA | 0xBA) && before.is_some_and(|x| x.is_ascii_digit())
}

fn is_shared_latin_letter(b: u8) -> bool {
    matches!(b, 0xE0 | 0xE2 | 0xE7..=0xEB | 0xEE..=0xEF | 0xF4 | 0xF9 | 0xFB..=0xFC)
}

fn is_cp1256_arabic(b: u8) -> bool {
    matches!(
        b,
        0x81 | 0x8A | 0x8D..=0x90 | 0x98 | 0x9A | 0x9F | 0xA1 | 0xAA | 0xBA | 0xBF..=0xD6 | 0xD8..=0xDB | 0xDD..=0xDF | 0xE1 | 0xE3..=0xE6 | 0xEC..=0xED | 0xFF
    )
}

/// Decodes CSV bytes with BOM-aware UTF-8 and UTF-16 handling. Without a BOM, valid UTF-8 is kept.
/// Other bytes decode as Windows-1256 when they read as Arabic (see [`Cp1256Evidence`]), else as
/// Windows-1252.
///
/// The result borrows the input when it is already UTF-8 (the common case), so a
/// hundred-megabyte file is not copied once more before it is read.
pub fn decode_text(bytes: &[u8]) -> Cow<'_, str> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest);
    }
    let utf16 = |b: &[u8], le: bool| -> String {
        let units = b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Cow::Owned(utf16(rest, true));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Cow::Owned(utf16(rest, false));
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => Cow::Owned(decode_legacy(bytes)),
    }
}

/// The single-byte code pages: Windows-1252, or Windows-1256 when the bytes read as Arabic.
fn decode_legacy(bytes: &[u8]) -> String {
    let use_windows_1256 = Cp1256Evidence::of(bytes).is_arabic();
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0xFF if use_windows_1256 => char::from_u32(CP1256.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
            0x80..=0x9F => char::from_u32(CP1252.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
            _ => b as char,
        })
        .collect()
}

/// Picks comma, semicolon or tab: the candidate with the most consistent count per line
/// (outside quotes) over the first non-blank lines. A candidate must appear on every such line, otherwise comma wins.
pub fn sniff_delimiter(text: &str, quote: char) -> u8 {
    let mut best = (b',', 0usize, 0usize);
    for d in [',', ';', '\t'] {
        let mut counts = vec![];
        let mut n = 0usize;
        let mut in_q = false;
        let mut blank = true;
        for ch in text.chars().take(64 * 1024) {
            if !in_q && ch == '\n' {
                // Blank lines carry no delimiter evidence.
                if !blank {
                    counts.push(n);
                }
                n = 0;
                blank = true;
                if counts.len() >= 20 {
                    break;
                }
                continue;
            }
            if ch != '\r' {
                blank = false;
            }
            if ch == quote {
                in_q = !in_q;
            } else if !in_q && ch == d {
                n += 1;
            }
        }
        if !blank {
            counts.push(n);
        }
        let min = counts.iter().copied().min().unwrap_or(0);
        let total: usize = counts.iter().sum();
        if min > 0 && (min, total) > (best.1, best.2) {
            best = (d as u8, min, total);
        }
    }
    best.0
}

/// Splits CSV text into records of fields. A record keeps at most [`MAX_COLS`] fields; the rest are
/// dropped as they are read, so a row of endless delimiters cannot allocate a string per field.
pub(crate) fn parse_records(text: &str, delim: char, quote: char, mut on_record: impl FnMut(Vec<String>) -> bool) {
    let mut field = String::new();
    let mut record: Vec<String> = vec![];
    let mut in_q = false;
    let mut chars = text.chars().peekable();
    let mut field_started_quoted = false;
    while let Some(c) = chars.next() {
        if in_q {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    field.push(quote);
                    chars.next();
                } else {
                    in_q = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if c == quote && field.is_empty() && !field_started_quoted {
            in_q = true;
            field_started_quoted = true;
        } else if c == delim {
            let f = std::mem::take(&mut field);
            if record.len() < MAX_COLS as usize {
                record.push(f);
            }
            field_started_quoted = false;
        } else if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            let f = std::mem::take(&mut field);
            if record.len() < MAX_COLS as usize {
                record.push(f);
            }
            field_started_quoted = false;
            if !on_record(std::mem::take(&mut record)) {
                return;
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !record.is_empty() || field_started_quoted {
        if record.len() < MAX_COLS as usize {
            record.push(field);
        }
        on_record(record);
    }
}

/// One field of a parsed record: a run of the text, or of the parser's unescape scratch.
#[derive(Clone, Copy)]
struct FieldSpan {
    start: usize,
    end: usize,
    /// The run lives in the scratch: a quoted field holding a doubled quote, or text after the
    /// closing quote.
    scratch: bool,
}

/// One parsed record, as [`parse_records_bytes`] hands it to the reader: field `i` is
/// [`Record::get`]`(i)`. Every field but an unescaped one is a slice of the text, so a record
/// costs one reused buffer and no string per field.
pub(crate) struct Record<'a> {
    text: &'a str,
    scratch: &'a str,
    spans: &'a [FieldSpan],
}

impl Record<'_> {
    /// The fields kept for this record (a record holds at most [`MAX_COLS`] fields).
    pub(crate) fn len(&self) -> usize {
        self.spans.len()
    }
    /// Field `i`'s content; empty when the record has no such field.
    pub(crate) fn get(&self, i: usize) -> &str {
        match self.spans.get(i) {
            Some(span) if span.scratch => self.scratch.get(span.start..span.end).unwrap_or_default(),
            Some(span) => self.text.get(span.start..span.end).unwrap_or_default(),
            None => "",
        }
    }
}

/// Ends the field whose content is `text[run..run_end]` and, when it had to be unescaped,
/// `scratch[at..]` in front of it. The field joins the record when there is still room for it.
/// Returns whether the field was empty, which is what decides whether a text that ends without a
/// line end holds a record at all.
fn end_field(scratch: &mut String, spans: &mut Vec<FieldSpan>, text: &str, run: usize, run_end: usize, at: Option<usize>) -> bool {
    let span = match at {
        Some(start) => {
            scratch.push_str(text.get(run..run_end).unwrap_or_default());
            FieldSpan { start, end: scratch.len(), scratch: true }
        }
        None => FieldSpan { start: run, end: run_end, scratch: false },
    };
    let empty = span.start == span.end;
    if spans.len() < MAX_COLS as usize {
        spans.push(span);
    }
    empty
}

/// Splits CSV bytes into records of fields, scanning bytes: with an ASCII delimiter and quote (the
/// common case) both are one byte that no character contains as part of itself, so a byte walk sees
/// exactly what a character walk would and nothing has to be decoded first. Every boundary falls on
/// an ASCII byte, so the fields are the text's own [`str`] slices: no copy, and no UTF-8 check of
/// text that was already known to be UTF-8.
///
/// A record keeps at most [`MAX_COLS`] fields; the fields past that are still scanned (a delimiter
/// inside them is not content) but dropped, so a row of endless delimiters cannot allocate a string
/// per field. A quoted field with a doubled quote is unescaped once, into one scratch buffer that
/// every record reuses.
pub(crate) fn parse_records_bytes(text: &str, delim: u8, quote: u8, mut on_record: impl FnMut(&Record<'_>) -> bool) {
    let bytes = text.as_bytes();
    let mut scratch = String::new();
    let mut spans: Vec<FieldSpan> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        scratch.clear();
        spans.clear();
        let mut in_q = false;
        let mut started_quoted = false;
        // The field being read: its content is `text[run..run_end]`, and `scratch[at..]` in
        // front of that once unescaping has made it more than one run.
        let mut run = i;
        let mut run_end = i;
        let mut at: Option<usize> = None;
        loop {
            let Some(&b) = bytes.get(i) else {
                // The end of the text ends the last record, unless that field is empty and the
                // record is still empty: a file ending in a line end has already ended both.
                let empty = end_field(&mut scratch, &mut spans, text, run, run_end, at);
                if (!empty || !spans.is_empty() || started_quoted) && !on_record(&Record { text, scratch: &scratch, spans: &spans }) {
                    return;
                }
                return;
            };
            if in_q {
                if b != quote {
                    run_end = i + 1;
                    i += 1;
                } else if bytes.get(i + 1) == Some(&quote) {
                    // A doubled quote is one literal quote: the content is no longer one run of the
                    // text, so what it holds so far moves to the scratch.
                    if at.is_none() {
                        at = Some(scratch.len());
                    }
                    scratch.push_str(text.get(run..run_end).unwrap_or_default());
                    scratch.push(char::from(quote));
                    run = i + 2;
                    run_end = run;
                    i += 2;
                } else {
                    // The closing quote: the content stops before it. Anything after it that is not
                    // a delimiter or a line end continues the field, as text appended to a quoted
                    // field (`"a"b` is `ab`).
                    in_q = false;
                    i += 1;
                    if bytes.get(i).is_some_and(|&next| next != delim && next != b'\r' && next != b'\n') {
                        if at.is_none() {
                            at = Some(scratch.len());
                        }
                        scratch.push_str(text.get(run..run_end).unwrap_or_default());
                        run = i;
                        run_end = i;
                    }
                }
                continue;
            }
            if b == quote && run_end == run && at.is_none() && !started_quoted {
                // A quote opens a field only where the field has no content yet; anywhere else it is
                // a character.
                in_q = true;
                started_quoted = true;
                i += 1;
                run = i;
                run_end = i;
            } else if b == delim {
                end_field(&mut scratch, &mut spans, text, run, run_end, at);
                i += 1;
                run = i;
                run_end = i;
                at = None;
                started_quoted = false;
            } else if b == b'\r' || b == b'\n' {
                end_field(&mut scratch, &mut spans, text, run, run_end, at);
                i += if b == b'\r' && bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                if !on_record(&Record { text, scratch: &scratch, spans: &spans }) {
                    return;
                }
                break;
            } else {
                run_end = i + 1;
                i += 1;
            }
        }
    }
}

/// Writes parsed records into the sheet as rows: one store write per row (the reader's row path)
/// instead of one write per cell, which is what a million-row import spends its time on.
struct RowWriter<'a> {
    opts: &'a CsvOptions,
    styles: &'a mut StyleTable,
    /// The number formats already derived, so a million dates share one style.
    fmt_cache: Vec<(&'static str, StyleId)>,
    /// The row being built, reused for every record.
    row_cells: Vec<(u32, Cell)>,
    row: u32,
    sheet: Sheet,
}

impl<'a> RowWriter<'a> {
    fn new(opts: &'a CsvOptions, styles: &'a mut StyleTable) -> Self {
        RowWriter { opts, styles, fmt_cache: Vec::new(), row_cells: Vec::new(), row: 0, sheet: Sheet::new("Sheet1") }
    }

    /// One record from the byte scanner.
    fn record(&mut self, rec: &Record<'_>) -> bool {
        for col in 0..rec.len() {
            self.field(col as u32, rec.get(col));
        }
        self.end_row()
    }

    /// One record from the character scanner, which is used for a delimiter or a quote outside
    /// ASCII.
    fn record_chars(&mut self, fields: Vec<String>) -> bool {
        for (col, text) in fields.iter().enumerate() {
            self.field(col as u32, text);
        }
        self.end_row()
    }

    /// Writes one field: sniffed (a number, a date, a boolean, an error or text, with the number
    /// format the sniffing chose) or kept as the text it is.
    fn field(&mut self, col: u32, text: &str) {
        if text.is_empty() || col >= MAX_COLS {
            return;
        }
        let cell = if self.opts.parse_values {
            let p = parse_input(text, self.opts.date_system);
            let style = match p.format {
                Some(code) => match self.fmt_cache.iter().find(|(c, _)| *c == code) {
                    Some((_, id)) => *id,
                    None => {
                        let id = self.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
                        self.fmt_cache.push((code, id));
                        id
                    }
                },
                None => StyleId::DEFAULT,
            };
            Cell { value: p.value, formula: None, style }
        } else {
            Cell::value(Value::Text(Arc::from(text)))
        };
        self.row_cells.push((col, cell));
    }

    /// Ends the row with one store write, then says whether to go on: a file with more rows than the
    /// grid holds stops here. A row of empty fields is a blank row and writes nothing.
    fn end_row(&mut self) -> bool {
        if !self.row_cells.is_empty() {
            self.sheet.set_row_cells(self.row, &mut self.row_cells);
        }
        // The row write drains the batch; a row it dropped has to be emptied here.
        self.row_cells.clear();
        self.row = self.row.saturating_add(1);
        self.row < MAX_ROWS
    }

    fn into_sheet(self) -> Sheet {
        self.sheet
    }
}

pub fn read_csv(bytes: &[u8], opts: &CsvOptions) -> Result<Workbook, IoError> {
    let text = decode_text(bytes);
    let quote = if opts.quote == 0 { '"' } else { opts.quote as char };
    let delim = if opts.delimiter == 0 { sniff_delimiter(&text, quote) } else { opts.delimiter } as char;
    if delim == quote {
        return Err(IoError::Format("the delimiter and quote character must differ".into()));
    }
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = u8::try_from(delim).ok();
    let mut styles = std::mem::take(&mut wb.styles);
    let mut writer = RowWriter::new(opts, &mut styles);
    if delim.is_ascii() && quote.is_ascii() {
        parse_records_bytes(&text, delim as u8, quote as u8, |rec| writer.record(rec));
    } else {
        // A delimiter or a quote outside ASCII (a § separator, say) is a character, not a byte:
        // those files take the character scanner, which reads only correct multi-byte text.
        parse_records(&text, delim, quote, |rec| writer.record_chars(rec));
    }
    let sheet = writer.into_sheet();
    wb.styles = styles;
    wb.sheets = vec![Arc::new(sheet)];
    Ok(wb)
}

/// Writes the sheet as CSV from A1 to the end of its used range, so leading empty columns and rows
/// stay as empty fields and every cell keeps its position (CRLF line ends, UTF-8 without BOM). `cell_text` gives
/// each cell's field text: the caller decides how values look (Save As CSV writes them as
/// displayed, number formats applied); quoting is done here.
///
/// The output is capped at [`MAX_CSV_BYTES`]: a cell far from A1 makes the file as big as the whole
/// rectangle between them (`A1` and `XFD1048576` are 16 GiB of commas).
/// An export that would pass the cap is refused before anything is allocated, or at the first
/// field or line end that takes it past the cap (a field is measured before it is written), with
/// [`IoError::TooLarge`]; accepted exports keep every field in its place.
pub fn write_csv(sheet: &Sheet, delimiter: u8, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_limited(sheet, delimiter, MAX_CSV_BYTES, cell_text)
}

/// [`write_csv`] with the output cap `limit` (bytes) given.
pub fn write_csv_limited(sheet: &Sheet, delimiter: u8, limit: usize, mut cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    let delim = if delimiter == 0 { ',' } else { delimiter as char };
    let mut out = String::new();
    let Some(r) = sheet.used_range() else { return Ok(Vec::new()) };
    let too_large =
        |what: String| IoError::TooLarge(format!("the CSV would be {what}, over the limit of {} MiB; export a smaller range", limit >> 20));
    // Every row has a delimiter between its fields and a CRLF, whatever the cells hold: a lower
    // bound on the output that needs no cell text.
    let (rows, cols) = (u64::from(r.end.row) + 1, u64::from(r.end.col) + 1);
    let per_row = (cols - 1).saturating_mul(delim.len_utf8() as u64).saturating_add(2);
    let least = rows.saturating_mul(per_row);
    if least > limit as u64 {
        return Err(too_large(format!("at least {} MiB ({rows} rows by {cols} columns)", least >> 20)));
    }
    for row in 0..=r.end.row {
        for col in 0..=r.end.col {
            if col > 0 {
                out.push(delim);
            }
            let t = cell_text(CellRef::new(row, col));
            let quoted = t.contains(delim) || t.contains('"') || t.contains('\n') || t.contains('\r');
            // The exact size of the field as written (the delimiter was pushed above), so a row
            // never grows past the cap unseen and no doubled-quote copy of the text is built.
            let field_len = if quoted { t.len() + t.matches('"').count() + 2 } else { t.len() };
            if out.len().saturating_add(field_len) > limit {
                return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
            }
            if quoted {
                out.push('"');
                for ch in t.chars() {
                    if ch == '"' {
                        out.push('"');
                    }
                    out.push(ch);
                }
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push_str("\r\n");
        if out.len() > limit {
            return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
        }
    }
    Ok(out.into_bytes())
}
