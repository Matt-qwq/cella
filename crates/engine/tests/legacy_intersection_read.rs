//! A .xlsx written before dynamic arrays (no `cm` on its formulas) is evaluated the way Excel
//! evaluates it: the range a single-cell formula names is intersected with the formula's row or
//! column. A dynamic-array formula in the same file still spills. Both come out of one open.

use std::io::Write;

use cella_engine::core::{CellRef, Value};
use cella_engine::io;

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

/// A one-sheet package whose worksheet part is `body`.
fn book(body: &str) -> Vec<u8> {
    const CT: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>"#;
    const ROOT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    const WB: &str = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    const WB_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{body}</worksheet>"#
    );
    let parts: [(&str, &[u8]); 5] = [
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", WB.as_bytes()),
        ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ];
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// A1:A3 = 1, 2, 3. B2 is a legacy formula over the range (`=A1:A3*2`, as a 2003 file saves it);
/// D2 is the same formula marked dynamic (`cm`) and spilling into D2:D4; F1 sums the range.
fn legacy_and_dynamic() -> Vec<u8> {
    let body = r#"<sheetData>
      <row r="1"><c r="A1"><v>1</v></c><c r="F1" cm="1"><f>SUM(A1:A3)</f><v>6</v></c><c r="H1"><f>IFERROR(__xludf.DUMMYFUNCTION("SUM(A1:A3)"),6)</f><v>6</v></c></row>
      <row r="2"><c r="A2"><v>2</v></c><c r="B2"><f>A1:A3*2</f><v>4</v></c><c r="D2" cm="1"><f t="array" ref="D2:D4">A1:A3*2</f><v>2</v></c></row>
      <row r="3"><c r="A3"><v>3</v></c><c r="D3"><v>4</v></c></row>
      <row r="4"><c r="D4"><v>6</v></c></row>
    </sheetData>"#;
    book(body)
}

#[test]
fn a_legacy_formula_is_intersected_and_a_dynamic_one_still_spills() {
    let (wb, _) = io::open_bytes("legacy.xlsx", &legacy_and_dynamic()).unwrap();
    let sheet = wb.sheet(0).unwrap();
    // The legacy formula reads as the intersected one and evaluates at its own row.
    let b2 = sheet.cell(at("B2")).unwrap();
    assert_eq!(b2.formula.as_ref().unwrap().text_at(at("B2")).as_deref(), Some("@A1:A3*2"));
    assert_eq!(b2.value, Value::Number(4.0));
    // The dynamic-array formula in the same file is untouched and spills over the range it named.
    let d2 = sheet.cell(at("D2")).unwrap();
    assert_eq!(d2.formula.as_ref().unwrap().text_at(at("D2")).as_deref(), Some("A1:A3*2"));
    assert_eq!(sheet.value(at("D2")), Value::Number(2.0));
    assert_eq!(sheet.value(at("D3")), Value::Number(4.0));
    assert_eq!(sheet.value(at("D4")), Value::Number(6.0));
    // An ordinary dynamic-array formula is read and evaluated as before.
    assert_eq!(sheet.value(at("F1")), Value::Number(6.0));
    // Google Sheets' export wrapper is unwrapped by the engine's own function list, and the
    // unwrapped formula is dynamic (it must not be intersected as a legacy one).
    let h1 = sheet.cell(at("H1")).unwrap();
    let f = h1.formula.as_ref().unwrap();
    assert_eq!(f.text_at(at("H1")).as_deref(), Some("SUM(A1:A3)"));
    assert!(f.dynamic);
    assert_eq!(h1.value, Value::Number(6.0));
}
