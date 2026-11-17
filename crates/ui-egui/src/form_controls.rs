//! Form controls on the sheet: a button, a slider and a spin button are drawn over the cells, a
//! click runs a button's action, and working a slider or a spin button writes the control's value
//! to its linked cell through `control.setValue`.
//!
//! The values are a control's own, so they are not cell values: a drag writes the cell (and its
//! dependents recalculate) on every step, which means one drag is more than one undo step. That is
//! the one thing this first cut does less than Excel, which keeps a whole drag as one step.

use cella_engine::model::{ControlKind, FormControl, Sheet};
use egui::epaint::PathShape;
use egui::{Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;

use crate::SheetApp;
use crate::grid::Geo;
use crate::theme::{self, Tokens};

/// The part of a control under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Button,
    /// A spin button's arrow that adds a step.
    Up,
    /// A spin button's arrow that takes a step away.
    Down,
    /// A slider's thumb.
    Thumb,
    /// The track on the minimum's side of the thumb.
    TrackBefore,
    /// The track on the maximum's side of the thumb.
    TrackAfter,
}

/// How full a slider is, from 0 to 1.
fn fraction(c: &FormControl) -> f32 {
    let span = c.max.saturating_sub(c.min).max(1) as f32;
    (c.value.saturating_sub(c.min) as f32 / span).clamp(0.0, 1.0)
}

/// A slider's track and thumb in `r`. A horizontal slider has its minimum on the left; a vertical
/// one has it at the top (as Excel's scroll bars do).
pub fn slider_parts(r: Rect, c: &FormControl, z: f32) -> (Rect, Rect) {
    let f = fraction(c);
    if c.horizontal {
        let knob = (r.height() - 4.0 * z).clamp(6.0 * z, 16.0 * z).min(r.width() / 2.0).max(1.0);
        let track = Rect::from_center_size(r.center(), vec2((r.width() - knob).max(1.0), (4.0 * z).min(r.height())));
        let x = track.left() + f * track.width();
        (track, Rect::from_center_size(pos2(x, r.center().y), vec2(knob, (r.height() - 4.0 * z).max(4.0))))
    } else {
        let knob = (r.width() - 4.0 * z).clamp(6.0 * z, 16.0 * z).min(r.height() / 2.0).max(1.0);
        let track = Rect::from_center_size(r.center(), vec2((4.0 * z).min(r.width()), (r.height() - knob).max(1.0)));
        let y = track.top() + f * track.height();
        (track, Rect::from_center_size(pos2(r.center().x, y), vec2((r.width() - 4.0 * z).max(4.0), knob)))
    }
}

/// A spin button's halves: (the arrow that adds, the arrow that subtracts). Tall spin buttons
/// stack them (up on top); wide ones put them side by side (up on the right).
pub fn spin_parts(r: Rect) -> (Rect, Rect) {
    if r.height() >= r.width() {
        let (top, bottom) = r.split_top_bottom_at_fraction(0.5);
        (top, bottom)
    } else {
        let (left, right) = r.split_left_right_at_fraction(0.5);
        (right, left)
    }
}

/// The part of control `c` (drawn in `r`) under `p`.
pub fn part_at(r: Rect, c: &FormControl, z: f32, p: Pos2) -> Option<Part> {
    if !r.contains(p) {
        return None;
    }
    Some(match c.kind {
        ControlKind::Button => Part::Button,
        ControlKind::Spin => {
            let (up, _) = spin_parts(r);
            if up.contains(p) { Part::Up } else { Part::Down }
        }
        ControlKind::Slider => {
            let (_, thumb) = slider_parts(r, c, z);
            if thumb.expand(2.0).contains(p) {
                Part::Thumb
            } else if (c.horizontal && p.x < thumb.center().x) || (!c.horizontal && p.y < thumb.center().y) {
                Part::TrackBefore
            } else {
                Part::TrackAfter
            }
        }
    })
}

/// The value a slider's thumb dragged to `p` stands for.
pub fn value_at(r: Rect, c: &FormControl, z: f32, p: Pos2) -> f64 {
    let (track, _) = slider_parts(r, c, z);
    let f = if c.horizontal { (p.x - track.left()) / track.width().max(1.0) } else { (p.y - track.top()) / track.height().max(1.0) };
    f64::from(c.min) + f64::from(f.clamp(0.0, 1.0)) * f64::from(c.max.saturating_sub(c.min))
}

/// The control's rectangle on screen.
fn control_rect(geo: &Geo, sh: &Sheet, c: &FormControl) -> Rect {
    crate::chartview::anchor_rect(geo, sh, &c.anchor)
}

/// The topmost control under the logical point `p`.
fn hit<'a>(sh: &'a Sheet, geo: &Geo, p: Pos2) -> Option<&'a FormControl> {
    if !geo.cells.contains(p) {
        return None;
    }
    sh.controls.iter().rev().find(|c| control_rect(geo, sh, c).contains(p))
}

/// Paints the sheet's form controls, above every other object.
pub(crate) fn paint(app: &SheetApp, p: &egui::Painter, geo: &Geo, sh: &Sheet) {
    if sh.controls.is_empty() {
        return;
    }
    let t = Tokens::get(p.ctx());
    let hover = p.ctx().pointer_hover_pos().map(|q| geo.mirror_pos(q));
    for c in &sh.controls {
        let r = control_rect(geo, sh, c);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let part = hover.and_then(|q| part_at(r, c, geo.z, q));
        paint_control(p, &t, r, c, geo.z, part, app.selected_chart == Some(c.id));
    }
}

/// One control, its hovered part (if any) tinted.
fn paint_control(p: &egui::Painter, t: &Tokens, r: Rect, c: &FormControl, z: f32, hover: Option<Part>, selected: bool) {
    let border = Stroke::new(1.0, t.header_line);
    let bg = |part: Part| if hover == Some(part) { t.hover } else { t.ribbon };
    let clip = p.with_clip_rect(r.expand(2.0).intersect(p.clip_rect()));
    match c.kind {
        ControlKind::Button => {
            clip.rect_filled(r, 3.0 * z, bg(Part::Button));
            clip.rect_stroke(r, 3.0 * z, border, StrokeKind::Inside);
            let g = clip.layout(c.text.clone(), theme::ui_font(12.0 * z), t.cell_text, (r.width() - 8.0 * z).max(1.0));
            clip.with_clip_rect(r.shrink(1.0).intersect(clip.clip_rect())).galley(r.center() - g.size() / 2.0, g, t.cell_text);
        }
        ControlKind::Slider => {
            clip.rect_filled(r, 2.0 * z, t.ribbon);
            clip.rect_stroke(r, 2.0 * z, border, StrokeKind::Inside);
            let (track, thumb) = slider_parts(r, c, z);
            clip.rect_filled(track, 2.0 * z, t.header_sel_bg);
            let filled = if c.horizontal {
                Rect::from_min_max(track.min, pos2(thumb.center().x, track.max.y))
            } else {
                Rect::from_min_max(track.min, pos2(track.max.x, thumb.center().y))
            };
            clip.rect_filled(filled, 2.0 * z, t.accent);
            let knob = if hover == Some(Part::Thumb) { t.accent_dark } else { t.input_border };
            clip.rect_filled(thumb, 3.0 * z, knob);
            clip.rect_stroke(thumb, 3.0 * z, Stroke::new(1.0, t.ribbon), StrokeKind::Inside);
        }
        ControlKind::Spin => {
            let (up, down) = spin_parts(r);
            let tall = r.height() >= r.width();
            for (half, part) in [(up, Part::Up), (down, Part::Down)] {
                clip.rect_filled(half, 2.0 * z, bg(part));
                clip.rect_stroke(half, 2.0 * z, border, StrokeKind::Inside);
                let s = (half.width().min(half.height()) * 0.28).max(1.5);
                let m = half.center();
                let tri = match (tall, part) {
                    (true, Part::Up) => vec![pos2(m.x, m.y - s * 0.6), pos2(m.x - s, m.y + s * 0.5), pos2(m.x + s, m.y + s * 0.5)],
                    (true, _) => vec![pos2(m.x, m.y + s * 0.6), pos2(m.x + s, m.y - s * 0.5), pos2(m.x - s, m.y - s * 0.5)],
                    (false, Part::Up) => vec![pos2(m.x + s * 0.6, m.y), pos2(m.x - s * 0.5, m.y + s), pos2(m.x - s * 0.5, m.y - s)],
                    (false, _) => vec![pos2(m.x - s * 0.6, m.y), pos2(m.x + s * 0.5, m.y - s), pos2(m.x + s * 0.5, m.y + s)],
                };
                clip.add(PathShape::convex_polygon(tri, t.cell_text, Stroke::NONE));
            }
        }
    }
    if selected {
        crate::chartview::selection_outline(p, r);
    }
}

/// Sets a slider or spin button's value.
fn set_value(app: &mut SheetApp, c: &FormControl, value: f64) {
    match app.run("control.setValue", json!({"id": c.id, "value": value})) {
        Ok(_) => {}
        Err(e) => app.message = Some(("Form Control".into(), e)),
    }
}

/// Moves a slider or spin button by `steps` steps (negative toward the minimum), or by pages.
fn nudge(app: &mut SheetApp, c: &FormControl, steps: i64, page: bool) {
    let by = if page && c.kind == ControlKind::Slider { c.page } else { c.step };
    let to = f64::from(c.value) + steps as f64 * f64::from(by.max(1));
    set_value(app, c, to);
}

/// Runs a button's action.
fn press(app: &mut SheetApp, c: &FormControl) {
    if let Err(e) = app.run("control.press", json!({"id": c.id})) {
        app.message = Some((c.display_name(), e));
    }
}

/// Pointer handling for the sheet's form controls. Returns true when it used the pointer this
/// frame, so the grid neither selects cells nor drags objects under it.
pub(crate) fn interact(app: &mut SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) -> bool {
    if sh.controls.is_empty() {
        return false;
    }
    let pointer = ctx.input(|i| i.pointer.clone());
    let Some(pos) = pointer.interact_pos().or(pointer.hover_pos()).map(|p| geo.mirror_pos(p)) else { return false };
    let Some(c) = hit(sh, geo, pos).cloned() else { return false };
    let r = control_rect(geo, sh, &c);
    let Some(part) = part_at(r, &c, geo.z, pos) else { return false };
    let primary_clicked = resp.clicked_by(egui::PointerButton::Primary);
    // A press on a control is the control's, not the grid's: it starts no cell selection or drag.
    if pointer.primary_down() && c.kind == ControlKind::Button {
        return true;
    }
    // A slider follows the pointer while the button is held: a click on the track or the thumb
    // and a drag are the same gesture.
    if pointer.primary_down() && c.kind == ControlKind::Slider {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        let v = c.snap(value_at(r, &c, geo.z, pos));
        if v != c.value {
            set_value(app, &c, f64::from(v));
        }
        return true;
    }
    if !primary_clicked {
        return false;
    }
    match part {
        Part::Button => press(app, &c),
        Part::Up => nudge(app, &c, 1, false),
        Part::Down => nudge(app, &c, -1, false),
        Part::TrackBefore => nudge(app, &c, -1, true),
        Part::TrackAfter => nudge(app, &c, 1, true),
        Part::Thumb => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(value: u32, horizontal: bool) -> FormControl {
        FormControl { kind: ControlKind::Slider, min: 0, max: 100, value, horizontal, ..Default::default() }
    }

    #[test]
    fn the_thumb_sits_at_the_value_and_dragging_reads_it_back() {
        let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(200.0, 20.0));
        for v in [0, 25, 50, 100] {
            let c = slider(v, true);
            let (_, thumb) = slider_parts(r, &c, 1.0);
            assert!((value_at(r, &c, 1.0, thumb.center()) - f64::from(v)).abs() < 0.5, "{v}");
            assert_eq!(part_at(r, &c, 1.0, thumb.center()), Some(Part::Thumb));
        }
        let c = slider(50, true);
        assert_eq!(part_at(r, &c, 1.0, pos2(15.0, 20.0)), Some(Part::TrackBefore));
        assert_eq!(part_at(r, &c, 1.0, pos2(205.0, 20.0)), Some(Part::TrackAfter));
        assert_eq!(part_at(r, &c, 1.0, pos2(5.0, 20.0)), None);
        // Past the ends clamps.
        assert_eq!(value_at(r, &c, 1.0, pos2(-500.0, 20.0)), 0.0);
        assert_eq!(value_at(r, &c, 1.0, pos2(900.0, 20.0)), 100.0);
        // Vertical: the minimum is at the top.
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(20.0, 200.0));
        let (_, thumb) = slider_parts(r, &slider(0, false), 1.0);
        assert!(thumb.center().y < 20.0);
        assert_eq!(part_at(r, &slider(50, false), 1.0, pos2(10.0, 190.0)), Some(Part::TrackAfter));
    }

    #[test]
    fn spin_halves() {
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(18.0, 36.0));
        let c = FormControl { kind: ControlKind::Spin, ..Default::default() };
        assert_eq!(part_at(tall, &c, 1.0, pos2(9.0, 5.0)), Some(Part::Up));
        assert_eq!(part_at(tall, &c, 1.0, pos2(9.0, 30.0)), Some(Part::Down));
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 18.0));
        assert_eq!(part_at(wide, &c, 1.0, pos2(35.0, 9.0)), Some(Part::Up));
        assert_eq!(part_at(wide, &c, 1.0, pos2(5.0, 9.0)), Some(Part::Down));
    }

    #[test]
    fn degenerate_ranges_do_not_divide_by_zero() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(1.0, 1.0));
        let c = FormControl { kind: ControlKind::Slider, min: 5, max: 5, value: 5, ..Default::default() };
        let (track, thumb) = slider_parts(r, &c, 1.0);
        assert!(track.width().is_finite() && thumb.center().x.is_finite());
        assert!(value_at(r, &c, 1.0, pos2(0.5, 0.5)).is_finite());
    }
}
