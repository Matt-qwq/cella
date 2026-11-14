//! The part our model PivotTables live in: written when a workbook has one, read back whole, and
//! absent when it has none.

use gridcraft_core::Value;
use gridcraft_model::{ModelTable, PivotSourceKind, PivotTable, Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn model_workbook() -> Workbook {
    let mut wb = Workbook::new();
    wb.sheets[0] = std::sync::Arc::new(Sheet::new("Data"));
    wb.model.tables.push(ModelTable {
        name: "Sales".into(),
        columns: vec!["Amount".into()],
        rows: vec![vec![Value::number(10.0)]],
        source_query: None,
    });
    let mut model_pt = PivotTable { name: "ModelOne".into(), source: "Sales".into(), ..PivotTable::default() };
    model_pt.source_kind = PivotSourceKind::DataModel;
    let mut range_pt = PivotTable { name: "RangeOne".into(), source: "Data!A1:B9".into(), ..PivotTable::default() };
    range_pt.read_only_reason = Some("carried".into());
    let sh = std::sync::Arc::make_mut(&mut wb.sheets[0]);
    sh.pivots.push(model_pt);
    sh.pivots.push(range_pt);
    wb
}

#[test]
fn a_model_pivot_is_saved_in_our_own_part_and_comes_back() {
    let wb = model_workbook();
    let bytes = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).expect("sheet");
    let restored = sh.pivots.iter().find(|p| p.name == "ModelOne").expect("the model report came back");
    assert_eq!(restored.source, "Sales");
    assert_eq!(restored.source_kind, PivotSourceKind::DataModel);
    assert_eq!(restored.read_only_reason, None, "a computed report has no reason to be read-only");
    assert!(back.model.tables.iter().any(|t| t.name == "Sales"), "and its table came back too");
}

#[test]
fn a_workbook_without_model_pivots_writes_no_pivot_part() {
    let wb = Workbook::new();
    let bytes = write_xlsx(&wb).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(zip.by_name(crate::model_pivots::PART).is_err(), "no model report, no part");
}

#[test]
fn a_carried_report_is_not_written_into_our_part() {
    // A report read from a file has its own parts and a reason; writing it here as well would
    // restore it twice.
    let mut wb = Workbook::new();
    wb.sheets[0] = std::sync::Arc::new(Sheet::new("Data"));
    let mut carried = PivotTable { name: "Carried".into(), source: "Sales".into(), ..PivotTable::default() };
    carried.source_kind = PivotSourceKind::DataModel;
    carried.read_only_reason = Some("built on the workbook's data model".into());
    std::sync::Arc::make_mut(&mut wb.sheets[0]).pivots.push(carried);
    let bytes = write_xlsx(&wb).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(zip.by_name(crate::model_pivots::PART).is_err(), "a carried report is not ours to keep");
}

#[test]
fn a_broken_part_is_a_warning_and_the_rest_of_the_file_reads() {
    let wb = Workbook::new();
    let bytes = write_xlsx(&wb).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        parts.push((f.name().to_string(), {
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut b).unwrap();
            b
        }));
    }
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        let opts = zip::write::SimpleFileOptions::default();
        out.start_file(name, opts).unwrap();
        std::io::Write::write_all(&mut out, &bytes).unwrap();
    }
    out.start_file(crate::model_pivots::PART, zip::write::SimpleFileOptions::default()).unwrap();
    std::io::Write::write_all(&mut out, b"{not json").unwrap();
    let bytes = out.finish().unwrap().into_inner();
    let (wb, report) = read_xlsx(&bytes).expect("the workbook still opens");
    assert_eq!(wb.sheets.len(), 1);
    assert!(report.warnings.iter().any(|w| w.contains(crate::model_pivots::PART)), "{:?}", report.warnings);
}
