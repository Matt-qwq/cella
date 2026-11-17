//! Settings Excel writes that are not cell data but must survive an open + save: the workbook
//! window's display options (`workbookView`), filter privacy, the R1C1 reference style, the
//! outline's summary placement and the symbol buttons, and a cell format's reading order.

use std::sync::Arc;

use cella_core::CellRef;
use cella_model::{Alignment, ReadingOrder, Sheet, Style, StyleId, Workbook, WorkbookView};

use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

/// One part of a saved package, as text.
fn part(bytes: &[u8], name: &str) -> String {
    let mut pkg = Package::open(bytes).expect("package");
    let data = pkg.read(name).expect("read part").unwrap_or_default();
    String::from_utf8(data).expect("utf-8 part")
}

fn round_trip(wb: &Workbook) -> Workbook {
    let saved = write_xlsx(wb).expect("save");
    read_xlsx(&saved).expect("reopen").0
}

/// The window's display settings (File › Options › Advanced, "Display options for this workbook"):
/// a workbook Excel saved with the tabs or a bar hidden keeps it hidden.
#[test]
fn workbook_view_display_settings_survive_a_round_trip() {
    let mut wb = Workbook::new();
    wb.view = WorkbookView { show_sheet_tabs: false, show_horizontal_scroll: false, show_vertical_scroll: true, tab_ratio: 850 };
    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/workbook.xml");
    assert!(
        xml.contains(r#"<workbookView activeTab="0" showSheetTabs="0" showHorizontalScroll="0" tabRatio="850"/>"#),
        "the window's settings are written: {xml}"
    );
    assert_eq!(round_trip(&wb).view, wb.view);

    // The other way round: hidden vertical bar, tabs shown, a narrow tab share.
    let mut wb = Workbook::new();
    wb.view = WorkbookView { show_sheet_tabs: true, show_horizontal_scroll: true, show_vertical_scroll: false, tab_ratio: 0 };
    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/workbook.xml");
    assert!(xml.contains(r#"<workbookView activeTab="0" showVerticalScroll="0" tabRatio="0"/>"#), "{xml}");
    assert_eq!(round_trip(&wb).view, wb.view);
}

/// Excel writes all four attributes even at their defaults, and `tabRatio="600"` must come back as
/// 600 (the format's default) rather than being dropped or reset.
#[test]
fn an_excels_workbook_view_is_read_as_written() {
    let view = r#"<bookViews><workbookView xWindow="0" yWindow="0" windowWidth="28800" windowHeight="12000" tabRatio="600" activeTab="0" showSheetTabs="0" showHorizontalScroll="1" showVerticalScroll="0"/></bookViews>"#;
    let file = super::minimal("", &[], "", view);
    let (wb, report) = read_xlsx(&file).expect("read");
    assert!(report.warnings.iter().all(|w| w.contains("no styles part")), "{:?}", report.warnings);
    assert_eq!(wb.view.tab_ratio, 600);
    assert!(!wb.view.show_sheet_tabs);
    assert!(wb.view.show_horizontal_scroll);
    assert!(!wb.view.show_vertical_scroll, "a hidden vertical bar is read, not defaulted");

    // Saving keeps the settings (the default tab share is simply not written again).
    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/workbook.xml");
    assert!(xml.contains(r#"showSheetTabs="0" showVerticalScroll="0"/>"#), "{xml}");
    assert!(!xml.contains("tabRatio"), "600 is the default and needs no attribute: {xml}");
    assert_eq!(read_xlsx(&saved).expect("reopen").0.view, wb.view);
}

/// Trust Center › Privacy Options › "Remove personal information from file properties on save"
/// (`workbookPr filterPrivacy`).
#[test]
fn filter_privacy_survives_a_round_trip() {
    let mut wb = Workbook::new();
    wb.remove_personal_info = true;
    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/workbook.xml");
    assert!(xml.contains("<workbookPr filterPrivacy=\"1\"/>"), "{xml}");
    assert!(round_trip(&wb).remove_personal_info);
}

/// File › Options › Formulas › "R1C1 reference style" (`calcPr refMode`).
#[test]
fn r1c1_reference_style_survives_a_round_trip() {
    let mut wb = Workbook::new();
    wb.calc.r1c1 = true;
    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/workbook.xml");
    assert!(xml.contains(r#"refMode="R1C1""#), "{xml}");
    assert!(round_trip(&wb).calc.r1c1, "the reference style is kept, so Excel opens it in R1C1");

    // A workbook in A1 writes no refMode and reads back as A1.
    let plain = write_xlsx(&Workbook::new()).expect("save");
    let xml = part(&plain, "xl/workbook.xml");
    assert!(!xml.contains("refMode"), "{xml}");
    assert!(!read_xlsx(&plain).expect("reopen").0.calc.r1c1);
}

/// Data ▸ Outline ▸ Settings (`sheetPr outlinePr`) and Show Outline Symbols (Ctrl+8,
/// `sheetView showOutlineSymbols`).
#[test]
fn outline_settings_survive_a_round_trip() {
    let mut sheet = Sheet::new("Sheet1");
    sheet.summary_below = false;
    sheet.summary_right = true;
    sheet.show_outline_symbols = false;
    let mut wb = Workbook::new();
    wb.sheets = vec![Arc::new(sheet)];

    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/worksheets/sheet1.xml");
    assert!(xml.contains(r#"<sheetPr><outlinePr summaryBelow="0"/></sheetPr>"#), "{xml}");
    assert!(xml.contains(r#"showOutlineSymbols="0""#), "{xml}");

    let back = round_trip(&wb);
    let sheet = back.sheet(0).expect("one sheet");
    assert!(!sheet.summary_below);
    assert!(sheet.summary_right);
    assert!(!sheet.show_outline_symbols);

    // Summary columns left of their detail: the other flag alone.
    let mut sheet = Sheet::new("Sheet1");
    sheet.summary_right = false;
    let mut wb = Workbook::new();
    wb.sheets = vec![Arc::new(sheet)];
    let saved = write_xlsx(&wb).expect("save");
    let xml = part(&saved, "xl/worksheets/sheet1.xml");
    assert!(xml.contains(r#"<outlinePr summaryRight="0"/>"#), "{xml}");
    let back = round_trip(&wb);
    let sheet = back.sheet(0).expect("one sheet");
    assert!(sheet.summary_below);
    assert!(!sheet.summary_right);
    assert!(sheet.show_outline_symbols, "outline symbols stay on unless hidden");
}

/// A cell format's `readingOrder` (`ST_ReadingOrder`): 0 context, 1 left to right, 2 right to left.
/// Anything else says nothing.
#[test]
fn reading_order_survives_a_round_trip() {
    let file = |attribute: &str| {
        let styles = format!(
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment horizontal="right"{attribute}/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
        );
        let rels =
            r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
        let body = r#"<sheetData><row r="1"><c r="A1" s="1"><v>1</v></c></row></sheetData>"#;
        super::minimal(body, &[("xl/styles.xml", &styles)], rels, "")
    };
    let order_of = |wb: &Workbook| -> Option<ReadingOrder> {
        let sheet = wb.sheet(0).expect("one sheet");
        wb.styles.get(sheet.style_id(CellRef::new(0, 0))).align.reading_order
    };

    for (attribute, want) in [
        ("", None),
        (r#" readingOrder="0""#, Some(ReadingOrder::Context)),
        (r#" readingOrder="1""#, Some(ReadingOrder::LeftToRight)),
        (r#" readingOrder="2""#, Some(ReadingOrder::RightToLeft)),
        // Not a value the schema defines: no reading order, not a guess.
        (r#" readingOrder="9""#, None),
    ] {
        let (wb, _) = read_xlsx(&file(attribute)).expect("read");
        assert_eq!(order_of(&wb), want, "reading {attribute:?}");
        let back = round_trip(&wb);
        assert_eq!(order_of(&back), want, "round trip of {attribute:?}");
        if want.is_none() {
            let xml = part(&write_xlsx(&back).expect("save"), "xl/styles.xml");
            assert!(!xml.contains("readingOrder"), "{attribute:?} left no readingOrder: {xml}");
        }
    }
    // The written value is Excel's own spelling.
    let (wb, _) = read_xlsx(&file(r#" readingOrder="2""#)).expect("read");
    let xml = part(&write_xlsx(&wb).expect("save"), "xl/styles.xml");
    assert!(xml.contains(r#"<alignment horizontal="right" readingOrder="2"/>"#), "{xml}");

    // A named cell style carries it too, like any other part of a format.
    let mut wb = Workbook::new();
    let rtl = Style { align: Alignment { reading_order: Some(ReadingOrder::LeftToRight), ..Alignment::default() }, ..Style::default() };
    wb.cell_styles.push(("Reading".into(), rtl));
    let back = round_trip(&wb);
    let (name, style) = back.cell_styles.first().expect("the named style");
    assert_eq!(name, "Reading");
    assert_eq!(style.align.reading_order, Some(ReadingOrder::LeftToRight));

    // The default format needs no reading order, and old JSON without the field still loads.
    assert_eq!(Style::default().align.reading_order, None);
    let mut value: serde_json::Value = serde_json::from_str(&serde_json::to_string(&Style::default()).expect("json")).expect("value");
    let align = value.get_mut("align").and_then(serde_json::Value::as_object_mut).expect("an alignment object");
    align.remove("reading_order");
    let older: Style = serde_json::from_value(value).expect("older JSON");
    assert_eq!(older.align.reading_order, None);
}

/// A file that says nothing about any of this loads with exactly the defaults we have today, and
/// saving it does not invent attributes.
#[test]
fn a_file_without_these_settings_keeps_the_current_defaults() {
    // No workbookPr, bookViews, calcPr, sheetPr or styles at all.
    let (wb, report) = read_xlsx(&super::minimal("<sheetData/>", &[], "", "")).expect("read");
    assert!(report.warnings.iter().all(|w| w.contains("no styles part")), "{:?}", report.warnings);
    assert_eq!(wb.view, WorkbookView::default());
    assert_eq!(wb.view.tab_ratio, 600);
    assert!(wb.view.show_sheet_tabs && wb.view.show_horizontal_scroll && wb.view.show_vertical_scroll);
    assert!(!wb.remove_personal_info);
    assert!(!wb.calc.r1c1);
    let sheet = wb.sheet(0).expect("one sheet");
    assert!(sheet.summary_below);
    assert!(sheet.summary_right);
    assert!(sheet.show_outline_symbols);

    // What a brand-new workbook writes: none of the attributes, and they come back as defaults.
    let saved = write_xlsx(&Workbook::new()).expect("save");
    let workbook_xml = part(&saved, "xl/workbook.xml");
    for attribute in ["filterPrivacy", "refMode", "tabRatio", "showSheetTabs", "showHorizontalScroll", "showVerticalScroll"] {
        assert!(!workbook_xml.contains(attribute), "{attribute} is only written when set: {workbook_xml}");
    }
    let sheet_xml = part(&saved, "xl/worksheets/sheet1.xml");
    assert!(!sheet_xml.contains("outlinePr") && !sheet_xml.contains("showOutlineSymbols"), "{sheet_xml}");
    assert!(!part(&saved, "xl/styles.xml").contains("readingOrder"));

    let back = read_xlsx(&saved).expect("reopen").0;
    assert_eq!(back.view, WorkbookView::default());
    assert!(!back.remove_personal_info);
    assert!(!back.calc.r1c1);
    let sheet = back.sheet(0).expect("one sheet");
    assert!(sheet.summary_below && sheet.summary_right && sheet.show_outline_symbols);
    assert_eq!(back.styles.get(StyleId::DEFAULT).align.reading_order, None);
}
