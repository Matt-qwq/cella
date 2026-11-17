//! The data table under the plot: where its cells sit, what they say, and which of its three
//! border sets are drawn.

use super::*;

/// A column chart with nothing but the plot and its table in it, so a cell's position is the
/// plot's: no title, no legend, no axis titles, no data labels.
fn table_chart(kind: ChartKind) -> Chart {
    let mut c = chart(kind);
    c.title = None;
    c.x_title = None;
    c.y_title = None;
    c.legend = LegendPos::None;
    c.data_labels = false;
    c.data_table = Some(cella_model::ChartDataTable::default());
    c
}

fn rate_and_units() -> ChartData {
    let mut rate = SeriesData {
        name: "Rate".into(),
        values: vec![Some(0.25), None, Some(0.75)],
        number_format: "0.0%".into(),
        color: [1, 2, 3, 255],
        ..SeriesData::default()
    };
    rate.kind = ChartKind::ColumnClustered;
    ChartData {
        categories: ["Jan", "Feb", "Mar"].map(String::from).to_vec(),
        series: vec![
            rate,
            SeriesData {
                name: "Units".into(),
                values: vec![Some(1200.0), Some(950.0), Some(3050.0)],
                number_format: "#,##0".into(),
                color: [9, 8, 7, 255],
                kind: ChartKind::ColumnClustered,
                ..SeriesData::default()
            },
        ],
        ..ChartData::default()
    }
}

/// The (x, y) a drawn text sits at; the first one with that text.
fn at(prims: &[Prim], text: &str) -> (f32, f32) {
    prims
        .iter()
        .find_map(|p| match p {
            Prim::Text { text: t, x, y, .. } if t == text => Some((*x, *y)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {text}: {:?}", texts(prims)))
}

#[test]
fn three_rows_and_two_series_land_on_the_plots_columns() {
    let c = table_chart(ChartKind::ColumnClustered);
    let d = rate_and_units();
    let p = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    assert_sane(&p, 600.0, 400.0);
    // The header row names the categories, once each: the table replaces the axis labels.
    let (jx, header_y) = at(&p, "Jan");
    let (fx, fy) = at(&p, "Feb");
    let (mx, my) = at(&p, "Mar");
    assert_eq!((fy, my), (header_y, header_y), "the category row is one row");
    assert_eq!(texts(&p).iter().filter(|t| **t == "Jan").count(), 1);
    // A value sits under its own category, on its series' row.
    let (v25, y25) = at(&p, "25.0%");
    let (v75, y75) = at(&p, "75.0%");
    assert_eq!((v25, v75), (jx, mx), "values centre on their column");
    assert!((y25 - y75).abs() < 0.01);
    assert!(y25 > header_y, "the values are below the header");
    assert!(fx > jx && mx > fx, "the columns run left to right");
    // The second series is the next row down, and a missing source value is blank, not a zero.
    let (u, uy) = at(&p, "1,200");
    assert!((u - jx).abs() < 0.01);
    assert!(uy > y25);
    let row: Vec<&str> = p
        .iter()
        .filter_map(|q| match q {
            Prim::Text { y, text, .. } if (*y - y25).abs() < 0.01 => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(row, ["Rate", "25.0%", "75.0%"], "a gap stays blank, not a zero");
    // The series names stand in the stub, left of the plot's first column.
    let (r, ry) = at(&p, "Rate");
    let (units, _) = at(&p, "Units");
    assert!(r < jx && units < jx, "the stub is left of the plot");
    assert!((r - units).abs() < 0.01, "and one column wide");
    assert!((ry - y25).abs() < 0.01, "a series name sits on its own row");
}

#[test]
fn the_legend_key_and_the_table_are_independent_of_the_chart_legend() {
    let c = table_chart(ChartKind::ColumnClustered);
    let d = rate_and_units();
    let color = [1, 2, 3, 255];
    let keys =
        |p: &[Prim], header_y: f32| p.iter().filter(|q| matches!(q, Prim::Rect { y, fill: Some(f), .. } if *f == color && *y > header_y)).count();
    let p = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    let header_y = at(&p, "Jan").1;
    assert_eq!(keys(&p, header_y), 1, "one key row per series, with the legend itself off");
    let mut no_keys = c.clone();
    no_keys.data_table = Some(cella_model::ChartDataTable { show_keys: false, ..Default::default() });
    let p = render(&no_keys, &d, 600.0, 400.0, &ApproxMeasure);
    assert_eq!(keys(&p, at(&p, "Jan").1), 0);
    // A table-less chart draws no header row at all, and the axis labels come back.
    let mut off = c.clone();
    off.data_table = None;
    let plain = render(&off, &d, 600.0, 400.0, &ApproxMeasure);
    assert!(!texts(&plain).contains(&"Units"), "no series stub without a table");
}

#[test]
fn the_three_border_sets_are_drawn_independently() {
    let mut c = table_chart(ChartKind::Line);
    let mut d = sample(ChartKind::Line);
    d.category_levels = vec![vec!["2025".into(), "".into(), "".into(), "2026".into(), "".into(), "".into()]];
    assert_eq!(d.series.len(), 3);
    for (h, v, outline, want) in [(false, false, false, 0), (true, false, false, 4), (false, true, false, 6), (false, false, true, 4)] {
        c.data_table = Some(cella_model::ChartDataTable { horizontal_border: h, vertical_border: v, outline, show_keys: false });
        let p = render(&c, &d, 900.0, 600.0, &ApproxMeasure);
        // The outer level's name is in the table's first row, so everything below it is the table.
        let year_y = at(&p, "2025").1;
        assert!(texts(&p).contains(&"2026"), "both groups are named");
        let lines = p.iter().filter(|q| matches!(q, Prim::Line { pts, .. } if pts.iter().all(|pt| pt[1] > year_y - 9.0))).count();
        assert_eq!(lines, want, "horizontal={h} vertical={v} outline={outline}");
        assert_sane(&p, 900.0, 600.0);
    }
}

#[test]
fn a_table_too_dense_to_print_says_so_and_stays_bounded() {
    let mut c = table_chart(ChartKind::ColumnClustered);
    let mut d = rate_and_units();
    d.categories = (0..20_000).map(|i| i.to_string()).collect();
    let p = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    assert!(texts(&p).contains(&"Data table needs more space"), "{:?}", texts(&p));
    assert_sane(&p, 600.0, 400.0);
    // A chart too small for the band draws nothing of it, and never leaves the box.
    d.categories.clear();
    for (w, h) in [(1.0, 1.0), (10.0, 10.0), (40.0, 35.0), (200.0, 100.0)] {
        assert_sane(&render(&c, &d, w, h, &ApproxMeasure), w, h);
    }
    // A chart kind with no table in Excel draws none, however the model is filled in.
    c.kind = ChartKind::Pie;
    assert_eq!(render(&c, &d, 600.0, 400.0, &ApproxMeasure), {
        let mut off = c.clone();
        off.data_table = None;
        render(&off, &d, 600.0, 400.0, &ApproxMeasure)
    });
}
