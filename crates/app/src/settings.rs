//! What Warble remembers between runs (settings.json) and the folder its sound copies live in.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use warble_engine::{Config, Preset};

/// The audio files the soundboard takes, by extension (compared without case).
pub const AUDIO_EXTS: [&str; 4] = ["wav", "mp3", "ogg", "flac"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Device names; `None` is the system default (or auto-detect, for the cable).
    pub input: Option<String>,
    pub cable: Option<String>,
    pub headphones: Option<String>,
    /// `Preset::id`.
    pub preset: String,
    pub effect: bool,
    /// Linear, 0.25..=4.0.
    pub mic_gain: f32,
    pub monitor: bool,
    pub monitor_volume: f32,
    pub sound_volume: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            input: None,
            cable: None,
            headphones: None,
            preset: Preset::Normal.id().into(),
            effect: true,
            mic_gain: 1.0,
            monitor: false,
            monitor_volume: 0.8,
            sound_volume: 0.8,
        }
    }
}

impl Settings {
    pub fn preset(&self) -> Preset {
        Preset::from_id(&self.preset).unwrap_or_default()
    }

    pub fn config(&self) -> Config {
        Config {
            input: self.input.clone(),
            cable: self.cable.clone(),
            monitor: self.headphones.clone(),
        }
    }

    /// The saved settings, or the defaults when there are none or they don't read.
    pub fn load(dir: &Path) -> Settings {
        let read = std::fs::read_to_string(dir.join("settings.json")).ok();
        let mut s: Settings = read
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        s.mic_gain = s.mic_gain.clamp(0.25, 4.0);
        s.monitor_volume = s.monitor_volume.clamp(0.0, 1.0);
        s.sound_volume = s.sound_volume.clamp(0.0, 1.0);
        s
    }

    /// Writes through a temporary file, so a crash leaves the old settings or the new.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("settings.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, dir.join("settings.json"))
    }
}

/// `%APPDATA%\Warble` on Windows, `$XDG_CONFIG_HOME/warble` (else `~/.config/warble`) elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    let id = if cfg!(windows) { "Warble" } else { "warble" };
    rune::dirs::AppDirs::new(id).map(|d| d.config().to_path_buf())
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// A tile's label: the file name without its extension.
pub fn sound_name(path: &Path) -> String {
    path.file_stem().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Every audio file in `dir`, sorted by name.
pub fn list_sounds(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_audio(p))
        .collect();
    paths.sort_by_key(|p| sound_name(p).to_lowercase());
    paths
}

/// Copies `src` into `dir` under its own name, or "name (2).ext" and so on when that's taken.
pub fn copy_in(src: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stem = sound_name(src);
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut dest = dir.join(src.file_name().unwrap_or_default());
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

/// Deletes the app's copy of a sound. Only files directly inside `dir` are ever removed, so a
/// user's original elsewhere is never touched.
pub fn delete_copy(path: &Path, dir: &Path) -> std::io::Result<()> {
    if path.parent() != Some(dir) {
        return Ok(());
    }
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("warble-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_round_trip_and_fill_gaps_with_defaults() {
        let dir = scratch("settings");
        let s = Settings {
            preset: Preset::Chipmunk.id().into(),
            mic_gain: 2.0,
            cable: Some("CABLE".into()),
            ..Default::default()
        };
        s.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), s);
        std::fs::write(
            dir.join("settings.json"),
            r#"{"preset": "nope", "mic_gain": 99}"#,
        )
        .unwrap();
        let s = Settings::load(&dir);
        assert_eq!(
            (s.preset(), s.mic_gain, s.effect),
            (Preset::Normal, 4.0, true)
        );
    }

    #[test]
    fn copies_never_clobber_and_only_the_apps_copy_is_deleted() {
        let (outside, sounds) = (scratch("outside"), scratch("sounds"));
        let original = outside.join("Air horn.MP3");
        std::fs::write(&original, b"x").unwrap();
        let a = copy_in(&original, &sounds).unwrap();
        let b = copy_in(&original, &sounds).unwrap();
        assert_eq!(b.file_name().unwrap(), "Air horn (2).MP3");
        assert_eq!(list_sounds(&sounds), vec![a.clone(), b]);
        delete_copy(&original, &sounds).unwrap();
        assert!(original.exists(), "a file outside the sounds folder stays");
        delete_copy(&a, &sounds).unwrap();
        assert!(!a.exists());
    }
}
