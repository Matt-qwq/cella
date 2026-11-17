//! More ribbon commands: Chart Design and Format, object arrangement, workbook views, table
//! tools, Get Data, scripts (Automate), cell checkboxes, theme colours and fonts.

use cella_core::{CellRef, RangeRef, Value};
use cella_model::*;
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // Chart Design / Format
        cmd!(
            "chart.addElement",
            "Add Chart Element",
            ["Chart Design", "Chart Layouts"],
            None,
            "{chart?, element: title|legend|dataLabels|dataTable|gridlines|axisTitles|smooth|trendline, on?: bool, position?: bottom|top|left|right; dataTable: dataTableKeys?: bool, dataTableHorizontalBorder?: bool, dataTableVerticalBorder?: bool, dataTableOutline?: bool (a column, bar, line or area chart, or a combo of those); trendline: series?: index (default all), type?: linear|exponential|linearForecast|movingAverage|logarithmic|power|polynomial, order?: 2..6, period?: 2..255, forward?, backward?, displayEquation?, displayRSquared?}",
            has_doc,
            add_element
        ),
        cmd!("chart.quickLayout", "Quick Layout", ["Chart Design", "Chart Layouts"], None, "{chart?, layout: 1..6}", has_doc, quick_layout),
        cmd!(
            "chart.changeColors",
            "Change Colors",
            ["Chart Design", "Chart Styles"],
            None,
            "{chart?, palette: colorful|monochrome|accent1..accent6 | colors: [hex…]}",
            has_doc,
            change_colors
        ),
        cmd!(
            "chart.selectData",
            "Select Data…",
            ["Chart Design", "Data"],
            None,
            "{chart?, range?: \"Sheet1!A1:D7\", byRows?: bool, categories?: \"Sheet1!A2:A7\" (Horizontal (Category) Axis Labels for every series; \"\" clears), removeSeries?: [index…]}",
            has_doc,
            select_data
        ),
        cmd!("chart.changeType", "Change Chart Type…", ["Chart Design", "Type"], None, "{chart?, type, subtype?}", has_doc, |s, p| s
            .execute("chart.set", p.clone())),
        cmd!(
            "chart.move",
            "Move Chart…",
            ["Chart Design", "Location"],
            None,
            "{chart?, sheet?: name (moves to that sheet), at?: \"B2\"}",
            has_doc,
            move_chart
        ),
        cmd!(
            "chart.formatSelection",
            "Format Selection",
            ["Format", "Current Selection"],
            None,
            "{chart?, series?: index, color?: hex}",
            has_doc,
            format_series
        ),
        cmd!(
            "chart.shapeFill",
            "Shape Fill",
            ["Format", "Shape Styles"],
            None,
            "{chart? | kind: shape, id?, series?, color: hex}",
            has_doc,
            shape_fill
        ),
        cmd!("chart.shapeOutline", "Shape Outline", ["Format", "Shape Styles"], None, "{kind: shape, id?, color: hex}", has_doc, shape_outline),
        // Arrange
        cmd!(
            "arrange.bringForward",
            "Bring Forward",
            ["Page Layout", "Arrange"],
            None,
            "{kind: chart|image|shape, id, toFront?: bool}",
            has_doc,
            |s, p| arrange(s, p, true)
        ),
        cmd!(
            "arrange.sendBackward",
            "Send Backward",
            ["Page Layout", "Arrange"],
            None,
            "{kind: chart|image|shape, id, toBack?: bool}",
            has_doc,
            |s, p| arrange(s, p, false)
        ),
        cmd!(query "arrange.selectionPane", "Selection Pane", ["Page Layout", "Arrange"], None, "{} → objects on the sheet in stacking order", has_doc, selection_pane),
        cmd!(
            "arrange.align",
            "Align",
            ["Page Layout", "Arrange"],
            None,
            "{kind: chart|image|shape, ids: [..], align: left|center|right|top|middle|bottom}",
            has_doc,
            align
        ),
        cmd!(
            "arrange.group",
            "Group",
            ["Page Layout", "Arrange"],
            None,
            "{kind, ids} (moves objects together: aligns their anchors to the first)",
            has_doc,
            |s, p| align(s, &super::format::merge_json(p.clone(), &json!({"align": "left"})))
        ),
        cmd!(
            "arrange.rotate",
            "Rotate",
            ["Page Layout", "Arrange"],
            None,
            "{kind: shape, id} (swaps width/height for a 90° turn; `flip` is rejected: shapes have no flip state)",
            has_doc,
            rotate
        ),
        // Views
        cmd!(noundo "view.normal", "Normal", ["View", "Workbook Views"], None, "{sheet?} → {view} (per sheet, saved with the file)", has_doc, |s, p| set_view(
            s,
            p,
            SheetViewMode::Normal
        )),
        cmd!(
            noundo "view.pageLayout",
            "Page Layout",
            ["View", "Workbook Views"],
            None,
            "{sheet?} → {view}: pages on paper with rulers; headers and footers are edited in place (per sheet, saved with the file)",
            has_doc,
            |s, p| set_view(s, p, SheetViewMode::PageLayout)
        ),
        cmd!(
            noundo "view.pageBreakPreview",
            "Page Break Preview",
            ["View", "Workbook Views"],
            None,
            "{sheet?} → {view} (per sheet, saved with the file)",
            has_doc,
            |s, p| set_view(s, p, SheetViewMode::PageBreakPreview)
        ),
        // Saving or deleting a view writes it into the workbook's hidden names (`_gc_custom_views_<n>`),
        // which is workbook state like any other edit, so the command records a history step (a
        // `show` changes the sheet's zoom and panes, which are saved with the file too). Without
        // the step, undoing every later action left the view behind and the workbook no longer
        // matched the one the user started from.
        cmd!(
            "view.customViews",
            "Custom Views…",
            ["View", "Workbook Views"],
            None,
            "{save?: name, show?: name, delete?: name} → list",
            has_doc,
            custom_views
        ),
        cmd!(noundo "view.split", "Split", ["View", "Window"], None, "{} (toggles a split at the active cell; shown as frozen panes in this version)", has_doc, split),
        cmd!(
            noundo "view.newWindow",
            "New Window",
            ["View", "Window"],
            None,
            "{} → {window, index, windows, sheet, selection}: another window on the active document — the same workbook, its undo history and its save, in a window of its own. Both windows start on the sheet and selection the document shows; from then on each keeps its own (selection, active sheet, scroll position). Disabled, with that reason in `enabled`, in the browser build: it has one window, the page",
            can_open_window,
            new_window
        ),
        cmd!(noundo "view.arrangeAll", "Arrange All", ["View", "Window"], None, "{}", has_doc, |_, _| ok()),
        cmd!(noundo "view.hideWindow", "Hide", ["View", "Window"], None, "{window?, index?} hides a window (the active one without them) and focuses the next visible one; the last visible window stays", has_doc, hide_window),
        cmd!(noundo "view.unhideWindow", "Unhide…", ["View", "Window"], None, "{window? | index?} shows a hidden window again and focuses it (the first hidden one without them)", has_doc, unhide_window),
        cmd!(
            query "view.switchWindows",
            "Switch Windows",
            ["View", "Window"],
            None,
            "{} → [{index, window, title, document, sheet, selection, active, hidden}] every open window, in window-list order (sheet and selection are what that window shows, not what the active one shows)",
            always,
            switch_windows
        ),
        cmd!(
            noundo "view.closeWindow",
            "Close Window",
            ["View", "Window"],
            None,
            "{window?, index?, force?: bool} closes one window (the active one without them); the last window of a document closes the document too, asking about unsaved changes unless `force`",
            has_doc,
            close_window
        ),
        cmd!(noundo "view.ruler", "Ruler", ["View", "Show"], None, "{on?} → {on}: rulers in Page Layout view (an application option)", has_doc, ruler),
        cmd!(query "view.macros", "Macros", ["View", "Macros"], None, "{} → scripts", has_doc, all_scripts),
        // Table tools
        cmd!("table.resize", "Resize Table", ["Table Design", "Properties"], None, "{table?, range: \"A1:F20\"}", has_doc, table_resize),
        cmd!("table.removeDuplicates", "Remove Duplicates", ["Table Design", "Tools"], None, "{table?, columns?}", has_doc, |s, p| s
            .execute("data.removeDuplicates", p.clone())),
        // Get & Transform
        cmd!(
            "data.fromTextCsv",
            "From Text/CSV",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text, delimiter?: \",\", sheet?: new sheet name} → imports into a new sheet as a table",
            has_doc,
            from_csv
        ),
        cmd!(
            "data.getData",
            "Get Data",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text} (CSV/TSV/JSON array of objects) → new sheet table",
            has_doc,
            from_csv
        ),
        cmd!(
            "data.fromTableRange",
            "From Table/Range",
            ["Data", "Get & Transform Data"],
            None,
            "{range?} → copies the table/range to a new sheet as a table",
            has_doc,
            from_table_range
        ),
        cmd!(query "data.queriesConnections", "Queries & Connections", ["Data", "Queries & Connections"], None, "{} → imported tables", has_doc, queries),
        // Automate
        // Stopping a recording saves its steps as a script in the workbook (`automate.newScript`,
        // undoable); starting one changes nothing. A not-undoable command that saved the script
        // left it in the workbook after undoing everything.
        cmd!(
            "automate.recordActions",
            "Record Actions",
            ["Automate", "Scripting Tools"],
            None,
            "{stop?: bool, cancel?: bool (ends the recording without saving it), name?: script name when stopping}",
            has_doc,
            record
        ),
        cmd!(
            "automate.newScript",
            "New Script",
            ["Automate", "Scripting Tools"],
            None,
            "{name, commands: [{command, params}] }",
            has_doc,
            new_script
        ),
        cmd!(
            "automate.scriptFromJournal",
            "Save Steps as Script",
            ["Automate", "Scripting Tools"],
            None,
            "{name, steps?: 50 (max 2000), drop?: [command ids]} -> {name, steps} - turns what already ran into a script (the journal, without the automation commands)",
            has_doc,
            script_from_journal
        ),
        cmd!(query "automate.allScripts", "All Scripts", ["Automate", "Scripting Tools"], None, "{} → saved scripts", has_doc, all_scripts),
        cmd!(
            "automate.runScript",
            "Run Script",
            [],
            None,
            "{name, trusted?: bool (runs a script from an untrusted workbook this once)} — one undo step: the first failing step stops the run, names it and rolls the whole run back; a workbook opened from a file runs nothing until it is trusted, a step that reads a file needs that trust, and file.*, link.*, undo/redo and another script are refused",
            has_doc,
            run_script
        ),
        cmd!(
            noundo "automate.trustScripts",
            "Trust Scripts",
            ["Automate", "Scripting Tools"],
            None,
            "{trusted?: bool (default true)} → {trusted}: lets this workbook's scripts run for the rest of the session",
            has_doc,
            trust_scripts
        ),
        cmd!("automate.deleteScript", "Delete Script", [], None, "{name}", has_doc, delete_script),
        // The script editor's commands: the Macros dialog shows a script's steps and changes them
        // one at a time, and every change is an ordinary undoable edit of the workbook.
        cmd!(query "automate.scriptSteps", "Script Steps", [], None, "{name} → the script's steps ([{command, params}], in order, at most 1000)", has_doc, script_steps),
        cmd!(
            "automate.editStep",
            "Edit Step",
            [],
            None,
            "{name, index, command?: registered command id, params?: object} — changes one step; a field left out keeps the step's own, and a command id the registry doesn't have or a step the script doesn't have is refused",
            has_doc,
            edit_step
        ),
        cmd!(
            "automate.removeStep",
            "Remove Step",
            [],
            None,
            "{name, index} — drops one step (a step the script doesn't have is refused)",
            has_doc,
            remove_step
        ),
        cmd!(
            "automate.appendStep",
            "Add Step",
            [],
            None,
            "{name, command, params?: object} — adds one step at the end; refuses a command id the registry doesn't have, params that aren't a JSON object, and a script that already holds the 1000 steps one may",
            has_doc,
            append_step
        ),
        cmd!(
            "automate.renameScript",
            "Rename Script",
            [],
            None,
            "{name, to} — refuses a blank name and one another script already uses",
            has_doc,
            rename_script
        ),
        // Cell controls, themes, borders, styles
        cmd!(
            "insert.checkbox",
            "Checkbox",
            ["Insert", "Cell Controls"],
            None,
            "{range?} (TRUE/FALSE and empty cells draw as checkboxes, empty ones become FALSE, other values show as they are; click or Space toggles; Delete unchecks, then removes; Clear Formats removes and keeps the values)",
            has_doc,
            checkbox
        ),
        cmd!("cell.toggleCheckbox", "Toggle Checkbox", [], Some("Space"), "{cell?}", has_doc, toggle_checkbox),
        cmd!("pageLayout.themeColors", "Colors", ["Page Layout", "Themes"], None, "{name: theme name | colors: [12 hex]}", has_doc, theme_colors),
        cmd!("pageLayout.themeFonts", "Fonts", ["Page Layout", "Themes"], None, "{major, minor}", has_doc, theme_fonts),
        cmd!(
            "pageLayout.themeEffects",
            "Effects",
            ["Page Layout", "Themes"],
            None,
            "{name} (chart effects preset; stored with the theme name)",
            has_doc,
            |_, _| ok()
        ),
        cmd!(
            "pageLayout.background",
            "Background",
            ["Page Layout", "Page Setup"],
            None,
            "{path? | clear?: bool} (sheet background picture)",
            has_doc,
            background
        ),
        cmd!(
            "home.drawBorder",
            "Draw Border",
            ["Home", "Font"],
            None,
            "{range, edge: top|bottom|left|right|grid, style?, color?}",
            has_doc,
            draw_border
        ),
        cmd!(
            "home.newCellStyle",
            "New Cell Style…",
            ["Home", "Styles"],
            None,
            "{name, fromCell?: \"A1\" (default: active cell)}",
            has_doc,
            new_cell_style
        ),
        cmd!(query "formulas.recentlyUsed", "Recently Used", ["Formulas", "Function Library"], None, "{} → recently used functions in this workbook", has_doc, recently_used),
        cmd!(query "formulas.financial", "Financial", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Financial"}))),
        cmd!(query "formulas.logical", "Logical", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Logical"}))),
        cmd!(query "formulas.text", "Text", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Text"}))),
        cmd!(query "formulas.dateTime", "Date & Time", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "DateTime"}))),
        cmd!(query "formulas.lookupReference", "Lookup & Reference", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Lookup"}))),
        cmd!(query "formulas.mathTrig", "Math & Trig", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "MathTrig"}))),
        cmd!(query "formulas.moreFunctions", "More Functions", ["Formulas", "Function Library"], None, "{category?: Statistical|Engineering|Information|Database|Compatibility|Web|Cube}", always, |s, p| s.execute("formulas.functions", json!({"category": str_param(p, "category").unwrap_or("Statistical")}))),
        cmd!(
            "formulas.useInFormula",
            "Use in Formula",
            ["Formulas", "Defined Names"],
            None,
            "{name} (inserts the name into the cell being edited)",
            has_doc,
            use_in_formula
        ),
        cmd!(query "formulas.watchWindow", "Watch Window", ["Formulas", "Formula Auditing"], None, "{add?: \"Sheet1!B5\", remove?: \"…\"} → watched cells with current values", has_doc, watch_window),
    ]
}

fn find_chart(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let idx = match u32_param(p, "chart") {
        Some(id) => sh.charts.iter().position(|c| c.id == id),
        None => sh.charts.len().checked_sub(1),
    };
    idx.map(|i| (si, i)).ok_or_else(|| bad("chart", "no chart on this sheet"))
}

fn with_chart(s: &mut Session, p: &Json, f: impl FnOnce(&mut Chart) -> Result<()>) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    edit(s, |cx| {
        let c = cx.sheet_mut(si)?.charts.get_mut(ci).ok_or_else(|| bad("chart", "no such chart"))?;
        f(c)?;
        Ok(json!({"chart": c.id}))
    })
}

fn add_element(s: &mut Session, p: &Json) -> Result<Json> {
    let el = str_param(p, "element").unwrap_or("title").to_string();
    let on = bool_param(p, "on").unwrap_or(true);
    let pos = str_param(p, "position").map(str::to_string);
    with_chart(s, p, |c| {
        match el.as_str() {
            "title" => c.title = if on { c.title.clone().filter(|t| !t.is_empty()).or(Some("Chart Title".into())) } else { Some(String::new()) },
            "legend" => {
                c.legend = if !on {
                    LegendPos::None
                } else {
                    match pos.as_deref() {
                        Some("top") => LegendPos::Top,
                        Some("left") => LegendPos::Left,
                        Some("right") => LegendPos::Right,
                        _ => LegendPos::Bottom,
                    }
                }
            }
            "dataLabels" => c.data_labels = on,
            "dataTable" => update_data_table(c, p, Some(on))?,
            "gridlines" => c.gridlines = on,
            "axisTitles" => {
                c.x_title = on.then(|| c.x_title.clone().unwrap_or_else(|| "Axis Title".into()));
                c.y_title = on.then(|| c.y_title.clone().unwrap_or_else(|| "Axis Title".into()));
            }
            "smooth" => {
                for s in &mut c.series {
                    s.smooth = on;
                }
            }
            "trendline" => {
                let only = u32_param(p, "series").map(|i| i as usize);
                if only.is_some_and(|i| i >= c.series.len()) {
                    return Err(bad("chart.addElement", "no such series"));
                }
                let targets: Vec<usize> = (0..c.series.len()).filter(|i| only.is_none_or(|o| o == *i)).collect();
                if !on {
                    for i in targets {
                        if let Some(se) = c.series.get_mut(i) {
                            se.trendlines.clear();
                        }
                    }
                    return Ok(());
                }
                let t = trendline_param(p)?;
                let eligible: Vec<usize> =
                    targets.into_iter().filter(|i| cella_chart::supports_trendline(cella_chart::series_plot_kind(c, *i))).collect();
                if eligible.is_empty() {
                    return Err(bad("chart.addElement", "trendlines need a 2-D column, bar, line, area, scatter or bubble series"));
                }
                for i in eligible {
                    if let Some(se) = c.series.get_mut(i) {
                        se.trendlines.push(t.clone());
                    }
                }
            }
            other => return Err(bad("chart.addElement", format!("unknown element `{other}`"))),
        }
        Ok(())
    })
}

/// The chart's data table (Add Chart Element ▸ Data Table): on or off, and its legend keys and
/// three border sets. A chart kind without a table in Excel refuses it instead of storing a setting
/// that would never be drawn.
pub(crate) fn update_data_table(c: &mut Chart, p: &Json, on: Option<bool>) -> Result<()> {
    const C: &str = "chart.addElement";
    if p.get("dataTable").is_some_and(|v| !v.is_boolean()) {
        return Err(bad(C, "`dataTable` must be a boolean"));
    }
    if on == Some(false) {
        c.data_table = None;
        return Ok(());
    }
    let fields = ["dataTableKeys", "dataTableHorizontalBorder", "dataTableVerticalBorder", "dataTableOutline"];
    if on == Some(true) || fields.iter().any(|k| p.get(*k).is_some()) {
        if !c.supports_data_table() {
            return Err(bad(C, "a data table needs a column, bar, line or area chart (or a combo of those)"));
        }
        let table = c.data_table.get_or_insert_with(ChartDataTable::default);
        for (key, value) in
            fields.into_iter().zip([&mut table.show_keys, &mut table.horizontal_border, &mut table.vertical_border, &mut table.outline])
        {
            if let Some(v) = p.get(key) {
                *value = v.as_bool().ok_or_else(|| bad(C, format!("`{key}` must be a boolean")))?;
            }
        }
    }
    Ok(())
}

/// The trendline described by `chart.addElement`'s trendline parameters (Excel's Add Chart
/// Element ▸ Trendline choices, plus the Format Trendline options).
fn trendline_param(p: &Json) -> Result<Trendline> {
    let num = |k: &str| Trendline::clamp_forecast(f64_param(p, k).unwrap_or(0.0));
    let mut t = Trendline {
        forward: num("forward"),
        backward: num("backward"),
        display_equation: bool_param(p, "displayEquation").unwrap_or(false),
        display_r_squared: bool_param(p, "displayRSquared").unwrap_or(false),
        ..Trendline::default()
    };
    t.kind = match str_param(p, "type").unwrap_or("linear") {
        "linear" => TrendlineKind::Linear,
        // Excel's Linear Forecast: a linear trendline projected two periods ahead.
        "linearForecast" => {
            if t.forward == 0.0 {
                t.forward = 2.0;
            }
            TrendlineKind::Linear
        }
        "exponential" => TrendlineKind::Exponential,
        "logarithmic" => TrendlineKind::Logarithmic,
        "power" => TrendlineKind::Power,
        "polynomial" => TrendlineKind::Polynomial(u32_param(p, "order").unwrap_or(2).clamp(2, 6) as u8),
        "movingAverage" => TrendlineKind::MovingAverage(u32_param(p, "period").unwrap_or(2).clamp(2, 255)),
        other => return Err(bad("chart.addElement", format!("unknown trendline type `{other}`"))),
    };
    Ok(t)
}

fn quick_layout(s: &mut Session, p: &Json) -> Result<Json> {
    let n = u32_param(p, "layout").unwrap_or(1);
    with_chart(s, p, |c| {
        let (legend, labels, grid, axes) = match n {
            2 => (LegendPos::Top, true, false, false),
            3 => (LegendPos::Bottom, false, true, false),
            4 => (LegendPos::Bottom, true, false, false),
            5 => (LegendPos::None, false, true, true),
            6 => (LegendPos::Right, false, true, true),
            _ => (LegendPos::Right, false, true, false),
        };
        c.legend = legend;
        c.data_labels = labels;
        c.gridlines = grid;
        if axes {
            c.x_title.get_or_insert_with(|| "Axis Title".into());
            c.y_title.get_or_insert_with(|| "Axis Title".into());
        } else {
            c.x_title = None;
            c.y_title = None;
        }
        Ok(())
    })
}

fn change_colors(s: &mut Session, p: &Json) -> Result<Json> {
    let explicit: Option<Vec<Color>> =
        p.get("colors").and_then(Json::as_array).map(|a| a.iter().filter_map(|c| c.as_str().and_then(Color::from_hex)).collect());
    let palette = str_param(p, "palette").unwrap_or("colorful").to_string();
    with_chart(s, p, |c| {
        for (i, se) in c.series.iter_mut().enumerate() {
            se.color = match &explicit {
                Some(cols) if !cols.is_empty() => cols.get(i % cols.len()).copied(),
                _ => match palette.as_str() {
                    "monochrome" => Some(Color::Theme(4, (-400 + (i as i16 * 200)).clamp(-800, 800))),
                    p if p.starts_with("accent") => {
                        let k: u8 = p.trim_start_matches("accent").parse().unwrap_or(1);
                        Some(Color::Theme(3 + k.clamp(1, 6), (-500 + i as i16 * 250).clamp(-800, 800)))
                    }
                    _ => None,
                },
            };
        }
        Ok(())
    })
}

fn select_data(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    // A new data range rebuilds the series, as in Excel's Select Data › Chart data range.
    let rebuilt = match str_param(p, "range") {
        Some(range) => {
            let range = range.trim_start_matches('=');
            let (sheet_name, body) = split_sheet(range);
            let src_si = match sheet_name {
                Some(n) => d.wb.sheet_index(&n).ok_or_else(|| bad("chart.selectData", "no such sheet"))?,
                None => d.wb.active_sheet,
            };
            let r = RangeRef::parse(body).ok_or_else(|| bad("chart.selectData", "not a range"))?;
            if r.count() > 1_000_000 {
                return Err(bad("chart.selectData", "range too large for a chart"));
            }
            let src = d.wb.sheet(src_si).ok_or(EngineError::NoDocument)?;
            let by_rows = bool_param(p, "byRows").unwrap_or(r.width() > r.height() + 1);
            let series = super::insert::series_from_range(src, r, by_rows);
            Some((series, format!("{}!{}", cella_formula::quote_sheet(&src.name), r.a1()), by_rows))
        }
        None => None,
    };
    // Horizontal (Category) Axis Labels › Edit: one labels range for every series ("" clears).
    let categories = match str_param(p, "categories") {
        None => None,
        Some(t) if t.trim().trim_start_matches('=').is_empty() => Some(None),
        Some(t) => {
            let t = t.trim().trim_start_matches('=');
            let (sheet_name, body) = split_sheet(t);
            let si = match sheet_name {
                Some(n) => d.wb.sheet_index(&n).ok_or_else(|| bad("chart.selectData", "no such sheet"))?,
                None => d.wb.active_sheet,
            };
            let r = RangeRef::parse(body).ok_or_else(|| bad("chart.selectData", "`categories` is not a range"))?;
            let name = d.wb.sheet(si).map(|sh| sh.name.clone()).ok_or(EngineError::NoDocument)?;
            let abs = |c: CellRef| format!("${}${}", cella_core::col_to_letters(c.col), c.row + 1);
            Some(Some(format!("{}!{}:{}", cella_formula::quote_sheet(&name), abs(r.start), abs(r.end))))
        }
    };
    let mut remove: Vec<usize> = p
        .get("removeSeries")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_u64).filter_map(|i| usize::try_from(i).ok()).collect())
        .unwrap_or_default();
    remove.sort_unstable();
    remove.dedup();
    if rebuilt.is_none() && categories.is_none() && remove.is_empty() {
        return Err(bad("chart.selectData", "missing `range`, `categories` or `removeSeries`"));
    }
    with_chart(s, p, move |c| {
        if let Some((series, source, by_rows)) = rebuilt {
            c.series = series;
            super::insert::scatter_x_from_first(c.kind, &mut c.series);
            c.source = Some(source);
            c.by_rows = by_rows;
        }
        for i in remove.iter().rev() {
            if *i < c.series.len() && c.series.len() > 1 {
                c.series.remove(*i);
            }
        }
        if let Some(cats) = categories {
            for se in &mut c.series {
                se.categories = cats.clone();
            }
        }
        Ok(())
    })
}

fn move_chart(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    let target = match p.get("sheet").and_then(Json::as_str) {
        Some(n) => Some(s.doc()?.wb.sheet_index(n).ok_or_else(|| bad("chart.move", "no such sheet"))?),
        None => None,
    };
    let at = cell_param_on(s, p, "at", target.unwrap_or(si))?;
    edit(s, |cx| {
        let mut chart = cx.sheet_mut(si)?.charts.remove(ci);
        if let Some(a) = at {
            chart.anchor.cell = a;
        }
        let dest = target.unwrap_or(si);
        cx.sheet_mut(dest)?.charts.push(chart);
        Ok(Json::Null)
    })
}

fn format_series(s: &mut Session, p: &Json) -> Result<Json> {
    let idx = u32_param(p, "series").unwrap_or(0) as usize;
    let color = str_param(p, "color").and_then(Color::from_hex);
    with_chart(s, p, |c| {
        if let Some(se) = c.series.get_mut(idx) {
            se.color = color;
        }
        Ok(())
    })
}

fn shape_fill(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "kind") == Some("shape") {
        let color = str_param(p, "color").and_then(Color::from_hex).unwrap_or_default();
        return with_shape(s, p, |sh| sh.fill = color);
    }
    format_series(s, p)
}

fn shape_outline(s: &mut Session, p: &Json) -> Result<Json> {
    let color = str_param(p, "color").and_then(Color::from_hex).unwrap_or_default();
    with_shape(s, p, |sh| sh.line = color)
}

fn with_shape(s: &mut Session, p: &Json, f: impl FnOnce(&mut Shape)) -> Result<Json> {
    let si = s.doc()?.wb.active_sheet;
    let id = u32_param(p, "id");
    edit(s, |cx| {
        let shapes = &mut cx.sheet_mut(si)?.shapes;
        let sh = match id {
            Some(id) => shapes.iter_mut().find(|x| x.id == id),
            None => shapes.last_mut(),
        }
        .ok_or_else(|| bad("shape", "no such shape"))?;
        f(sh);
        Ok(Json::Null)
    })
}

fn arrange(s: &mut Session, p: &Json, forward: bool) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("arrange", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("shape").to_string();
    let extreme = bool_param(p, "toFront").or(bool_param(p, "toBack")).unwrap_or(false);
    let si = s.doc()?.wb.active_sheet;
    fn reorder<T>(v: &mut Vec<T>, i: usize, forward: bool, extreme: bool) {
        if i >= v.len() {
            return;
        }
        let item = v.remove(i);
        let j = match (forward, extreme) {
            (true, true) => v.len(),
            (false, true) => 0,
            (true, false) => (i + 1).min(v.len()),
            (false, false) => i.saturating_sub(1),
        };
        v.insert(j, item);
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        match kind.as_str() {
            "chart" => {
                if let Some(i) = sh.charts.iter().position(|c| c.id == id) {
                    reorder(&mut sh.charts, i, forward, extreme);
                }
            }
            "image" => {
                if let Some(i) = sh.images.iter().position(|c| c.id == id) {
                    reorder(&mut sh.images, i, forward, extreme);
                }
            }
            _ => {
                if let Some(i) = sh.shapes.iter().position(|c| c.id == id) {
                    reorder(&mut sh.shapes, i, forward, extreme);
                }
            }
        }
        Ok(Json::Null)
    })
}

fn selection_pane(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let mut v = Vec::new();
    for c in &sh.shapes {
        v.push(json!({"kind": "shape", "id": c.id, "name": format!("{:?} {}", c.kind, c.id), "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    for c in &sh.images {
        v.push(json!({"kind": "image", "id": c.id, "name": format!("Picture {}", c.id), "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    for c in &sh.charts {
        v.push(json!({"kind": "chart", "id": c.id, "name": format!("Chart {}", c.id), "title": c.title, "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    Ok(Json::Array(v))
}

fn anchors_of<'a>(sh: &'a mut Sheet, kind: &str) -> Vec<(u32, &'a mut Anchor)> {
    match kind {
        "chart" => sh.charts.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
        "image" => sh.images.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
        _ => sh.shapes.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
    }
}

fn align(s: &mut Session, p: &Json) -> Result<Json> {
    let kind = str_param(p, "kind").unwrap_or("shape").to_string();
    let ids: Vec<u32> =
        p.get("ids").and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect()).unwrap_or_default();
    let how = str_param(p, "align").unwrap_or("left").to_string();
    let si = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sheet = cx.sheet_mut(si)?;
        let geo: Vec<(u32, f64, f64, f32, f32)> = {
            let shc = sheet.clone();
            anchors_of(sheet, &kind)
                .into_iter()
                .filter(|(id, _)| ids.contains(id))
                .map(|(id, a)| (id, shc.col_left(a.cell.col) + a.dx as f64, shc.row_top(a.cell.row) + a.dy as f64, a.width, a.height))
                .collect()
        };
        if geo.len() < 2 {
            return Ok(Json::Null);
        }
        let left = geo.iter().map(|g| g.1).fold(f64::INFINITY, f64::min);
        let right = geo.iter().map(|g| g.1 + g.3 as f64).fold(f64::NEG_INFINITY, f64::max);
        let top = geo.iter().map(|g| g.2).fold(f64::INFINITY, f64::min);
        let bottom = geo.iter().map(|g| g.2 + g.4 as f64).fold(f64::NEG_INFINITY, f64::max);
        let shc = sheet.clone();
        for (id, a) in anchors_of(sheet, &kind) {
            let Some(g) = geo.iter().find(|g| g.0 == id) else { continue };
            let (mut x, mut y) = (g.1, g.2);
            match how.as_str() {
                "left" => x = left,
                "right" => x = right - g.3 as f64,
                "center" => x = (left + right) / 2.0 - g.3 as f64 / 2.0,
                "top" => y = top,
                "bottom" => y = bottom - g.4 as f64,
                _ => y = (top + bottom) / 2.0 - g.4 as f64 / 2.0,
            }
            let c = CellRef::new(shc.row_at(y), shc.col_at(x));
            a.cell = c;
            a.dx = (x - shc.col_left(c.col)) as f32;
            a.dy = (y - shc.row_top(c.row)) as f32;
        }
        Ok(Json::Null)
    })
}

fn rotate(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("flip").is_some_and(|f| !f.is_null()) {
        return Err(bad("arrange.rotate", "`flip` is not supported: shapes have no flip state"));
    }
    with_shape(s, p, |sh| std::mem::swap(&mut sh.anchor.width, &mut sh.anchor.height))
}

/// Switches a sheet's view. Like zoom, the view is saved with the file but is not an edit: it
/// neither records an undo step nor marks a clean workbook as changed.
fn set_view(s: &mut Session, p: &Json, mode: SheetViewMode) -> Result<Json> {
    let si = target_any_sheet(s, p)?;
    let d = s.doc_mut()?;
    let dirty = d.is_dirty();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    if let Some(sh) = wb.sheet_mut(si) {
        sh.view = mode;
    }
    if !dirty {
        d.saved = d.wb.clone();
    }
    Ok(json!({"view": mode.as_str()}))
}

fn ruler(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.hide_ruler);
    s.hide_ruler = !on;
    Ok(json!({"on": on}))
}

/// The stash keys of the workbook-level JSON Cella keeps in hidden defined names: the custom
/// views, the scripts and the Watch Window list. Each is stored as `_gc_<key>_<n>` (see
/// [`super::stash`] for the chunking, the limits and the escape); a workbook written before that
/// keeps it in one `_sc_<key>` name, which is still read.
const VIEWS_KEY: &str = "custom_views";
const SCRIPTS_KEY: &str = "scripts";
const WATCH_KEY: &str = "watch";

/// Workbook-level JSON stored in hidden defined names (round-trips through XLSX).
pub(crate) fn stash_get_shared(wb: &Workbook, key: &str) -> Json {
    stash_get(wb, key)
}

/// The JSON stored under stash `key`: `{}` when there is none, or when what is there is not JSON.
fn stash_get(wb: &Workbook, key: &str) -> Json {
    super::stash::get_json(wb, key).unwrap_or_else(|| json!({}))
}

/// Like `stash_get`, for the name-keyed lists (views, scripts): a stored value that is not a JSON object is treated as empty.
fn stash_map(wb: &Workbook, key: &str) -> Json {
    let v = stash_get(wb, key);
    if v.is_object() { v } else { json!({}) }
}

pub(crate) fn stash_set_shared(wb: &mut Workbook, key: &str, v: &Json) -> Result<()> {
    stash_set(wb, key, v)
}

fn stash_set(wb: &mut Workbook, key: &str, v: &Json) -> Result<()> {
    super::stash::set_json(wb, key, v)
}

/// Whether `name` is one of the workbook's own names holding stash item `key`. A visible name of
/// that shape is someone else's, so it is not touched.
fn is_item_name(wb: &Workbook, key: &str, name: &str) -> bool {
    super::stash::item_key(name).as_deref() == Some(key) && super::stash::holds_item(wb, name)
}

/// Copies of the workbook's names holding stash item `key`.
fn item_names(wb: &Workbook, key: &str) -> Vec<DefinedName> {
    wb.names.iter().filter(|n| is_item_name(wb, key, &n.name)).cloned().collect()
}

/// The Watch Window list isn't an edit (Excel doesn't undo it): undo and redo keep the current
/// list when they swap in another snapshot of the workbook.
pub(crate) fn keep_watch_list(from: &Workbook, to: &mut std::sync::Arc<Workbook>) {
    let keep = item_names(from, WATCH_KEY);
    if keep == item_names(to, WATCH_KEY) {
        return;
    }
    let wb = std::sync::Arc::make_mut(to);
    let gone: Vec<String> = item_names(wb, WATCH_KEY).into_iter().map(|n| n.name).collect();
    wb.names.retain(|n| !gone.iter().any(|g| g.eq_ignore_ascii_case(&n.name)));
    wb.names.extend(keep);
}

fn custom_views(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let mut views = stash_map(&d.wb, VIEWS_KEY);
    if let Some(name) = str_param(p, "save") {
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        views[name] = json!({"sheet": d.wb.active_sheet, "selection": d.selection.a1(), "zoom": sh.zoom, "freeze": sh.freeze});
        let v = views.clone();
        edit(s, |cx| {
            stash_set(&mut cx.wb, VIEWS_KEY, &v)?;
            Ok(())
        })?;
    } else if let Some(name) = str_param(p, "show") {
        let v = views.get(name).cloned().ok_or_else(|| bad("view.customViews", "no such view"))?;
        if let Some(si) = v["sheet"].as_u64() {
            s.execute("sheet.activate", json!({"sheet": si}))?;
        }
        if let Some(sel) = v["selection"].as_str() {
            s.execute("selection.set", json!({"range": sel}))?;
        }
        if let Some(z) = v["zoom"].as_u64() {
            s.execute("view.zoom", json!({"percent": z}))?;
        }
        if let Some(f) = v.get("freeze") {
            let freeze = match (f[0].as_u64(), f[1].as_u64()) {
                (Some(r), Some(c)) => u32::try_from(r).ok().zip(u32::try_from(c).ok()),
                _ => None,
            };
            edit(s, |cx| {
                let si = cx.sheet_index();
                cx.sheet_mut(si)?.freeze = freeze;
                Ok(())
            })?;
        }
    } else if let Some(name) = str_param(p, "delete") {
        if let Some(o) = views.as_object_mut() {
            o.remove(name);
        }
        let v = views.clone();
        edit(s, |cx| {
            stash_set(&mut cx.wb, VIEWS_KEY, &v)?;
            Ok(())
        })?;
    }
    Ok(stash_get(&s.doc()?.wb, VIEWS_KEY))
}

fn split(s: &mut Session, _: &Json) -> Result<Json> {
    let has = s.doc()?.wb.active().is_some_and(|sh| sh.freeze.is_some());
    if has { s.execute("view.unfreezePanes", json!({})) } else { s.execute("view.freezePanes", json!({})) }
}

/// View ▸ New Window needs a shell with more than one window: the desktop app draws the extra
/// windows, while the browser build has one window (the page) and says so instead of opening a
/// window it cannot show.
fn can_open_window(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if cfg!(target_arch = "wasm32") {
        return Err("the browser build has one window: every workbook opens in this page".into());
    }
    Ok(())
}

/// The window a `{window, index}` parameter names, the active one without them.
fn window_param(s: &Session, p: &Json, id: &str) -> Result<crate::WindowId> {
    if let Some(n) = p.get("window").and_then(Json::as_u64) {
        let window = crate::WindowId(n);
        return if s.window_doc(window).is_some() { Ok(window) } else { Err(bad(id, "no such window")) };
    }
    if let Some(i) = p.get("index").and_then(Json::as_u64).and_then(|v| usize::try_from(v).ok()) {
        return s.windows().get(i).map(|(_, w)| w.id).ok_or_else(|| bad(id, "no such window"));
    }
    s.active_window().ok_or(EngineError::NoDocument)
}

/// Its position in the window list (View ▸ Switch Windows order).
fn window_index(s: &Session, window: crate::WindowId) -> Option<usize> {
    s.windows().iter().position(|(_, w)| w.id == window)
}

/// View ▸ New Window: another window on the *same* document. The document is not copied — the
/// windows share its workbook, its undo history and its save, and closing one leaves it open; what
/// each window shows of it (selection, active sheet) is the window's own.
fn new_window(s: &mut Session, _: &Json) -> Result<Json> {
    can_open_window(s).map_err(|e| bad("view.newWindow", e))?;
    let window = s.add_window()?;
    let view = s.window_view(window);
    Ok(json!({
        "window": window.0,
        "index": window_index(s, window),
        "windows": s.window_count(),
        "sheet": view.as_ref().map(|v| v.sheet),
        "selection": view.as_ref().map(|v| v.selection.a1()),
    }))
}

fn hide_window(s: &mut Session, p: &Json) -> Result<Json> {
    let window = window_param(s, p, "view.hideWindow")?;
    s.hide_window(window)?;
    // The last visible window stays visible, so `hidden` says what actually happened.
    let hidden = s.windows().iter().find(|(_, w)| w.id == window).map(|(_, w)| w.hidden).unwrap_or(false);
    Ok(json!({"window": window.0, "hidden": hidden, "active": s.active_window().map(|w| w.0), "windows": s.window_count()}))
}

fn unhide_window(s: &mut Session, p: &Json) -> Result<Json> {
    // A parameter names the window; without one, the first hidden window (View ▸ Unhide…).
    let window = if p.get("window").is_some() || p.get("index").is_some() {
        window_param(s, p, "view.unhideWindow")?
        // Nothing hidden is not an error: the command does nothing, as it always has.
    } else if let Some(w) = s.windows().iter().find(|(_, w)| w.hidden).map(|(_, w)| w.id) {
        w
    } else {
        return Ok(json!({"window": Json::Null, "windows": s.window_count()}));
    };
    if !s.unhide_window(window) {
        return Err(bad("view.unhideWindow", "no such window"));
    }
    Ok(json!({"window": window.0, "active": s.active_window().map(|w| w.0), "windows": s.window_count()}))
}

fn switch_windows(s: &mut Session, _: &Json) -> Result<Json> {
    let active = s.active_window();
    Ok(Json::Array(
        s.windows()
            .iter()
            .enumerate()
            .map(|(i, (doc, w))| {
                // Each window's own view of the workbook it shows, not the active window's.
                let view = s.window_view(w.id);
                json!({
                    "index": i,
                    "window": w.id.0,
                    "title": s.documents().get(*doc).map(|d| d.display_title()).unwrap_or_default(),
                    "document": doc,
                    "sheet": view.as_ref().map(|v| v.sheet),
                    "selection": view.as_ref().map(|v| v.selection.a1()),
                    "active": active == Some(w.id),
                    "hidden": w.hidden,
                })
            })
            .collect(),
    ))
}

/// The close button of a window: closing the last window of a document closes the document with
/// it, which asks about unsaved changes first unless `force`. Every other window just goes.
fn close_window(s: &mut Session, p: &Json) -> Result<Json> {
    let force = bool_param(p, "force").unwrap_or(false);
    let window = window_param(s, p, "view.closeWindow")?;
    s.focus_window(window);
    let last = s.doc().map(|d| d.windows.len() <= 1).unwrap_or(false);
    if last && !force && s.doc()?.is_dirty() {
        let title = s.doc()?.display_title();
        s.ui_requests.push(crate::UiRequest::Dialog("saveChanges".into(), json!({"title": title})));
        return Ok(json!({"closed": false, "dirty": true, "document": true, "window": window.0}));
    }
    let document = s.close_window(window);
    Ok(json!({"closed": true, "document": document, "window": window.0, "windows": s.window_count()}))
}

fn table_resize(s: &mut Session, p: &Json) -> Result<Json> {
    let r = str_param(p, "range").and_then(RangeRef::parse).ok_or_else(|| bad("table.resize", "missing `range`"))?;
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let ti = match str_param(p, "table") {
        Some(n) => sh.tables.iter().position(|t| t.name.eq_ignore_ascii_case(n)),
        None => sh.tables.iter().position(|t| t.range.contains(d.selection.active)),
    }
    .ok_or_else(|| bad("table.resize", "select a cell in a table"))?;
    edit(s, |cx| {
        let sheet = cx.sheet_mut(si)?;
        // The header row the resized range would have, and the unique names it must hold: a blank
        // cell takes the first free `ColumnN` at or after its position, so a column added by
        // resizing can't be handed a name a neighbouring column already has (which the XLSX
        // writer would then silently rename on save: `Column3` came back as `Column32`).
        let headers: Vec<Option<String>> = (r.start.col..=r.end.col)
            .map(|c| {
                let text = sheet.value(CellRef::new(r.start.row, c)).display();
                if text.trim().is_empty() { None } else { Some(text) }
            })
            .collect();
        let names = crate::tables::header_names(&headers);
        if sheet.tables.iter().enumerate().any(|(i, o)| i != ti && o.range.intersects(&r)) {
            return Err(EngineError::Other("A table can't overlap another table.".into()));
        }
        let t = sheet.tables.get_mut(ti).ok_or_else(|| bad("table", "gone"))?;
        if r.start.row != t.range.start.row || !r.intersects(&t.range) {
            return Err(EngineError::Other(
                "The headers must remain in the same row, and the resulting table range must overlap the original table range.".into(),
            ));
        }
        let mut cols = Vec::with_capacity(names.len());
        for name in names {
            let existing = t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&name)).cloned();
            cols.push(existing.unwrap_or(TableColumn {
                name,
                totals: TotalsFn::None,
                totals_label: None,
                formula: None,
                unique_name: None,
                xml: None,
            }));
        }
        t.columns = cols;
        t.range = r;
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn from_csv(s: &mut Session, p: &Json) -> Result<Json> {
    let bytes = if let Some(t) = str_param(p, "text") {
        t.as_bytes().to_vec()
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path)?
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let text = String::from_utf8_lossy(&bytes);
    // JSON array of objects → rows.
    let rows: Vec<Vec<Json>> =
        if text.trim_start().starts_with('[') {
            let arr: Vec<Json> = serde_json::from_str(&text).map_err(|e| bad("data.getData", e.to_string()))?;
            let mut keys: Vec<String> = Vec::new();
            for o in &arr {
                if let Some(m) = o.as_object() {
                    for k in m.keys() {
                        if !keys.contains(k) {
                            keys.push(k.clone());
                        }
                    }
                }
            }
            let mut out = vec![keys.iter().map(|k| json!(k)).collect::<Vec<_>>()];
            for o in &arr {
                out.push(keys.iter().map(|k| o.get(k).cloned().unwrap_or(Json::Null)).collect());
            }
            out
        } else {
            let delim = str_param(p, "delimiter")
                .and_then(|d| d.chars().next())
                .unwrap_or(if text.lines().next().is_some_and(|l| l.contains('\t')) { '\t' } else { ',' });
            let opts = cella_xlsx::CsvOptions { delimiter: delim as u8, ..Default::default() };
            let wb = cella_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
            let Some(u) = sh.used_range() else { return Err(EngineError::Other("The file is empty.".into())) };
            (u.start.row..=u.end.row)
                .map(|r| {
                    (u.start.col..=u.end.col)
                        .map(|c| match sh.value(CellRef::new(r, c)) {
                            Value::Number(n) => json!(n),
                            Value::Bool(b) => json!(b),
                            Value::Empty => Json::Null,
                            v => json!(v.display()),
                        })
                        .collect()
                })
                .collect()
        };
    if rows.is_empty() || rows.len() > 1_048_576 {
        return Err(EngineError::Other("Nothing to import.".into()));
    }
    let name = str_param(p, "sheet").map(str::to_string).or_else(|| {
        str_param(p, "path").and_then(|x| std::path::Path::new(x).file_stem().and_then(|f| f.to_str()).map(|f| f.chars().take(31).collect()))
    });
    let r = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    let w = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    s.execute("range.setValues", json!({"range": "A1", "values": rows}))?;
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(rows.len() as u32 - 1, w as u32 - 1));
    let _ = s.execute("insert.table", json!({"range": range.a1(), "header": true}));
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, w as u32 - 1).a1()}));
    Ok(json!({"sheet": r["name"], "rows": rows.len(), "cols": w}))
}

fn from_table_range(s: &mut Session, p: &Json) -> Result<Json> {
    let r = target_range(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let r = if r.is_single() { crate::selection::current_region(sh, r.start) } else { r };
    let text = crate::display::range_text(&d.wb, d.wb.active_sheet, r);
    from_csv(s, &json!({"text": text, "delimiter": "\t", "sheet": format!("{} (2)", sh.name).chars().take(31).collect::<String>()}))
}

fn queries(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.sheets
            .iter()
            .flat_map(|sh| {
                sh.tables
                    .iter()
                    .map(move |t| json!({"name": t.name, "sheet": sh.name, "range": t.range.a1(), "rows": t.range.height().saturating_sub(1)}))
            })
            .collect(),
    ))
}

/// Record Actions / `macro.record`: starts collecting the commands that run into a script, or
/// stops and saves what was collected as a script in the workbook (an ordinary undoable edit).
/// The recorder itself (its filter, its sheet and selection steps and its caps) is
/// [`crate::macros::recorder`]; what it refuses is counted in `skipped` and what it cut at its cap
/// in `dropped`.
pub(crate) fn record(s: &mut Session, p: &Json) -> Result<Json> {
    for k in ["stop", "cancel"] {
        if p.get(k).is_some_and(|v| !v.is_null() && !v.is_boolean()) {
            return Err(bad("automate.recordActions", format!("`{k}` must be true or false")));
        }
    }
    // `cancel` ends a recording without saving it: the way out of a recording that cannot be saved
    // (its workbook closed, the script list full).
    if bool_param(p, "cancel").unwrap_or(false) {
        let was = s.stop_recording().is_some();
        return Ok(json!({"recording": false, "cancelled": was}));
    }
    if bool_param(p, "stop").unwrap_or(false) || s.is_recording() {
        let Some(rec) = s.stop_recording() else { return Ok(json!({"recording": false})) };
        let (skipped, dropped) = (rec.skipped(), rec.dropped());
        // What the step list could not hold (a recording longer than a script) is counted with
        // what the buffer dropped.
        let (steps, cut) = rec.to_steps();
        let dropped = dropped.saturating_add(cut);
        let n = steps.len();
        let name = match str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => {
                let d = s.doc()?;
                next_script_name(&d.wb)
            }
        };
        // Saved into the workbook the recording started in, whichever one is in front now.
        let at = s.documents().iter().position(|d| d.uid == rec.doc());
        let Some(at) = at else {
            return Err(EngineError::Other("The workbook where the recording started was closed, so it was not saved.".into()));
        };
        s.set_active(at);
        let active = s.active_index();
        let saved = s.execute("automate.newScript", json!({"name": name, "commands": steps}));
        s.set_active(active);
        saved?;
        return Ok(json!({"recording": false, "saved": name, "steps": n, "skipped": skipped, "dropped": dropped}));
    }
    if !s.start_recording() {
        return Err(EngineError::NoDocument);
    }
    Ok(json!({"recording": true}))
}

/// The first free `Script N`, so a recording without a name never replaces a script.
fn next_script_name(wb: &Workbook) -> String {
    for n in 1..=10_000u32 {
        let candidate = format!("Script {n}");
        if script_steps_of(wb, &candidate).is_none() {
            return candidate;
        }
    }
    "Script".to_string()
}

fn new_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.newScript", "missing `name`"))?.to_string();
    let cmds = p.get("commands").cloned().unwrap_or(json!([]));
    if !cmds.is_array() {
        return Err(bad("automate.newScript", "`commands` must be an array"));
    }
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_KEY);
        all[name.as_str()] = cmds.clone();
        stash_set(&mut cx.wb, SCRIPTS_KEY, &all)?;
        Ok(json!({"name": name}))
    })
}

pub(crate) fn all_scripts(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (wb, trusted) = (&d.wb, d.scripts_trusted);
    let all = stash_get(wb, SCRIPTS_KEY);
    let mut list: Vec<Json> = all
        .as_object()
        .map(|o| o.iter().map(|(k, v)| json!({"name": k, "steps": v.as_array().map_or(0, Vec::len), "trusted": trusted})).collect())
        .unwrap_or_default();
    // A preserved VBA project is reported alongside Cella's own scripts; it is carried on
    // save but never executed here, so it has no step count.
    if let Some(bytes) = &wb.vba_project {
        list.push(json!({"name": "(VBA)", "steps": 0, "kind": "vba", "size": bytes.len(), "executed": false}));
    }
    Ok(Json::Array(list))
}

/// Whether `wb` carries Cella scripts (a workbook opened with them starts untrusted).
pub(crate) fn has_scripts(wb: &Workbook) -> bool {
    stash_map(wb, SCRIPTS_KEY).as_object().is_some_and(|o| !o.is_empty())
}

fn trust_scripts(s: &mut Session, p: &Json) -> Result<Json> {
    // Only a missing `trusted` means "trust"; `"false"`, `0` or `null` are mistakes, not consent.
    let trusted = match p.get("trusted") {
        None => true,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(bad("automate.trustScripts", "`trusted` must be true or false")),
    };
    s.doc_mut()?.scripts_trusted = trusted;
    Ok(json!({"trusted": trusted}))
}

/// Why a step is refused whatever the workbook's trust: [`crate::macros::policy`]'s deny and
/// file rules, so an extension (which shares them) can't hand a step a file either.
pub(crate) fn denied_script_step_shared(id: &str, params: &Json) -> Option<String> {
    crate::macros::policy::refused_step(id, params)
}

/// Runs a stored script: `automate.runScript`, and `macro.run` on the same runner. The policy, the
/// undo group, the step budget and the sandbox are [`crate::macros::run`]'s; this is the id the
/// ribbon, the dialogs and MCP have always called.
fn run_script(s: &mut Session, p: &Json) -> Result<Json> {
    crate::macros::run::run(s, p)
}

pub(crate) fn delete_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.deleteScript", "missing `name`"))?.to_string();
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_KEY);
        if let Some(o) = all.as_object_mut() {
            o.remove(&name);
        }
        stash_set(&mut cx.wb, SCRIPTS_KEY, &all)?;
        Ok(Json::Null)
    })
}

/// The most steps one script may hold, and the most `automate.scriptSteps` lists: a script from
/// a file (or a long recording) is input like any other, and an editor showing 10 000 rows is not
/// one anyone can use.
pub(crate) const MAX_SCRIPT_STEPS: usize = 1_000;
/// The largest JSON one step's `params` may serialize to, and the largest a single script's steps
/// may be. The stash is one defined name inside the workbook, not a place for a megabyte blob.
pub(crate) const MAX_STEP_PARAMS_BYTES: usize = 64 * 1024;
pub(crate) const MAX_SCRIPT_STEPS_BYTES: usize = 1024 * 1024;

/// The steps script `name` holds, in order. `None` when the workbook has no script of that name;
/// a stored value that isn't an array holds no steps (as `automate.allScripts` counts it).
pub(crate) fn script_steps_of(wb: &Workbook, name: &str) -> Option<Vec<Json>> {
    stash_map(wb, SCRIPTS_KEY).get(name).map(|v| v.as_array().cloned().unwrap_or_default())
}

fn no_such_script(cmd: &'static str, name: &str) -> EngineError {
    bad(cmd, format!("no script named \"{name}\""))
}

/// A step as the editing commands store it: a command id the registry has and a `params` object.
/// Refuses an id nothing registers, params that aren't a JSON object, and a params blob bigger
/// than one step may hold, so a hostile script is turned away with a message instead of being
/// written into the workbook.
fn checked_step(cmd: &'static str, id: &str, params: Option<&Json>) -> Result<Json> {
    let id = id.trim();
    if id.is_empty() {
        return Err(bad(cmd, "missing `command`"));
    }
    if find_command(id).is_none() {
        return Err(bad(cmd, format!("`{id}` is not a command")));
    }
    let params = match params {
        None | Some(Json::Null) => json!({}),
        Some(v @ Json::Object(_)) => {
            // Measured on the caller's value, so an oversized blob is never copied.
            let size = v.to_string().len();
            if size > MAX_STEP_PARAMS_BYTES {
                return Err(bad(cmd, format!("`params` is {size} bytes of JSON; one step may hold {MAX_STEP_PARAMS_BYTES}")));
            }
            v.clone()
        }
        Some(_) => return Err(bad(cmd, "`params` must be a JSON object")),
    };
    Ok(json!({"command": id, "params": params}))
}

/// Saves `steps` as the whole step list of script `name` (an ordinary undoable edit), refusing a
/// list longer or larger than one script may hold.
fn save_script_steps(s: &mut Session, name: &str, steps: Vec<Json>, cmd: &'static str) -> Result<()> {
    if steps.len() > MAX_SCRIPT_STEPS {
        return Err(bad(cmd, format!("\"{name}\" would hold {} steps; a script may hold {MAX_SCRIPT_STEPS}", steps.len())));
    }
    let list = Json::Array(steps);
    let size = list.to_string().len();
    if size > MAX_SCRIPT_STEPS_BYTES {
        return Err(bad(cmd, format!("the steps of \"{name}\" are {size} bytes of JSON; a script may hold {MAX_SCRIPT_STEPS_BYTES}")));
    }
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_KEY);
        all[name] = list.clone();
        stash_set(&mut cx.wb, SCRIPTS_KEY, &all)?;
        Ok(())
    })
}

/// The script's own steps, in order (the script editor's view of it).
fn script_steps(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.scriptSteps", "missing `name`"))?;
    let d = s.doc()?;
    let steps = script_steps_of(&d.wb, name).ok_or_else(|| no_such_script("automate.scriptSteps", name))?;
    Ok(Json::Array(steps.into_iter().take(MAX_SCRIPT_STEPS).collect()))
}

/// Changes one step of a script. A field left out of the call keeps the step's own value, so
/// editing only the params leaves the command, and moving a step to another command means passing
/// `params` when the old ones don't fit it.
fn edit_step(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.editStep", "missing `name`"))?.to_string();
    let index = u32_param(p, "index").ok_or_else(|| bad("automate.editStep", "missing `index`"))? as usize;
    let mut steps = {
        let d = s.doc()?;
        script_steps_of(&d.wb, &name).ok_or_else(|| no_such_script("automate.editStep", &name))?
    };
    let old = steps.get(index).cloned().ok_or_else(|| bad("automate.editStep", format!("\"{name}\" has no step {index}")))?;
    let id = str_param(p, "command").map(str::to_string).unwrap_or_else(|| old["command"].as_str().unwrap_or("").to_string());
    let params = p.get("params").or_else(|| old.get("params"));
    let step = checked_step("automate.editStep", &id, params)?;
    if let Some(slot) = steps.get_mut(index) {
        *slot = step;
    }
    let n = steps.len();
    save_script_steps(s, &name, steps, "automate.editStep")?;
    Ok(json!({"name": name, "index": index, "steps": n}))
}

fn remove_step(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.removeStep", "missing `name`"))?.to_string();
    let index = u32_param(p, "index").ok_or_else(|| bad("automate.removeStep", "missing `index`"))? as usize;
    let mut steps = {
        let d = s.doc()?;
        script_steps_of(&d.wb, &name).ok_or_else(|| no_such_script("automate.removeStep", &name))?
    };
    if index >= steps.len() {
        return Err(bad("automate.removeStep", format!("\"{name}\" has no step {index}")));
    }
    steps.remove(index);
    let n = steps.len();
    save_script_steps(s, &name, steps, "automate.removeStep")?;
    Ok(json!({"name": name, "steps": n}))
}

fn append_step(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.appendStep", "missing `name`"))?.to_string();
    let command = str_param(p, "command").ok_or_else(|| bad("automate.appendStep", "missing `command`"))?;
    let step = checked_step("automate.appendStep", command, p.get("params"))?;
    let mut steps = {
        let d = s.doc()?;
        script_steps_of(&d.wb, &name).ok_or_else(|| no_such_script("automate.appendStep", &name))?
    };
    if steps.len() >= MAX_SCRIPT_STEPS {
        return Err(bad("automate.appendStep", format!("\"{name}\" already holds the {MAX_SCRIPT_STEPS} steps a script may")));
    }
    steps.push(step);
    let n = steps.len();
    save_script_steps(s, &name, steps, "automate.appendStep")?;
    Ok(json!({"name": name, "index": n - 1, "steps": n}))
}

pub(crate) fn rename_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.renameScript", "missing `name`"))?.to_string();
    let to = str_param(p, "to").ok_or_else(|| bad("automate.renameScript", "missing `to`"))?.trim().to_string();
    if to.is_empty() {
        return Err(bad("automate.renameScript", "`to` is blank"));
    }
    // The name is a key in the stash's JSON: a megabyte of it is refused, not stored.
    if to.chars().count() > 255 {
        return Err(bad("automate.renameScript", "`to` is longer than the 255 characters a script name may hold"));
    }
    if to == name {
        return Ok(json!({"name": name}));
    }
    let all = stash_map(&s.doc()?.wb, SCRIPTS_KEY);
    if all.get(name.as_str()).is_none() {
        return Err(no_such_script("automate.renameScript", &name));
    }
    if all.get(to.as_str()).is_some() {
        return Err(bad("automate.renameScript", format!("\"{to}\" is already the name of a script")));
    }
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_KEY);
        if let Some(map) = all.as_object_mut()
            && let Some(steps) = map.remove(name.as_str())
        {
            map.insert(to.clone(), steps);
        }
        stash_set(&mut cx.wb, SCRIPTS_KEY, &all)?;
        Ok(json!({"name": to}))
    })
}

/// Number format that marks checkbox cells (TRUE/FALSE values drawn as boxes).
pub const CHECKBOX_FORMAT: &str = "\"☐\";\"☐\";\"☐\";[=1]\"☑\"";

/// Whether `c` has checkbox formatting.
pub(crate) fn is_checkbox_cell(wb: &cella_model::Workbook, sh: &cella_model::Sheet, c: CellRef) -> bool {
    wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "checkbox"
}

fn checkbox(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let si = target_sheet(s, p)?;
    edit(s, |cx| {
        for r in &ranges {
            if r.count() > 100_000 {
                return Err(bad("insert.checkbox", "range too large"));
            }
            for c in r.iter() {
                let st = cx.wb.sheet(si).map(|sh| sh.style_id(c)).unwrap_or_default();
                let new = cx.wb.styles.derive(st, |s| {
                    s.num_fmt = NumFmt::new("checkbox");
                    s.align.h = HAlign::Center;
                });
                let sh = cx.sheet_mut(si)?;
                // Values and formulas already there stay (only TRUE/FALSE draw as a box); an empty
                // cell starts unchecked.
                let mut cell = sh.cell(c).unwrap_or_default();
                if cell.value.is_empty() && cell.formula.is_none() {
                    cell.value = Value::Bool(false);
                }
                cell.style = new;
                sh.set_cell(c, cell);
                cx.touch(si, c);
            }
        }
        Ok(Json::Null)
    })
}

fn toggle_checkbox(s: &mut Session, p: &Json) -> Result<Json> {
    let si = target_sheet(s, p)?;
    let d = s.doc()?;
    let cells: Vec<CellRef> = match cell_param_on(s, p, "cell", si)? {
        Some(c) => vec![c],
        None => d.selection.ranges.iter().flat_map(|r| r.iter().take(100_000).collect::<Vec<_>>()).collect(),
    };
    edit(s, |cx| {
        for c in cells {
            let checkbox = cx.wb.sheet(si).is_some_and(|sh| is_checkbox_cell(&cx.wb, sh, c));
            let sh = cx.sheet_mut(si)?;
            if let Some(mut cell) = sh.cells.get(c)
                && cell.formula.is_none()
            {
                // An empty checkbox cell counts as FALSE.
                let next = match cell.value {
                    Value::Bool(b) if checkbox => Value::Bool(!b),
                    Value::Empty if checkbox => Value::Bool(true),
                    _ => continue,
                };
                cell.value = next;
                sh.cells.set(c, cell);
                cx.changed.push((si, c));
            }
        }
        Ok(Json::Null)
    })
}

/// Copy of the params that is always a JSON object, so keys can be assigned; any other JSON value counts as no params (like null).
fn object_params(p: &Json) -> Json {
    Json::Object(p.as_object().cloned().unwrap_or_default())
}

fn theme_colors(s: &mut Session, p: &Json) -> Result<Json> {
    let mut params = object_params(p);
    if params.get("name").is_none() {
        params["name"] = json!(s.doc()?.wb.theme.name.clone());
    }
    s.execute("pageLayout.theme", params)
}

fn theme_fonts(s: &mut Session, p: &Json) -> Result<Json> {
    let name = s.doc()?.wb.theme.name.clone();
    let colors: Vec<String> = s.doc()?.wb.theme.colors.iter().map(|c| format!("#{c:06X}")).collect();
    s.execute("pageLayout.theme", json!({"name": name, "colors": colors, "majorFont": p.get("major"), "minorFont": p.get("minor")}))
}

fn background(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "clear").unwrap_or(false) {
        let si = s.doc()?.wb.active_sheet;
        return edit(s, |cx| {
            cx.sheet_mut(si)?.images.retain(|i| i.alt != "Sheet background");
            Ok(Json::Null)
        });
    }
    let mut params = object_params(p);
    params["alt"] = json!("Sheet background");
    params["at"] = json!("A1");
    s.execute("insert.picture", params)
}

fn draw_border(s: &mut Session, p: &Json) -> Result<Json> {
    let edge = str_param(p, "edge").unwrap_or("grid");
    let preset = match edge {
        "top" => "top",
        "bottom" => "bottom",
        "left" => "left",
        "right" => "right",
        "outline" => "outside",
        _ => "all",
    };
    let mut params = object_params(p);
    params["preset"] = json!(preset);
    s.execute("home.borders", params)
}

fn new_cell_style(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("home.newCellStyle", "missing `name`"))?.to_string();
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let c = cell_param(s, p, "fromCell")?.unwrap_or(d.selection.active);
    let style = d.wb.styles.get(sh.style_id(c)).clone();
    edit(s, |cx| {
        cx.wb.cell_styles.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        cx.wb.cell_styles.push((name.clone(), style.clone()));
        Ok(json!({"name": name}))
    })
}

fn recently_used(s: &mut Session, _: &Json) -> Result<Json> {
    let mut seen: Vec<String> = Vec::new();
    for (id, p) in s.journal.iter().rev() {
        if id != "cell.set" {
            continue;
        }
        let input = p.get("input").and_then(Json::as_str).unwrap_or("");
        if let Ok(e) = cella_formula::parse(input.trim_start_matches('=')) {
            for f in e.functions() {
                if !seen.iter().any(|x| x == f) {
                    seen.push(f.to_string());
                }
            }
        }
        if seen.len() >= 10 {
            break;
        }
    }
    Ok(json!(seen))
}

fn use_in_formula(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("formulas.useInFormula", "missing `name`"))?;
    s.ui_requests.push(crate::UiRequest::EditCell(Some(format!("={name}"))));
    ok()
}

fn watch_window(s: &mut Session, p: &Json) -> Result<Json> {
    let mut list: Vec<String> =
        stash_get(&s.doc()?.wb, WATCH_KEY).as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut changed = false;
    if let Some(a) = str_param(p, "add") {
        if !list.iter().any(|x| x == a) {
            list.push(a.to_string());
        }
        changed = true;
    }
    if let Some(r) = str_param(p, "remove") {
        list.retain(|x| x != r);
        changed = true;
    }
    if changed {
        let v = json!(list.clone());
        let d = s.doc_mut()?;
        // The watch list is view state: don't add an undo step.
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        stash_set(wb, WATCH_KEY, &v)?;
    }
    let d = s.doc()?;
    let rows: Vec<Json> = list
        .iter()
        .map(|w| {
            let (sheet, body) = split_sheet(w);
            let si = sheet.and_then(|n| d.wb.sheet_index(&n)).unwrap_or(d.wb.active_sheet);
            let c = CellRef::parse(body).unwrap_or_default();
            let sh = d.wb.sheet(si);
            json!({"cell": w, "value": sh.map(|s| s.value(c).display()), "formula": sh.and_then(|s| s.cell(c)).and_then(|x| x.formula).map(|f| format!("={}", f.text))})
        })
        .collect();
    Ok(Json::Array(rows))
}
/// Turns what already happened into a script: the last steps commands the journal holds, minus the
/// automation ones, saved under name.
///
/// This is what a recorded script is *for* now that an agent is usually the one replaying it: the
/// journal is the record of what actually ran, so the steps it holds are a faithful, diffable,
/// replayable description of a procedure — the capture format, not a macro language for people who
/// cannot code. A step that needs editing is edited in the Macros dialog afterwards.
fn script_from_journal(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.scriptFromJournal", "missing name"))?.trim().to_string();
    if name.is_empty() || name.chars().count() > 255 {
        return Err(bad("automate.scriptFromJournal", "a script needs a name of 255 characters or fewer"));
    }
    let take = u32_param(p, "steps").unwrap_or(50).clamp(1, 2_000) as usize;
    let drop: Vec<String> =
        p.get("drop").and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(str::to_ascii_lowercase).collect()).unwrap_or_default();
    // The journal is session-wide and oldest-first; a script wants the newest steps last, and
    // without the automation commands themselves (recording one into another is how a loop gets
    // built).
    let cmds: Vec<Json> = s
        .journal
        .iter()
        .rev()
        .take(take)
        .rev()
        .filter(|(id, _)| !id.starts_with("automate.") && !id.starts_with("ext."))
        .filter(|(id, _)| !drop.iter().any(|d| id.eq_ignore_ascii_case(d)))
        .filter(|(id, _)| crate::find_command(id).is_some())
        .map(|(id, params)| json!({"command": id, "params": params}))
        .collect();
    if cmds.is_empty() {
        return Err(EngineError::Other(
            "There is nothing to save: the journal holds no commands yet (do the steps you want, then save them as a script).".into(),
        ));
    }
    let n = cmds.len();
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_KEY);
        if all.get(name.as_str()).is_some() {
            return Err(EngineError::Other(format!("There is already a script named \"{name}\".")));
        }
        all[name.as_str()] = Json::Array(cmds.clone());
        stash_set(&mut cx.wb, SCRIPTS_KEY, &all)?;
        Ok(Json::Null)
    })?;
    Ok(json!({"name": name, "steps": n}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session whose workbook holds one script, `Build`, with `steps`.
    fn with_script(steps: Json) -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("automate.newScript", json!({"name": "Build", "commands": steps})).unwrap();
        s
    }

    fn step(id: &str, params: Json) -> Json {
        json!({"command": id, "params": params})
    }

    fn listed(s: &mut Session, name: &str) -> Json {
        s.execute("automate.scriptSteps", json!({"name": name})).unwrap()
    }

    #[test]
    fn automate_script_steps_lists_the_scripts_own_steps_in_order() {
        let mut s = with_script(json!([step("cell.set", json!({"cell": "A1", "input": "x"})), step("home.bold", json!({}))]));
        assert_eq!(
            listed(&mut s, "Build"),
            json!([{"command": "cell.set", "params": {"cell": "A1", "input": "x"}}, {"command": "home.bold", "params": {}}])
        );
        // A script the workbook doesn't have, and a call with no name, are refused.
        let err = s.execute("automate.scriptSteps", json!({"name": "Nope"})).unwrap_err().to_string();
        assert!(err.contains("no script named"), "{err}");
        assert!(s.execute("automate.scriptSteps", json!({})).is_err());
        // A stored script whose value isn't an array lists no steps instead of failing. The
        // fixture is a workbook of the older shape, where the scripts are one `_sc_scripts` name:
        // a workbook with the chunked store reads that one, not a legacy name beside it.
        let mut old = Session::new();
        old.new_workbook();
        old.execute("formulas.defineName", json!({"name": "_sc_scripts", "refersTo": "\"{\"\"Odd\"\":7}\""})).unwrap();
        assert_eq!(listed(&mut old, "Odd"), json!([]));
    }

    #[test]
    fn automate_script_steps_lists_at_most_the_thousand_steps_an_editor_shows() {
        let many: Vec<Json> = (0..MAX_SCRIPT_STEPS + 500).map(|_| step("home.bold", json!({}))).collect();
        let mut s = with_script(json!(many));
        assert_eq!(listed(&mut s, "Build").as_array().map(Vec::len), Some(MAX_SCRIPT_STEPS));
    }

    #[test]
    fn automate_edit_step_changes_one_step_and_is_undoable() {
        let mut s =
            with_script(json!([step("cell.set", json!({"cell": "A1", "input": "1"})), step("cell.set", json!({"cell": "B1", "input": "2"})),]));
        // Only the params: the step keeps its command.
        let r = s.execute("automate.editStep", json!({"name": "Build", "index": 1, "params": {"cell": "B1", "input": "3"}})).unwrap();
        assert_eq!(r, json!({"name": "Build", "index": 1, "steps": 2}));
        assert_eq!(listed(&mut s, "Build")[1], step("cell.set", json!({"cell": "B1", "input": "3"})));
        // Undo puts the old step back: editing a script is an ordinary workbook edit.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(listed(&mut s, "Build")[1], step("cell.set", json!({"cell": "B1", "input": "2"})));
        // A new command with no params keeps the step's params, as the params doc says.
        s.execute("automate.editStep", json!({"name": "Build", "index": 0, "command": "cell.toggleCheckbox"})).unwrap();
        assert_eq!(listed(&mut s, "Build")[0], step("cell.toggleCheckbox", json!({"cell": "A1", "input": "1"})));
        // A new command with params replaces both.
        s.execute("automate.editStep", json!({"name": "Build", "index": 0, "command": "home.bold", "params": {}})).unwrap();
        assert_eq!(listed(&mut s, "Build")[0], step("home.bold", json!({})));
    }

    #[test]
    fn automate_edit_step_refuses_an_unknown_command_a_bad_index_and_hostile_params() {
        let mut s = with_script(json!([step("cell.set", json!({"cell": "A1", "input": "1"}))]));
        let original = listed(&mut s, "Build");
        let err = s.execute("automate.editStep", json!({"name": "Build", "index": 0, "command": "not.aCommand"})).unwrap_err().to_string();
        assert!(err.contains("is not a command"), "{err}");
        let err = s.execute("automate.editStep", json!({"name": "Build", "index": 9, "command": "home.bold"})).unwrap_err().to_string();
        assert!(err.contains("has no step 9"), "{err}");
        let err = s.execute("automate.editStep", json!({"name": "Nope", "index": 0})).unwrap_err().to_string();
        assert!(err.contains("no script named"), "{err}");
        // Params are an object: a bare array is refused rather than stored.
        let err = s.execute("automate.editStep", json!({"name": "Build", "index": 0, "params": [1, 2]})).unwrap_err().to_string();
        assert!(err.contains("must be a JSON object"), "{err}");
        // A megabyte of params is refused by its measured size, and nothing is written.
        let blob = json!({"text": "x".repeat(1_000_000)});
        let err = s.execute("automate.editStep", json!({"name": "Build", "index": 0, "params": blob})).unwrap_err().to_string();
        assert!(err.contains("bytes of JSON"), "{err}");
        assert_eq!(listed(&mut s, "Build"), original, "a refused edit leaves the script alone");
    }

    #[test]
    fn automate_remove_step_drops_one_step_and_refuses_a_bad_index() {
        let mut s = with_script(json!([
            step("cell.set", json!({"cell": "A1", "input": "1"})),
            step("home.bold", json!({})),
            step("cell.set", json!({"cell": "A2", "input": "2"})),
        ]));
        let r = s.execute("automate.removeStep", json!({"name": "Build", "index": 1})).unwrap();
        assert_eq!(r, json!({"name": "Build", "steps": 2}));
        assert_eq!(
            listed(&mut s, "Build"),
            json!([step("cell.set", json!({"cell": "A1", "input": "1"})), step("cell.set", json!({"cell": "A2", "input": "2"}))])
        );
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(listed(&mut s, "Build").as_array().map(Vec::len), Some(3));
        let err = s.execute("automate.removeStep", json!({"name": "Build", "index": 3})).unwrap_err().to_string();
        assert!(err.contains("has no step 3"), "{err}");
        let err = s.execute("automate.removeStep", json!({"name": "Nope", "index": 0})).unwrap_err().to_string();
        assert!(err.contains("no script named"), "{err}");
    }

    #[test]
    fn automate_append_step_adds_at_the_end_and_refuses_hostile_input() {
        let mut s = with_script(json!([step("cell.set", json!({"cell": "A1", "input": "1"}))]));
        let r = s.execute("automate.appendStep", json!({"name": "Build", "command": "home.bold", "params": {"range": "A1:A1"}})).unwrap();
        assert_eq!(r, json!({"name": "Build", "index": 1, "steps": 2}));
        assert_eq!(listed(&mut s, "Build")[1], step("home.bold", json!({"range": "A1:A1"})));
        // No params is an empty object, not a null step.
        s.execute("automate.appendStep", json!({"name": "Build", "command": "home.bold"})).unwrap();
        assert_eq!(listed(&mut s, "Build")[2], step("home.bold", json!({})));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(listed(&mut s, "Build").as_array().map(Vec::len), Some(2));

        let err = s.execute("automate.appendStep", json!({"name": "Build", "command": "not.aCommand"})).unwrap_err().to_string();
        assert!(err.contains("is not a command"), "{err}");
        let err = s.execute("automate.appendStep", json!({"name": "Build"})).unwrap_err().to_string();
        assert!(err.contains("missing `command`"), "{err}");
        let err = s.execute("automate.appendStep", json!({"name": "Build", "command": "home.bold", "params": "A1"})).unwrap_err().to_string();
        assert!(err.contains("must be a JSON object"), "{err}");
        // A megabyte blob is refused before it reaches the workbook.
        let blob = json!({"text": "x".repeat(1_000_000)});
        let err = s.execute("automate.appendStep", json!({"name": "Build", "command": "cell.set", "params": blob})).unwrap_err().to_string();
        assert!(err.contains("bytes of JSON"), "{err}");
        assert_eq!(listed(&mut s, "Build").as_array().map(Vec::len), Some(2), "a refused step is not added");
    }

    #[test]
    fn automate_append_step_refuses_a_script_at_the_step_cap() {
        let many: Vec<Json> = (0..MAX_SCRIPT_STEPS).map(|_| step("home.bold", json!({}))).collect();
        let mut s = with_script(json!(many));
        let err = s.execute("automate.appendStep", json!({"name": "Build", "command": "home.bold"})).unwrap_err().to_string();
        assert!(err.contains("already holds"), "{err}");
    }

    #[test]
    fn automate_rename_script_refuses_a_name_another_script_already_uses() {
        let mut s = with_script(json!([step("home.bold", json!({}))]));
        s.execute("automate.newScript", json!({"name": "Other", "commands": []})).unwrap();
        assert_eq!(s.execute("automate.renameScript", json!({"name": "Build", "to": " Renamed "})).unwrap(), json!({"name": "Renamed"}));
        assert_eq!(listed(&mut s, "Renamed"), json!([step("home.bold", json!({}))]));
        fn names(s: &mut Session) -> Vec<String> {
            let list = s.execute("automate.allScripts", json!({})).unwrap();
            let mut out: Vec<String> =
                list.as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            out.sort();
            out
        }
        assert_eq!(names(&mut s), vec!["Other".to_string(), "Renamed".to_string()]);
        // An existing name, a blank one, a script that isn't there and a name past the cap are refused.
        let err = s.execute("automate.renameScript", json!({"name": "Renamed", "to": "Other"})).unwrap_err().to_string();
        assert!(err.contains("already the name of a script"), "{err}");
        let err = s.execute("automate.renameScript", json!({"name": "Renamed", "to": "  "})).unwrap_err().to_string();
        assert!(err.contains("blank"), "{err}");
        let err = s.execute("automate.renameScript", json!({"name": "Nope", "to": "Fresh"})).unwrap_err().to_string();
        assert!(err.contains("no script named"), "{err}");
        let err = s.execute("automate.renameScript", json!({"name": "Renamed", "to": "n".repeat(256)})).unwrap_err().to_string();
        assert!(err.contains("255"), "{err}");
        // Renaming is an ordinary undoable edit.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(names(&mut s), vec!["Build".to_string(), "Other".to_string()]);
    }
}
