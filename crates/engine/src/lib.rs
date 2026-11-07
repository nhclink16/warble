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
    _todo: (),
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
    /// for its duration).
    pub fn mock() -> Engine {
        todo!("warble-engine: Engine::mock")
    }

    /// Switch devices. Keeps preset, gains, loaded sounds and monitor settings.
    pub fn restart(&self, config: Config) {
        let _ = config;
        todo!("warble-engine: Engine::restart")
    }

    pub fn set_preset(&self, preset: Preset) {
        let _ = preset;
        todo!("warble-engine: Engine::set_preset")
    }

    /// Off sends the dry voice (soundboard still plays).
    pub fn set_effect_enabled(&self, enabled: bool) {
        let _ = enabled;
        todo!("warble-engine: Engine::set_effect_enabled")
    }

    /// Linear gain on the mic before effects, 0.0..=4.0.
    pub fn set_mic_gain(&self, gain: f32) {
        let _ = gain;
        todo!("warble-engine: Engine::set_mic_gain")
    }

    /// Hear yourself: the exact cable mix, sent to the monitor device.
    pub fn set_monitor(&self, enabled: bool) {
        let _ = enabled;
        todo!("warble-engine: Engine::set_monitor")
    }

    /// 0.0..=1.0
    pub fn set_monitor_volume(&self, volume: f32) {
        let _ = volume;
        todo!("warble-engine: Engine::set_monitor_volume")
    }

    /// Soundboard level into the call (and monitor), 0.0..=1.0.
    pub fn set_sound_volume(&self, volume: f32) {
        let _ = volume;
        todo!("warble-engine: Engine::set_sound_volume")
    }

    /// Decode a WAV, MP3, OGG/Vorbis or FLAC file into memory. Runs on the caller's thread.
    /// Errors are plain English ("Couldn't read clip.mp3: unsupported format").
    pub fn load_sound(&self, path: &Path) -> Result<SoundId, String> {
        let _ = path;
        todo!("warble-engine: Engine::load_sound")
    }

    /// Stops it if playing and frees it.
    pub fn remove_sound(&self, id: SoundId) {
        let _ = id;
        todo!("warble-engine: Engine::remove_sound")
    }

    /// Play from the start; pressing a sound that is already playing restarts it.
    /// Several different sounds can play at once.
    pub fn play_sound(&self, id: SoundId) {
        let _ = id;
        todo!("warble-engine: Engine::play_sound")
    }

    pub fn stop_sounds(&self) {
        todo!("warble-engine: Engine::stop_sounds")
    }

    pub fn status(&self) -> Status {
        todo!("warble-engine: Engine::status")
    }
}
