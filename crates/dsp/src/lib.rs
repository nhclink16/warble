//! Real-time voice effects for Warble.
//!
//! Everything works on mono `f32` samples in place, at any sample rate given at construction.
//! CONTRACT: after `Voice::new` returns, nothing in this crate allocates, locks, blocks or
//! panics. `process` must be safe to call from an audio callback.

/// A voice preset. Order here is the order the app shows them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Preset {
    /// The dry voice, unchanged.
    #[default]
    Normal,
    /// Pitched down: a big, deep voice.
    Deep,
    /// Pitched well up.
    Chipmunk,
    /// Metallic and flat: ring modulation.
    Robot,
    /// Band-limited and a little crunchy, like a walkie-talkie.
    Radio,
    /// A clear slap-back echo.
    Echo,
    /// Very low and gritty with a dark room around it.
    Demon,
    /// A big reverberant space.
    Cave,
}

impl Preset {
    pub const ALL: [Preset; 8] = [
        Preset::Normal,
        Preset::Deep,
        Preset::Chipmunk,
        Preset::Robot,
        Preset::Radio,
        Preset::Echo,
        Preset::Demon,
        Preset::Cave,
    ];

    /// Display name, e.g. "Chipmunk".
    pub fn name(self) -> &'static str {
        match self {
            Preset::Normal => "Normal",
            Preset::Deep => "Deep",
            Preset::Chipmunk => "Chipmunk",
            Preset::Robot => "Robot",
            Preset::Radio => "Radio",
            Preset::Echo => "Echo",
            Preset::Demon => "Demon",
            Preset::Cave => "Cave",
        }
    }

    /// Stable lowercase id for settings files, e.g. "chipmunk".
    pub fn id(self) -> &'static str {
        match self {
            Preset::Normal => "normal",
            Preset::Deep => "deep",
            Preset::Chipmunk => "chipmunk",
            Preset::Robot => "robot",
            Preset::Radio => "radio",
            Preset::Echo => "echo",
            Preset::Demon => "demon",
            Preset::Cave => "cave",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.id() == id)
    }
}

/// The whole voice path: one preallocated effect chain per preset, with click-free
/// crossfades when the preset changes or the effect is switched on or off.
///
/// PLACEHOLDER: passes audio through unchanged. Replace with the real chains.
pub struct Voice {
    preset: Preset,
    enabled: bool,
}

impl Voice {
    /// Allocates every chain up front for `sample_rate` (Hz, e.g. 48000.0).
    pub fn new(sample_rate: f32) -> Voice {
        let _ = sample_rate;
        Voice { preset: Preset::Normal, enabled: true }
    }

    /// Switch preset. Real-time safe. Starts a short (~20 ms) crossfade from the old chain to
    /// the new one, whose state is reset first so it never replays stale audio.
    pub fn set_preset(&mut self, preset: Preset) {
        self.preset = preset;
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// `false` passes the dry voice. Real-time safe, crossfaded like a preset change.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Process `buf` in place. Any length, including 0. Output stays finite and within
    /// [-1.0, 1.0] for input within [-1.0, 1.0].
    pub fn process(&mut self, buf: &mut [f32]) {
        let _ = buf;
    }
}
