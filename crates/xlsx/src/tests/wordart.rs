//! A WordArt is a shape whose drawing part says so: a text body with the non-warping text warp
//! 'textNoShape' on an unfilled rectangle. It round-trips as a WordArt, not as a text box.

use cella_model::{Color, ShapeKind};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

/// A drawing part with two shapes: a WordArt (text warp, no fill, no outline, shadow and outline on
/// the run) and an ordinary filled rectangle that happens to carry the same warp.
fn drawing() -> String {
    r##"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>3</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="2" name="Shape 1"/><xdr:cNvSpPr/></xdr:nvSpPr>
<xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="571500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln><a:noFill/></a:ln></xdr:spPr>
<xdr:txBody><a:bodyPr wrap="square" anchor="ctr"><a:prstTxWarp prst="textNoShape"><a:avLst/></a:prstTxWarp></a:bodyPr><a:lstStyle/>
<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="4000" b="1"><a:ln w="31750"><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></a:ln><a:solidFill><a:srgbClr val="1B2A49"/></a:solidFill><a:effectLst><a:outerShdw blurRad="38100" dist="38100" dir="2700000" algn="tl" rotWithShape="0"><a:srgbClr val="8A94A6"/></a:outerShdw></a:effectLst><a:latin typeface="Arial"/></a:rPr><a:t>Hello</a:t></a:r></a:p>
<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="4000" b="1"/><a:t>World</a:t></a:r></a:p>
</xdr:txBody></xdr:sp><xdr:clientData/></xdr:twoCellAnchor>
<xdr:twoCellAnchor><xdr:from><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>7</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>3</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="3" name="Shape 2"/><xdr:cNvSpPr/></xdr:nvSpPr>
<xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="571500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="4472C4"/></a:solidFill></xdr:spPr>
<xdr:txBody><a:bodyPr wrap="square"><a:prstTxWarp prst="textNoShape"><a:avLst/></a:prstTxWarp></a:bodyPr><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>Rectangle</a:t></a:r></a:p></xdr:txBody></xdr:sp><xdr:clientData/></xdr:twoCellAnchor>
</xdr:wsDr>"##
        .to_string()
}

fn with_drawing(drawing: &str) -> Vec<u8> {
    let rels = r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"##;
    minimal(
        r##"<sheetData/><drawing r:id="rId1"/>"##,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/drawings/drawing1.xml", drawing)],
        "",
        "",
    )
}

#[test]
fn a_text_body_with_the_no_warp_marker_opens_as_word_art() {
    let wb = read_xlsx(&with_drawing(&drawing())).unwrap().0;
    let sheet = wb.sheet(0).unwrap();
    assert_eq!(sheet.shapes.len(), 2);
    let art = &sheet.shapes[0];
    assert_eq!(art.kind, ShapeKind::WordArt);
    assert_eq!(art.text, "Hello\nWorld", "both paragraphs, joined by a line break");
    assert_eq!(art.format.font_size, Some(40.0));
    assert!(art.format.bold);
    assert_eq!(art.format.font, Some("Arial".into()));
    assert_eq!(art.format.text_color, Some(Color::Rgb(0x1B2A49)));
    assert_eq!(art.format.text_outline, Some((Color::Rgb(0x00FF00), 2.5)));
    assert_eq!(art.format.text_shadow, Some(Color::Rgb(0x8A94A6)));
    assert_eq!(art.fill, Color::Auto, "no fill: the text is the shape");
    assert_eq!(art.line, Color::Auto);
    assert!(art.format.no_line);
    // The warp alone does not make a shape a WordArt: a filled rectangle stays a rectangle.
    assert_eq!(sheet.shapes[1].kind, ShapeKind::Rectangle);
}

#[test]
fn the_word_art_round_trips_through_the_drawing_part() {
    let wb = read_xlsx(&with_drawing(&drawing())).unwrap().0;
    let bytes = write_xlsx(&wb).unwrap();
    crate::testing::check_package(&bytes).expect("the package validates");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
    let mut part = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/drawings/drawing1.xml").unwrap(), &mut part).unwrap();
    // The written part parses, and carries the marker that makes the shape a WordArt again.
    crate::xml::parse(part.as_bytes()).expect("the written drawing is XML");
    assert!(part.contains("<a:prstTxWarp prst=\"textNoShape\">"), "{part}");
    assert!(part.contains("<a:outerShdw"), "the shadow is written: {part}");
    // Reopening reads back the same WordArt: kind, text and every run property that was in the file.
    let again = read_xlsx(&bytes).unwrap().0;
    let art = &again.sheet(0).unwrap().shapes[0];
    assert_eq!(art.kind, ShapeKind::WordArt);
    assert_eq!(art.text, "Hello\nWorld");
    assert_eq!(art.format.font_size, Some(40.0));
    assert!(art.format.bold);
    assert_eq!(art.format.text_color, Some(Color::Rgb(0x1B2A49)));
    assert_eq!(art.format.text_outline, Some((Color::Rgb(0x00FF00), 2.5)));
    assert_eq!(art.format.text_shadow, Some(Color::Rgb(0x8A94A6)));
    assert_eq!(art.format.font, Some("Arial".into()));
    assert_eq!(again.sheet(0).unwrap().shapes[1].kind, ShapeKind::Rectangle);
}

#[test]
fn a_word_art_with_no_text_still_writes_its_marker() {
    // The text is cleared: the body (and so the marker) is still written, or a reopen loses the kind.
    let mut wb = read_xlsx(&with_drawing(&drawing())).unwrap().0;
    wb.sheet_mut(0).unwrap().shapes[0].text.clear();
    let bytes = write_xlsx(&wb).unwrap();
    let again = read_xlsx(&bytes).unwrap().0;
    let art = &again.sheet(0).unwrap().shapes[0];
    assert_eq!(art.kind, ShapeKind::WordArt);
    assert_eq!(art.text, "");
}
/// A WordArt the engine wrote (not read from a file) is a WordArt when it comes back.
#[test]
fn a_written_word_art_reopens_as_one() {
    use cella_model::{Anchor, Shape, ShapeFormat, ShapeTextAlign, ShapeTextAnchor, Sheet, Workbook};
    let mut wb = Workbook::new();
    // Replace the default sheet so the WordArt is the only thing on the first one.
    wb.sheets.clear();
    let mut sheet = Sheet::new("Data");
    let format = ShapeFormat {
        no_line: true,
        font_size: Some(44.0),
        bold: true,
        italic: true,
        font: Some("Arial".into()),
        text_color: Some(Color::Rgb(0xD64527)),
        text_outline: Some((Color::Rgb(0x5A1A0E), 1.0)),
        text_shadow: Some(Color::Rgb(0x808080)),
        align: Some(ShapeTextAlign::Center),
        anchor: Some(ShapeTextAnchor::Middle),
        rtl: false,
    };
    sheet.shapes.push(Shape {
        id: 1,
        kind: ShapeKind::WordArt,
        anchor: Anchor { width: 240.0, height: 80.0, ..Default::default() },
        fill: Color::Auto,
        line: Color::Auto,
        text: "Cella".into(),
        alt: None,
        format: format.clone(),
    });
    wb.sheets.push(sheet.into());
    let bytes = write_xlsx(&wb).unwrap();
    let again = read_xlsx(&bytes).unwrap().0;
    let art = &again.sheet(0).unwrap().shapes[0];
    assert_eq!(art.kind, ShapeKind::WordArt);
    assert_eq!(art.text, "Cella");
    // Every run property survives (the size is written in hundredths of a point, so it comes back
    // as it went in).
    assert_eq!(art.format, format);
}
