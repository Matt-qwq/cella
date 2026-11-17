//! The window frame: who draws the caption, and the buttons and grips that come with it.
//!
//! Excel's Quick Access Toolbar row *is* the window's caption: one row, one surface, the window
//! controls at its right end. A window that keeps a system caption above an app-drawn strip has
//! two rows and — when both show the workbook's name — two titles, which is what this module
//! decides per platform (#54, #169, #170).
//!
//! The library owns the frame only while it also draws the title bar (`Chrome::title_bar`): a host
//! that draws its own window keeps its own frame. A host that wants the frame decided at creation
//! time (so the window never flashes a system caption first) calls `viewport` on its
//! `ViewportBuilder`; without that call the title bar asks for the same thing on its first frame,
//! which costs one frame of the system caption at start-up.

use egui::{Color32, Rect, Sense, Ui, Vec2, pos2, vec2};

use crate::SheetApp;
use crate::i18n;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// The platform this build runs on. The chrome differs per platform, and the differences are
/// decided in one place (`plan`) rather than with `cfg!` calls spread through the title bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
    Linux,
    Other,
}

/// The platform this build is for.
pub const HOST: Os = if cfg!(target_os = "windows") {
    Os::Windows
} else if cfg!(target_os = "macos") {
    Os::MacOs
} else if cfg!(target_os = "linux") {
    Os::Linux
} else {
    Os::Other
};

/// Who draws the window's caption, its buttons and the edges that resize it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// The system does: Windows' caption is what Alt+Tab lists and what the Windows 11 snap
    /// layouts hang off, and merging the app's strip into it takes Win32 calls, which are
    /// `unsafe` and forbidden here.
    System,
    /// The app does. On Linux every desktop draws the caption itself in its own toolkit (GNOME's
    /// header bar, KDE's decoration, a bare X11 title) and none of them match the app's strip, so
    /// the strip carries the buttons and the window's edges are dragged by `resize_grips`
    /// (#54).
    Client,
}

/// What the title bar has to assume about the window it is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Who draws the caption.
    pub frame: Frame,
    /// Whether the system caption shows the title text. When it does, the app's strip leaves the
    /// workbook's name to it instead of printing it a second time (Windows); when it does not —
    /// macOS hides the caption for the full-size content view, and Linux has no caption at all —
    /// the strip draws the name.
    pub caption_title: bool,
    /// macOS: draw into the caption band, with AppKit's traffic lights left where they are.
    pub fullsize_content_view: bool,
}

/// The chrome a platform asks for.
pub fn plan(os: Os) -> Plan {
    match os {
        // Windows: the system caption shows the title, so the strip must not repeat it.
        Os::Windows => Plan { frame: Frame::System, caption_title: true, fullsize_content_view: false },
        // Any other target — the web build, mobile, the BSDs — has no caption of the app's that
        // could carry the name (in a browser the page *is* the frame), so the strip draws it.
        Os::Other => Plan { frame: Frame::System, caption_title: false, fullsize_content_view: false },
        // macOS: AppKit keeps the traffic lights in a 28 pt band with no title; the strip draws
        // the name beside them (see `MAC_TITLE_BAR` in `ribbon`).
        Os::MacOs => Plan { frame: Frame::System, caption_title: false, fullsize_content_view: true },
        // Linux: the app owns the frame, and so draws the title.
        Os::Linux => Plan { frame: Frame::Client, caption_title: false, fullsize_content_view: false },
    }
}

/// The chrome this build draws.
pub fn host() -> Plan {
    plan(HOST)
}

/// The window the title bar assumes, for the app's `ViewportBuilder`.
///
/// ```ignore
/// let viewport = cella_ui_egui::chrome::viewport(egui::ViewportBuilder::default());
/// ```
pub fn viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    viewport_for(viewport, HOST)
}

/// [`viewport`] for a named platform, so every platform's chrome is decided (and tested) here
/// rather than only the one the build is compiled for.
pub fn viewport_for(viewport: egui::ViewportBuilder, os: Os) -> egui::ViewportBuilder {
    let plan = plan(os);
    match (plan.frame, plan.fullsize_content_view) {
        (Frame::Client, _) => viewport.with_decorations(false),
        (Frame::System, true) => viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false),
        (Frame::System, false) => viewport,
    }
}

/// The window buttons of a title bar, in the order every desktop draws them: minimize, restore or
/// maximize, close.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl WindowButton {
    /// The name the button is announced with (translated).
    pub fn label(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::Maximize => "Maximize",
            Self::Restore => "Restore Down",
            Self::Close => "Close",
        }
    }

    /// The drawing the button carries, from the app's own icon set.
    pub fn icon(self) -> Icon {
        match self {
            Self::Minimize => Icon::Minus,
            Self::Maximize => Icon::Maximize,
            Self::Restore => Icon::Restore,
            Self::Close => Icon::Close,
        }
    }

    /// What a click asks the window to do.
    pub fn command(self) -> egui::ViewportCommand {
        match self {
            Self::Minimize => egui::ViewportCommand::Minimized(true),
            Self::Maximize => egui::ViewportCommand::Maximized(true),
            Self::Restore => egui::ViewportCommand::Maximized(false),
            Self::Close => egui::ViewportCommand::Close,
        }
    }
}

/// Width of one window button: wide enough to aim at, flush with the window's corner.
pub const BUTTON_W: f32 = 46.0;
/// The width all three take, which the centred title has to stay clear of.
pub const BUTTONS_W: f32 = 3.0 * BUTTON_W;
/// The size the button's drawing is painted at (the icon set's own 16-unit grid).
const BUTTON_ICON: Vec2 = vec2(16.0, 16.0);

/// The buttons a title bar carries, left to right, and where each one is.
///
/// Painted rather than allocated by a layout so the row ends exactly at the window's corner: the
/// close button's corner is the window's, as in every caption.
pub fn button_rects(bar: Rect, maximized: bool) -> [(WindowButton, Rect); 3] {
    let order = [WindowButton::Minimize, if maximized { WindowButton::Restore } else { WindowButton::Maximize }, WindowButton::Close];
    // The trio is flush with the window's corner; left to right inside that run.
    let left = bar.right() - BUTTONS_W;
    let mut out = [(WindowButton::Minimize, Rect::NOTHING); 3];
    for (i, button) in order.into_iter().enumerate() {
        let x = left + i as f32 * BUTTON_W;
        let rect = Rect::from_min_max(pos2(x, bar.top()), pos2(x + BUTTON_W, bar.bottom()));
        if let Some(slot) = out.get_mut(i) {
            *slot = (button, rect);
        }
    }
    out
}

/// Draws the window buttons at the right end of `bar` when the app owns the frame, and returns the
/// width they took (0 when the system draws the caption). Called from inside the title bar, so the
/// buttons come first in its right-to-left row and sit against the window's corner.
pub fn window_buttons(ui: &mut Ui, bar: Rect) -> f32 {
    if host().frame != Frame::Client {
        return 0.0;
    }
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
    // Reserve the row before painting into it: in a right-to-left layout the next widget then
    // starts to the left of the buttons instead of underneath them.
    let (region, _) = ui.allocate_exact_size(vec2(BUTTONS_W, bar.height()), Sense::hover());
    for (button, rect) in button_rects(region, maximized) {
        let resp = ui.interact(rect, ui.id().with(("window_button", button)), Sense::click());
        let close = button == WindowButton::Close;
        let held = resp.is_pointer_button_down_on();
        if held || resp.hovered() {
            // Close turns red under the pointer, as every caption does; a press darkens it again.
            let fill = if close && held {
                t.danger.gamma_multiply(0.8)
            } else if close {
                t.danger
            } else if held {
                t.pressed
            } else {
                t.hover
            };
            ui.painter().rect_filled(rect, 0.0, fill);
        }
        let ink = if close && (held || resp.hovered()) { Color32::WHITE } else { t.text };
        let glyph = Rect::from_center_size(rect.center(), BUTTON_ICON);
        icons::paint(ui.painter(), glyph, button.icon(), ink);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, lang.tr(button.label())));
        if resp.clicked() {
            ui.ctx().send_viewport_cmd(button.command());
        }
    }
    BUTTONS_W
}

/// How wide the band along a window edge is that starts a resize.
pub const GRIP: f32 = 5.0;

/// The edges and corners of a frameless window: dragging one resizes the window, as the system
/// frame would (winit turns the drag into the platform's own resize request, so the compositor's
/// snapping still works). Called after the panels, so the band sits over everything at the edge.
///
/// The top edge keeps no grip: the title bar is there, and its empty area moves the window.
pub fn resize_grips(ui: &Ui, maximized: bool) {
    if host().frame != Frame::Client || maximized {
        return;
    }
    let r = ui.max_rect();
    let grab = |rect: Rect, dir: egui::ResizeDirection, cursor: egui::CursorIcon, id: &str| {
        let resp = ui.interact(rect, ui.id().with(("resize_grip", id)), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if resp.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        }
    };
    // Sides and bottom, then the corners over their ends.
    let side = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, r.top() + GRIP), pos2(x1, r.bottom() - GRIP));
    grab(side(r.left(), r.left() + GRIP), egui::ResizeDirection::West, egui::CursorIcon::ResizeHorizontal, "w");
    grab(side(r.right() - GRIP, r.right()), egui::ResizeDirection::East, egui::CursorIcon::ResizeHorizontal, "e");
    grab(
        Rect::from_min_max(pos2(r.left() + GRIP, r.bottom() - GRIP), pos2(r.right() - GRIP, r.bottom())),
        egui::ResizeDirection::South,
        egui::CursorIcon::ResizeVertical,
        "s",
    );
    let corner = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), Vec2::splat(GRIP));
    grab(corner(r.left(), r.bottom() - GRIP), egui::ResizeDirection::SouthWest, egui::CursorIcon::ResizeNeSw, "sw");
    grab(corner(r.right() - GRIP, r.bottom() - GRIP), egui::ResizeDirection::SouthEast, egui::CursorIcon::ResizeNwSe, "se");
}

impl SheetApp {
    /// Asks for the frame the title bar assumes, once per run: without it the app's strip would
    /// sit under a system caption that draws the same title (#54).
    pub(crate) fn own_window_frame(&mut self, ctx: &egui::Context) {
        if self.frame_asked {
            return;
        }
        self.frame_asked = true;
        if self.chrome.title_bar && host().frame == Frame::Client {
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each platform gets the chrome the module documents: Windows keeps its caption (and its
    /// snap layouts), macOS draws into the caption band, and Linux — where every desktop paints a
    /// caption the app cannot match — gets the app's own frame.
    #[test]
    fn the_platforms_get_the_chrome_they_expect() {
        let win = plan(Os::Windows);
        assert_eq!(win.frame, Frame::System, "Windows keeps its caption");
        assert!(win.caption_title, "and shows the title there, so the strip must not repeat it");
        assert!(!win.fullsize_content_view);

        let mac = plan(Os::MacOs);
        assert_eq!(mac.frame, Frame::System);
        assert!(!mac.caption_title, "AppKit's caption is hidden, so the strip draws the name");
        assert!(mac.fullsize_content_view, "the strip is drawn in the traffic lights' band");

        let linux = plan(Os::Linux);
        assert_eq!(linux.frame, Frame::Client, "the app draws the caption (#54)");
        assert!(!linux.caption_title, "there is no system title to leave the name to");

        // The web build and anything unlisted has no caption at all: the strip is the only place
        // the workbook's name can appear, so it must draw it.
        assert!(!plan(Os::Other).caption_title, "no caption to leave the name to");
        assert_eq!(plan(Os::Other).frame, Frame::System, "and no window buttons to draw");
    }

    /// The viewport builder gets what the title bar assumes, per platform.
    #[test]
    fn the_viewport_carries_the_frame_the_strip_expects() {
        let plain = || egui::ViewportBuilder::default();
        // Linux: no system caption at all.
        assert_eq!(viewport_for(plain(), Os::Linux).decorations, Some(false), "a frameless window");
        // Windows: the caption stays, untouched.
        let win = viewport_for(plain(), Os::Windows);
        assert_eq!(win.decorations, None, "the system frame is left as it is");
        assert_eq!(win.fullsize_content_view, None);
        assert_eq!(win.title_shown, None, "the caption keeps showing the title");
        // macOS: the content view fills the caption band and its title is hidden.
        let mac = viewport_for(plain(), Os::MacOs);
        assert_eq!(mac.fullsize_content_view, Some(true));
        assert_eq!(mac.titlebar_shown, Some(false));
        assert_eq!(mac.title_shown, Some(false));
        // A host's own choices survive: only the keys this platform decides are written.
        let host = plain().with_inner_size([800.0, 600.0]);
        assert_eq!(viewport_for(host, Os::Linux).inner_size, Some(vec2(800.0, 600.0)));
    }

    /// The buttons line up along the bar's right edge without overlapping, in every bar height,
    /// with the close button in the window's corner.
    #[test]
    fn the_window_buttons_fill_the_end_of_the_title_bar() {
        let bar = Rect::from_min_max(pos2(0.0, 0.0), pos2(1440.0, 38.0));
        for maximized in [false, true] {
            let buttons = button_rects(bar, maximized);
            let kinds: Vec<WindowButton> = buttons.iter().map(|(b, _)| *b).collect();
            let expect = [WindowButton::Minimize, if maximized { WindowButton::Restore } else { WindowButton::Maximize }, WindowButton::Close];
            assert_eq!(kinds, expect, "minimize, restore or maximize, then close");
            assert_eq!(buttons[2].1.right(), bar.right(), "close ends at the window's corner");
            assert_eq!(buttons[0].1.width(), BUTTON_W);
            for (i, (_, rect)) in buttons.iter().enumerate() {
                assert!(bar.contains_rect(*rect), "button {i} is inside the bar: {rect:?}");
                if i > 0 {
                    assert_eq!(buttons[i - 1].1.right(), rect.left(), "no gap and no overlap");
                }
            }
            assert_eq!(BUTTONS_W, buttons.iter().map(|(_, r)| r.width()).sum::<f32>());
        }
    }

    /// Each button is announced in the user's language and asks for the window state it shows.
    #[test]
    fn every_window_button_is_labelled_and_does_what_it_says() {
        for button in [WindowButton::Minimize, WindowButton::Maximize, WindowButton::Restore, WindowButton::Close] {
            let label = i18n::Language::Zh.tr(button.label());
            assert_ne!(label, button.label(), "{button:?} has a Chinese label");
        }
        // Restore and maximize are the two states of one button.
        assert_eq!(WindowButton::Minimize.command(), egui::ViewportCommand::Minimized(true));
        assert_eq!(format!("{:?}", WindowButton::Maximize.command()), "Maximized(true)");
        assert_eq!(format!("{:?}", WindowButton::Restore.command()), "Maximized(false)");
        assert_eq!(WindowButton::Close.command(), egui::ViewportCommand::Close);
        // The drawings come from the icon set, so they sit on the same grid and weight.
        assert_eq!(WindowButton::Close.icon(), Icon::Close);
        assert_eq!(WindowButton::Minimize.icon(), Icon::Minus);
        assert_ne!(WindowButton::Maximize.icon(), WindowButton::Restore.icon(), "restore is not maximize");
    }
}
