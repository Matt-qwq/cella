//! The machine's fonts are catalogued, and a workbook family the catalog has is bound.

use gridcraft_ui_egui::fonts;

#[test]
fn the_font_catalog_is_scanned_and_looked_up_by_name() {
    // Every entry point goes through the language variant (the desktop app does); it must start
    // the scan. Without it no installed font is ever found, which is what made a 宋体 workbook
    // draw in a substitute family.
    gridcraft_ui_egui::fonts::start_scan();
    let mut waited = 0;
    while !fonts::scan_done() && waited < 200 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        waited += 1;
    }
    assert!(fonts::scan_done(), "the scan never finished after 20 s");
    let first = fonts::catalog().first().map(|f| f.family.clone()).expect("a machine with no fonts at all");
    assert!(!fonts::faces_of(&first).is_empty(), "the catalog looks a family up by its name: {first}");
}
