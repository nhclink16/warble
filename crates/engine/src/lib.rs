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

mod device;
mod graph;
mod sound;

use std::ops::Range;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, MutexGuard};

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
    // A misbehaving audio driver must not take the UI down with it.
    std::panic::catch_unwind(device::list_devices).unwrap_or_default()
}

/// The output device a virtual cable exposes, if one is installed: VB-CABLE's playback side
/// (see `is_cable_input`), the stereo one ahead of its 16-channel twin.
pub fn find_virtual_cable(devices: &Devices) -> Option<String> {
    pick_cable(devices.outputs.iter().map(|d| d.name.as_str())).map(str::to_owned)
}

/// The first cable playback device in `names`, preferring the stereo one.
fn pick_cable<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    names
        .filter(|n| is_cable_input(n))
        .min_by_key(|n| n.to_lowercase().replace(' ', "").contains("16ch"))
}

/// VB-CABLE's playback side, whatever Windows calls it: "CABLE Input (VB-Audio Virtual
/// Cable)", sometimes "Speakers (VB-Audio Virtual Cable)", and the 16-channel twin "CABLE In
/// 16ch (...)". Its recording side, "CABLE Output", doesn't count.
fn is_cable_input(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("cable input")
        || (n.contains("vb-audio virtual cable") && !n.contains("cable output"))
}

/// Lock, shrugging off poisoning: every value behind these locks stays valid on its own.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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

/// A decoded clip in memory, mono at its own sample rate: what the trim editor shows and what a
/// preview plays. Cheap to clone.
#[derive(Clone)]
pub struct Clip(Arc<graph::Sound>);

impl Clip {
    pub fn new(samples: Vec<f32>, rate: u32) -> Clip {
        Clip(Arc::new(graph::Sound {
            samples,
            rate: rate.max(1),
        }))
    }

    /// Decode a file as `Engine::load_sound` does, refusing clips longer than `max_seconds`
    /// (a whole number of minutes). Runs on the caller's thread.
    pub fn decode(path: &Path, max_seconds: u64) -> Result<Clip, String> {
        sound::decode_capped(path, max_seconds).map(|s| Clip(Arc::new(s)))
    }

    pub fn samples(&self) -> &[f32] {
        &self.0.samples
    }

    pub fn rate(&self) -> u32 {
        self.0.rate
    }

    pub fn seconds(&self) -> f64 {
        self.0.samples.len() as f64 / self.0.rate as f64
    }
}

/// The longest clip `Engine::load_sound` takes, in seconds.
pub const MAX_SOUND_SECONDS: u64 = sound::MAX_SECONDS;

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
    /// While a preview plays: where it is in its clip, in the clip's samples.
    pub preview: Option<usize>,
    /// Times an output callback ran short of mic audio since start (diagnostics).
    pub underruns: u64,
}

pub struct Engine {
    inner: Inner,
}

enum Inner {
    Mock(Mutex<mock::Mock>),
    /// Device thread, lock-free control, atomics: see `device`.
    Real(device::Real),
}

impl Engine {
    /// Start with these devices. Never fails: problems show in `status().error`.
    /// Returns once the first attempt to open them has finished (a few seconds at most).
    pub fn start(config: Config) -> Engine {
        Engine {
            inner: Inner::Real(device::Real::start(config)),
        }
    }

    /// No devices or threads. `status()` reports a plausible running state (VB-CABLE found,
    /// gently moving meters) so the UI can render headless snapshots and tests. Every other
    /// method works and is reflected in `status()` (e.g. `play_sound` marks the sound playing
    /// for a while). Deterministic: meters advance per `status()` call, not by wall clock.
    pub fn mock() -> Engine {
        Engine {
            inner: Inner::Mock(Mutex::new(mock::Mock::default())),
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
        match &self.inner {
            Inner::Mock(m) => lock(m).config = config,
            Inner::Real(r) => r.restart(config),
        }
    }

    pub fn set_preset(&self, preset: Preset) {
        match &self.inner {
            Inner::Mock(m) => lock(m).preset = preset,
            Inner::Real(r) => r.controls().set_preset(preset),
        }
    }

    /// Off sends the dry voice (soundboard still plays).
    pub fn set_effect_enabled(&self, enabled: bool) {
        match &self.inner {
            Inner::Mock(m) => lock(m).effect_enabled = enabled,
            Inner::Real(r) => r.controls().effect_enabled.store(enabled, Relaxed),
        }
    }

    /// Linear gain on the mic before effects, 0.0..=4.0.
    pub fn set_mic_gain(&self, gain: f32) {
        match &self.inner {
            Inner::Mock(m) => lock(m).mic_gain = gain,
            Inner::Real(r) => r.controls().mic_gain.store(gain),
        }
    }

    /// Hear yourself: your (changed) voice in the monitor device.
    pub fn set_monitor(&self, enabled: bool) {
        match &self.inner {
            Inner::Mock(m) => lock(m).monitor = enabled,
            Inner::Real(r) => r.controls().monitor.store(enabled, Relaxed),
        }
    }

    /// 0.0..=1.0
    pub fn set_monitor_volume(&self, volume: f32) {
        match &self.inner {
            Inner::Mock(m) => lock(m).monitor_volume = volume,
            Inner::Real(r) => r.controls().monitor_volume.store(volume),
        }
    }

    /// Hear sounds: soundboard clips in the monitor device as well as the call. On by default,
    /// and separate from `set_monitor`.
    pub fn set_hear_sounds(&self, enabled: bool) {
        match &self.inner {
            Inner::Mock(m) => lock(m).hear_sounds = enabled,
            Inner::Real(r) => r.controls().hear_sounds.store(enabled, Relaxed),
        }
    }

    /// Soundboard level into the call (and monitor), 0.0..=1.0.
    pub fn set_sound_volume(&self, volume: f32) {
        match &self.inner {
            Inner::Mock(m) => lock(m).sound_volume = volume,
            Inner::Real(r) => r.controls().sound_volume.store(volume),
        }
    }

    /// Decode a WAV, MP3, OGG/Vorbis, FLAC or M4A (AAC) file into memory. Runs on the caller's thread.
    /// Errors are plain English and say what to do ("There’s no sound in “clip.mp3”. Try another file.").
    /// Clips longer than 5 minutes are refused. The mock accepts any path without reading it.
    pub fn load_sound(&self, path: &Path) -> Result<SoundId, String> {
        match &self.inner {
            Inner::Mock(m) => {
                let mut m = lock(m);
                m.next_sound += 1;
                Ok(SoundId(m.next_sound))
            }
            Inner::Real(r) => r.load_sound(path),
        }
    }

    /// Stops it if playing and frees it.
    pub fn remove_sound(&self, id: SoundId) {
        match &self.inner {
            Inner::Mock(m) => lock(m).playing.retain(|(s, _)| *s != id),
            Inner::Real(r) => r.remove_sound(id),
        }
    }

    /// Play from the start; pressing a sound that is already playing restarts it.
    /// Several different sounds can play at once.
    pub fn play_sound(&self, id: SoundId) {
        match &self.inner {
            Inner::Mock(m) => {
                let mut m = lock(m);
                m.playing.retain(|(s, _)| *s != id);
                m.playing.push((id, mock::PLAY_TICKS));
            }
            Inner::Real(r) => r.play_sound(id),
        }
    }

    pub fn stop_sounds(&self) {
        match &self.inner {
            Inner::Mock(m) => lock(m).playing.clear(),
            Inner::Real(r) => r.stop_sounds(),
        }
    }

    /// Play samples `range` of `clip` in the headphones only, never into the call, whatever
    /// "hear sounds" says. It replaces any preview already playing. Silent with no headphones.
    pub fn preview(&self, clip: &Clip, range: Range<usize>) {
        let end = range.end.min(clip.samples().len());
        let start = range.start.min(end);
        match &self.inner {
            Inner::Mock(m) => lock(m).preview = Some((start, end, clip.rate(), 0)),
            Inner::Real(r) => r.preview(clip.0.clone(), start, end),
        }
    }

    pub fn stop_preview(&self) {
        match &self.inner {
            Inner::Mock(m) => lock(m).preview = None,
            Inner::Real(r) => r.stop_preview(),
        }
    }

    pub fn status(&self) -> Status {
        match &self.inner {
            Inner::Mock(m) => lock(m).status(),
            Inner::Real(r) => r.status(),
        }
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
        pub hear_sounds: bool,
        pub next_sound: u32,
        pub playing: Vec<(SoundId, u32)>,
        /// (start, end, rate, status() calls since it started).
        pub preview: Option<(usize, usize, u32, u32)>,
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
                hear_sounds: true,
                next_sound: 0,
                playing: Vec::new(),
                preview: None,
                tick: 0,
            }
        }
    }

    pub fn devices() -> Devices {
        let d = |name: &str, is_default| DeviceInfo {
            name: name.to_string(),
            is_default,
        };
        Devices {
            inputs: vec![
                d("Microphone (USB Audio Device)", true),
                d("CABLE Output (VB-Audio Virtual Cable)", false),
            ],
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
            // A preview moves at real speed, counting 30 status() calls a second.
            let mut preview = None;
            if let Some((start, end, rate, ticks)) = &mut self.preview {
                let at = *start + (*ticks as u64 * *rate as u64 / 30) as usize;
                *ticks += 1;
                if at < *end {
                    preview = Some(at);
                } else {
                    self.preview = None;
                }
            }
            let t = self.tick as f32 * 0.21;
            // A talking-voice shape: two slow sines, clipped to silence between "words".
            let voice =
                ((t.sin() * 0.5 + (t * 2.7).sin() * 0.3) * self.mic_gain.min(2.0)).clamp(0.0, 1.0);
            let sounds = if self.playing.is_empty() {
                0.0
            } else {
                0.35 * self.sound_volume
            };
            let pick = |want: &Option<String>, fallback: &str| {
                Some(want.clone().unwrap_or_else(|| fallback.to_string()))
            };
            Status {
                running: true,
                input_name: pick(&self.config.input, "Microphone (USB Audio Device)"),
                cable_name: pick(&self.config.cable, "CABLE Input (VB-Audio Virtual Cable)"),
                monitor_name: pick(&self.config.monitor, "Headphones (USB Audio Device)"),
                error: None,
                input_level: voice,
                output_level: (voice * 0.9 + sounds).min(1.0),
                playing: self.playing.iter().map(|(id, _)| *id).collect(),
                preview,
                underruns: 0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str) -> DeviceInfo {
        DeviceInfo {
            name: name.to_string(),
            is_default: false,
        }
    }

    #[test]
    fn engine_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Engine>();
    }

    #[test]
    fn finds_vb_cable_among_outputs() {
        let mut devices = Devices {
            inputs: vec![
                info("Microphone (USB Audio Device)"),
                info("CABLE Output (VB-Audio Virtual Cable)"),
            ],
            outputs: vec![info("Speakers (High Definition Audio Device)")],
        };
        assert_eq!(
            find_virtual_cable(&devices),
            None,
            "the cable's recording side doesn't count"
        );
        devices
            .outputs
            .push(info("CABLE Input (VB-Audio Virtual Cable)"));
        assert_eq!(
            find_virtual_cable(&devices).as_deref(),
            Some("CABLE Input (VB-Audio Virtual Cable)")
        );
        devices.outputs = vec![
            info("Headphones"),
            info("cable input (vb-audio virtual cable)"),
        ];
        assert_eq!(
            find_virtual_cable(&devices).as_deref(),
            Some("cable input (vb-audio virtual cable)")
        );
        assert_eq!(
            find_virtual_cable(&mock::devices()).as_deref(),
            Some("CABLE Input (VB-Audio Virtual Cable)")
        );
        assert_eq!(find_virtual_cable(&Devices::default()), None);
        // Its playback side can also show as "Speakers", next to a 16-channel twin.
        devices.outputs = vec![
            info("CABLE In 16ch (VB-Audio Virtual Cable)"),
            info("Speakers (HyperX Cloud III S Wireless)"),
            info("Speakers (VB-Audio Virtual Cable)"),
        ];
        assert_eq!(
            find_virtual_cable(&devices).as_deref(),
            Some("Speakers (VB-Audio Virtual Cable)")
        );
        devices.outputs.pop();
        assert_eq!(
            find_virtual_cable(&devices).as_deref(),
            Some("CABLE In 16ch (VB-Audio Virtual Cable)"),
            "the 16-channel side still works when it's the only one"
        );
    }

    /// Names no machine has, so this behaves like a PC with no audio devices at all.
    fn nowhere() -> Config {
        Config {
            input: Some("No Such Microphone".into()),
            cable: Some("No Such Cable".into()),
            monitor: Some("No Such Headphones".into()),
        }
    }

    #[test]
    fn start_without_devices_reports_an_error() {
        let engine = Engine::start(nowhere());
        let status = engine.status();
        assert!(!status.running);
        assert_eq!(
            status.error.as_deref(),
            Some("Can't find \"No Such Cable\". Is it plugged in?")
        );
        assert_eq!(
            (status.input_name, status.cable_name, status.monitor_name),
            (None, None, None)
        );

        // Everything else still works without devices.
        engine.set_preset(Preset::Robot);
        engine.set_effect_enabled(false);
        engine.set_mic_gain(9.0);
        engine.set_monitor(true);
        engine.set_monitor_volume(0.5);
        engine.set_sound_volume(0.5);
        let path = sound::tests::temp_file("engine.wav", &sound::tests::small_wav());
        let id = engine.load_sound(&path).expect("loads");
        assert!(
            engine
                .load_sound(Path::new("missing.ogg"))
                .unwrap_err()
                .contains("missing.ogg")
        );
        engine.play_sound(id);
        engine.play_sound(SoundId(999));
        engine.stop_sounds();
        engine.remove_sound(id);
        engine.restart(Config {
            input: Some("Still Missing".into()),
            ..nowhere()
        });
        assert!(engine.status().playing.is_empty());
        drop(engine); // joins the device thread
    }

    #[test]
    fn previews_start_play_and_stop() {
        let engine = Engine::mock();
        let clip = Clip::new(vec![0.1; 3000], 3000);
        engine.preview(&clip, 1500..9999);
        assert_eq!(engine.status().preview, Some(1500));
        assert_eq!(engine.status().preview, Some(1600));
        engine.stop_preview();
        assert_eq!(engine.status().preview, None);
        assert!(
            engine.status().playing.is_empty(),
            "a preview isn't a soundboard sound"
        );

        // The real engine takes them with no devices too.
        let engine = Engine::start(nowhere());
        engine.preview(&clip, 0..3000);
        engine.stop_preview();
        assert_eq!(engine.status().preview, None);
    }

    #[test]
    fn mock_still_plays_pretend() {
        let engine = Engine::mock();
        let id = engine.load_sound(Path::new("anything.mp3")).unwrap();
        engine.play_sound(id);
        let status = engine.status();
        assert!(status.running && status.playing == vec![id]);
        assert_eq!(engine.devices(), mock::devices());
    }
}
