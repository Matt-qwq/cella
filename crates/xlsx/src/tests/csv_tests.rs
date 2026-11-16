use std::sync::Arc;

use gridcraft_core::parse::parse_input;
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};

use crate::csv::{decode_text, parse_records, sniff_delimiter};
use crate::{CsvOptions, Format, IoError, read_csv, sniff, write_csv, write_csv_limited};

fn v(wb: &Workbook, r: u32, c: u32) -> Value {
    wb.sheet(0).unwrap().value(CellRef::new(r, c))
}

#[test]
fn quoting_and_newlines() {
    let src = "a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",,3\n\nlast";
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(wb.sheets[0].name, "Sheet1");
    assert_eq!(v(&wb, 0, 0), Value::text("a"));
    assert_eq!(v(&wb, 0, 1), Value::text("b,c"));
    assert_eq!(v(&wb, 0, 2), Value::text("say \"hi\""));
    assert_eq!(v(&wb, 1, 0), Value::text("multi\nline"));
    assert_eq!(v(&wb, 1, 1), Value::Empty);
    assert_eq!(v(&wb, 1, 2), Value::Number(3.0));
    assert_eq!(v(&wb, 2, 0), Value::Empty);
    assert_eq!(v(&wb, 3, 0), Value::text("last"));
}

#[test]
fn values_and_formats() {
    let src = "1,2.5,\"1,234\",50%,TRUE,#N/A,2026-03-04,$5.00,'007,text";
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::Number(1.0));
    assert_eq!(v(&wb, 0, 1), Value::Number(2.5));
    assert_eq!(v(&wb, 0, 2), Value::Number(1234.0));
    assert_eq!(v(&wb, 0, 3), Value::Number(0.5));
    assert_eq!(v(&wb, 0, 4), Value::Bool(true));
    assert!(v(&wb, 0, 5).is_error());
    assert!(v(&wb, 0, 6).is_number());
    let s = wb.sheet(0).unwrap();
    let st = wb.styles.get(s.cell(CellRef::new(0, 6)).unwrap().style);
    assert_ne!(st.num_fmt.as_str(), "General");
    assert_eq!(v(&wb, 0, 8), Value::text("007"));
    // 1904 dates differ from 1900 dates.
    let wb2 = read_csv(b"2026-03-04", &CsvOptions { date_system: DateSystem::D1904, ..Default::default() }).unwrap();
    assert_eq!(wb2.date_system, DateSystem::D1904);
    assert_eq!(v(&wb, 0, 6).as_f64().unwrap() - v(&wb2, 0, 0).as_f64().unwrap(), 1462.0);
    // parse_values off keeps text.
    let wb3 = read_csv(b"1,TRUE", &CsvOptions { parse_values: false, ..Default::default() }).unwrap();
    assert_eq!(v(&wb3, 0, 0), Value::text("1"));
}

#[test]
fn encodings() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice("é,x".as_bytes());
    assert_eq!(decode_text(&b), "é,x");
    let mut le = vec![0xFF, 0xFE];
    for u in "ü;1".encode_utf16() {
        le.extend_from_slice(&u.to_le_bytes());
    }
    assert_eq!(decode_text(&le), "ü;1");
    let mut be = vec![0xFE, 0xFF];
    for u in "ß".encode_utf16() {
        be.extend_from_slice(&u.to_be_bytes());
    }
    assert_eq!(decode_text(&be), "ß");
    assert_eq!(decode_text(&[0x80, b'a', 0xE9]), "€aé");
    let wb = read_csv(&le, &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("ü"));
    assert_eq!(v(&wb, 0, 1), Value::Number(1.0));
}

#[test]
fn windows_1256_table_matches_the_unicode_mapping() {
    let at = |b: u8| crate::csv::CP1256.get(usize::from(b - 0x80)).and_then(|&c| char::from_u32(c));
    let expected = [
        (0x80, '€'),
        (0x81, 'پ'),
        (0x8D, 'چ'),
        (0x98, 'ک'),
        (0x9D, '\u{200C}'),
        (0xA1, '،'),
        (0xBA, '؛'),
        (0xBF, '؟'),
        (0xC7, 'ا'),
        (0xC8, 'ب'),
        (0xDC, 'ـ'),
        (0xE1, 'ل'),
        (0xE3, 'م'),
        (0xE9, 'é'),
        (0xFD, '\u{200E}'),
        (0xFE, '\u{200F}'),
        (0xFF, 'ے'),
    ];
    for (b, c) in expected {
        assert_eq!(at(b), Some(c), "0x{b:02X}");
    }
}

#[test]
fn arabic_csv_in_windows_1256_reads_as_arabic() {
    let bytes = [
        0xC7, 0xE1, 0xC7, 0xD3, 0xE3, b',', 0xC7, 0xE1, 0xDE, 0xED, 0xE3, 0xC9, b'\n', 0xE3, 0xD1, 0xCD, 0xC8, 0xC7, b' ', b'1', b'2', b'3', 0xA1,
        b' ', b'4', b'2', 0xBF, b',', 0xC8, 0xED, 0xC7, 0xE4, 0xC7, 0xCA, b' ', b'7', b'\n',
    ];
    let wb = read_csv(&bytes, &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("الاسم"));
    assert_eq!(v(&wb, 0, 1), Value::text("القيمة"));
    assert_eq!(v(&wb, 1, 0), Value::text("مرحبا 123، 42؟"));
    assert_eq!(v(&wb, 1, 1), Value::text("بيانات 7"));
}

fn reads_as_arabic(bytes: &[u8]) -> bool {
    decode_text(bytes).chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))
}

#[test]
fn western_1252_text_stays_latin() {
    let french = [
        b'C', b'r', 0xE8, b'm', b'e', b' ', b'b', b'r', 0xFB, b'l', 0xE9, b'e', b',', b' ', 0xE0, b' ', b'c', 0xF4, b't', 0xE9, b',', b' ', 0xE9,
        b't', 0xE9,
    ];
    let german = [
        b'G', b'r', 0xF6, 0xDF, b'e', b',', b' ', 0xDC, b'b', b'e', b'r', b'm', b'a', 0xDF, b',', b' ', b'F', b'u', 0xDF, b'g', 0xE4, b'n', b'g',
        b'e', b'r',
    ];
    assert_eq!(decode_text(&french), "Crème brûlée, à côté, été");
    assert_eq!(decode_text(&german), "Größe, Übermaß, Fußgänger");
    let portuguese = [
        b'A', 0xC7, 0xC3, b'O', b',', b' ', b'i', b'n', b'f', b'o', b'r', b'm', b'a', 0xE7, 0xE3, b'o', b',', b' ', b's', b'i', b't', b'u', b'a',
        0xE7, 0xE3, b'o', b',', b' ', b'P', 0xE3, b'o',
    ];
    assert_eq!(decode_text(&portuguese), "AÇÃO, informação, situação, Pão");
    let nordic = [
        0xC6, b'r', 0xF8, b',', 0xD8, b's', b't', b',', 0xC5, b's', b'e', b',', 0xDE, 0xF3, b'r', 0xF0, b'u', b'r', b',', b'G', b'u', 0xF0, b'r',
        0xFA, b'n', b',', 0xC4, 0xD6, 0xDC,
    ];
    assert_eq!(decode_text(&nordic), "Ærø,Øst,Åse,Þórður,Guðrún,ÄÖÜ");
    let ordinals = [b'1', 0xBA, b',', b'2', 0xBA, b',', b'3', 0xBA, b'\n', b'1', 0xAA, b',', b'2', 0xAA];
    assert_eq!(decode_text(&ordinals), "1º,2º,3º\n1ª,2ª");
    let inside_words = [b'K', 0xE4, b's', b'e', b',', b'B', 0xE4, b'c', b'k', b'e', b'r'];
    assert_eq!(decode_text(&inside_words), "Käse,Bäcker");
}

#[test]
fn short_arabic_words_read_as_arabic() {
    assert_eq!(decode_text(&[0xDD, 0xED, b',', 0xE3, 0xE4, b',', 0xDA, 0xE4, b'\n']), "في,من,عن\n");
    assert_eq!(decode_text(&[0xE4, 0xDA, 0xE3, b',', 0xE1, 0xC7, b'\n', 0xE1, 0xC7, b',', 0xE4, 0xDA, 0xE3, b'\n']), "نعم,لا\nلا,نعم\n");
    assert_eq!(decode_text(&[0x98, 0xCA, 0xC7, 0xC8, b' ', 0x81, 0xC7, 0xD1, 0xD3, 0xED]), "کتاب پارسي");
    let with_marks = [
        0xDF, 0xCA, 0xC7, 0xC8, 0x9D, 0xE5, 0xC7, b',', 0xCE, 0xC7, 0xE4, 0xE5, 0x9D, 0xE5, 0xC7, b',', 0xE3, 0xCD, 0xE3, 0xCF, 0xFE, b',', 0xDA,
        0xE1, 0xED, 0xFE,
    ];
    assert_eq!(decode_text(&with_marks), "كتاب\u{200C}ها,خانه\u{200C}ها,محمد\u{200F},علي\u{200F}");
}

#[test]
fn hebrew_1255_bytes_are_not_read_as_arabic() {
    let plain = [0xF9, 0xEC, 0xE5, 0xED, b',', 0xF2, 0xE5, 0xEC, 0xED];
    let pointed = [0xF9, 0xC8, 0xD1, 0xEC, 0xE5, 0xC9, 0xED, b' ', 0xF2, 0xE5, 0xC9, 0xEC, 0xC8, 0xED];
    let maqaf = [0xFA, 0xEC, 0xCE, 0xE0, 0xE1, 0xE9, 0xE1, b',', 0xE1, 0xEF, 0xCE, 0xE2, 0xE5, 0xF8, 0xE9, 0xE5, 0xEF];
    let gershayim = [0xF6, 0xE4, 0xD8, 0xEC, b',', 0xE3, 0xD8, 0xF8];
    for bytes in [&plain[..], &pointed, &maqaf, &gershayim] {
        assert!(!reads_as_arabic(bytes), "{:?}", decode_text(bytes));
    }
}

#[test]
fn delimiter_sniffing() {
    assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n", '"'), b';');
    assert_eq!(sniff_delimiter("a\tb\n1\t2\n", '"'), b'\t');
    assert_eq!(sniff_delimiter("a,b\n\"x;y;z\",2\n", '"'), b',');
    assert_eq!(sniff_delimiter("single", '"'), b',');
    // A delimiter missing from some lines is text, not a separator (single-column export round trip).
    assert_eq!(sniff_delimiter("alpha;beta\r\ngamma\r\n", '"'), b',');
    assert_eq!(sniff_delimiter("alpha\tbeta\r\ngamma\r\n", '"'), b',');
    assert_eq!(sniff_delimiter("a;b\r\n\r\nc;d\r\n", '"'), b';');
    let wb = read_csv(b"x;y\n1,5;2", &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 1, 1), Value::Number(2.0));
    // The workbook remembers the separator it was read with, sniffed or given.
    assert_eq!(wb.csv_delimiter, Some(b';'));
    assert_eq!(read_csv(b"a\tb", &CsvOptions { delimiter: 0, ..Default::default() }).unwrap().csv_delimiter, Some(b'\t'));
    assert_eq!(read_csv(b"a;b", &CsvOptions::default()).unwrap().csv_delimiter, Some(b','));
    assert_eq!(Workbook::new().csv_delimiter, None);
    assert!(read_csv(b"a", &CsvOptions { delimiter: b'"', ..Default::default() }).is_err());
}

#[test]
fn writing() {
    let mut wb = Workbook::new();
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(1, 1), Value::text("a,b"));
    s.set_value(CellRef::new(1, 2), Value::text("q\"t"));
    s.set_value(CellRef::new(2, 1), Value::Number(0.1));
    s.set_value(CellRef::new(2, 2), Value::Bool(false));
    s.set_value(CellRef::new(3, 3), Value::text("x\ny"));
    s.set_value(CellRef::new(3, 1), Value::Error(gridcraft_core::CellError::Div0));
    wb.sheets = vec![std::sync::Arc::new(s)];
    // The caller supplies each field's text; quoting and line ends are the writer's.
    let sh = wb.sheet(0).unwrap();
    let raw = |c: CellRef| match sh.value(c) {
        Value::Empty => String::new(),
        Value::Number(n) => gridcraft_core::number_to_text(n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => String::new(),
    };
    let out = String::from_utf8(write_csv(sh, b',', raw).unwrap()).unwrap();
    // The first cell is B2: the file starts at A1, so the leading empty row and column stay.
    assert_eq!(out, ",,,\r\n,\"a,b\",\"q\"\"t\",\r\n,0.1,FALSE,\r\n,#DIV/0!,,\"x\ny\"\r\n");
    let semi = String::from_utf8(write_csv(sh, b';', raw).unwrap()).unwrap();
    assert!(semi.starts_with(";;;\r\n;a,b;"));
    assert!(write_csv(&Sheet::new("E"), b',', raw).unwrap().is_empty());
    // Round trip.
    let back = read_csv(out.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&back, 1, 1), Value::text("a,b"));
    assert_eq!(v(&back, 3, 3), Value::text("x\ny"));
}

#[test]
fn sniffing_formats() {
    assert_eq!(sniff(b"PK\x03\x04rest"), Format::Xlsx);
    assert_eq!(sniff(b"a,b\n1,2"), Format::Csv);
    // An Excel password-protected file is a CFB/OLE2 compound file with an EncryptedPackage
    // stream; any other compound file is a legacy binary (.xls), not an encrypted workbook.
    assert_eq!(sniff(&crate::cfb::tests::build(&["EncryptionInfo", "EncryptedPackage"])), Format::Encrypted);
    assert_eq!(sniff(&crate::cfb::tests::build(&["Workbook"])), Format::LegacyBinary);
    assert_eq!(sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0]), Format::LegacyBinary);
    assert_eq!(sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0, 0, 1, 2]), Format::Unknown);
    assert_eq!(sniff(b""), Format::Unknown);
    assert_eq!(sniff(&[0xFF, 0xFE, b'a', 0]), Format::Csv);
}

#[test]
fn hostile_csv() {
    let _ = read_csv(&[0xFF; 1000], &CsvOptions::default()).unwrap();
    let _ = read_csv(b"\"unterminated,quote\n1,2", &CsvOptions::default()).unwrap();
    let wide = ",".repeat(20_000);
    let wb = read_csv(format!("{wide}x").as_bytes(), &CsvOptions::default()).unwrap();
    assert!(wb.sheet(0).unwrap().cells.is_empty(), "fields past column XFD are dropped");
}

/// A sheet with `a` in A1 and `b` in `corner`.
fn two_cells(corner: (u32, u32)) -> Sheet {
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(0, 0), Value::text("a"));
    s.set_value(CellRef::new(corner.0, corner.1), Value::text("b"));
    s
}

fn text_of(sh: &Sheet) -> impl FnMut(CellRef) -> String + '_ {
    |c| match sh.value(c) {
        Value::Text(t) => t.to_string(),
        _ => String::new(),
    }
}

#[test]
fn two_far_apart_cells_are_refused_before_anything_is_allocated() {
    // A1 and XFD1048576 span the whole grid: 16 GiB of commas. Refused at once, with the numbers.
    let s = two_cells((gridcraft_core::MAX_ROWS - 1, gridcraft_core::MAX_COLS - 1));
    let err = write_csv(&s, b',', text_of(&s)).unwrap_err();
    assert!(matches!(err, IoError::TooLarge(_)), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("1048576 rows by 16384 columns") && msg.contains("512 MiB"), "{msg}");
    // A single row or column across the grid is refused too when it is over the cap.
    let wide = two_cells((gridcraft_core::MAX_ROWS - 1, 0));
    assert!(write_csv_limited(&wide, b',', 1 << 20, text_of(&wide)).is_err());
}

#[test]
fn the_cap_counts_the_bytes_the_cells_hold_and_accepted_exports_keep_every_position() {
    // 3 rows x 3 columns, 14 bytes: accepted at the cap, refused one byte under.
    let s = two_cells((2, 2));
    let ok = write_csv_limited(&s, b',', 14, text_of(&s)).unwrap();
    assert_eq!(String::from_utf8(ok).unwrap(), "a,,\r\n,,\r\n,,b\r\n");
    assert!(write_csv_limited(&s, b',', 13, text_of(&s)).is_err());
    // Rows that pass the lower bound but hold long text stop as soon as the cap is passed.
    let mut long = Sheet::new("L");
    for r in 0..50u32 {
        long.set_value(CellRef::new(r, 0), Value::text("x".repeat(100)));
    }
    let err = write_csv_limited(&long, b',', 1000, text_of(&long)).unwrap_err();
    assert!(err.to_string().contains("stopped at row"), "{err}");
    assert!(write_csv_limited(&long, b',', 50 * 102, text_of(&long)).is_ok());
    // Within one row the cap is checked before each field is written: a wide row of long and
    // quote-heavy fields is refused at the field that crosses it, without asking for the rest.
    let mut wide = Sheet::new("W");
    for c in 0..10u32 {
        wide.set_value(CellRef::new(0, c), Value::text("\"".repeat(100)));
    }
    let mut asked = 0;
    let err = write_csv_limited(&wide, b',', 500, |c| {
        asked += 1;
        text_of(&wide)(c)
    })
    .unwrap_err();
    assert!(err.to_string().contains("stopped at row 1"), "{err}");
    assert_eq!(asked, 3, "each field is 202 bytes; the third passes 500");
    // Exact boundary: a quoted field of 202 bytes plus CRLF fits in 204 and not in 203.
    let mut one = Sheet::new("O");
    one.set_value(CellRef::new(0, 0), Value::text("\"".repeat(100)));
    assert_eq!(write_csv_limited(&one, b',', 204, text_of(&one)).unwrap().len(), 204);
    assert!(write_csv_limited(&one, b',', 203, text_of(&one)).is_err());
    // A tab is one byte, like a comma.
    let tabs = write_csv_limited(&s, b'\t', 14, text_of(&s)).unwrap();
    assert_eq!(tabs.len(), 14);
}

#[test]
fn parser_stops_collecting_fields_at_max_cols() {
    let extra = 50;
    // MAX_COLS + extra fields on the first row (the last one filled), then a short second row.
    let mut text = ",".repeat((MAX_COLS + extra) as usize - 1);
    text.push_str("end\nx,y\n");
    let mut lens = vec![];
    parse_records(&text, ',', '"', |rec| {
        lens.push(rec.len());
        true
    });
    assert_eq!(lens, vec![MAX_COLS as usize, 2]);
    // An unterminated last row is capped the same way.
    let mut lens = vec![];
    parse_records(&",".repeat(MAX_COLS as usize + extra as usize), ',', '"', |rec| {
        lens.push(rec.len());
        true
    });
    assert_eq!(lens, vec![MAX_COLS as usize]);
}

// ---------------------------------------------------------------- the old reader, kept as the oracle

/// The CSV reader as it was before the byte scanner and the row batches: a `Vec<String>` per
/// record, then one store write per cell. Deliberately the slow shape — it is the reference every
/// input in [`comparison_inputs`] has to read cell for cell identically.
fn reference_read(bytes: &[u8], opts: &CsvOptions) -> Workbook {
    let text = decode_text(bytes);
    let quote = if opts.quote == 0 { '"' } else { opts.quote as char };
    let delim = if opts.delimiter == 0 { sniff_delimiter(&text, quote) } else { opts.delimiter } as char;
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = u8::try_from(delim).ok();
    let mut sheet = Sheet::new("Sheet1");
    let mut styles = std::mem::take(&mut wb.styles);
    let mut fmt_cache: Vec<(&'static str, StyleId)> = vec![];
    let mut row: u32 = 0;
    parse_records(&text, delim, quote, |rec| {
        if row >= MAX_ROWS {
            return false;
        }
        for (col, f) in rec.into_iter().enumerate() {
            if col as u32 >= MAX_COLS {
                break;
            }
            if f.is_empty() {
                continue;
            }
            let pos = CellRef::new(row, col as u32);
            let cell = if opts.parse_values {
                let p = parse_input(&f, opts.date_system);
                let style = match p.format {
                    Some(code) => match fmt_cache.iter().find(|(c, _)| *c == code) {
                        Some((_, id)) => *id,
                        None => {
                            let id = styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
                            fmt_cache.push((code, id));
                            id
                        }
                    },
                    None => StyleId::DEFAULT,
                };
                Cell { value: p.value, formula: None, style }
            } else {
                Cell::value(Value::Text(Arc::from(f)))
            };
            sheet.cells.set(pos, cell);
        }
        row += 1;
        true
    });
    wb.styles = styles;
    wb.sheets = vec![Arc::new(sheet)];
    wb
}

/// Every stored cell in order: position, value and the number format the sniffing chose. Two
/// readers agree only when this list matches entry for entry.
fn cells_of(wb: &Workbook) -> Vec<(u32, u32, Value, String)> {
    let sh = wb.sheet(0).expect("a csv has one sheet");
    sh.cells.iter().map(|(c, cell)| (c.row, c.col, cell.value.clone(), wb.styles.get(cell.style).num_fmt.as_str().to_string())).collect()
}

/// Reads `bytes` both ways and demands the same sheet: same cells in the same places, same number
/// formats, same remembered delimiter.
fn assert_reads_like_the_reference(name: &str, bytes: &[u8], opts: &CsvOptions) {
    let new = match read_csv(bytes, opts) {
        Ok(wb) => wb,
        Err(e) => panic!("{name}: the reader refused it: {e}"),
    };
    let old = reference_read(bytes, opts);
    assert_eq!(cells_of(&new), cells_of(&old), "{name}");
    assert_eq!(new.csv_delimiter, old.csv_delimiter, "{name}");
    assert_eq!(new.date_system, old.date_system, "{name}");
    assert_eq!(new.sheets.len(), old.sheets.len(), "{name}");
    assert_eq!(new.sheet(0).and_then(Sheet::used_range), old.sheet(0).and_then(Sheet::used_range), "{name}");
}

/// The shape a daily import has: 100,000 rows of six fields, one of them holding far more text
/// than a field usually does. Header included, so 100,001 rows.
fn a_100k_row_export() -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(3 << 20);
    s.push_str("region,units,price,share,sku,note\r\n");
    let long = "y".repeat(100_000);
    for r in 0..100_000u32 {
        let note = if r == 50_000 { long.as_str() } else { "\"plain, \"\"often\"\"\"" };
        let _ = write!(s, "Region {},{},{}.50,{}%,SKU-{:07},{note}\r\n", r % 17, r * 3, r % 1000, r % 13, r % 97);
    }
    s
}

/// The inputs the reader has to read exactly like the old one did: this module's own CSV text, the
/// encodings it accepts, and the hostile shapes a file exported by something else has.
fn comparison_inputs() -> Vec<(&'static str, Vec<u8>, CsvOptions)> {
    let text = |s: &str| s.as_bytes().to_vec();
    let raw = |bytes: &[u8]| bytes.to_vec();
    let as_text = CsvOptions { parse_values: false, ..Default::default() };
    let sniffed = CsvOptions { delimiter: 0, ..Default::default() };
    let mut out: Vec<(&'static str, Vec<u8>, CsvOptions)> = vec![
        ("quoting and newlines", text("a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",,3\n\nlast"), CsvOptions::default()),
        ("values and formats", text("1,2.5,\"1,234\",50%,TRUE,#N/A,2026-03-04,$5.00,'007,text"), CsvOptions::default()),
        ("values kept as text", text("1,2.5,\"1,234\",50%,TRUE,#N/A,2026-03-04,$5.00,'007,text"), as_text.clone()),
        ("a comma inside quotes and quotes at the field start", text("\"a,b\",\"\"\"x\"\"\",\"\",\" \""), CsvOptions::default()),
        ("quotes in the middle of a field", text("a\"b,c\"d,\"e\"f\",g\"\"h"), CsvOptions::default()),
        ("an unterminated quote", text("\"unterminated,quote\n1,2"), CsvOptions::default()),
        ("crlf, lf and a bare cr", text("a,b\r\nc,d\ne,f\rg,h"), CsvOptions::default()),
        ("ragged rows", text("a,b,c\nd\ne,f,g,h,i\r\n,,\n"), CsvOptions::default()),
        ("empty fields and blank lines", text(",\r\n\r\n,,y\r\n\n"), CsvOptions::default()),
        ("only a delimiter", text(","), CsvOptions::default()),
        ("two quotes alone", text("\"\""), CsvOptions::default()),
        ("one quote alone", text("\""), CsvOptions::default()),
        ("no line end at all", text("last"), CsvOptions::default()),
        ("a semicolon file", text("a;b;c\n1;2;3\n"), sniffed.clone()),
        ("a tab file", text("a\tb\n1\t2\n"), sniffed.clone()),
        ("a semicolon that is text on one line", text("alpha;beta\r\ngamma\r\n"), sniffed.clone()),
        ("spaces are content", text(" a , b \n 1 , 2 "), CsvOptions::default()),
        ("a delimiter no field uses", text("a\tb\n1\t2"), CsvOptions { delimiter: b'\t', ..Default::default() }),
        ("1904 dates", text("2026-03-04,1"), CsvOptions { date_system: DateSystem::D1904, ..Default::default() }),
        ("many blank lines at the end", text("a,b\n\n\n\n\n\n"), CsvOptions::default()),
        ("only blank lines", text("\r\n\n\r\n"), CsvOptions::default()),
        ("an empty file", raw(b""), CsvOptions::default()),
        ("every high byte at once", raw(&[0xFF; 1000]), CsvOptions::default()),
    ];
    // A UTF-8 BOM is stripped, not kept in the first field.
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice("é,ü,x\r\n1,2,3\r\n".as_bytes());
    out.push(("a utf-8 bom", bom, CsvOptions::default()));
    // UTF-16, both byte orders, with a BOM either way.
    let mut le = vec![0xFF, 0xFE];
    for u in "ü;1\nß 2".encode_utf16() {
        le.extend_from_slice(&u.to_le_bytes());
    }
    out.push(("utf-16le", le, sniffed.clone()));
    let mut be = vec![0xFE, 0xFF];
    for u in "ü;1\nß 2".encode_utf16() {
        be.extend_from_slice(&u.to_be_bytes());
    }
    out.push(("utf-16be", be, sniffed.clone()));
    // Windows-1252 Western text, Windows-1256 Arabic, Windows-1255 Hebrew (read as 1252) and the
    // ordinals that look like Arabic.
    let french = [b'C', b'r', 0xE8, b'm', b'e', b',', 0xE0, b' ', b'c', 0xF4, b't', 0xE9, b'\n', b'1', b',', b'2', b'\n'];
    out.push(("windows-1252 french", raw(&french), CsvOptions::default()));
    let arabic = [0xC7, 0xE1, 0xC7, 0xD3, 0xE3, b',', 0xC7, 0xE1, 0xDE, 0xED, 0xE3, 0xC9, b'\n', 0xDD, 0xED, b',', 0xE3, 0xE4, b'\n'];
    out.push(("windows-1256 arabic", raw(&arabic), CsvOptions::default()));
    out.push(("windows-1255 hebrew bytes", raw(&[0xF9, 0xEC, 0xE5, 0xED, b',', 0xF2, 0xE5, 0xEC, 0xED]), CsvOptions::default()));
    out.push(("western ordinals", raw(&[b'1', 0xBA, b',', b'2', 0xBA, b'\n', b'1', 0xAA, b',', b'2', 0xAA]), CsvOptions::default()));
    // A delimiter or a quote outside ASCII: the reader scans characters for those.
    out.push(("a section-sign delimiter", text("a§b§c\n1§2§3\n"), CsvOptions { delimiter: 0xA7, ..Default::default() }));
    out.push(("a section-sign delimiter, sniffed", text("a§b§c\n1§2§3\n"), sniffed.clone()));
    out.push(("a guillemet quote", text("«a,b«,2\n«multi\nline«,3"), CsvOptions { quote: 0xAB, ..Default::default() }));
    out.push(("an accented quote", text("éa,bé,2"), CsvOptions { quote: 0xE9, ..Default::default() }));
    // A field holding every other dialect's delimiter, and a very long one.
    out.push(("every dialect in one field", text("\"a,b;c\td\",\"line\r\nbreak\",\"quote \"\" and \"\" more\""), CsvOptions::default()));
    out.push(("a megabyte in one field", text(&format!("\"{}\",2", "x".repeat(1 << 20))), CsvOptions::default()));
    // A field as wide as the grid, and a row of endless delimiters.
    out.push(("a field as wide as the grid", text(&format!("{}x", ",".repeat(MAX_COLS as usize))), CsvOptions::default()));
    out.push(("a row of endless delimiters", text(&format!("{}x", ",".repeat(20_001))), CsvOptions::default()));
    // The real shape: 100k rows, one of them with a field of 100,000 bytes.
    out.push(("a 100k-row export", a_100k_row_export().into_bytes(), CsvOptions::default()));
    out.push(("a 100k-row export as text", a_100k_row_export().into_bytes(), as_text));
    out
}

#[test]
fn every_input_reads_cell_for_cell_like_the_old_reader() {
    for (name, bytes, opts) in comparison_inputs() {
        assert_reads_like_the_reference(name, &bytes, &opts);
    }
}

/// FNV-1a over a sheet's cells: position, value and number format, in row-major order.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Digest(0xcbf2_9ce4_8422_2325)
    }
    fn byte(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(0x100_0000_01b3);
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
    }
    fn u64(&mut self, n: u64) {
        self.bytes(&n.to_le_bytes());
    }
    /// The digest of a read sheet, so a 100k-row import can be pinned without a golden file.
    fn of(wb: &Workbook) -> u64 {
        let sh = wb.sheet(0).expect("a csv has one sheet");
        let mut d = Digest::new();
        for (c, cell) in sh.cells.iter() {
            d.u64(u64::from(c.row));
            d.u64(u64::from(c.col));
            match &cell.value {
                Value::Empty => d.byte(0),
                Value::Number(n) => {
                    d.byte(1);
                    d.u64(n.to_bits());
                }
                Value::Text(t) => {
                    d.byte(2);
                    d.u64(t.len() as u64);
                    d.bytes(t.as_bytes());
                }
                Value::Bool(b) => {
                    d.byte(3);
                    d.byte(u8::from(*b));
                }
                Value::Error(e) => {
                    d.byte(4);
                    d.bytes(e.as_str().as_bytes());
                }
                Value::Array(a) => {
                    d.byte(5);
                    d.u64(a.data.len() as u64);
                }
            }
            d.byte(6);
            d.bytes(wb.styles.get(cell.style).num_fmt.as_str().as_bytes());
        }
        d.byte(7);
        d.u64(sh.cells.len() as u64);
        d.0
    }
}

/// The sheet a 100k-row export read to with the reader as it was before the byte scanner and the
/// row batches ([`reference_read`]): neither may move a cell. Recorded on the old reader.
const DIGEST_OF_A_100K_ROW_EXPORT: u64 = 0x1973_5a93_ea86_5c1f;

#[test]
fn a_100k_row_export_reads_to_the_recorded_sheet() {
    let src = a_100k_row_export();
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(Digest::of(&wb), DIGEST_OF_A_100K_ROW_EXPORT);
    // The recorded value is the old reader's, not this one's: both have to agree with it.
    assert_eq!(Digest::of(&reference_read(src.as_bytes(), &CsvOptions::default())), DIGEST_OF_A_100K_ROW_EXPORT);
    assert_eq!(wb.sheet(0).unwrap().cells.len(), 100_001 * 6);
}
