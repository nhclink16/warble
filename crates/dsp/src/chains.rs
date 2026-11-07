//! One effect chain per preset. The constants at the top of each chain are the tuning knobs.
//! Every chain ends in `limit`, so its output stays within [-1, 1] whatever comes in.

use crate::Preset;
use crate::biquad::Biquad;
use crate::delay::Delay;
use crate::fx::{Envelope, Noise, RingMod, flush, limit, soft_clip};
use crate::pitch::{History, PitchShift};
use crate::reverb::Reverb;

/// Pitch-shifter windows. Longer is smoother but adds latency (about half the window, plus
/// 5 ms for lining up splices, on average).
const DOWN_WINDOW_S: f32 = 0.030;
const UP_WINDOW_S: f32 = 0.024;
pub const MAX_WINDOW_S: f32 = DOWN_WINDOW_S;

pub const DEEP_SEMITONES: f32 = -5.0;
pub const CHIPMUNK_SEMITONES: f32 = 7.5;
pub const DEMON_SEMITONES: f32 = -10.0;

/// Every chain, allocated once. `Preset::Normal` is the dry voice and has no chain.
pub struct Chains {
    deep: Deep,
    chipmunk: Chipmunk,
    robot: Robot,
    radio: Radio,
    echo: Echo,
    demon: Demon,
    cave: Cave,
}

impl Chains {
    pub fn new(sr: f32) -> Chains {
        Chains {
            deep: Deep::new(sr),
            chipmunk: Chipmunk::new(sr),
            robot: Robot::new(sr),
            radio: Radio::new(sr),
            echo: Echo::new(sr),
            demon: Demon::new(sr),
            cave: Cave::new(sr),
        }
    }

    /// Clear all state so the chain starts from silence. No allocation.
    pub fn reset(&mut self, preset: Preset) {
        match preset {
            Preset::Normal => {}
            Preset::Deep => self.deep.reset(),
            Preset::Chipmunk => self.chipmunk.reset(),
            Preset::Robot => self.robot.reset(),
            Preset::Radio => self.radio.reset(),
            Preset::Echo => self.echo.reset(),
            Preset::Demon => self.demon.reset(),
            Preset::Cave => self.cave.reset(),
        }
    }

    /// One sample. `history` must already hold `x` as its newest sample.
    #[inline]
    pub fn tick(&mut self, preset: Preset, history: &History, x: f32) -> f32 {
        match preset {
            Preset::Normal => x,
            Preset::Deep => self.deep.tick(history),
            Preset::Chipmunk => self.chipmunk.tick(history),
            Preset::Robot => self.robot.tick(x),
            Preset::Radio => self.radio.tick(x),
            Preset::Echo => self.echo.tick(x),
            Preset::Demon => self.demon.tick(history),
            Preset::Cave => self.cave.tick(x),
        }
    }
}

/// Down a fourth, with a little low-end warmth.
struct Deep {
    shift: PitchShift,
    warmth: Biquad,
}

impl Deep {
    const WARMTH_HZ: f32 = 220.0;
    const WARMTH_DB: f32 = 3.0;
    const GAIN: f32 = 1.0;

    fn new(sr: f32) -> Deep {
        Deep {
            shift: PitchShift::new(sr, DEEP_SEMITONES, DOWN_WINDOW_S),
            warmth: Biquad::low_shelf(sr, Self::WARMTH_HZ, Self::WARMTH_DB),
        }
    }

    fn reset(&mut self) {
        self.shift.reset();
        self.warmth.reset();
    }

    fn tick(&mut self, history: &History) -> f32 {
        limit(self.warmth.tick(self.shift.tick(history)) * Self::GAIN)
    }
}

/// Up a fifth and a bit, with the shifted-up sibilance softened.
struct Chipmunk {
    shift: PitchShift,
    soften: Biquad,
}

impl Chipmunk {
    const SOFTEN_HZ: f32 = 9_000.0;
    const GAIN: f32 = 0.9;

    fn new(sr: f32) -> Chipmunk {
        Chipmunk {
            shift: PitchShift::new(sr, CHIPMUNK_SEMITONES, UP_WINDOW_S),
            soften: Biquad::lowpass(sr, Self::SOFTEN_HZ, 0.6),
        }
    }

    fn reset(&mut self) {
        self.shift.reset();
        self.soften.reset();
    }

    fn tick(&mut self, history: &History) -> f32 {
        limit(self.soften.tick(self.shift.tick(history)) * Self::GAIN)
    }
}

/// Ring modulation (the classic Dalek trick) into a short resonant comb that adds a fixed,
/// metallic buzz.
struct Robot {
    rumble: Biquad,
    ring: RingMod,
    comb: Delay,
}

impl Robot {
    const CARRIER_HZ: f32 = 45.0;
    /// Comb resonance: harmonics of this pitch ring out.
    const COMB_HZ: f32 = 110.0;
    const COMB_FEEDBACK: f32 = 0.45;
    const GAIN: f32 = 1.2;

    fn new(sr: f32) -> Robot {
        Robot {
            rumble: Biquad::highpass(sr, 100.0, 0.707),
            ring: RingMod::new(sr, Self::CARRIER_HZ),
            comb: Delay::seconds(sr, 1.0 / Self::COMB_HZ),
        }
    }

    fn reset(&mut self) {
        self.rumble.reset();
        self.ring.reset();
        self.comb.reset();
    }

    fn tick(&mut self, x: f32) -> f32 {
        let r = self.ring.tick(self.rumble.tick(x));
        let y = r + Self::COMB_FEEDBACK * self.comb.read();
        self.comb.write(flush(y));
        limit(y * Self::GAIN)
    }
}

/// Walkie-talkie: steep 400-3000 Hz band, a nasal bump, drive, and a faint hiss that only
/// rides along while you speak.
struct Radio {
    low_cut: [Biquad; 2],
    honk: Biquad,
    high_cut: [Biquad; 2],
    dc_cut: Biquad,
    envelope: Envelope,
    noise: Noise,
}

impl Radio {
    const LOW_HZ: f32 = 400.0;
    const HIGH_HZ: f32 = 3_000.0;
    const HONK_HZ: f32 = 1_700.0;
    const HONK_DB: f32 = 5.0;
    const DRIVE: f32 = 3.0;
    /// Hiss level relative to the voice's envelope.
    const HISS: f32 = 0.08;
    const GAIN: f32 = 0.5;

    fn new(sr: f32) -> Radio {
        Radio {
            low_cut: [Biquad::highpass(sr, Self::LOW_HZ, 0.707); 2],
            honk: Biquad::peaking(sr, Self::HONK_HZ, 1.0, Self::HONK_DB),
            high_cut: [Biquad::lowpass(sr, Self::HIGH_HZ, 0.707); 2],
            dc_cut: Biquad::highpass(sr, Self::LOW_HZ * 0.75, 0.707),
            envelope: Envelope::new(sr, 0.005, 0.15),
            noise: Noise::new(),
        }
    }

    fn reset(&mut self) {
        self.low_cut.iter_mut().for_each(Biquad::reset);
        self.honk.reset();
        self.high_cut.iter_mut().for_each(Biquad::reset);
        self.dc_cut.reset();
        self.envelope.reset();
        self.noise.reset();
    }

    fn tick(&mut self, x: f32) -> f32 {
        let env = self.envelope.tick(x);
        let v = self.low_cut.iter_mut().fold(x, |v, f| f.tick(v));
        let v = soft_clip(self.honk.tick(v) * Self::DRIVE) + self.noise.next() * env * Self::HISS;
        let v = self.high_cut.iter_mut().fold(v, |v, f| f.tick(v));
        limit(self.dc_cut.tick(v) * Self::GAIN)
    }
}

/// A feedback echo whose repeats get darker and thinner, like a far wall.
struct Echo {
    line: Delay,
    thin: Biquad,
    darken: Biquad,
}

impl Echo {
    const DELAY_S: f32 = 0.28;
    const FEEDBACK: f32 = 0.35;
    const DRY: f32 = 0.9;
    const WET: f32 = 0.45;

    fn new(sr: f32) -> Echo {
        Echo {
            line: Delay::seconds(sr, Self::DELAY_S),
            thin: Biquad::highpass(sr, 180.0, 0.707),
            darken: Biquad::lowpass(sr, 3_800.0, 0.707),
        }
    }

    fn reset(&mut self) {
        self.line.reset();
        self.thin.reset();
        self.darken.reset();
    }

    fn tick(&mut self, x: f32) -> f32 {
        let d = self.line.read();
        let fed = self.darken.tick(self.thin.tick(x + Self::FEEDBACK * d));
        self.line.write(flush(fed));
        limit(Self::DRY * x + Self::WET * d)
    }
}

/// Far down, boosted low end, driven hard, dark, in a smaller gloomy room.
struct Demon {
    shift: PitchShift,
    weight: Biquad,
    darken: Biquad,
    room: Reverb,
}

impl Demon {
    const WEIGHT_HZ: f32 = 180.0;
    const WEIGHT_DB: f32 = 5.0;
    const DRIVE: f32 = 3.0;
    const DARKEN_HZ: f32 = 3_200.0;
    const DRY: f32 = 0.45;
    const WET: f32 = 0.53;

    fn new(sr: f32) -> Demon {
        Demon {
            shift: PitchShift::new(sr, DEMON_SEMITONES, DOWN_WINDOW_S),
            weight: Biquad::low_shelf(sr, Self::WEIGHT_HZ, Self::WEIGHT_DB),
            darken: Biquad::lowpass(sr, Self::DARKEN_HZ, 0.707),
            room: Reverb::new(sr, 0.8, 0.7, 0.8),
        }
    }

    fn reset(&mut self) {
        self.shift.reset();
        self.weight.reset();
        self.darken.reset();
        self.room.reset();
    }

    fn tick(&mut self, history: &History) -> f32 {
        let v = self.weight.tick(self.shift.tick(history));
        let v = self.darken.tick(soft_clip(v * Self::DRIVE));
        let w = self.room.tick(v);
        limit(Self::DRY * v + Self::WET * w)
    }
}

/// A big stone space: a short pre-delay, then a long Freeverb tail.
struct Cave {
    pre_delay: Delay,
    mud_cut: Biquad,
    space: Reverb,
}

impl Cave {
    const PRE_DELAY_S: f32 = 0.025;
    const DRY: f32 = 0.85;
    const WET: f32 = 1.0;

    fn new(sr: f32) -> Cave {
        Cave {
            pre_delay: Delay::seconds(sr, Self::PRE_DELAY_S),
            mud_cut: Biquad::highpass(sr, 160.0, 0.707),
            space: Reverb::new(sr, 1.0, 0.86, 0.35),
        }
    }

    fn reset(&mut self) {
        self.pre_delay.reset();
        self.mud_cut.reset();
        self.space.reset();
    }

    fn tick(&mut self, x: f32) -> f32 {
        let p = self.pre_delay.read();
        self.pre_delay.write(self.mud_cut.tick(x));
        let w = self.space.tick(p);
        limit(Self::DRY * x + Self::WET * w)
    }
}
