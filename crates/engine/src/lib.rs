//! Warble's audio engine.
//!
//! Signal path (the virtual cable's output stream is the master clock):
//!
//! ```text
//! mic (cpal input) ──ring──▶ drift/rate resampler ──▶ mic gain ──▶ Voice (warble-dsp)
//!                                                                     │
//!                                   soundboard voices (mixed) ──▶  sum ──▶ VB-CABLE output
//!                                                                     │
//!                                                                     └──ring──▶ resampler ──▶ monitor output
//! ```
//!
//! People in the call hear the VB-CABLE output: in Discord they pick
//! "CABLE Output (VB-Audio Virtual Cable)" as their input device.
//!
//! CONTRACT:
//! - `Engine` is a cheap handle: `Send + Sync`, every method takes `&self` and returns quickly.
//!   cpal streams live on a thread the engine owns (they are not `Send` on every host).
//! - Audio callbacks never allocate, lock, block, log or panic. UI → audio goes over lock-free
//!   queues and atomics; audio → UI goes over atomics read by `status()`.
//! - Separate devices run on separate clocks: the mic and monitor paths must absorb drift
//!   (adaptive resampling on ring fill level) so hours-long calls neither grow latency nor click
//!   repeatedly. Target end-to-end mic → cable latency under ~40 ms.
//! - Errors never panic the app: they land in `Status::error` and the engine keeps what it can
//!   running (e.g. no cable found → monitor can still work so you can preview effects).

pub use warble_dsp::Preset;

use std::path::Path;

/// One audio device as the OS names it. The name is also its id in settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Devices {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

/// Every input and output device on the default host (WASAPI on Windows).
pub fn list_devices() -> Devices {
    todo!("warble-engine: list_devices")
}

/// The output device a virtual cable exposes, if one is installed: VB-CABLE's
/// "CABLE Input (VB-Audio Virtual Cable)" (match on "CABLE Input", case-insensitive).
pub fn find_virtual_cable(devices: &Devices) -> Option<String> {
    let _ = devices;
    todo!("warble-engine: find_virtual_cable")
}

/// Which devices to use. `None` means: system default (input, monitor) or auto-detect (cable).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub input: Option<String>,
    pub cable: Option<String>,
    pub monitor: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SoundId(pub u32);

/// A cheap snapshot for the UI, safe to call every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// The mic → cable path is running.
    pub running: bool,
    pub input_name: Option<String>,
    pub cable_name: Option<String>,
    pub monitor_name: Option<String>,
    /// Plain-English problem for the UI, e.g. "VB-CABLE isn't installed".
    pub error: Option<String>,
    /// Peak levels, 0.0..=1.0, with a short decay so meters look smooth.
    pub input_level: f32,
    pub output_level: f32,
    /// Sounds playing right now.
    pub playing: Vec<SoundId>,
    /// Times an output callback ran short of mic audio since start (diagnostics).
    pub underruns: u64,
}

pub struct Engine {
    inner: Inner,
}

enum Inner {
    Mock(std::sync::Mutex<mock::Mock>),
    /// TODO(warble-engine): the real engine (device thread, lock-free control, atomics).
    Real(()),
}

impl Engine {
    /// Start with these devices. Never fails: problems show in `status().error`.
    pub fn start(config: Config) -> Engine {
        let _ = config;
        todo!("warble-engine: Engine::start")
    }

    /// No devices or threads. `status()` reports a plausible running state (VB-CABLE found,
    /// gently moving meters) so the UI can render headless snapshots and tests. Every other
    /// method works and is reflected in `status()` (e.g. `play_sound` marks the sound playing
    /// for a while). Deterministic: meters advance per `status()` call, not by wall clock.
    pub fn mock() -> Engine {
        Engine { inner: Inner::Mock(std::sync::Mutex::new(mock::Mock::default())) }
    }

    fn mock_state(&self) -> Option<std::sync::MutexGuard<'_, mock::Mock>> {
        match &self.inner {
            Inner::Mock(m) => Some(m.lock().unwrap_or_else(|e| e.into_inner())),
            Inner::Real(_) => None,
        }
    }

    /// Devices for the settings pickers: `list_devices()` for a real engine, a fixed plausible
    /// set (mic, headphones, VB-CABLE) for the mock.
    pub fn devices(&self) -> Devices {
        match &self.inner {
            Inner::Mock(_) => mock::devices(),
            Inner::Real(_) => list_devices(),
        }
    }

    /// Switch devices. Keeps preset, gains, loaded sounds and monitor settings.
    pub fn restart(&self, config: Config) {
        if let Some(mut m) = self.mock_state() {
            m.config = config;
            return;
        }
        todo!("warble-engine: Engine::restart")
    }

    pub fn set_preset(&self, preset: Preset) {
        if let Some(mut m) = self.mock_state() {
            m.preset = preset;
            return;
        }
        todo!("warble-engine: Engine::set_preset")
    }

    /// Off sends the dry voice (soundboard still plays).
    pub fn set_effect_enabled(&self, enabled: bool) {
        if let Some(mut m) = self.mock_state() {
            m.effect_enabled = enabled;
            return;
        }
        todo!("warble-engine: Engine::set_effect_enabled")
    }

    /// Linear gain on the mic before effects, 0.0..=4.0.
    pub fn set_mic_gain(&self, gain: f32) {
        if let Some(mut m) = self.mock_state() {
            m.mic_gain = gain;
            return;
        }
        todo!("warble-engine: Engine::set_mic_gain")
    }

    /// Hear yourself: the exact cable mix, sent to the monitor device.
    pub fn set_monitor(&self, enabled: bool) {
        if let Some(mut m) = self.mock_state() {
            m.monitor = enabled;
            return;
        }
        todo!("warble-engine: Engine::set_monitor")
    }

    /// 0.0..=1.0
    pub fn set_monitor_volume(&self, volume: f32) {
        if let Some(mut m) = self.mock_state() {
            m.monitor_volume = volume;
            return;
        }
        todo!("warble-engine: Engine::set_monitor_volume")
    }

    /// Soundboard level into the call (and monitor), 0.0..=1.0.
    pub fn set_sound_volume(&self, volume: f32) {
        if let Some(mut m) = self.mock_state() {
            m.sound_volume = volume;
            return;
        }
        todo!("warble-engine: Engine::set_sound_volume")
    }

    /// Decode a WAV, MP3, OGG/Vorbis or FLAC file into memory. Runs on the caller's thread.
    /// Errors are plain English ("Couldn't read clip.mp3: unsupported format").
    /// The mock accepts any path without reading it.
    pub fn load_sound(&self, path: &Path) -> Result<SoundId, String> {
        if let Some(mut m) = self.mock_state() {
            let _ = path;
            m.next_sound += 1;
            return Ok(SoundId(m.next_sound));
        }
        todo!("warble-engine: Engine::load_sound")
    }

    /// Stops it if playing and frees it.
    pub fn remove_sound(&self, id: SoundId) {
        if let Some(mut m) = self.mock_state() {
            m.playing.retain(|(s, _)| *s != id);
            return;
        }
        todo!("warble-engine: Engine::remove_sound")
    }

    /// Play from the start; pressing a sound that is already playing restarts it.
    /// Several different sounds can play at once.
    pub fn play_sound(&self, id: SoundId) {
        if let Some(mut m) = self.mock_state() {
            m.playing.retain(|(s, _)| *s != id);
            m.playing.push((id, mock::PLAY_TICKS));
            return;
        }
        todo!("warble-engine: Engine::play_sound")
    }

    pub fn stop_sounds(&self) {
        if let Some(mut m) = self.mock_state() {
            m.playing.clear();
            return;
        }
        todo!("warble-engine: Engine::stop_sounds")
    }

    pub fn status(&self) -> Status {
        if let Some(mut m) = self.mock_state() {
            return m.status();
        }
        todo!("warble-engine: Engine::status")
    }
}

mod mock {
    use super::*;

    /// How many `status()` calls a mock sound stays "playing" (about 3 s at 30 polls a second).
    pub const PLAY_TICKS: u32 = 90;

    pub struct Mock {
        pub config: Config,
        pub preset: Preset,
        pub effect_enabled: bool,
        pub mic_gain: f32,
        pub monitor: bool,
        pub monitor_volume: f32,
        pub sound_volume: f32,
        pub next_sound: u32,
        pub playing: Vec<(SoundId, u32)>,
        tick: u32,
    }

    impl Default for Mock {
        fn default() -> Mock {
            Mock {
                config: Config::default(),
                preset: Preset::Normal,
                effect_enabled: true,
                mic_gain: 1.0,
                monitor: false,
                monitor_volume: 0.8,
                sound_volume: 0.8,
                next_sound: 0,
                playing: Vec::new(),
                tick: 0,
            }
        }
    }

    pub fn devices() -> Devices {
        let d = |name: &str, is_default| DeviceInfo { name: name.to_string(), is_default };
        Devices {
            inputs: vec![d("Microphone (USB Audio Device)", true), d("CABLE Output (VB-Audio Virtual Cable)", false)],
            outputs: vec![
                d("Headphones (USB Audio Device)", true),
                d("Speakers (High Definition Audio Device)", false),
                d("CABLE Input (VB-Audio Virtual Cable)", false),
            ],
        }
    }

    impl Mock {
        pub fn status(&mut self) -> Status {
            self.tick = self.tick.wrapping_add(1);
            for (_, left) in &mut self.playing {
                *left = left.saturating_sub(1);
            }
            self.playing.retain(|(_, left)| *left > 0);
            let t = self.tick as f32 * 0.21;
            // A talking-voice shape: two slow sines, clipped to silence between "words".
            let voice = ((t.sin() * 0.5 + (t * 2.7).sin() * 0.3) * self.mic_gain.min(2.0)).max(0.0).min(1.0);
            let sounds = if self.playing.is_empty() { 0.0 } else { 0.35 * self.sound_volume };
            let pick = |want: &Option<String>, fallback: &str| Some(want.clone().unwrap_or_else(|| fallback.to_string()));
            Status {
                running: true,
                input_name: pick(&self.config.input, "Microphone (USB Audio Device)"),
                cable_name: pick(&self.config.cable, "CABLE Input (VB-Audio Virtual Cable)"),
                monitor_name: pick(&self.config.monitor, "Headphones (USB Audio Device)"),
                error: None,
                input_level: voice,
                output_level: (voice * 0.9 + sounds).min(1.0),
                playing: self.playing.iter().map(|(id, _)| *id).collect(),
                underruns: 0,
            }
        }
    }
}
