//! File › Options ▸ Renderer, and what a start says after a run that never closed (#76).
//!
//! The backend is chosen before the window exists, so the setting lives in the shell
//! (`RendererService`): these tests drive the same service the desktop app provides, and read the
//! labels the page and the info bar draw.

use std::sync::{Arc, Mutex};

use cella_engine::Session;
use cella_ui_egui::backstage::Page;
use cella_ui_egui::i18n::Language;
use cella_ui_egui::{RendererService, Services, SheetApp, backend_label};
use egui::vec2;
use egui_kittest::kittest::{NodeT, Queryable};

/// A shell whose `renderer.json` is one string, and whose adapter never changes.
fn shell(saved: Arc<Mutex<String>>) -> Services {
    let (choice_of, set_to) = (saved.clone(), saved.clone());
    Services {
        renderer: Some(RendererService {
            choice: Box::new(move || choice_of.lock().map(|s| s.clone()).unwrap_or_else(|_| "auto".into())),
            set: Box::new(move |key: &str| {
                if !matches!(key, "auto" | "dx12" | "gl" | "vulkan") {
                    return Err(format!("unknown renderer `{key}`"));
                }
                let mut s = set_to.lock().map_err(|_| "locked".to_string())?;
                *s = key.to_string();
                Ok(())
            }),
            current: Box::new(|| "Dx12 · AMD Radeon RX 9070 GRE".into()),
        }),
        ..Default::default()
    }
}

fn harness(app: SheetApp) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    h
}

/// How many drop-downs the page draws.
fn combos(h: &egui_kittest::Harness<'static, SheetApp>) -> usize {
    h.query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox).count()
}

fn labels(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for node in h.query_all_by(|n| n.label().is_some()) {
        if let Some(label) = node.accesskit_node().label()
            && seen.insert(label.clone())
        {
            out.push(label);
        }
    }
    out
}

/// The Options page offers the choice, and only where the shell can actually keep one: a web build
/// has no renderer setting because its renderer is the browser's.
#[test]
fn the_options_page_offers_the_renderer_only_when_the_shell_has_one() {
    let saved = Arc::new(Mutex::new(String::from("auto")));
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, shell(saved.clone()));
    app.ui.language = Language::En;
    app.open_backstage(Page::Options);
    assert!(app.services.renderer.is_some(), "the shell's service reaches the app");
    let h = harness(app);
    // The page already has the theme and language combos; the renderer adds exactly one more.
    // (A combo reports its text as a value rather than a label, so the count is what is checked.)
    assert_eq!(combos(&h), 3, "the renderer row is there: {} combos", combos(&h));
    assert!(labels(&h).join(" | ").contains("Save .xlsx workbooks automatically"), "and the page around it");

    // The page without a shell service: everything else, and no renderer row to be wrong about.
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::En;
    app.open_backstage(Page::Options);
    let h = harness(app);
    assert_eq!(combos(&h), 2, "no service, no renderer combo");
    assert!(labels(&h).join(" | ").contains("Save .xlsx workbooks automatically"), "the page is the same one");
}

/// The key a shell saves is the key the choice names, and an unknown one is refused instead of
/// written (the page shows that error).
#[test]
fn the_setting_takes_the_keys_the_page_offers() {
    let saved = Arc::new(Mutex::new(String::from("auto")));
    let mut session = Session::new();
    session.new_workbook();
    let app = SheetApp::new(session, shell(saved.clone()));
    let service = app.services.renderer.as_ref().expect("the shell provides one");
    for key in ["auto", "dx12", "gl", "vulkan"] {
        (service.set)(key).expect("a key the page offers is accepted");
        assert_eq!(saved.lock().unwrap().as_str(), key);
    }
    assert!((service.set)("metal").is_err(), "Metal is not a choice on Windows");
    assert_eq!(saved.lock().unwrap().as_str(), "vulkan", "a refused key changed nothing");
}

/// A start after a run that died says which backend that run used and which one this start is on.
#[test]
fn the_start_after_a_crash_says_what_changed() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::En;
    app.after_unclean_exit("dx12", "gl");
    let notice = app.notice.as_ref().expect("a notice");
    assert!(notice.text.contains("did not close properly"), "{}", notice.text);
    assert!(notice.text.contains("DirectX 12") && notice.text.contains("OpenGL"), "{}", notice.text);
    // The same backend both times says nothing changed, and where to change it.
    app.after_unclean_exit("dx12", "dx12");
    let notice = app.notice.as_ref().expect("a notice");
    assert!(notice.text.contains("nothing changed"), "{}", notice.text);
    assert!(notice.text.contains("Renderer"), "it points at the setting: {}", notice.text);
}

/// Every key the shell can report has a name the interface can draw, and an unknown one still reads
/// as something.
#[test]
fn every_backend_key_has_a_label() {
    for (key, label) in [("dx12", "DirectX 12"), ("gl", "OpenGL"), ("vulkan", "Vulkan"), ("metal", "Metal")] {
        assert_eq!(backend_label(key), label);
    }
    assert!(!backend_label("wgpu").is_empty(), "an unknown backend still draws as words");
}
