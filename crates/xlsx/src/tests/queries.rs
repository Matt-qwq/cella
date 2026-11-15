//! Get & Transform queries persist in a part of our own.
//!
//! Excel keeps its own queries in a DataMashup blob that a clean-room implementation cannot read or
//! write, so GridCraft stores its queries in a part of its own. These tests hold the file format to
//! its promises: exact round trip (every step variant, both loads, non-ASCII), no part when the
//! model has none, and a lenient read of a part that is junk.

use std::io::Read;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Query, QueryAggregate, QueryColumnType, QueryFilterOp, QueryLoad, QuerySource, QueryStep, Workbook};

use crate::tests::{ROOT_RELS, make_zip};
use crate::{read_xlsx, write_xlsx};

/// The part every test here is about. Spelled out rather than taken from the module: the file
/// format's name is a promise, and a test that renames itself with the code proves nothing.
const PART: &str = "xl/gridcraft/queries.json";

fn part_names(bytes: &[u8]) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).collect()
}

fn part_text(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// Every variant of every enum, both loads, a non-empty column list: the round trip has to keep all
/// of it or the answer is "which step did we lose?".
fn queries() -> Vec<Query> {
    vec![
        Query {
            id: 1,
            name: "Sales".into(),
            source: QuerySource::Csv { path: Some("/data/sales.csv".into()), text: None, delimiter: ',' },
            steps: vec![
                QueryStep::PromoteHeaders,
                QueryStep::ChangeType { columns: vec!["Amount".into()], to: QueryColumnType::Number },
                QueryStep::Filter { column: "Region".into(), op: QueryFilterOp::Equals, value: Some("West".into()) },
                QueryStep::Sort { keys: vec![("Amount".into(), true), ("Region".into(), false)] },
            ],
            load: QueryLoad::Sheet { sheet: Some("Sales (2)".into()) },
            columns: vec!["Region".into(), "Amount".into()],
        },
        Query {
            id: 2,
            name: "Orders".into(),
            source: QuerySource::Xlsx { path: "book.xlsx".into(), sheet: Some("Orders".into()) },
            steps: vec![
                QueryStep::PromoteHeaders,
                QueryStep::RemoveColumns { columns: vec!["Internal".into()] },
                QueryStep::KeepColumns { columns: vec!["Region".into(), "Amount".into()] },
                QueryStep::RenameColumn { from: "Amount".into(), to: "Total".into() },
                QueryStep::RemoveDuplicates { columns: vec!["Region".into()] },
                QueryStep::ReplaceValues { column: "Region".into(), from: "W".into(), to: "West".into() },
                QueryStep::FillDown { column: "Region".into() },
                QueryStep::Unpivot { attributes: vec!["Region".into()], values: Vec::new() },
                QueryStep::GroupBy {
                    by: vec!["Region".into()],
                    aggregates: vec![(QueryAggregate::Sum, Some("Total".into()), "Sum".into()), (QueryAggregate::Count, None, "Rows".into())],
                },
                QueryStep::DemoteHeaders,
                QueryStep::Filter { column: "Region".into(), op: QueryFilterOp::IsNotBlank, value: None },
            ],
            load: QueryLoad::Model,
            columns: vec![],
        },
        Query {
            id: 3,
            name: "Orders (db)".into(),
            source: QuerySource::Sqlite { path: "/data/shop.db".into(), sql: "SELECT region, amount FROM orders WHERE amount > 10".into() },
            steps: vec![],
            load: QueryLoad::Sheet { sheet: None },
            columns: vec![],
        },
        Query {
            id: 4,
            name: "Rates".into(),
            source: QuerySource::Http { url: "https://example.invalid/rates.json?day=today".into(), format: gridcraft_model::HttpFormat::Json },
            steps: vec![QueryStep::PromoteHeaders],
            load: QueryLoad::Sheet { sheet: None },
            columns: vec![],
        },
    ]
}

#[test]
fn two_queries_survive_the_round_trip() {
    let wb = Workbook { queries: queries(), ..Default::default() };
    let bytes = write_xlsx(&wb).unwrap();
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.queries, queries());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Saving what was read gives the same queries again.
    let again = read_xlsx(&write_xlsx(&back).unwrap()).unwrap().0;
    assert_eq!(again.queries, queries());
}

#[test]
fn a_chinese_name_and_column_survive_byte_for_byte() {
    let mut q = queries();
    q[0].name = "销售数据".into();
    q[0].source = QuerySource::Csv { path: Some("C:/数据/销售.csv".into()), text: None, delimiter: '\t' };
    q[0].steps.push(QueryStep::RenameColumn { from: "金额".into(), to: "总金额".into() });
    q[0].steps.push(QueryStep::Filter { column: "地区".into(), op: QueryFilterOp::Contains, value: Some("华".into()) });
    q[0].columns = vec!["地区".into(), "总金额".into()];
    let wb = Workbook { queries: q.clone(), ..Default::default() };
    let bytes = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.queries, q);
}

#[test]
fn an_empty_query_list_writes_no_part_and_drops_a_stale_one() {
    let wb = Workbook { queries: queries(), ..Default::default() };
    let with = write_xlsx(&wb).unwrap();
    assert!(part_names(&with).iter().any(|n| n == PART), "{:?}", part_names(&with));
    assert!(part_text(&with, "[Content_Types].xml").contains("Extension=\"json\""));

    // The model is the truth: reading the file back and deleting the queries writes no part — the
    // pass-through carries only parts a relationship reaches, and nothing reaches this one.
    let (mut back, _) = read_xlsx(&with).unwrap();
    back.queries.clear();
    let without = write_xlsx(&back).unwrap();
    assert_eq!(part_names(&without).iter().filter(|n| n.as_str() == PART).count(), 0, "{:?}", part_names(&without));
    assert!(!part_text(&without, "[Content_Types].xml").contains("json"), "no extension is declared for a part that is not written");

    // A workbook that never had queries writes none either.
    let empty = write_xlsx(&Workbook::new()).unwrap();
    assert_eq!(part_names(&empty).iter().filter(|n| n.as_str() == PART).count(), 0);
}

/// A one-sheet package carrying the queries part, with its content type declared.
fn file_with_part(json: &[u8]) -> Vec<u8> {
    let types = concat!(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Default Extension="json" ContentType="application/json"/>"#,
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"</Types>"#,
    );
    let wb = concat!(
        r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    );
    let rels = concat!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>"#,
        r#"</Relationships>"#,
    );
    let sheet = concat!(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
        r#"<sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData></worksheet>"#,
    );
    make_zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        (PART, json),
    ])
}

#[test]
fn a_garbage_part_warns_and_the_rest_of_the_workbook_still_reads() {
    let cases: [&[u8]; 6] = [b"this is not JSON", b"", b"[{\"id\":\"one\"}]", b"{\"not\":\"a list\"}", b"[1,2,3]", b"[[]"];
    for json in cases {
        let (wb, report) = read_xlsx(&file_with_part(json)).unwrap();
        assert!(wb.queries.is_empty(), "{json:?} read as queries");
        // The workbook itself is fine: the sheet, its name and its value all came through.
        assert_eq!(wb.sheets.len(), 1);
        assert_eq!(wb.sheets[0].name, "Data");
        assert_eq!(wb.sheets[0].value(CellRef::new(0, 0)), Value::Number(7.0));
        let warning = match report.warnings.iter().find(|w| w.contains(PART)) {
            Some(w) => w,
            None => panic!("no warning names the part: {:?}", report.warnings),
        };
        assert!(warning.contains("no queries"), "{warning}");
    }
}

#[test]
fn an_unknown_variant_is_a_short_warning() {
    let long_name = "Z".repeat(4000);
    let json = format!(r#"[{{"id":1,"name":"q","source":{{"{long_name}":{{}}}},"steps":[],"load":"Model"}}]"#);
    let (wb, report) = read_xlsx(&file_with_part(json.as_bytes())).unwrap();
    assert!(wb.queries.is_empty());
    let warning = report.warnings.iter().find(|w| w.contains(PART)).unwrap();
    // The 4 KB of junk is shortened instead of becoming the report, and the cut says so.
    assert!(warning.len() < 400, "{} bytes: {warning}", warning.len());
    assert!(warning.starts_with(PART), "{warning}");
    assert!(warning.contains('\u{2026}'), "the cut is said out loud: {warning}");
}

#[test]
fn deeply_nested_junk_is_a_warning_not_a_stack_overflow() {
    let json = "[".repeat(100_000);
    let (wb, report) = read_xlsx(&file_with_part(json.as_bytes())).unwrap();
    assert!(wb.queries.is_empty());
    assert!(report.warnings.iter().any(|w| w.contains(PART)), "{:?}", report.warnings);
}

#[test]
fn a_part_over_the_cap_is_refused_with_a_warning() {
    // The declared size is checked before anything is decompressed, so no 33 MB has to exist.
    let file = crate::tests::declaring(file_with_part(b"[]"), PART, 33 * 1024 * 1024);
    let (wb, report) = read_xlsx(&file).unwrap();
    assert!(wb.queries.is_empty());
    let warning = report.warnings.iter().find(|w| w.contains(PART)).unwrap();
    assert!(warning.contains("larger than"), "{warning}");
    // The package was not refused whole: the sheet still read.
    assert_eq!(wb.sheets[0].value(CellRef::new(0, 0)), Value::Number(7.0));
}

#[test]
fn content_types_declares_json_once_and_only_with_the_part() {
    let wb = Workbook { queries: queries(), ..Default::default() };
    let bytes = write_xlsx(&wb).unwrap();
    let types = part_text(&bytes, "[Content_Types].xml");
    assert_eq!(types.matches(r#"Extension="json""#).count(), 1, "{types}");
    assert!(types.contains(r#"<Default Extension="json" ContentType="application/json"/>"#), "{types}");

    // A file that already declared the extension does not gain a second one: the input's default is
    // not carried (the package is written from the model, which brings its own) and the map holds
    // the extension once.
    let (back, _) = read_xlsx(&file_with_part(b"[]")).unwrap();
    let rewritten = write_xlsx(&Workbook { queries: queries(), ..back }).unwrap();
    let types = part_text(&rewritten, "[Content_Types].xml");
    assert_eq!(types.matches(r#"Extension="json""#).count(), 1, "{types}");
    crate::testing::check_package(&rewritten).expect("the package with the queries part is valid");
}

#[test]
fn every_source_kind_round_trips() {
    let sources = vec![
        QuerySource::Csv { path: None, text: Some("a,b\n1,2".into()), delimiter: ';' },
        QuerySource::Json { path: Some("/tmp/a.json".into()), text: None },
        QuerySource::Json { path: None, text: Some("[{\"a\":1}]".into()) },
        QuerySource::Xlsx { path: "/tmp/a.xlsx".into(), sheet: None },
        QuerySource::Parquet { path: "/tmp/a.parquet".into() },
        QuerySource::Folder { path: "/tmp/csv".into(), filter: "*.csv".into() },
        QuerySource::Table { sheet: "Data".into(), name: "Table1".into() },
        QuerySource::Range { sheet: "Data".into(), range: RangeRef::new(CellRef::new(0, 0), CellRef::new(8, 2)) },
        QuerySource::Query { name: "Raw".into() },
    ];
    let queries: Vec<Query> = sources
        .into_iter()
        .enumerate()
        .map(|(i, source)| Query {
            id: i as u32 + 1,
            name: format!("Query{}", i + 1),
            source,
            steps: vec![],
            load: if i % 2 == 0 { QueryLoad::Model } else { QueryLoad::Sheet { sheet: None } },
            columns: vec![],
        })
        .collect();
    let bytes = write_xlsx(&Workbook { queries: queries.clone(), ..Default::default() }).unwrap();
    assert_eq!(read_xlsx(&bytes).unwrap().0.queries, queries);
}

/// The reason the format is "the model's serde shape": the two agree in both directions, so the
/// file cannot drift from the model without this test failing.
#[test]
fn the_json_is_the_models_own_serde_shape() {
    let wb = Workbook { queries: queries(), ..Default::default() };
    let bytes = write_xlsx(&wb).unwrap();
    let json = part_text(&bytes, PART);
    assert_eq!(serde_json::from_str::<Vec<Query>>(&json).unwrap(), queries(), "our JSON is the model's serde shape");
    // And what serde writes reads back: the reader is not merely the writer's mirror image.
    let theirs = serde_json::to_vec(&queries()).unwrap();
    let (back, report) = read_xlsx(&file_with_part(&theirs)).unwrap();
    assert_eq!(back.queries, queries(), "{:?}", report.warnings);
}
