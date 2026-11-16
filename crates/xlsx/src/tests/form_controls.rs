//! Form controls: the three parts written for each control agree, they read back, a file's own
//! unmodelled controls stay kept beside the modelled ones, and odd values give valid controls.

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, AnchorMode, Comment, ControlAction, ControlKind, FormControl, MacroInput, Sheet, Workbook};

use super::minimal;
use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

fn anchor(cell: &str, width: f32, height: f32) -> Anchor {
    Anchor { cell: at(cell), dx: 4.0, dy: 3.0, width, height, mode: AnchorMode::MoveOnly }
}

fn note(author: &str, text: &str) -> Comment {
    Comment {
        author: author.into(),
        text: text.into(),
        replies: vec![],
        threaded: false,
        resolved: false,
        visible: false,
        dates: vec![],
        note_box: None,
    }
}

/// Four controls covering every kind, and a note, on one sheet with a second sheet the last
/// control links to.
fn book() -> Workbook {
    let mut wb = Workbook::default();
    wb.sheets.clear();
    let mut sh = Sheet::new("Data");
    sh.comments.insert(at("A1"), note("Me", "a note"));
    sh.controls.push(FormControl {
        id: 1,
        kind: ControlKind::Slider,
        anchor: anchor("C3", 150.0, 20.0),
        link: Some("$B$1".into()),
        min: 5,
        max: 500,
        step: 5,
        page: 50,
        value: 35,
        name: "Rate".into(),
        ..Default::default()
    });
    sh.controls.push(FormControl {
        id: 2,
        kind: ControlKind::Spin,
        anchor: Anchor { mode: AnchorMode::Absolute, ..anchor("F3", 20.0, 40.0) },
        link: Some("'Other Sheet'!$A$2".into()),
        max: 30_000,
        step: 100,
        value: 30_000,
        print: false,
        ..Default::default()
    });
    sh.controls.push(FormControl {
        id: 3,
        kind: ControlKind::Button,
        anchor: Anchor { mode: AnchorMode::MoveAndSize, ..anchor("C8", 100.0, 28.0) },
        text: "Run <it> & go".into(),
        action: Some(ControlAction::Macro { name: "Fill Down".into(), input: MacroInput::Selection, output: None }),
        locked: false,
        ..Default::default()
    });
    sh.controls.push(FormControl { id: 4, kind: ControlKind::Slider, anchor: anchor("H3", 20.0, 120.0), horizontal: false, ..Default::default() });
    wb.sheets.push(sh.into());
    wb.sheets.push(Sheet::new("Other Sheet").into());
    wb
}

fn text(pkg: &mut Package<'_>, part: &str) -> String {
    String::from_utf8(pkg.read(part).unwrap().unwrap_or_default()).unwrap_or_default()
}

/// The package's part names, lower-cased as the zip stores them.
fn part_names(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect()
}

#[test]
fn the_three_parts_agree_and_read_back() {
    let wb = book();
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = text(&mut pkg, "xl/worksheets/sheet1.xml");
    let vml = text(&mut pkg, "xl/drawings/vmlDrawing1.vml");
    let types = text(&mut pkg, "[Content_Types].xml");
    assert!(sheet.contains("<mc:Choice xmlns:x14=") && sheet.contains("<controls>"), "{sheet}");
    // Each control's shape id is a VML shape of the same kind; the note keeps its own id.
    let ids: Vec<String> = sheet.split("<control shapeId=\"").skip(1).filter_map(|s| s.split('"').next()).map(str::to_string).collect();
    assert_eq!(ids.len(), 8, "a Choice and a Fallback per control");
    for (i, kind) in ["Scroll", "Spin", "Button", "Scroll"].iter().enumerate() {
        let id = &ids[i * 2];
        let shape = vml.split(&format!("id=\"_x0000_s{id}\"")).nth(1).unwrap_or_else(|| panic!("no shape {id}"));
        assert!(shape.starts_with(" type=\"#_x0000_t201\""), "{shape}");
        assert!(shape.split("</v:shape>").next().unwrap_or_default().contains(&format!("ObjectType=\"{kind}\"")), "{id}");
    }
    assert!(vml.contains("id=\"_x0000_s1025\"") && vml.contains("ObjectType=\"Note\""), "the note is shape 1025");
    assert!(!ids.contains(&"1025".to_string()));
    for n in 1..=4 {
        assert!(types.contains(&format!("/xl/ctrlProps/ctrlProp{n}.xml")), "content type {n}");
    }
    let prop = text(&mut pkg, "xl/ctrlProps/ctrlProp1.xml");
    assert!(
        prop.contains("objectType=\"Scroll\"") && prop.contains("fmlaLink=\"$B$1\"") && prop.contains("page=\"50\"") && prop.contains("horiz=\"1\""),
        "{prop}"
    );
    assert!(sheet.contains("macro=\"[0]!Fill Down\""));

    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.contains("form controls are not shown")), "{:?}", report.warnings);
    let got = &back.sheet(0).unwrap().controls;
    assert_eq!(got.len(), 4);
    for (a, b) in wb.sheet(0).unwrap().controls.iter().zip(got) {
        let same = |x: &FormControl| {
            (x.kind, x.link.clone(), x.min, x.max, x.step, x.value, x.horizontal, x.print, x.locked, x.action.clone(), x.anchor.cell, x.anchor.mode)
        };
        assert_eq!(same(a), same(b));
        assert!(
            (a.anchor.width - b.anchor.width).abs() < 1.0
                && (a.anchor.height - b.anchor.height).abs() < 1.0
                && (a.anchor.dx - b.anchor.dx).abs() < 1.0
        );
        if a.kind == ControlKind::Slider {
            assert_eq!(a.page, b.page);
        }
    }
    assert_eq!(got[0].name, "Rate");
    assert_eq!(got[2].text, "Run <it> & go");
    assert_eq!(back.sheet(0).unwrap().comments.len(), 1, "notes still read");
}

/// A saved workbook with controls opens and saves again identically: one `<controls>` element, one
/// VML shape per control, and no kept copy of anything the model holds.
#[test]
fn a_saved_workbook_with_controls_round_trips_twice() {
    let mut wb = book();
    wb.sheet_mut(0).unwrap().comments.clear();
    let once = write_xlsx(&wb).unwrap();
    let (back, report) = read_xlsx(&once).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let twice = write_xlsx(&back).unwrap();
    let mut pkg = Package::open(&twice).unwrap();
    let sheet = text(&mut pkg, "xl/worksheets/sheet1.xml");
    assert_eq!(sheet.matches("<controls").count(), 1, "one controls element: {sheet}");
    assert_eq!(sheet.matches("<legacyDrawing ").count(), 1, "{sheet}");
    assert_eq!(sheet.matches("<control shapeId=").count(), 8, "{sheet}");
    let vml = text(&mut pkg, "xl/drawings/vmlDrawing1.vml");
    assert_eq!(vml.matches("<v:shape ").count(), 4, "one shape per control: {vml}");
    let rels = text(&mut pkg, "xl/worksheets/_rels/sheet1.xml.rels");
    assert_eq!(rels.matches("/ctrlProp\"").count(), 4, "{rels}");
    let parts: Vec<String> = part_names(&twice).into_iter().filter(|n| n.to_ascii_lowercase().starts_with("xl/ctrlprops/")).collect();
    assert_eq!(parts.len(), 4, "{parts:?}");
    let (again, _) = read_xlsx(&twice).unwrap();
    assert_eq!(again.sheet(0).unwrap().controls.len(), 4);
}

const VML_ONLY: &str = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel">
<v:shape id="_x0000_s1025" type="#_x0000_t201"><x:ClientData ObjectType="Scroll"><x:Anchor>2, 0, 3, 0, 5, 0, 4, 0</x:Anchor><x:FmlaLink>$B$1</x:FmlaLink><x:Val>50000</x:Val><x:Min>-5</x:Min><x:Max>99999</x:Max><x:Inc>0</x:Inc><x:Page>-3</x:Page><x:Horiz/></x:ClientData></v:shape>
<v:shape id="_x0000_s1026" type="#_x0000_t201"><x:ClientData ObjectType="Spin"><x:Anchor>1, 0, 1, 0, 1, 20, 3, 0</x:Anchor><x:Val>7</x:Val><x:Min>9</x:Min><x:Max>9</x:Max></x:ClientData></v:shape>
<v:shape id="_x0000_s1027" o:button="t"><v:textbox><div>Go</div></v:textbox><x:ClientData ObjectType="Button"><x:Anchor>1, 0, 6, 0, 3, 0, 7, 0</x:Anchor><x:FmlaMacro>[0]!Tidy</x:FmlaMacro></x:ClientData></v:shape>
<v:shape id="_x0000_s1028"><x:ClientData ObjectType="Drop"><x:Anchor>1, 0, 1, 0, 2, 0, 2, 0</x:Anchor></x:ClientData></v:shape>
<v:shape id="_x0000_s1029"><x:ClientData ObjectType="Scroll"><x:Anchor>junk</x:Anchor></x:ClientData></v:shape>
</xml>"##;

/// A file with no `<controls>` element and no properties parts: the VML drawing alone names the
/// controls, and the numbers it gives that no control can take are repaired.
#[test]
fn vml_only_controls_are_read_and_clamped() {
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><legacyDrawing r:id="rId9"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/drawings/vmlDrawing1.vml", VML_ONLY)],
        "",
        "",
    );
    let (wb, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.iter().all(|w| !w.contains("were dropped")), "{:?}", report.warnings);
    let c = &wb.sheet(0).unwrap().controls;
    assert_eq!(c.len(), 3, "the drop-down and the shape without an anchor are skipped");
    assert_eq!((c[0].kind, c[0].min, c[0].max, c[0].step, c[0].value, c[0].horizontal), (ControlKind::Slider, 0, 30_000, 1, 30_000, true));
    assert!(c[0].page >= c[0].step && c[0].check_numbers().is_ok());
    assert_eq!((c[1].min, c[1].max), (9, 10), "a range with no width gets one");
    assert!(c[1].check_numbers().is_ok());
    assert_eq!(c[2].text, "Go");
    assert_eq!(c[2].action, Some(ControlAction::Macro { name: "Tidy".into(), input: MacroInput::Selection, output: None }));
    // Saved again, each is written once and nothing of them is kept twice.
    let out = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&out).unwrap();
    assert_eq!(back.sheet(0).unwrap().controls.len(), 3);
}

/// One `mc:AlternateContent` holds a scroll bar, which is modelled, and a list box, which stays with
/// pass-through: only the scroll bar leaves the kept content, and the list box keeps its worksheet
/// entry, its VML shape and its properties part through a save.
#[test]
fn an_unsupported_control_beside_a_modelled_one_is_kept() {
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing" Target="../drawings/vmlDrawing1.vml"/><Relationship Id="rIdA" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/ctrlProp" Target="../ctrlProps/a.xml"/><Relationship Id="rIdB" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/ctrlProp" Target="../ctrlProps/b.xml"/></Relationships>"#;
    let prop_a = r#"<?xml version="1.0"?><formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="Scroll" fmlaLink="$B$1" min="0" max="100" inc="1" page="10" val="40"/>"#;
    let prop_b = r#"<?xml version="1.0"?><formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="List" fmlaRange="$D$1:$D$3" sel="2"/>"#;
    let vml = r##"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office" xmlns:x="urn:schemas-microsoft-com:office:excel"><v:shape id="_x0000_s1025" type="#_x0000_t201"><x:ClientData ObjectType="Scroll"><x:Anchor>2, 0, 3, 0, 5, 0, 4, 0</x:Anchor><x:FmlaLink>$B$1</x:FmlaLink><x:Val>40</x:Val><x:Min>0</x:Min><x:Max>100</x:Max><x:Inc>1</x:Inc><x:Page>10</x:Page></x:ClientData></v:shape><v:shape id="_x0000_s1026" type="#_x0000_t201"><x:ClientData ObjectType="List"><x:Anchor>2, 0, 6, 0, 5, 0, 9, 0</x:Anchor><x:FmlaRange>$D$1:$D$3</x:FmlaRange><x:Sel>2</x:Sel></x:ClientData></v:shape></xml>"##;
    let anchor = |from: u32, to: u32| {
        format!(
            r#"<controlPr><anchor><from><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{from}</xdr:row><xdr:rowOff>0</xdr:rowOff></from><to><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{to}</xdr:row><xdr:rowOff>0</xdr:rowOff></to></anchor></controlPr>"#
        )
    };
    let controls = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="x14" xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"><controls><mc:AlternateContent><mc:Choice Requires="x14"><control shapeId="1025" r:id="rIdA" name="Scroll 1">{}</control><control shapeId="1026" r:id="rIdB" name="List 1">{}</control></mc:Choice><mc:Fallback><control shapeId="1025" r:id="rIdA" name="Scroll 1"/><control shapeId="1026" r:id="rIdB" name="List 1"/></mc:Fallback></mc:AlternateContent></controls></mc:Choice></mc:AlternateContent>"#,
        anchor(3, 4),
        anchor(6, 9)
    );
    let bytes = minimal(
        &format!(r#"<sheetData/><legacyDrawing r:id="rId9"/>{controls}"#),
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", rels),
            ("xl/ctrlProps/a.xml", prop_a),
            ("xl/ctrlProps/b.xml", prop_b),
            ("xl/drawings/vmlDrawing1.vml", vml),
        ],
        "",
        "",
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.controls.len(), 1, "the scroll bar is modelled, the list box is not");
    assert_eq!((sh.controls[0].kind, sh.controls[0].name.as_str()), (ControlKind::Slider, "Scroll 1"));
    let kept = sh.preserved.as_ref().and_then(|k| k.elements.iter().find(|e| e.slot == "controls")).map(|e| e.xml.clone()).unwrap_or_default();
    assert!(kept.contains("List 1") && kept.contains("<mc:Fallback>"), "the list box stays, in its wrapper: {kept}");
    assert!(!kept.contains("Scroll 1") && !kept.contains("shapeId=\"1025\""), "the scroll bar leaves every branch: {kept}");
    let out = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&out).unwrap();
    let sheet = text(&mut pkg, "xl/worksheets/sheet1.xml");
    assert_eq!(sheet.matches("<controls").count(), 1, "{sheet}");
    assert_eq!(sheet.matches("name=\"Scroll 1\"").count(), 2, "the modelled control, in its Choice and Fallback: {sheet}");
    assert_eq!(sheet.matches("name=\"List 1\"").count(), 2, "the kept control, in its Choice and Fallback: {sheet}");
    let vml_out = text(&mut pkg, "xl/drawings/vmlDrawing1.vml");
    assert!(vml_out.contains("_x0000_s1026") && vml_out.contains("ObjectType=\"List\""), "the list box's shape is kept: {vml_out}");
    assert_eq!(vml_out.matches("ObjectType=\"Scroll\"").count(), 1, "{vml_out}");
    let props: Vec<String> = part_names(&out).into_iter().filter(|n| n.to_ascii_lowercase().starts_with("xl/ctrlprops/")).collect();
    assert_eq!(props.len(), 2, "ours and the kept one: {props:?}");
    let (back, _) = read_xlsx(&out).unwrap();
    let kept = back
        .sheet(0)
        .unwrap()
        .preserved
        .as_ref()
        .and_then(|k| k.elements.iter().find(|e| e.slot == "controls"))
        .map(|e| e.xml.clone())
        .unwrap_or_default();
    assert!(kept.contains("List 1") && !kept.contains("Scroll 1"), "{kept}");
}

/// A hostile sheet: more controls than a sheet keeps, properties parts that are not XML or hold
/// numbers no control can take, shape ids that are not numbers, and oversized names and links.
/// Reading must not panic, must keep at most the cap, and every control it keeps must pass the
/// model's rules.
#[test]
fn hostile_form_controls_are_capped_and_repaired() {
    let n = 13_000;
    let mut controls = String::from("<controls>");
    let anchor = r#"<controlPr><anchor><from><xdr:col>1</xdr:col><xdr:colOff>-99999999999</xdr:colOff><xdr:row>99999999999</xdr:row><xdr:rowOff>0</xdr:rowOff></from><to><xdr:col>-4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></to></anchor></controlPr>"#;
    for i in 0..n {
        let rid = match i % 10 {
            0 => "rIdB",
            1 => "rIdC",
            _ => "rIdA",
        };
        let _ = std::fmt::Write::write_fmt(
            &mut controls,
            format_args!(r#"<control shapeId="{}" r:id="{rid}" name="{}">{anchor}</control>"#, 2000 + i, "n".repeat(400)),
        );
    }
    controls
        .push_str(r#"<control shapeId="99999999999999999999999" r:id="rIdA"/><control shapeId="x" r:id="rIdA"/><control r:id="rIdA"/></controls>"#);
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdA" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/a.xml"/><Relationship Id="rIdB" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/b.xml"/><Relationship Id="rIdC" Type="http://schemas.microsoft.com/office/2006/relationships/ctrlProp" Target="../ctrlProps/missing.xml"/></Relationships>"#;
    let link = "$B$".to_string() + &"9".repeat(500);
    let prop_a = format!(
        r#"<?xml version="1.0"?><formControlPr xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" objectType="Scroll" fmlaLink="{link}" min="1e400" max="NaN" inc="-inf" page="18446744073709551616" val="-7"/>"#
    );
    let body = format!(r#"<sheetData/>{controls}"#)
        .replace("<controls>", r#"<controls xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing">"#);
    let bytes = minimal(
        &body,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/ctrlProps/a.xml", &prop_a), ("xl/ctrlProps/b.xml", "not xml <<<")],
        "",
        "",
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    let c = &wb.sheet(0).unwrap().controls;
    assert_eq!(c.len(), 10_000, "the cap of one sheet");
    for x in c {
        assert_eq!(x.kind, ControlKind::Slider);
        assert!(x.check_numbers().is_ok(), "{x:?}");
        assert!(x.value >= x.min && x.value <= x.max, "{x:?}");
        assert!(x.name.chars().count() <= 255 && x.link.is_none(), "{x:?}");
        assert!(x.anchor.width >= 1.0 && x.anchor.height >= 1.0 && x.anchor.cell.row < gridcraft_core::MAX_ROWS, "{x:?}");
    }
    // Written back and read again, the kept controls stay valid.
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(0).unwrap().controls.len(), c.len());
}

/// The sheet relates each control properties part with the relationship type MS-XLSX defines and
/// Excel writes (in the officeDocument namespace); Excel finds the part by that type.
#[test]
fn the_properties_parts_use_the_ctrl_prop_relationship_type() {
    let bytes = write_xlsx(&book()).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let rels = text(&mut pkg, "xl/worksheets/_rels/sheet1.xml.rels");
    let types: Vec<&str> = rels.split("Type=\"").skip(1).filter_map(|r| r.split('"').next()).filter(|t| t.ends_with("/ctrlProp")).collect();
    assert_eq!(types.len(), 4, "{rels}");
    assert!(types.iter().all(|t| *t == "http://schemas.openxmlformats.org/officeDocument/2006/relationships/ctrlProp"), "{types:?}");
}
