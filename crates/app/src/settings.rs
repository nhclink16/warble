//! What Warble remembers between runs (settings.json) and the folder its sound copies live in.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use warble_engine::{Config, Preset};

/// The audio files the soundboard takes, by extension (compared without case).
pub const AUDIO_EXTS: [&str; 4] = ["wav", "mp3", "ogg", "flac"];
/// Video (and m4a) files: dropped on the window, they open in the trim editor.
pub const VIDEO_EXTS: [&str; 4] = ["mp4", "m4v", "mov", "m4a"];
/// Everything the window takes when files are dropped on it.
pub const DROP_EXTS: [&str; 8] = ["wav", "mp3", "ogg", "flac", "mp4", "m4v", "mov", "m4a"];

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
    /// Put the voice effect on soundboard sounds too. Off until it's turned on, on the Voice tab.
    pub effect_on_sounds: bool,
    /// Linear, 0.25..=4.0.
    pub mic_gain: f32,
    pub monitor: bool,
    pub monitor_volume: f32,
    pub sound_volume: f32,
    /// Soundboard clips in your headphones too (separate from `monitor`, your voice).
    pub hear_sounds: bool,
    /// The user's own Freesound API key; empty searches Myinstants only.
    pub freesound_key: String,
    /// The phone remote: off until it's turned on, on the Setup tab.
    pub remote: bool,
    /// The secret in the remote's link (32 hex digits). A new one signs every phone out.
    pub remote_key: String,
    /// Push to talk: the mic goes into the call only while `talk_key` is held. Off until it's
    /// turned on, on the Voice tab.
    pub push_to_talk: bool,
    /// The push-to-talk key or mouse button, as a Windows virtual-key code (see `ptt`).
    pub talk_key: Option<u16>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            input: None,
            cable: None,
            headphones: None,
            preset: Preset::Normal.id().into(),
            effect: true,
            effect_on_sounds: false,
            mic_gain: 1.0,
            monitor: false,
            monitor_volume: 0.8,
            sound_volume: 0.8,
            hear_sounds: true,
            freesound_key: String::new(),
            remote: false,
            remote_key: String::new(),
            push_to_talk: false,
            talk_key: None,
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
        // A key Warble doesn't take is no key, and push to talk needs one.
        s.talk_key = s.talk_key.filter(|&vk| crate::ptt::name(vk).is_some());
        s.push_to_talk &= s.talk_key.is_some();
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
    has_ext(path, &AUDIO_EXTS)
}

pub fn is_video(path: &Path) -> bool {
    has_ext(path, &VIDEO_EXTS)
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// A tile's label: the file name without its extension.
pub fn sound_name(path: &Path) -> String {
    path.file_stem().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Every audio file in `dir`, sorted by name. Folders, such as the holding folder, are skipped.
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
    let mut input = std::fs::File::open(src)?;
    let (mut output, dest) = create_in(dir, src.file_name().unwrap_or_default().as_ref())?;
    if let Err(e) = std::io::copy(&mut input, &mut output) {
        drop(output);
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(dest)
}

/// A new, empty file in `dir` named `name`, or "stem (2).ext" and so on when that's taken. The
/// name is claimed by creating the file, so two imports at once never share one.
pub fn create_in(dir: &Path, name: &Path) -> std::io::Result<(std::fs::File, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let stem = sound_name(name);
    let ext = name
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut dest = dir.join(name);
    let mut n = 2;
    loop {
        match std::fs::File::create_new(&dest) {
            Ok(f) => return Ok((f, dest)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                dest = dir.join(format!("{stem} ({n}).{ext}"));
                n += 1;
            }
            Err(e) => return Err(e),
        }
    }
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

/// Inside the sounds folder: where a removed sound waits while it can still be undone.
pub const HOLD_DIR: &str = ".removed";

/// Moves the app's copy of a removed sound into the holding folder, and says where it went.
/// A file outside `dir` is the user's own and stays where it is (`None`).
pub fn hold_copy(path: &Path, dir: &Path) -> std::io::Result<Option<PathBuf>> {
    if path.parent() != Some(dir) {
        return Ok(None);
    }
    let hold = dir.join(HOLD_DIR);
    std::fs::create_dir_all(&hold)?;
    let to = hold.join(path.file_name().unwrap_or_default());
    match std::fs::rename(path, &to) {
        Ok(()) => Ok(Some(to)),
        // Already gone (deleted outside Warble): nothing to hold, and the tile can still go.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Moves a held sound back into `dir` under `name`, or "stem (2).ext" if that's been taken since.
pub fn restore_copy(held: &Path, dir: &Path, name: &Path) -> std::io::Result<PathBuf> {
    let (file, dest) = create_in(dir, name)?;
    drop(file);
    if let Err(e) = std::fs::rename(held, &dest) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(dest)
}

/// Deletes whatever waits in the holding folder: removals that can't be undone any more.
pub fn empty_hold(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.join(HOLD_DIR));
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
            push_to_talk: true,
            talk_key: Some(crate::ptt::MOUSE_4),
            effect_on_sounds: true,
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
        assert_eq!((s.push_to_talk, s.talk_key), (false, None), "an older file");
        assert!(!s.effect_on_sounds, "sounds stay clean in an older file");
        // The left mouse button can't be the key, so push to talk stays off.
        std::fs::write(
            dir.join("settings.json"),
            r#"{"push_to_talk": true, "talk_key": 1}"#,
        )
        .unwrap();
        let s = Settings::load(&dir);
        assert_eq!((s.push_to_talk, s.talk_key), (false, None));
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

    #[test]
    fn a_held_sound_goes_back_under_a_free_name_and_the_list_skips_the_hold() {
        let (outside, sounds) = (scratch("hold-outside"), scratch("hold-sounds"));
        let original = outside.join("Bruh.wav");
        std::fs::write(&original, b"x").unwrap();
        assert_eq!(hold_copy(&original, &sounds).unwrap(), None);
        assert!(
            original.exists(),
            "a file outside the sounds folder isn't moved"
        );

        let copy = copy_in(&original, &sounds).unwrap();
        let held = hold_copy(&copy, &sounds).unwrap().expect("moved aside");
        assert!(!copy.exists() && held.exists());
        assert!(
            list_sounds(&sounds).is_empty(),
            "the holding folder isn't a sound"
        );

        // A new "Bruh.wav" arrived in the meantime: the old one comes back beside it.
        std::fs::write(&copy, b"new").unwrap();
        let back = restore_copy(&held, &sounds, Path::new("Bruh.wav")).unwrap();
        assert_eq!(back.file_name().unwrap(), "Bruh (2).wav");
        assert_eq!(std::fs::read(&copy).unwrap(), b"new");
        assert!(!held.exists());

        let again = hold_copy(&back, &sounds).unwrap().unwrap();
        empty_hold(&sounds);
        assert!(!again.exists() && !sounds.join(HOLD_DIR).exists());
    }
}
