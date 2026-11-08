//! The app's state and every action the window, hotkeys and tray take on it. Each action
//! pushes its change to the engine at once and schedules a settings save.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::tray::{Tray, TrayItem};
use rune::{AppHandle, FileDialogOptions, Live, PointerPress, Tx};
use warble_engine::{Devices, Engine, Preset, SoundId, Status};

use crate::find::Find;
use crate::remote::Remote;
use crate::settings::{self, AUDIO_EXTS, Settings};
use crate::trim::Trim;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Voice,
    Soundboard,
    Setup,
}

/// The three device pickers on the Setup tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Input,
    Cable,
    Headphones,
}

pub struct Sound {
    pub id: SoundId,
    pub name: String,
    /// The app's copy in the sounds folder (or, with no folder, the file itself).
    pub path: PathBuf,
}

/// A removed sound, kept aside while its Undo shows.
pub struct Removed {
    pub name: String,
    /// Where it was on the board.
    pub path: PathBuf,
    /// Where it waits in the holding folder; `None` for a file outside the sounds folder,
    /// which was never moved.
    pub held: Option<PathBuf>,
    /// When the Undo goes and the removal is for good.
    pub until: Instant,
}

/// How long a removal can be undone.
pub const UNDO_TIME: Duration = Duration::from_secs(10);

/// Lit and unlit segments in each level meter.
pub const SEGMENTS: usize = 28;
pub const SEGMENT_OFF: f32 = 0.13;

/// The two level meters, one live opacity per segment, so a level change never re-runs the
/// view or its layout, and a silent mic wakes nothing.
pub struct Meters {
    pub mic: Vec<Live<f32>>,
    pub out: Vec<Live<f32>>,
}

impl Meters {
    fn new() -> Meters {
        let segments = || (0..SEGMENTS).map(|_| Live::new(SEGMENT_OFF)).collect();
        Meters {
            mic: segments(),
            out: segments(),
        }
    }

    pub fn show(&self, mic: f32, out: f32) {
        light(&self.mic, mic);
        light(&self.out, out);
    }
}

fn light(segments: &[Live<f32>], level: f32) {
    let lit = loudness(level) * segments.len() as f32;
    for (i, s) in segments.iter().enumerate() {
        s.set(SEGMENT_OFF + (1.0 - SEGMENT_OFF) * (lit - i as f32).clamp(0.0, 1.0));
    }
}

/// A peak level (0..=1, linear) on a -54..0 dB scale, which is how loud it looks to people.
fn loudness(level: f32) -> f32 {
    if level <= 0.0 {
        0.0
    } else {
        (1.0 + 20.0 * level.log10() / 54.0).clamp(0.0, 1.0)
    }
}

/// Global hotkeys, as `rune::Shortcut` parses them.
pub const EFFECT_KEY: &str = "ctrl+alt+v";
pub const MONITOR_KEY: &str = "ctrl+alt+m";
pub const STOP_KEY: &str = "ctrl+alt+s";
pub fn preset_key(i: usize) -> String {
    format!("ctrl+alt+{}", i + 1)
}
pub fn sound_key(i: usize) -> String {
    format!("ctrl+shift+{}", i + 1)
}

/// "ctrl+alt+v" as keycaps: ["Ctrl", "Alt", "V"].
pub fn keycaps(chord: &str) -> Vec<String> {
    chord
        .split('+')
        .map(|p| {
            let mut c = p.chars();
            c.next()
                .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect()
}

pub struct State {
    pub engine: Arc<Engine>,
    pub settings: Settings,
    /// Where settings.json and the sounds folder live. `None` (mock mode, tests, snapshots)
    /// means nothing is read from or written to disk.
    pub dir: Option<PathBuf>,
    pub tab: Tab,
    pub status: Status,
    pub devices: Devices,
    pub sounds: Vec<Sound>,
    /// Hotkeys that didn't register: (chord, why).
    pub hotkey_failures: Vec<(String, String)>,
    /// A problem worth telling the user about, such as a file that didn't load.
    pub notice: Option<String>,
    /// The last sound removed, while it can be undone. It shares the notice's place, and a
    /// notice that takes the place makes the removal permanent.
    pub removed: Option<Removed>,
    pub open_picker: Option<Picker>,
    /// A sound tile's context menu (Trim, Remove), and where it opened.
    pub sound_menu: Option<(SoundId, PointerPress)>,
    pub drop_hover: bool,
    pub meters: Meters,
    pub handle: Option<AppHandle<State>>,
    pub tray: Option<Tray>,
    /// A headless render (snapshot or script): there is no real window to watch.
    pub headless: bool,
    /// The no-cable scene: report VB-CABLE as missing whatever the engine says.
    pub sim_no_cable: bool,
    /// The audio-problem scene: report this error whatever the engine says.
    pub sim_error: Option<String>,
    /// Setup's reference sections, closed until opened (not saved).
    pub discord_open: bool,
    pub hotkeys_open: bool,
    pub dialog_open: bool,
    /// The Find sounds page, and the trim editor while it's open (both on the Soundboard tab).
    pub find: Find,
    pub trim: Option<Trim>,
    pub remote: Remote,
    tray_shows: Option<(bool, Preset)>,
    save_due: Option<Instant>,
    last_poll: f64,
}

/// Status polls a second while the window shows.
const POLL: f64 = 1.0 / 30.0;
const SAVE_DELAY: Duration = Duration::from_millis(400);

impl State {
    /// The mock engine and no folder: tests, snapshots and `WARBLE_MOCK=1`.
    pub fn mock() -> State {
        State::with(Engine::mock(), Settings::default(), None)
    }

    /// The real engine, with settings and sounds from the config folder.
    pub fn load() -> State {
        let dir = settings::config_dir();
        let settings = dir.as_deref().map(Settings::load).unwrap_or_default();
        let engine = Engine::start(settings.config());
        State::with(engine, settings, dir)
    }

    fn with(engine: Engine, settings: Settings, dir: Option<PathBuf>) -> State {
        engine.set_preset(settings.preset());
        engine.set_effect_enabled(settings.effect);
        engine.set_mic_gain(settings.mic_gain);
        engine.set_monitor(settings.monitor);
        engine.set_monitor_volume(settings.monitor_volume);
        engine.set_sound_volume(settings.sound_volume);
        engine.set_hear_sounds(settings.hear_sounds);
        State {
            find: Find::new(&settings.freesound_key),
            status: engine.status(),
            devices: engine.devices(),
            engine: Arc::new(engine),
            settings,
            dir,
            tab: Tab::Voice,
            sounds: Vec::new(),
            hotkey_failures: Vec::new(),
            notice: None,
            removed: None,
            open_picker: None,
            sound_menu: None,
            drop_hover: false,
            meters: Meters::new(),
            handle: None,
            tray: None,
            headless: false,
            sim_no_cable: false,
            sim_error: None,
            discord_open: false,
            hotkeys_open: false,
            dialog_open: false,
            trim: None,
            remote: Remote::default(),
            tray_shows: None,
            save_due: None,
            last_poll: f64::NEG_INFINITY,
        }
    }

    pub fn sounds_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("sounds"))
    }

    /// The config folder, or with none (mock mode, tests) a temporary one: where yt-dlp lives.
    pub fn work_dir(&self) -> PathBuf {
        self.dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("warble-mock"))
    }

    /// Runs `work` on a worker thread once the app runs, then `done` with its result on the UI
    /// thread; inline in tests and snapshots.
    pub fn off_thread<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut State, T) + Send + 'static,
    ) {
        match self.handle.clone() {
            Some(h) if !self.headless => {
                std::thread::spawn(move || {
                    let result = work();
                    h.update(move |s, tx| {
                        done(s, result);
                        tx.instant();
                    });
                });
            }
            _ => {
                let result = work();
                done(self, result);
            }
        }
    }

    /// Loads what's already in the sounds folder, first deleting any removal that was waiting
    /// on an Undo when Warble last closed.
    pub fn load_saved_sounds(&mut self) {
        if let Some(dir) = self.sounds_dir() {
            settings::empty_hold(&dir);
            self.load_paths(settings::list_sounds(&dir), false);
        }
    }

    // --- Status ---

    /// Reads the engine about 30 times a second while the window shows, and never while it's
    /// hidden. True when something the view shows changed; the meters move on their own.
    pub fn poll(&mut self, t: f64, tx: &mut Tx) -> bool {
        let now = Instant::now();
        if let Some(due) = self.save_due {
            if now >= due {
                self.save_now();
            } else {
                tx.wake_after(due - now);
            }
        }
        // The Undo goes when its time is up or another notice takes its place.
        let mut undo_gone = false;
        if let Some(until) = self.removed.as_ref().map(|r| r.until) {
            if now >= until || self.notice.is_some() {
                self.forget_removed();
                undo_gone = true;
            } else {
                tx.wake_after(until - now);
            }
        }
        if !self.headless && self.handle.as_ref().is_some_and(|h| !h.is_visible()) {
            return undo_gone;
        }
        let since = t - self.last_poll;
        if since < POLL * 0.9 {
            tx.wake_after(Duration::from_secs_f64(POLL - since));
            return undo_gone;
        }
        self.last_poll = t;
        tx.wake_after(Duration::from_secs_f64(POLL));
        self.refresh_status() || undo_gone
    }

    pub fn refresh_status(&mut self) -> bool {
        let mut st = self.engine.status();
        if self.sim_no_cable {
            st.running = false;
            st.cable_name = None;
            st.error = Some("VB-CABLE isn’t installed".into());
        }
        if let Some(e) = &self.sim_error {
            st.running = false;
            st.error = Some(e.clone());
        }
        self.meters.show(st.input_level, st.output_level);
        let old = &self.status;
        let changed = st.running != old.running
            || st.error != old.error
            || st.input_name != old.input_name
            || st.cable_name != old.cable_name
            || st.monitor_name != old.monitor_name
            || st.playing != old.playing
            || st.preview != old.preview;
        self.status = st;
        changed
    }

    /// The cable couldn't be found: auto-detect is on and the engine says why nothing runs.
    pub fn cable_missing(&self) -> bool {
        let st = &self.status;
        !st.running
            && st.cable_name.is_none()
            && st.error.is_some()
            && self.settings.cable.is_none()
    }

    /// What the header's status pill says, and its tone: "live", "warn", "danger" or "muted".
    /// The phone remote shows the same words. A device error's full text is on the Setup tab.
    pub fn status_label(&self) -> (&'static str, &'static str) {
        let st = &self.status;
        if st.running && !self.hotkey_failures.is_empty() {
            ("Hotkeys blocked", "warn")
        } else if st.running {
            ("Live", "live")
        } else if self.cable_missing() {
            ("VB-CABLE missing", "warn")
        } else if st.error.is_some() {
            ("Audio problem", "danger")
        } else {
            ("Starting…", "muted")
        }
    }

    pub fn is_playing(&self, id: SoundId) -> bool {
        self.status.playing.contains(&id)
    }

    // --- Voice ---

    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.open_picker = None;
        if tab != Tab::Soundboard && self.status.preview.is_some() {
            self.stop_preview();
        }
        if tab == Tab::Setup {
            // Headsets and networks come and go; show what's here now.
            self.devices = self.engine.devices();
            if self.settings.remote {
                self.remote.find_addresses();
            }
        }
    }

    pub fn set_effect(&mut self, on: bool) {
        self.settings.effect = on;
        self.engine.set_effect_enabled(on);
        self.changed();
    }

    pub fn select_preset(&mut self, preset: Preset) {
        self.settings.preset = preset.id().into();
        self.engine.set_preset(preset);
        self.changed();
    }

    pub fn set_mic_gain(&mut self, gain: f32) {
        self.settings.mic_gain = gain.clamp(0.25, 4.0);
        self.engine.set_mic_gain(self.settings.mic_gain);
        self.changed();
    }

    pub fn set_monitor(&mut self, on: bool) {
        self.settings.monitor = on;
        self.engine.set_monitor(on);
        self.changed();
    }

    pub fn set_monitor_volume(&mut self, volume: f32) {
        self.settings.monitor_volume = volume.clamp(0.0, 1.0);
        self.engine.set_monitor_volume(self.settings.monitor_volume);
        self.changed();
    }

    pub fn set_device(&mut self, picker: Picker, name: Option<String>) {
        self.open_picker = None;
        let slot = match picker {
            Picker::Input => &mut self.settings.input,
            Picker::Cable => &mut self.settings.cable,
            Picker::Headphones => &mut self.settings.headphones,
        };
        if *slot == name {
            return;
        }
        *slot = name;
        self.engine.restart(self.settings.config());
        self.changed();
    }

    // --- Soundboard ---

    pub fn set_sound_volume(&mut self, volume: f32) {
        self.settings.sound_volume = volume.clamp(0.0, 1.0);
        self.engine.set_sound_volume(self.settings.sound_volume);
        self.changed();
    }

    pub fn set_hear_sounds(&mut self, on: bool) {
        self.settings.hear_sounds = on;
        self.engine.set_hear_sounds(on);
        self.changed();
    }

    pub fn play(&mut self, id: SoundId) {
        self.engine.play_sound(id);
        if !self.status.playing.contains(&id) {
            self.status.playing.push(id);
        }
    }

    /// The `i`th tile (hotkeys Ctrl+Shift+1..9).
    pub fn play_index(&mut self, i: usize) {
        if let Some(id) = self.sounds.get(i).map(|s| s.id) {
            self.play(id);
        }
    }

    pub fn stop_sounds(&mut self) {
        self.engine.stop_sounds();
        self.status.playing.clear();
    }

    /// Removes the tile and moves the app's copy into the holding folder, so Undo can bring it
    /// back for a few seconds. The file the user added it from is never touched.
    pub fn remove_sound(&mut self, id: SoundId) {
        let Some(i) = self.sounds.iter().position(|s| s.id == id) else {
            return;
        };
        // One removal can be undone at a time: the one before is now for good.
        self.forget_removed();
        let held = match self.sounds_dir() {
            Some(dir) => match settings::hold_copy(&self.sounds[i].path, &dir) {
                Ok(held) => held,
                Err(e) => {
                    self.notice = Some(format!(
                        "Couldn’t remove “{}”: {e}. Close any app that’s using it, then try again.",
                        self.sounds[i].name
                    ));
                    return;
                }
            },
            None => None,
        };
        let sound = self.sounds.remove(i);
        self.engine.remove_sound(id);
        self.status.playing.retain(|p| *p != id);
        self.notice = None;
        self.removed = Some(Removed {
            name: sound.name,
            path: sound.path,
            held,
            until: Instant::now() + UNDO_TIME,
        });
    }

    /// Puts the last removed sound back on the board.
    pub fn undo_remove(&mut self) {
        let Some(r) = self.removed.take() else {
            return;
        };
        let path = match (&r.held, self.sounds_dir()) {
            (Some(held), Some(dir)) => {
                let name = r.path.file_name().unwrap_or_default();
                match settings::restore_copy(held, &dir, name.as_ref()) {
                    Ok(path) => path,
                    Err(e) => {
                        self.notice = Some(format!(
                            "Couldn’t put “{}” back: {e}. Add it again from where you got it.",
                            r.name
                        ));
                        return;
                    }
                }
            }
            _ => r.path,
        };
        self.load_paths(vec![path], false);
    }

    /// Makes the last removal permanent: its Undo goes and the holding folder is emptied.
    pub fn forget_removed(&mut self) {
        if self.removed.take().is_some()
            && let Some(dir) = self.sounds_dir()
        {
            settings::empty_hold(&dir);
        }
    }

    /// Copies audio files into the sounds folder and loads them; other files are skipped.
    /// A video among them (the first, if there are several) opens in the trim editor instead.
    pub fn add_sounds(&mut self, paths: Vec<PathBuf>) {
        let video = paths.iter().find(|p| settings::is_video(p)).cloned();
        let paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| settings::is_audio(p))
            .collect();
        if !paths.is_empty() {
            self.set_tab(Tab::Soundboard);
            self.load_paths(paths, true);
        }
        if let Some(video) = video {
            let name = settings::sound_name(&video);
            self.trim_file(video, name);
        }
    }

    /// The native picker. Windows can pick several files at once; elsewhere Rune's own
    /// picker takes one.
    pub fn pick_sounds(&mut self, tx: &mut Tx) {
        if self.dialog_open {
            return;
        }
        #[cfg(windows)]
        if let (Some(h), false) = (self.handle.clone(), self.headless) {
            self.dialog_open = true;
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_title("Add sounds")
                    .add_filter("Audio", &AUDIO_EXTS)
                    .pick_files();
                h.update(move |s, _| {
                    s.dialog_open = false;
                    s.add_sounds(picked.unwrap_or_default());
                });
            });
            return;
        }
        let options = FileDialogOptions {
            title: "Add sounds".into(),
            ..Default::default()
        }
        .filter("Audio", &AUDIO_EXTS);
        tx.open_file(options, |s: &mut State, result, _| match result {
            Ok(Some(path)) => s.add_sounds(vec![path]),
            Ok(None) => {}
            Err(e) => {
                s.notice = Some(format!(
                    "Couldn’t open the file picker: {e}. Drop your sound files on this window instead."
                ))
            }
        });
    }

    /// Decodes (and with `copy`, first copies in) on a worker thread once the app runs, so a
    /// big file never stalls the window; inline in tests.
    pub(crate) fn load_paths(&mut self, paths: Vec<PathBuf>, copy: bool) {
        let engine = self.engine.clone();
        let into = if copy { self.sounds_dir() } else { None };
        let work = move || load_files(&engine, paths, into.as_deref());
        match self.handle.clone() {
            Some(h) if !self.headless => {
                std::thread::spawn(move || {
                    let done = work();
                    h.update(move |s, _| s.loaded(done));
                });
            }
            _ => {
                let done = work();
                self.loaded(done);
            }
        }
    }

    /// Puts newly loaded sounds on the board, and says which files didn't load, one per line.
    pub(crate) fn loaded(&mut self, (sounds, errors): (Vec<Sound>, Vec<String>)) {
        for sound in sounds {
            // A file already on the board (re-added from the sounds folder): keep one tile, or
            // removing either would delete the file the other still plays.
            if self.sounds.iter().any(|s| same_file(&s.path, &sound.path)) {
                self.engine.remove_sound(sound.id);
            } else {
                self.sounds.push(sound);
            }
        }
        self.sounds.sort_by_key(|s| s.name.to_lowercase());
        if !errors.is_empty() {
            self.notice = Some(errors.join("\n"));
        }
    }

    // --- Saving and the tray ---

    pub(crate) fn changed(&mut self) {
        if self.dir.is_some() {
            self.save_due = Some(Instant::now() + SAVE_DELAY);
        }
        self.sync_tray();
    }

    pub fn save_now(&mut self) {
        self.save_due = None;
        if let Some(dir) = &self.dir
            && let Err(e) = self.settings.save(dir)
        {
            eprintln!("warble: couldn't save settings: {e}");
        }
    }

    /// Keeps the tray's effect item and tooltip in step with the window.
    pub fn sync_tray(&mut self) {
        let Some(tray) = &self.tray else { return };
        let shows = (self.settings.effect, self.settings.preset());
        if self.tray_shows == Some(shows) {
            return;
        }
        self.tray_shows = Some(shows);
        tray.set_menu(tray_menu(shows.0));
        tray.set_tooltip(tray_tooltip(shows.0, shows.1));
    }
}

pub fn tray_menu(effect: bool) -> Vec<TrayItem> {
    vec![
        TrayItem::action("show", "Show Warble"),
        TrayItem::action(
            "effect",
            if effect {
                "Turn voice effect off"
            } else {
                "Turn voice effect on"
            },
        ),
        TrayItem::Separator,
        TrayItem::action("quit", "Quit Warble"),
    ]
}

pub fn tray_tooltip(effect: bool, preset: Preset) -> String {
    if effect {
        format!("Warble: {} voice", preset.name())
    } else {
        "Warble: voice effect off".into()
    }
}

/// Windows paths differ in case and form for the same file, so compare resolved paths too.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

fn load_files(
    engine: &Engine,
    paths: Vec<PathBuf>,
    into: Option<&Path>,
) -> (Vec<Sound>, Vec<String>) {
    let (mut sounds, mut errors) = (Vec::new(), Vec::new());
    for src in paths {
        let path = match into {
            // Dropped from the sounds folder itself: it's already the app's copy.
            Some(dir) if src.parent() != Some(dir) => match settings::copy_in(&src, dir) {
                Ok(p) => p,
                Err(e) => {
                    let name = src.file_name().unwrap_or_default().to_string_lossy();
                    errors.push(format!(
                        "Couldn’t copy “{name}” into Warble: {e}. Check the file is still there \
                         and your PC has free space."
                    ));
                    continue;
                }
            },
            _ => src,
        };
        match engine.load_sound(&path) {
            Ok(id) => sounds.push(Sound {
                id,
                name: settings::sound_name(&path),
                path,
            }),
            Err(e) => {
                errors.push(e);
                if let Some(dir) = into {
                    let _ = settings::delete_copy(&path, dir);
                }
            }
        }
    }
    (sounds, errors)
}
