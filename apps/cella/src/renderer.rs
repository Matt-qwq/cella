//! The renderer choice and the session marker, kept next to `ui.json` and `window.json`.
//!
//! Cella picks a graphics backend **before the window exists**, so the choice cannot live in
//! `ui.json` (which the app loads after the window does): it is read here at startup and applied by
//! [`crate::graphics`].
//!
//! The marker is the other half of a driver crash. A run writes `session.json` with the backend it
//! actually got as soon as that is known, and removes it on a clean exit. A start that finds the
//! file knows the previous run died — a GPU driver crash is exactly that shape (#76) — and can
//! start on the other backend instead. Neither file can stop a start: a missing, unreadable or
//! hand-edited one reads as "nothing remembered".

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Which renderer the user asked for. `Auto` is the default and the only one that may switch
/// itself after a crash: an explicit choice is kept even then, and the interface says so.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    #[default]
    Auto,
    Dx12,
    Gl,
    Vulkan,
}

impl Choice {
    /// Every choice, in the order the Options page lists them.
    pub const ALL: [Choice; 4] = [Choice::Auto, Choice::Dx12, Choice::Gl, Choice::Vulkan];

    /// The value `renderer.json` holds and the interface passes.
    pub fn key(self) -> &'static str {
        match self {
            Choice::Auto => "auto",
            Choice::Dx12 => "dx12",
            Choice::Gl => "gl",
            Choice::Vulkan => "vulkan",
        }
    }

    /// The choice a key names; `None` for anything else.
    pub fn from_key(key: &str) -> Option<Choice> {
        Choice::ALL.into_iter().find(|c| c.key() == key.trim().to_ascii_lowercase())
    }
}

/// What `renderer.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The user's choice; a hand-edited value that names nothing is `Auto`.
    #[serde(deserialize_with = "choice_from_any")]
    pub choice: Choice,
}

impl Settings {
    /// The file name inside the settings directory.
    pub const FILE: &'static str = "renderer.json";

    pub fn load(dir: Option<&Path>) -> Settings {
        let Some(dir) = dir else { return Settings::default() };
        std::fs::read(dir.join(Self::FILE)).ok().and_then(|b| Self::parse(&b)).unwrap_or_default()
    }

    /// Parse `renderer.json`'s bytes; `None` when they are not a JSON object at all. A byte-order
    /// mark is skipped: the file is meant to be edited by hand, and some editors write one.
    pub fn parse(bytes: &[u8]) -> Option<Settings> {
        serde_json::from_slice(without_bom(bytes)).ok()
    }

    /// Write the settings into `dir`, creating it first (best effort, like the window's size).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(dir.join(Self::FILE), bytes)
    }
}

/// `bytes` without a UTF-8 byte-order mark (both files are meant to be human-editable).
fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

/// A setting's `choice` may also be written as an unknown string by hand: it reads as `Auto`
/// rather than failing the whole file (and with it, the remembered backend).
fn choice_from_any<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Choice, D::Error> {
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw.as_deref().and_then(Choice::from_key).unwrap_or_default())
}

/// What `session.json` holds: the backend a run started on. Present means "the run that wrote this
/// never got to remove it", i.e. the process died instead of quitting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    #[serde(deserialize_with = "choice_from_any")]
    pub backend: Choice,
}

impl Marker {
    /// The file name inside the settings directory.
    pub const FILE: &'static str = "session.json";

    /// The marker a previous run left, if any: a start that finds one is starting after a crash.
    pub fn load(dir: Option<&Path>) -> Option<Marker> {
        let dir = dir?;
        let bytes = std::fs::read(dir.join(Self::FILE)).ok()?;
        serde_json::from_slice(without_bom(&bytes)).ok().filter(|m: &Marker| m.backend != Choice::Auto)
    }

    /// Record `backend` as the one this run is drawing with.
    pub fn write(dir: &Path, backend: Choice) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let bytes = serde_json::to_vec_pretty(&Marker { backend }).map_err(std::io::Error::other)?;
        std::fs::write(dir.join(Self::FILE), bytes)
    }

    /// Remove the marker: the run is leaving on purpose, so the next start is not a crash.
    pub fn clear(dir: &Path) {
        let _ = std::fs::remove_file(dir.join(Self::FILE));
    }
}

/// The backend name wgpu reports (`"dx12"`, `"vulkan"`, `"gl"`, `"metal"`), as a [`Choice`].
pub fn choice_of_backend(backend: &str) -> Option<Choice> {
    Choice::from_key(&backend.to_ascii_lowercase()).filter(|c| *c != Choice::Auto)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cella-renderer-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_choice_round_trips_through_its_key() {
        for choice in Choice::ALL {
            assert_eq!(Choice::from_key(choice.key()), Some(choice));
        }
        assert_eq!(Choice::from_key(" DX12 "), Some(Choice::Dx12), "case and spaces are forgiven");
        assert_eq!(Choice::from_key("directx"), None);
        assert_eq!(Choice::from_key(""), None);
    }

    #[test]
    fn settings_survive_a_save_and_load() {
        let dir = temp_dir("save");
        let settings = Settings { choice: Choice::Gl };
        settings.save(&dir).unwrap();
        assert_eq!(Settings::load(Some(&dir)), settings);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_broken_file_is_the_default() {
        let dir = temp_dir("broken");
        assert_eq!(Settings::load(Some(&dir)), Settings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(Settings::FILE), b"{ not json").unwrap();
        assert_eq!(Settings::load(Some(&dir)), Settings::default());
        // A hand-edited choice that names nothing reads as Auto instead of breaking the file.
        std::fs::write(dir.join(Settings::FILE), br#"{"choice":"direct3d"}"#).unwrap();
        assert_eq!(Settings::load(Some(&dir)).choice, Choice::Auto);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_marker_means_the_previous_run_died() {
        let dir = temp_dir("marker");
        assert_eq!(Marker::load(Some(&dir)), None, "no file, no crash");
        Marker::write(&dir, Choice::Dx12).unwrap();
        assert_eq!(Marker::load(Some(&dir)), Some(Marker { backend: Choice::Dx12 }));
        Marker::clear(&dir);
        assert_eq!(Marker::load(Some(&dir)), None, "a clean exit removed it");
        // A hand-edited marker that names no backend is not a crash to act on.
        std::fs::write(dir.join(Marker::FILE), br#"{"backend":"auto"}"#).unwrap();
        assert_eq!(Marker::load(Some(&dir)), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_file() {
        // Notepad and PowerShell's `Set-Content -Encoding UTF8` write one; the hand-edited file has
        // to keep working.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"choice":"vulkan"}"#);
        assert_eq!(Settings::parse(&bytes).map(|s| s.choice), Some(Choice::Vulkan));
        let dir = temp_dir("bom");
        std::fs::create_dir_all(&dir).unwrap();
        let mut marker = vec![0xEF, 0xBB, 0xBF];
        marker.extend_from_slice(br#"{"backend":"gl"}"#);
        std::fs::write(dir.join(Marker::FILE), &marker).unwrap();
        assert_eq!(Marker::load(Some(&dir)), Some(Marker { backend: Choice::Gl }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_backend_name_is_a_choice() {
        assert_eq!(choice_of_backend("Dx12"), Some(Choice::Dx12));
        assert_eq!(choice_of_backend("gl"), Some(Choice::Gl));
        assert_eq!(choice_of_backend("vulkan"), Some(Choice::Vulkan));
        assert_eq!(choice_of_backend("metal"), None, "Metal is not a choice on Windows");
        assert_eq!(choice_of_backend("auto"), None);
    }
}
