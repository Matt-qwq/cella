//! GridCraft's icon set, drawn in code on a 16×16 design grid (original artwork; see
//! ATTRIBUTION.md). Large ribbon icons use the same drawings at 32×32 with accent colours.
//!
//! Every drawing is one of two weights (see [`HAIRLINE`] and [`MARK`]), scaled with the box it is
//! painted in, and the ink of every icon stays inside its 16-unit box
//! (`every_icon_stays_inside_its_box`).

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

/// The design grid every icon is drawn on: the `paint` rectangle is scaled to this, so a drawing
/// reads the same at 16 (ribbon buttons), 30 (large ribbon buttons) and 32 (unscaled doubles).
pub const GRID: f32 = 16.0;

/// The structural weight: outlines, table grids, page edges, separators. Everything that is shape
/// rather than message is this thickness.
const HAIRLINE: f32 = 1.15;
/// The weight of the mark an icon is about: a check, a cross, an arrow, a plus, a chevron. One step
/// heavier than the structure it sits on, so the glyph reads first at 16 px.
const MARK: f32 = 1.5;
/// Radius of a filled dot: the More button's three, a calculator's keys, Info's i.
const DOT: f32 = 0.9;
/// The edge of a paper-white shape inside an icon (a sheet, a page, a speech bubble). A sheet's
/// edge is its own neutral, not the theme's ink: on the dark ribbon that ink is nearly white, so
/// an outline drawn in it vanished into the paper and left a blank rectangle.
const PAPER_INK: Color32 = Color32::from_gray(0x60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Paste,
    Cut,
    Copy,
    Brush,
    Bold,
    Italic,
    Underline,
    Strike,
    Borders,
    BorderAll,
    BorderNone,
    BorderOutside,
    BorderThick,
    Fill,
    FontColor,
    AlignTop,
    AlignMiddle,
    AlignBottom,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Orientation,
    IndentDec,
    IndentInc,
    Wrap,
    Merge,
    Currency,
    Percent,
    Comma,
    DecInc,
    DecDec,
    CondFormat,
    FormatTable,
    CellStyles,
    Insert,
    Delete,
    Format,
    Sum,
    FillDown,
    Eraser,
    SortFilter,
    Find,
    Undo,
    Redo,
    Save,
    Home,
    More,
    Chevron,
    ChevronUp,
    Close,
    Check,
    Fx,
    Plus,
    Left,
    Right,
    Normal,
    PageLayout,
    PageBreak,
    Minus,
    /// Window caption buttons: maximize (one square) and restore (a square behind a square).
    Maximize,
    Restore,
    Table,
    PivotTable,
    Chart,
    ChartBar,
    ChartLine,
    ChartPie,
    ChartScatter,
    Picture,
    Shapes,
    Link,
    Comment,
    Note,
    TextBox,
    /// Decorative text (Insert › Text › WordArt): a big outlined A.
    WordArt,
    Symbol,
    Sparkline,
    Checkbox,
    Function,
    Name,
    Trace,
    Calc,
    Filter,
    SortAsc,
    SortDesc,
    Duplicates,
    TextColumns,
    Validation,
    Group,
    Ungroup,
    Subtotal,
    Lock,
    Spell,
    Freeze,
    Zoom,
    Gridlines,
    Theme,
    Margins,
    Orient,
    SheetRtl,
    Paper,
    PrintArea,
    Script,
    Pen,
    Share,
    Search,
    Settings,
    /// Light/dark appearance: a circle, its right half filled.
    Appearance,
    Sheet,
    /// All Sheets navigator next to the sheet tabs: a list of rows with a bullet each.
    SheetList,
    Book,
    Folder,
    /// Header & Footer: a page with its top / bottom band marked.
    Header,
    Footer,
    PageNumber,
    PageCount,
    Calendar,
    Clock,
    FileName,
    /// File › Backstage navigation (line art, drawn in the navigation colour).
    Back,
    NewFile,
    OpenFolder,
    Info,
    Print,
}

struct G<'a> {
    p: &'a Painter,
    r: Rect,
    s: f32,
}

impl G<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x * self.s, self.r.top() + y * self.s)
    }
    /// How the design weights grow with the box: a 16 px icon draws them as written, a 30 px large
    /// ribbon icon at up to twice the thickness so both read at the same visual weight.
    fn scaled(&self) -> f32 {
        self.s.clamp(1.0, 2.0)
    }
    /// A stroke in design units (see [`HAIRLINE`], [`MARK`]), for the shapes that take a
    /// `Stroke` of their own: beziers.
    fn weight(&self, w: f32, c: Color32) -> Stroke {
        Stroke::new(w * self.scaled(), c)
    }
    fn line(&self, pts: &[(f32, f32)], c: Color32, w: f32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(PathShape::line(v, PathStroke::new(w * self.scaled(), c)));
    }
    fn stroke(&self, pts: &[(f32, f32)], c: Color32) {
        self.line(pts, c, HAIRLINE);
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, fill: Option<Color32>, stroke: Option<Color32>) {
        let r = Rect::from_min_size(self.pt(x, y), vec2(w * self.s, h * self.s));
        if let Some(f) = fill {
            self.p.rect_filled(r, 1.0, f);
        }
        if let Some(c) = stroke {
            // The outline scales with the box like `line` does; a fixed 1 px edge left large icons
            // with hairline rectangles inside 2 px strokes.
            self.p.rect_stroke(r, 1.0, Stroke::new(HAIRLINE * self.scaled(), c), egui::StrokeKind::Inside);
        }
    }
    fn poly(&self, pts: &[(f32, f32)], fill: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.p.add(PathShape::convex_polygon(v, fill, Stroke::NONE));
    }
    fn circle(&self, x: f32, y: f32, r: f32, fill: Option<Color32>, stroke: Option<Color32>) {
        if let Some(f) = fill {
            self.p.circle_filled(self.pt(x, y), r * self.s, f);
        }
        if let Some(c) = stroke {
            self.p.circle_stroke(self.pt(x, y), r * self.s, Stroke::new(HAIRLINE * self.scaled(), c));
        }
    }
    /// A letter or digit of the drawing, centred on its ink rather than on its line box.
    ///
    /// egui centres a galley including the descent and line gap below the baseline, which are
    /// empty for a capital or a digit: centring the box drew every letter icon (B, $, Σ, fx…)
    /// about a fifth of an em above the middle of its 16-unit box, so a row of them read as a
    /// row of bobbing glyphs. The mesh bounds are the ink, so this centres what is seen.
    fn text(&self, x: f32, y: f32, t: &str, size: f32, c: Color32, bold: bool, italic: bool) {
        let fam = if bold {
            crate::theme::CELL_BOLD
        } else if italic {
            crate::theme::CELL_ITALIC
        } else {
            crate::theme::CELL
        };
        let font = egui::FontId::new(size * self.s, egui::FontFamily::Name(fam.into()));
        let galley = self.p.layout_no_wrap(t.to_string(), font, c);
        let ink = galley.mesh_bounds;
        // An empty galley has no ink to centre on; leave it where the line box puts it.
        let at = if ink.is_positive() { self.pt(x, y) - ink.center().to_vec2() } else { self.pt(x, y) };
        self.p.galley(at, galley, c);
    }
}

/// Accent palette for icons (our own).
pub const GREEN: Color32 = Color32::from_rgb(0x10, 0x7C, 0x41);
pub const BLUE: Color32 = Color32::from_rgb(0x2B, 0x7C, 0xD3);
pub const RED: Color32 = Color32::from_rgb(0xD1, 0x3B, 0x2F);
pub const YELLOW: Color32 = Color32::from_rgb(0xF2, 0xC3, 0x2E);
pub const ORANGE: Color32 = Color32::from_rgb(0xE8, 0x7D, 0x2B);
pub const PURPLE: Color32 = Color32::from_rgb(0x88, 0x55, 0xC8);

/// Draws `icon` into `rect` (square-ish), in colour `c`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    let side = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(side, side));
    let g = G { p, r, s: side / GRID };
    let light = c.gamma_multiply(0.45);
    match icon {
        Icon::Paste => {
            g.rect(3.0, 2.5, 9.0, 12.0, Some(Color32::from_rgb(0xC8, 0x9B, 0x5C)), None);
            g.rect(5.0, 1.5, 5.0, 2.5, Some(Color32::from_rgb(0x8A, 0x8A, 0x8A)), None);
            g.rect(6.5, 6.0, 7.5, 9.0, Some(Color32::WHITE), Some(PAPER_INK));
            for y in [8.5, 10.5, 12.5] {
                g.stroke(&[(8.0, y), (12.5, y)], light);
            }
        }
        Icon::Cut => {
            g.circle(5.0, 12.0, 2.0, None, Some(c));
            g.circle(11.0, 12.0, 2.0, None, Some(c));
            g.stroke(&[(6.3, 10.6), (11.5, 2.0)], c);
            g.stroke(&[(9.7, 10.6), (4.5, 2.0)], c);
        }
        Icon::Copy => {
            g.rect(2.5, 2.5, 7.5, 9.0, Some(Color32::WHITE), Some(PAPER_INK));
            g.rect(6.0, 5.5, 7.5, 9.0, Some(Color32::WHITE), Some(PAPER_INK));
        }
        Icon::Brush => {
            g.rect(3.0, 2.0, 10.0, 4.0, Some(YELLOW), Some(c));
            g.stroke(&[(8.0, 6.0), (8.0, 8.5), (5.0, 8.5), (5.0, 10.0)], c);
            g.rect(4.0, 10.0, 2.0, 4.5, Some(c), None);
        }
        Icon::Bold => g.text(8.0, 8.5, "B", 12.0, c, true, false),
        Icon::Italic => g.text(8.0, 8.5, "I", 12.0, c, false, true),
        Icon::Underline => {
            g.text(8.0, 7.5, "U", 11.0, c, false, false);
            g.line(&[(4.0, 14.0), (12.0, 14.0)], c, MARK);
        }
        Icon::Strike => {
            g.text(8.0, 8.0, "ab", 10.0, c, false, false);
            g.line(&[(2.5, 8.5), (13.5, 8.5)], c, MARK);
        }
        Icon::Borders | Icon::BorderAll => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(light));
            g.stroke(&[(8.0, 2.5), (8.0, 13.5)], light);
            g.stroke(&[(2.5, 8.0), (13.5, 8.0)], light);
            if icon == Icon::Borders {
                g.line(&[(2.0, 13.5), (14.0, 13.5)], c, MARK);
            } else {
                g.rect(2.5, 2.5, 11.0, 11.0, None, Some(c));
                g.stroke(&[(8.0, 2.5), (8.0, 13.5)], c);
                g.stroke(&[(2.5, 8.0), (13.5, 8.0)], c);
            }
        }
        Icon::BorderNone => {
            for i in 0..6 {
                let t = 2.5 + i as f32 * 2.2;
                g.circle(t, 2.5, 0.4, Some(light), None);
                g.circle(t, 13.5, 0.4, Some(light), None);
                g.circle(2.5, t, 0.4, Some(light), None);
                g.circle(13.5, t, 0.4, Some(light), None);
            }
        }
        Icon::BorderOutside | Icon::BorderThick => {
            g.stroke(&[(8.0, 2.5), (8.0, 13.5)], light);
            g.stroke(&[(2.5, 8.0), (13.5, 8.0)], light);
            g.line(&[(2.5, 2.5), (13.5, 2.5), (13.5, 13.5), (2.5, 13.5), (2.5, 2.5)], c, if icon == Icon::BorderThick { 2.2 } else { 1.2 });
        }
        Icon::Fill => {
            g.poly(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0)], Color32::WHITE);
            g.line(&[(3.0, 7.0), (8.0, 2.0), (13.0, 7.0), (8.0, 12.0), (3.0, 7.0)], PAPER_INK, HAIRLINE);
            g.circle(13.5, 10.0, 1.2, Some(c), None);
        }
        Icon::FontColor => {
            g.text(8.0, 7.0, "A", 11.0, c, false, false);
        }
        Icon::AlignTop | Icon::AlignMiddle | Icon::AlignBottom => {
            let ys: [f32; 3] = match icon {
                Icon::AlignTop => [3.0, 5.5, 8.0],
                Icon::AlignMiddle => [5.5, 8.0, 10.5],
                _ => [8.0, 10.5, 13.0],
            };
            // The edge the bars stack against: the top line for Align Top, the bottom for Align
            // Bottom, and both for Align Middle, which has no edge of its own.
            for edge in match icon {
                Icon::AlignTop => &[1.5][..],
                Icon::AlignBottom => &[14.5][..],
                _ => &[1.5, 14.5][..],
            } {
                g.stroke(&[(2.0, *edge), (14.0, *edge)], light);
            }
            for (i, y) in ys.iter().enumerate() {
                let w = if i == 1 { 8.0 } else { 10.0 };
                g.stroke(&[(8.0 - w / 2.0, *y), (8.0 + w / 2.0, *y)], c);
            }
        }
        Icon::AlignLeft | Icon::AlignCenter | Icon::AlignRight | Icon::IndentDec | Icon::IndentInc => {
            for (i, y) in [3.5f32, 6.5, 9.5, 12.5].iter().enumerate() {
                let w = if i % 2 == 0 { 12.0 } else { 8.0 };
                let (x0, x1) = match icon {
                    Icon::AlignLeft => (2.0, 2.0 + w),
                    Icon::AlignRight => (14.0 - w, 14.0),
                    Icon::AlignCenter => (8.0 - w / 2.0, 8.0 + w / 2.0),
                    _ => (if i == 0 || i == 3 { 2.0 } else { 7.0 }, 14.0),
                };
                g.stroke(&[(x0, *y), (x1, *y)], c);
            }
            if icon == Icon::IndentInc {
                g.poly(&[(2.0, 6.0), (5.0, 8.0), (2.0, 10.0)], BLUE);
            }
            if icon == Icon::IndentDec {
                g.poly(&[(5.0, 6.0), (2.0, 8.0), (5.0, 10.0)], BLUE);
            }
        }
        Icon::Orientation => {
            g.text(6.0, 9.0, "ab", 8.5, c, false, false);
            g.stroke(&[(9.0, 13.0), (14.0, 3.0)], BLUE);
            g.poly(&[(14.5, 2.0), (12.0, 4.0), (14.6, 5.2)], BLUE);
        }
        Icon::Wrap => {
            g.stroke(&[(2.0, 4.0), (14.0, 4.0)], c);
            g.stroke(&[(2.0, 8.0), (12.0, 8.0)], c);
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(12.0, 8.0), g.pt(15.0, 8.0), g.pt(15.0, 12.0), g.pt(11.0, 12.0)],
                false,
                Color32::TRANSPARENT,
                g.weight(HAIRLINE, BLUE),
            ));
            g.poly(&[(9.0, 12.0), (11.5, 10.5), (11.5, 13.5)], BLUE);
            g.stroke(&[(2.0, 12.0), (7.0, 12.0)], c);
        }
        Icon::Merge => {
            g.rect(1.5, 3.5, 13.0, 9.0, None, Some(c));
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], BLUE);
            g.poly(&[(3.0, 8.0), (5.5, 6.5), (5.5, 9.5)], BLUE);
            g.poly(&[(13.0, 8.0), (10.5, 6.5), (10.5, 9.5)], BLUE);
        }
        Icon::Currency => g.text(8.0, 8.5, "$", 12.0, c, false, false),
        Icon::Percent => g.text(8.0, 8.5, "%", 11.0, c, false, false),
        Icon::Comma => g.text(8.0, 8.0, "1,0", 9.0, c, false, false),
        Icon::DecInc => {
            g.text(5.0, 5.0, ".0", 6.5, c, false, false);
            g.text(10.0, 11.0, ".00", 6.5, c, false, false);
            g.stroke(&[(2.0, 12.0), (5.0, 12.0)], BLUE);
            g.poly(&[(1.0, 12.0), (2.8, 10.6), (2.8, 13.4)], BLUE);
        }
        Icon::DecDec => {
            g.text(6.0, 5.0, ".00", 6.5, c, false, false);
            g.text(11.0, 11.0, ".0", 6.5, c, false, false);
            g.stroke(&[(2.0, 12.0), (5.5, 12.0)], BLUE);
            g.poly(&[(7.0, 12.0), (5.2, 10.6), (5.2, 13.4)], BLUE);
        }
        Icon::CondFormat => {
            g.rect(2.0, 2.0, 12.0, 12.0, Some(Color32::WHITE), Some(light));
            g.rect(3.0, 3.5, 4.0, 2.5, Some(RED), None);
            g.rect(3.0, 7.0, 7.0, 2.5, Some(YELLOW), None);
            g.rect(3.0, 10.5, 10.0, 2.5, Some(BLUE), None);
        }
        Icon::FormatTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(Color32::WHITE), Some(light));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(BLUE), None);
            g.rect(2.0, 8.0, 12.0, 2.5, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], light);
        }
        Icon::CellStyles => {
            g.rect(2.0, 3.0, 12.0, 4.5, Some(Color32::from_rgb(0xC6, 0xEF, 0xCE)), Some(light));
            g.rect(2.0, 8.5, 12.0, 4.5, Some(Color32::from_rgb(0xFF, 0xC7, 0xCE)), Some(light));
            g.stroke(&[(4.0, 5.2), (9.0, 5.2)], GREEN);
            g.stroke(&[(4.0, 10.7), (9.0, 10.7)], RED);
        }
        Icon::Insert | Icon::Delete | Icon::Format => {
            g.rect(2.0, 3.0, 12.0, 10.0, Some(Color32::WHITE), Some(PAPER_INK));
            g.stroke(&[(2.0, 6.5), (14.0, 6.5)], light);
            g.stroke(&[(2.0, 10.0), (14.0, 10.0)], light);
            g.stroke(&[(6.0, 3.0), (6.0, 13.0)], light);
            match icon {
                Icon::Insert => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), Some(BLUE));
                    g.line(&[(1.0, 8.2), (4.5, 8.2)], GREEN, MARK);
                    g.line(&[(2.75, 6.5), (2.75, 10.0)], GREEN, MARK);
                }
                Icon::Delete => {
                    g.line(&[(8.0, 6.0), (13.0, 11.0)], RED, MARK);
                    g.line(&[(13.0, 6.0), (8.0, 11.0)], RED, MARK);
                }
                _ => {
                    g.rect(6.0, 6.5, 8.0, 3.5, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
                }
            }
        }
        Icon::Sum => g.text(8.0, 8.0, "Σ", 13.0, c, false, false),
        Icon::FillDown => {
            g.rect(4.0, 2.0, 8.0, 12.0, Some(Color32::WHITE), Some(PAPER_INK));
            g.stroke(&[(8.0, 4.0), (8.0, 11.0)], BLUE);
            g.poly(&[(5.5, 9.5), (10.5, 9.5), (8.0, 13.0)], BLUE);
        }
        Icon::Eraser => {
            g.poly(&[(2.0, 10.0), (8.0, 4.0), (12.0, 8.0), (6.0, 14.0)], Color32::from_rgb(0xE9, 0x8A, 0xB8));
            g.poly(&[(8.0, 4.0), (10.5, 1.5), (14.5, 5.5), (12.0, 8.0)], Color32::from_rgb(0xB6, 0x4E, 0x86));
        }
        Icon::SortFilter => {
            g.text(4.5, 4.5, "A", 6.0, c, false, false);
            g.text(4.5, 11.5, "Z", 6.0, c, false, false);
            g.stroke(&[(8.0, 3.0), (8.0, 13.0)], BLUE);
            g.poly(&[(6.5, 11.0), (9.5, 11.0), (8.0, 13.5)], BLUE);
            g.poly(&[(10.0, 3.0), (15.0, 3.0), (13.0, 7.5), (13.0, 12.0), (12.0, 11.0), (12.0, 7.5)], light);
        }
        Icon::Find | Icon::Search => {
            g.circle(6.5, 6.5, 4.0, None, Some(c));
            g.line(&[(9.5, 9.5), (14.0, 14.0)], c, MARK);
        }
        Icon::Zoom => {
            g.circle(6.5, 6.5, 4.5, None, Some(c));
            g.line(&[(10.0, 10.0), (14.5, 14.5)], c, MARK);
            g.line(&[(4.5, 6.5), (8.5, 6.5)], BLUE, MARK);
            g.line(&[(6.5, 4.5), (6.5, 8.5)], BLUE, MARK);
        }
        Icon::Undo | Icon::Redo => {
            let flip = icon == Icon::Redo;
            let x = |v: f32| if flip { 16.0 - v } else { v };
            g.p.add(CubicBezierShape::from_points_stroke(
                [g.pt(x(4.0), 7.0), g.pt(x(9.0), 3.0), g.pt(x(15.0), 6.0), g.pt(x(12.0), 12.5)],
                false,
                Color32::TRANSPARENT,
                g.weight(HAIRLINE, c),
            ));
            g.poly(&[(x(2.5), 7.5), (x(6.5), 4.5), (x(6.5), 9.5)], c);
        }
        Icon::Save => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(c));
            g.rect(5.0, 2.5, 6.0, 4.0, None, Some(c));
            g.rect(4.5, 9.0, 7.0, 4.5, None, Some(c));
        }
        Icon::Home => {
            g.line(&[(2.0, 8.0), (8.0, 2.5), (14.0, 8.0)], c, HAIRLINE);
            g.line(&[(4.0, 6.5), (4.0, 13.5), (12.0, 13.5), (12.0, 6.5)], c, HAIRLINE);
        }
        Icon::SheetList => {
            for y in [4.0, 8.0, 12.0] {
                g.rect(2.5, y - 1.0, 2.0, 2.0, Some(c), None);
                g.stroke(&[(6.5, y), (13.5, y)], c);
            }
        }
        Icon::More => {
            for x in [4.0, 8.0, 12.0] {
                g.circle(x, 8.0, DOT, Some(c), None);
            }
        }
        Icon::Chevron => g.line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)], c, MARK),
        Icon::ChevronUp => g.line(&[(4.5, 10.0), (8.0, 6.5), (11.5, 10.0)], c, MARK),
        Icon::Left => g.line(&[(10.0, 4.0), (6.0, 8.0), (10.0, 12.0)], c, MARK),
        Icon::Right => g.line(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)], c, MARK),
        Icon::Close => {
            g.line(&[(4.0, 4.0), (12.0, 12.0)], c, MARK);
            g.line(&[(12.0, 4.0), (4.0, 12.0)], c, MARK);
        }
        Icon::Check => g.line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)], c, MARK),
        Icon::Checkbox => {
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(c));
            g.line(&[(4.5, 8.0), (7.0, 10.5), (11.5, 5.0)], c, MARK);
        }
        Icon::Fx => g.text(8.0, 8.0, "fx", 10.5, c, false, true),
        Icon::Plus => {
            g.line(&[(8.0, 3.0), (8.0, 13.0)], c, MARK);
            g.line(&[(3.0, 8.0), (13.0, 8.0)], c, MARK);
        }
        Icon::Minus => g.line(&[(3.0, 8.0), (13.0, 8.0)], c, MARK),
        // The window buttons of the caption, drawn to the same 10-unit width as the minus so the
        // three read as one row: a square, and a square behind a square.
        Icon::Maximize => g.rect(3.0, 3.0, 10.0, 10.0, None, Some(c)),
        Icon::Restore => {
            g.line(&[(3.0, 8.0), (3.0, 3.0), (8.0, 3.0)], c, HAIRLINE);
            g.rect(5.5, 5.5, 7.5, 7.5, None, Some(c));
        }
        Icon::Normal | Icon::Gridlines | Icon::Sheet => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], c);
            g.stroke(&[(10.0, 2.5), (10.0, 13.5)], c);
            g.stroke(&[(2.0, 6.2), (14.0, 6.2)], c);
            g.stroke(&[(2.0, 9.8), (14.0, 9.8)], c);
        }
        Icon::PageLayout | Icon::Paper => {
            g.rect(3.5, 1.5, 9.0, 13.0, None, Some(c));
            for y in [5.0, 7.5, 10.0] {
                g.stroke(&[(5.5, y), (10.5, y)], light);
            }
        }
        Icon::PageBreak => {
            g.rect(2.5, 2.0, 11.0, 12.0, None, Some(c));
            g.line(&[(2.5, 8.0), (13.5, 8.0)], BLUE, MARK);
        }
        Icon::Table | Icon::PivotTable => {
            g.rect(2.0, 2.5, 12.0, 11.0, Some(Color32::WHITE), Some(PAPER_INK));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(if icon == Icon::Table { BLUE } else { GREEN }), None);
            g.stroke(&[(6.0, 2.5), (6.0, 13.5)], light);
            g.stroke(&[(2.0, 9.5), (14.0, 9.5)], light);
            if icon == Icon::PivotTable {
                g.poly(&[(9.0, 7.0), (12.5, 7.0), (12.5, 10.5)], ORANGE);
            }
        }
        Icon::Chart | Icon::ChartBar => {
            g.stroke(&[(2.0, 14.0), (14.0, 14.0)], c);
            g.rect(3.0, 8.0, 2.5, 6.0, Some(BLUE), None);
            g.rect(6.8, 4.0, 2.5, 10.0, Some(GREEN), None);
            g.rect(10.6, 6.0, 2.5, 8.0, Some(ORANGE), None);
        }
        Icon::ChartLine | Icon::Sparkline => {
            g.stroke(&[(2.0, 14.0), (14.0, 14.0)], light);
            g.line(&[(2.0, 11.0), (5.5, 7.0), (8.5, 9.5), (13.5, 3.5)], BLUE, MARK);
        }
        Icon::ChartPie => {
            g.circle(8.0, 8.0, 6.0, Some(BLUE), None);
            g.poly(&[(8.0, 8.0), (8.0, 2.0), (12.0, 3.4), (14.0, 8.0)], ORANGE);
        }
        Icon::ChartScatter => {
            g.stroke(&[(2.0, 2.0), (2.0, 14.0), (14.0, 14.0)], c);
            for (x, y) in [(5.0, 11.0), (7.0, 8.0), (9.5, 9.5), (11.0, 5.0), (13.0, 4.0)] {
                g.circle(x, y, 1.1, Some(BLUE), None);
            }
        }
        Icon::Picture => {
            g.rect(2.0, 3.0, 12.0, 10.0, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), Some(PAPER_INK));
            g.poly(&[(3.0, 12.0), (7.0, 7.0), (10.0, 10.0), (11.5, 8.5), (13.0, 12.0)], GREEN);
            g.circle(11.0, 5.5, 1.2, Some(YELLOW), None);
        }
        Icon::Shapes => {
            g.circle(5.5, 5.5, 3.5, Some(BLUE), None);
            g.rect(7.0, 7.0, 7.0, 7.0, Some(ORANGE), None);
        }
        Icon::Link => {
            g.line(&[(6.0, 10.0), (10.0, 6.0)], c, HAIRLINE);
            g.rect(2.0, 8.0, 6.0, 4.0, None, Some(c));
            g.rect(8.0, 4.0, 6.0, 4.0, None, Some(c));
        }
        Icon::Comment | Icon::Note => {
            g.poly(
                &[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (2.0, 11.0)],
                if icon == Icon::Note { Color32::from_rgb(0xFF, 0xF2, 0xA8) } else { Color32::WHITE },
            );
            g.line(&[(2.0, 3.0), (14.0, 3.0), (14.0, 11.0), (7.0, 11.0), (4.0, 14.0), (4.0, 11.0), (2.0, 11.0), (2.0, 3.0)], PAPER_INK, HAIRLINE);
        }
        Icon::TextBox => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.text(8.0, 8.0, "A", 8.0, c, false, false);
        }
        Icon::WordArt => {
            // The drop shadow first, then the glyph, as a WordArt draws its text.
            g.text(8.6, 8.6, "A", 12.0, light, true, false);
            g.text(8.0, 8.0, "A", 12.0, c, true, false);
        }
        Icon::Symbol => g.text(8.0, 8.0, "Ω", 12.0, c, false, false),
        Icon::Function => g.text(8.0, 8.0, "fx", 11.0, c, false, true),
        Icon::Name => {
            g.rect(1.5, 4.0, 13.0, 8.0, None, Some(c));
            g.text(8.0, 8.0, "ab", 8.0, c, false, false);
        }
        Icon::Trace => {
            g.circle(4.0, 12.0, 1.6, Some(BLUE), None);
            g.circle(12.0, 4.0, 1.6, Some(BLUE), None);
            g.line(&[(5.0, 11.0), (11.0, 5.0)], BLUE, HAIRLINE);
            g.poly(&[(12.5, 3.5), (9.0, 4.5), (11.5, 7.0)], BLUE);
        }
        Icon::Calc => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.rect(4.5, 3.0, 7.0, 3.0, Some(light), None);
            for (x, y) in [(5.5, 8.5), (8.0, 8.5), (10.5, 8.5), (5.5, 11.5), (8.0, 11.5), (10.5, 11.5)] {
                g.circle(x, y, DOT, Some(c), None);
            }
        }
        Icon::Filter => g.poly(&[(2.0, 3.0), (14.0, 3.0), (9.5, 8.5), (9.5, 13.5), (6.5, 12.0), (6.5, 8.5)], c),
        Icon::SortAsc | Icon::SortDesc => {
            let (a, b) = if icon == Icon::SortAsc { ("A", "Z") } else { ("Z", "A") };
            g.text(5.0, 4.5, a, 6.5, c, false, false);
            g.text(5.0, 11.5, b, 6.5, c, false, false);
            g.stroke(&[(11.0, 2.5), (11.0, 13.0)], BLUE);
            g.poly(&[(9.0, 11.0), (13.0, 11.0), (11.0, 14.0)], BLUE);
        }
        Icon::Duplicates => {
            g.rect(2.0, 2.0, 9.0, 4.0, Some(Color32::WHITE), Some(c));
            g.rect(5.0, 7.0, 9.0, 4.0, Some(Color32::WHITE), Some(c));
            g.line(&[(9.0, 12.0), (14.0, 15.0)], RED, MARK);
            g.line(&[(14.0, 12.0), (9.0, 15.0)], RED, MARK);
        }
        Icon::TextColumns => {
            g.rect(1.5, 3.0, 5.5, 10.0, None, Some(c));
            g.rect(9.0, 3.0, 5.5, 10.0, None, Some(c));
            g.poly(&[(7.0, 6.0), (9.0, 8.0), (7.0, 10.0)], BLUE);
        }
        Icon::Validation => {
            g.rect(2.0, 3.0, 12.0, 10.0, None, Some(c));
            g.line(&[(4.0, 8.0), (6.5, 10.5), (11.5, 5.5)], GREEN, MARK);
        }
        Icon::Group | Icon::Ungroup | Icon::Subtotal => {
            g.stroke(&[(3.0, 3.0), (3.0, 13.0), (6.0, 13.0)], c);
            for y in [4.0, 7.0, 10.0] {
                g.stroke(&[(7.0, y), (14.0, y)], light);
            }
            match icon {
                Icon::Group => g.text(3.4, 3.6, "+", 5.5, BLUE, true, false),
                Icon::Ungroup => g.text(3.4, 3.6, "−", 5.5, BLUE, true, false),
                _ => g.text(11.0, 12.4, "Σ", 6.0, BLUE, false, false),
            }
        }
        Icon::Lock => {
            g.rect(3.5, 7.0, 9.0, 7.0, Some(YELLOW), Some(c));
            g.line(&[(5.5, 7.0), (5.5, 4.5), (8.0, 2.5), (10.5, 4.5), (10.5, 7.0)], c, 1.2);
        }
        Icon::Spell => {
            g.text(7.0, 6.0, "abc", 7.0, c, false, false);
            g.line(&[(4.0, 11.0), (6.5, 13.5), (12.0, 8.5)], GREEN, MARK);
        }
        Icon::Freeze => {
            g.rect(2.0, 2.5, 12.0, 11.0, None, Some(c));
            g.rect(2.0, 2.5, 12.0, 3.0, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
            g.rect(2.0, 2.5, 3.5, 11.0, Some(Color32::from_rgb(0xD6, 0xE6, 0xF7)), None);
            g.line(&[(2.0, 5.5), (14.0, 5.5)], BLUE, MARK);
            g.line(&[(5.5, 2.5), (5.5, 13.5)], BLUE, MARK);
        }
        Icon::Theme => {
            g.rect(2.0, 2.0, 5.5, 5.5, Some(BLUE), None);
            g.rect(8.5, 2.0, 5.5, 5.5, Some(ORANGE), None);
            g.rect(2.0, 8.5, 5.5, 5.5, Some(GREEN), None);
            g.rect(8.5, 8.5, 5.5, 5.5, Some(PURPLE), None);
        }
        Icon::Margins => {
            g.rect(3.0, 1.5, 10.0, 13.0, None, Some(c));
            g.rect(5.0, 3.5, 6.0, 9.0, None, Some(BLUE));
        }
        Icon::Orient => {
            g.rect(2.0, 4.0, 8.0, 10.0, None, Some(c));
            g.rect(6.0, 2.0, 8.0, 6.0, Some(Color32::WHITE), Some(BLUE));
        }
        Icon::SheetRtl => {
            // A sheet with an arrow running right to left.
            g.rect(2.0, 2.5, 12.0, 8.0, None, Some(c));
            g.stroke(&[(6.0, 2.5), (6.0, 10.5)], light);
            g.stroke(&[(10.0, 2.5), (10.0, 10.5)], light);
            g.line(&[(13.5, 13.0), (4.5, 13.0)], BLUE, MARK);
            g.poly(&[(2.0, 13.0), (5.0, 10.5), (5.0, 15.5)], BLUE);
        }
        Icon::PrintArea => {
            g.rect(2.5, 2.5, 11.0, 11.0, None, Some(light));
            g.line(&[(4.0, 4.0), (11.0, 4.0), (11.0, 10.0), (4.0, 10.0), (4.0, 4.0)], BLUE, MARK);
        }
        Icon::Script => {
            g.rect(2.0, 2.0, 12.0, 12.0, None, Some(c));
            g.line(&[(4.5, 6.0), (7.0, 8.0), (4.5, 10.0)], GREEN, MARK);
            g.stroke(&[(8.0, 10.5), (11.5, 10.5)], c);
        }
        Icon::Pen => {
            g.poly(&[(3.0, 13.0), (4.0, 10.0), (11.0, 3.0), (13.0, 5.0), (6.0, 12.0)], BLUE);
            g.stroke(&[(3.0, 13.0), (4.0, 10.0)], c);
        }
        Icon::Share => {
            g.line(&[(4.0, 7.0), (4.0, 13.0), (12.0, 13.0), (12.0, 7.0)], c, MARK);
            g.line(&[(8.0, 10.0), (8.0, 2.5)], c, MARK);
            g.line(&[(5.5, 5.0), (8.0, 2.5), (10.5, 5.0)], c, MARK);
        }
        Icon::Settings => {
            g.circle(8.0, 8.0, 5.0, None, Some(c));
            g.circle(8.0, 8.0, 2.0, Some(c), None);
        }
        Icon::Appearance => {
            g.circle(8.0, 8.0, 5.5, None, Some(c));
            let half: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 12.0 - 0.5);
                    (8.0 + 5.5 * a.cos(), 8.0 + 5.5 * a.sin())
                })
                .collect();
            g.poly(&half, c);
        }
        Icon::Book => {
            g.rect(2.0, 2.0, 12.0, 12.0, Some(GREEN), None);
            g.rect(4.0, 4.0, 8.0, 8.0, Some(Color32::WHITE), None);
            g.stroke(&[(8.0, 4.0), (8.0, 12.0)], GREEN);
            g.stroke(&[(4.0, 8.0), (12.0, 8.0)], GREEN);
        }
        Icon::Folder => {
            g.poly(&[(1.5, 4.0), (6.0, 4.0), (7.5, 5.5), (14.5, 5.5), (14.5, 13.0), (1.5, 13.0)], YELLOW);
        }
        Icon::Header | Icon::Footer => {
            g.rect(3.5, 1.5, 9.0, 13.0, None, Some(c));
            let y = if icon == Icon::Header { 2.5 } else { 11.0 };
            g.rect(4.5, y, 7.0, 2.5, Some(BLUE), None);
            for yy in if icon == Icon::Header { [7.0, 9.5] } else { [4.5, 7.0] } {
                g.stroke(&[(5.5, yy), (10.5, yy)], light);
            }
        }
        Icon::PageNumber | Icon::PageCount => {
            if icon == Icon::PageCount {
                g.rect(5.5, 1.0, 8.5, 11.5, None, Some(light));
            }
            g.rect(2.5, 3.0, 8.5, 12.0, Some(Color32::WHITE), Some(PAPER_INK));
            // A "#" mark.
            g.stroke(&[(5.3, 6.5), (4.8, 12.5)], BLUE);
            g.stroke(&[(8.3, 6.5), (7.8, 12.5)], BLUE);
            g.stroke(&[(4.0, 8.4), (9.6, 8.4)], BLUE);
            g.stroke(&[(3.8, 10.8), (9.4, 10.8)], BLUE);
        }
        Icon::Calendar => {
            g.rect(2.0, 3.0, 12.0, 11.0, Some(Color32::WHITE), Some(PAPER_INK));
            g.rect(2.0, 3.0, 12.0, 3.0, Some(BLUE), None);
            for (x, y) in [(4.5, 8.5), (7.5, 8.5), (10.5, 8.5), (4.5, 11.5), (7.5, 11.5)] {
                g.rect(x - 0.6, y - 0.6, 1.4, 1.4, Some(c), None);
            }
        }
        Icon::Clock => {
            g.circle(8.0, 8.0, 6.0, None, Some(c));
            g.line(&[(8.0, 4.5), (8.0, 8.0), (10.8, 9.6)], BLUE, MARK);
        }
        Icon::FileName => {
            g.poly(&[(3.0, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (3.0, 14.5)], Color32::WHITE);
            g.line(&[(3.0, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (3.0, 14.5), (3.0, 1.5)], PAPER_INK, HAIRLINE);
            g.stroke(&[(10.0, 1.5), (10.0, 4.5), (13.0, 4.5)], PAPER_INK);
            g.line(&[(5.0, 9.0), (11.0, 9.0)], BLUE, MARK);
            g.stroke(&[(5.0, 11.5), (9.0, 11.5)], light);
        }
        Icon::Back => {
            g.circle(8.0, 8.0, 6.5, None, Some(c));
            g.line(&[(11.5, 8.0), (4.8, 8.0)], c, MARK);
            g.line(&[(7.5, 5.3), (4.8, 8.0), (7.5, 10.7)], c, MARK);
        }
        Icon::NewFile => {
            g.line(&[(9.5, 1.5), (3.5, 1.5), (3.5, 14.5), (12.5, 14.5), (12.5, 4.5), (9.5, 1.5), (9.5, 4.5), (12.5, 4.5)], c, HAIRLINE);
        }
        Icon::OpenFolder => {
            g.line(&[(13.0, 6.5), (13.0, 4.5), (7.5, 4.5), (6.0, 3.0), (1.5, 3.0), (1.5, 13.0)], c, HAIRLINE);
            g.line(&[(1.5, 13.0), (4.0, 7.0), (15.0, 7.0), (12.5, 13.0), (1.5, 13.0)], c, HAIRLINE);
        }
        Icon::Info => {
            g.circle(8.0, 8.0, 6.5, None, Some(c));
            g.line(&[(8.0, 7.0), (8.0, 11.8)], c, MARK);
            g.circle(8.0, 4.8, DOT, Some(c), None);
        }
        Icon::Print => {
            g.line(&[(4.5, 5.5), (4.5, 1.5), (11.5, 1.5), (11.5, 5.5)], c, HAIRLINE);
            g.line(&[(4.0, 11.5), (1.5, 11.5), (1.5, 5.5), (14.5, 5.5), (14.5, 11.5), (12.0, 11.5)], c, HAIRLINE);
            g.line(&[(4.5, 9.0), (4.5, 14.5), (11.5, 14.5), (11.5, 9.0), (4.5, 9.0)], c, HAIRLINE);
            g.circle(12.3, 7.6, DOT, Some(c), None);
        }
    }
}

/// Icons offered by Insert › Icons, by name.
pub const LIBRARY: &[(&str, Icon)] = &[
    ("check", Icon::Check),
    ("close", Icon::Close),
    ("plus", Icon::Plus),
    ("minus", Icon::Minus),
    ("chart", Icon::Chart),
    ("line chart", Icon::ChartLine),
    ("pie chart", Icon::ChartPie),
    ("scatter", Icon::ChartScatter),
    ("table", Icon::Table),
    ("sum", Icon::Sum),
    ("filter", Icon::Filter),
    ("lock", Icon::Lock),
    ("comment", Icon::Comment),
    ("note", Icon::Note),
    ("picture", Icon::Picture),
    ("book", Icon::Book),
    ("folder", Icon::Folder),
    ("search", Icon::Search),
    ("calculator", Icon::Calc),
    ("link", Icon::Link),
    ("pen", Icon::Pen),
    ("share", Icon::Share),
    ("settings", Icon::Settings),
    ("home", Icon::Home),
    ("save", Icon::Save),
    ("theme", Icon::Theme),
    ("shapes", Icon::Shapes),
    ("text", Icon::TextBox),
    ("trace", Icon::Trace),
    ("validation", Icon::Validation),
];

pub fn from_name(name: &str) -> Option<Icon> {
    LIBRARY.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, i)| *i)
}

/// Every icon in the set, for the tests that walk them (LIBRARY holds only the ones Insert ▸ Icons
/// offers). Keep in step with the enum: a new variant belongs here, so it is measured too.
#[cfg(test)]
const ALL: &[Icon] = &[
    Icon::Paste,
    Icon::Cut,
    Icon::Copy,
    Icon::Brush,
    Icon::Bold,
    Icon::Italic,
    Icon::Underline,
    Icon::Strike,
    Icon::Borders,
    Icon::BorderAll,
    Icon::BorderNone,
    Icon::BorderOutside,
    Icon::BorderThick,
    Icon::Fill,
    Icon::FontColor,
    Icon::AlignTop,
    Icon::AlignMiddle,
    Icon::AlignBottom,
    Icon::AlignLeft,
    Icon::AlignCenter,
    Icon::AlignRight,
    Icon::Orientation,
    Icon::IndentDec,
    Icon::IndentInc,
    Icon::Wrap,
    Icon::Merge,
    Icon::Currency,
    Icon::Percent,
    Icon::Comma,
    Icon::DecInc,
    Icon::DecDec,
    Icon::CondFormat,
    Icon::FormatTable,
    Icon::CellStyles,
    Icon::Insert,
    Icon::Delete,
    Icon::Format,
    Icon::Sum,
    Icon::FillDown,
    Icon::Eraser,
    Icon::SortFilter,
    Icon::Find,
    Icon::Undo,
    Icon::Redo,
    Icon::Save,
    Icon::Home,
    Icon::More,
    Icon::Chevron,
    Icon::ChevronUp,
    Icon::Close,
    Icon::Check,
    Icon::Fx,
    Icon::Plus,
    Icon::Left,
    Icon::Right,
    Icon::Normal,
    Icon::PageLayout,
    Icon::PageBreak,
    Icon::Minus,
    Icon::Maximize,
    Icon::Restore,
    Icon::Table,
    Icon::PivotTable,
    Icon::Chart,
    Icon::ChartBar,
    Icon::ChartLine,
    Icon::ChartPie,
    Icon::ChartScatter,
    Icon::Picture,
    Icon::Shapes,
    Icon::Link,
    Icon::Comment,
    Icon::Note,
    Icon::TextBox,
    Icon::WordArt,
    Icon::Symbol,
    Icon::Sparkline,
    Icon::Checkbox,
    Icon::Function,
    Icon::Name,
    Icon::Trace,
    Icon::Calc,
    Icon::Filter,
    Icon::SortAsc,
    Icon::SortDesc,
    Icon::Duplicates,
    Icon::TextColumns,
    Icon::Validation,
    Icon::Group,
    Icon::Ungroup,
    Icon::Subtotal,
    Icon::Lock,
    Icon::Spell,
    Icon::Freeze,
    Icon::Zoom,
    Icon::Gridlines,
    Icon::Theme,
    Icon::Margins,
    Icon::Orient,
    Icon::SheetRtl,
    Icon::Paper,
    Icon::PrintArea,
    Icon::Script,
    Icon::Pen,
    Icon::Share,
    Icon::Search,
    Icon::Settings,
    Icon::Appearance,
    Icon::Sheet,
    Icon::SheetList,
    Icon::Book,
    Icon::Folder,
    Icon::Header,
    Icon::Footer,
    Icon::PageNumber,
    Icon::PageCount,
    Icon::Calendar,
    Icon::Clock,
    Icon::FileName,
    Icon::Back,
    Icon::NewFile,
    Icon::OpenFolder,
    Icon::Info,
    Icon::Print,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// What the icon paints on a 16×16 design box, in design units: the shapes and their bounds.
    fn painted(icon: Icon, color: Color32) -> (Vec<egui::epaint::ClippedShape>, Rect) {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::font_definitions());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui.painter(), Rect::from_min_size(Pos2::ZERO, vec2(GRID, GRID)), icon, color);
        });
        let mut bounds = Rect::NOTHING;
        for shape in &out.shapes {
            bounds |= shape.shape.visual_bounding_rect();
        }
        out.textures_delta.clear(); // nothing consumes the atlas in a headless test
        (out.shapes, bounds)
    }

    /// The ink every icon leaves on its design box, in design units.
    fn ink(icon: Icon) -> Rect {
        painted(icon, Color32::WHITE).1
    }

    /// Whether a fill is one of the paper whites an icon draws a sheet, a page or a bubble with.
    fn is_paper(c: Color32) -> bool {
        c.a() > 200 && c.r() > 200 && c.g() > 200 && c.b() > 200
    }

    /// A paper-white shape keeps an edge you can see on the paper, in both themes.
    ///
    /// The icon ink is very light in the dark theme (0xEE), so an outline drawn in it vanished into
    /// a white sheet and left a blank rectangle on the dark ribbon. Every opaque paper fill that
    /// carries an outline is measured against the paper here, not against the chrome.
    #[test]
    fn a_paper_fill_keeps_a_visible_edge() {
        /// The dark theme's `text` token, the lightest ink an icon is ever drawn in.
        const DARK_INK: Color32 = Color32::from_gray(0xEE);
        /// WCAG asks 3:1 of graphical objects; an edge that merely exists can be fainter.
        const EDGE: f32 = 2.0;
        let mut flat = Vec::new();
        for &icon in ALL {
            let (shapes, _) = painted(icon, DARK_INK);
            for shape in &shapes {
                let (fill, stroke) = match &shape.shape {
                    egui::Shape::Rect(r) => (r.fill, Some(r.stroke)),
                    egui::Shape::Circle(c) => (c.fill, Some(c.stroke)),
                    _ => continue,
                };
                let Some(stroke) = stroke.filter(|s| !s.is_empty()) else { continue };
                if !is_paper(fill) {
                    continue;
                }
                let ratio = crate::theme::contrast_ratio(stroke.color, fill);
                if ratio < EDGE {
                    flat.push(format!("{icon:?}: {stroke:?} on {fill:?} is {ratio:.2}:1"));
                }
            }
        }
        assert!(flat.is_empty(), "paper shapes whose edge is invisible: {flat:#?}");
    }

    /// The drawing stays inside the box it is painted in: a glyph that spills past its design grid
    /// draws over the button's hover highlight, and over the neighbour in a tight ribbon row. The
    /// half stroke of an edge is part of the bounds, so this measures the ink, not the path.
    #[test]
    fn every_icon_stays_inside_its_box() {
        // A stroke on the very edge halves outside it, and glyph meshes round outwards.
        const SLACK: f32 = 0.35;
        let mut outside = Vec::new();
        for &icon in ALL {
            let r = ink(icon);
            if r.min.x < -SLACK || r.min.y < -SLACK || r.max.x > GRID + SLACK || r.max.y > GRID + SLACK {
                outside.push(format!("{icon:?} {r:?}"));
            }
        }
        assert!(outside.is_empty(), "icons drawn outside their 16×16 box: {outside:#?}");
    }

    /// The ink sits in the middle of the box, whatever the drawing is made of.
    ///
    /// A drawing that hangs to one side reads as a different size from its neighbours in a row of
    /// icons, and a text glyph centred on its line box (rather than on its ink) sits high, which is
    /// what this caught when every letter icon was drawn about 2 units above the middle.
    #[test]
    fn every_icon_is_centred_on_its_ink() {
        // Icons with a deliberate asymmetry (a page behind a page, bars above an axis) stay well
        // inside this; a glyph nudged off the middle does not.
        const OFF_CENTRE: f32 = 1.5;
        // Two drawings mean their own asymmetry: Align Top stacks its bars against the top edge,
        // Align Bottom against the bottom one (Align Middle is the centred member of the trio).
        const EDGE_ALIGNED: &[Icon] = &[Icon::AlignTop, Icon::AlignBottom];
        let mut off = Vec::new();
        for &icon in ALL.iter().filter(|i| !EDGE_ALIGNED.contains(i)) {
            let c = ink(icon).center();
            if (c.x - GRID / 2.0).abs() > OFF_CENTRE || (c.y - GRID / 2.0).abs() > OFF_CENTRE {
                off.push(format!("{icon:?} at {c:?}"));
            }
        }
        assert!(off.is_empty(), "icons drawn off the middle of their box: {off:#?}");
    }

    /// Every icon draws enough ink to read at 16 px: nothing is a stray dot or an empty box.
    #[test]
    fn every_icon_draws_a_legible_mark() {
        let mut faint = Vec::new();
        for &icon in ALL {
            let r = ink(icon);
            if r.width().max(r.height()) < 5.0 {
                faint.push(format!("{icon:?} {r:?}"));
            }
        }
        assert!(faint.is_empty(), "icons with too little ink: {faint:#?}");
    }

    /// The library lists each icon once, offers lowercase names, and finds them again.
    #[test]
    fn the_icon_library_is_a_round_trip() {
        for (i, (name, icon)) in LIBRARY.iter().enumerate() {
            assert!(!name.is_empty());
            assert_eq!(*name, name.to_ascii_lowercase(), "{name:?} is lowercase");
            assert!(LIBRARY.iter().skip(i + 1).all(|(other, _)| other != name), "{name:?} is listed once");
            assert_eq!(from_name(name), Some(*icon), "{name:?} finds its icon back");
            assert_eq!(from_name(&name.to_ascii_uppercase()), Some(*icon), "{name:?} is found whatever the case");
            assert!(ALL.contains(icon), "{name:?} is covered by the drawing tests");
        }
        assert_eq!(from_name("no such icon"), None);
    }
}
