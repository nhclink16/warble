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

mod biquad;
mod chains;
mod delay;
mod fx;
mod pitch;
mod reverb;

use chains::{Chains, MAX_WINDOW_S};
use pitch::History;

/// How long a preset change or on/off switch takes to crossfade.
const FADE_S: f32 = 0.020;

/// The whole voice path: one preallocated effect chain per preset, with click-free
/// crossfades when the preset changes or the effect is switched on or off.
///
/// Latency added: none for Normal, Robot, Radio, Echo and Cave. The pitch shifters run about
/// 20 ms behind on average in Deep and Demon (sweeping 0-40 ms) and 17 ms in Chipmunk (0-34 ms).
pub struct Voice {
    preset: Preset,
    enabled: bool,
    /// Recent dry input for the pitch shifters, written every sample whatever is running.
    history: History,
    chains: Chains,
    /// Crossfade gain of each chain, indexed by `Preset as usize` (`Normal` is the dry voice).
    gains: [f32; 8],
    /// The chain fading in, or playing alone once `fading` is false.
    target: Preset,
    fading: bool,
    fade_step: f32,
}

impl Voice {
    /// Allocates every chain up front for `sample_rate` (Hz, e.g. 48000.0).
    pub fn new(sample_rate: f32) -> Voice {
        let sr = if sample_rate.is_finite() {
            sample_rate.clamp(8_000.0, 384_000.0)
        } else {
            48_000.0
        };
        let mut gains = [0.0; 8];
        gains[Preset::Normal as usize] = 1.0;
        Voice {
            preset: Preset::Normal,
            enabled: true,
            history: History::new(sr, MAX_WINDOW_S),
            chains: Chains::new(sr),
            gains,
            target: Preset::Normal,
            fading: false,
            fade_step: 1.0 / (FADE_S * sr),
        }
    }

    /// Switch preset. Real-time safe. Starts a short (~20 ms) crossfade from the old chain to
    /// the new one, whose state is reset first so it never replays stale audio.
    pub fn set_preset(&mut self, preset: Preset) {
        self.preset = preset;
        self.retarget();
    }

    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// `false` passes the dry voice. Real-time safe, crossfaded like a preset change.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.retarget();
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn retarget(&mut self) {
        let target = if self.enabled {
            self.preset
        } else {
            Preset::Normal
        };
        if target == self.target {
            return;
        }
        // A chain still fading out is audible, so it carries on (no reset, no click);
        // a silent one starts clean.
        if self.gains[target as usize] == 0.0 {
            self.chains.reset(target);
        }
        self.target = target;
        self.fading = Preset::ALL
            .iter()
            .any(|&p| self.gains[p as usize] != if p == target { 1.0 } else { 0.0 });
    }

    /// Process `buf` in place. Any length, including 0. Output stays finite and within
    /// [-1.0, 1.0] for input within [-1.0, 1.0].
    pub fn process(&mut self, buf: &mut [f32]) {
        let mut i = 0;
        while self.fading && i < buf.len() {
            buf[i] = self.fade_tick(buf[i]);
            i += 1;
        }
        let rest = &mut buf[i..];
        if self.target == Preset::Normal {
            // Bit-identical dry voice; only the shifters' history needs the input.
            for &x in rest.iter() {
                self.history.push(sanitize(x));
            }
        } else {
            for x in rest {
                let v = sanitize(*x);
                self.history.push(v);
                *x = self.chains.tick(self.target, &self.history, v);
            }
        }
    }

    /// One sample while chains are crossfading: every audible chain runs and each gain
    /// ramps linearly toward 1 (the target) or 0 (the rest). Gains never sum above 1.
    fn fade_tick(&mut self, x: f32) -> f32 {
        let x = sanitize(x);
        self.history.push(x);
        let mut out = 0.0;
        let mut settled = true;
        for p in Preset::ALL {
            let r = p as usize;
            let g = self.gains[r];
            let g = if p == self.target {
                (g + self.fade_step).min(1.0)
            } else if g > 0.0 {
                (g - self.fade_step).max(0.0)
            } else {
                continue;
            };
            self.gains[r] = g;
            settled &= g == if p == self.target { 1.0 } else { 0.0 };
            out += g * self.chains.tick(p, &self.history, x);
        }
        self.fading = !settled;
        out.clamp(-1.0, 1.0)
    }
}

/// Keeps NaN and infinity out of the filters' state, where they would stick.
#[inline]
fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biquad::Biquad;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.0;

    /// A speech-like test signal: a 120 Hz vowel with three formants, in 3 Hz syllables.
    fn vowel(seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let formant = |f: f32| {
            1.0 + 4.0 * (-((f - 700.0) / 150.0).powi(2)).exp()
                + 3.0 * (-((f - 1200.0) / 200.0).powi(2)).exp()
                + 2.0 * (-((f - 2600.0) / 300.0).powi(2)).exp()
        };
        let mut out: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / SR;
                let voice: f32 = (1..60)
                    .map(|k| k as f32 * 120.0)
                    .take_while(|&f| f < 8_000.0)
                    .map(|f| formant(f) / (f / 120.0) * (TAU * f * t).sin())
                    .sum();
                voice * (std::f32::consts::PI * 3.0 * t).sin().powi(2)
            })
            .collect();
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        out.iter_mut().for_each(|x| *x *= 0.5 / peak);
        out
    }

    /// Rough perceived loudness: RMS after a K-weighting-like presence tilt, in dB.
    fn loudness(signal: &[f32]) -> f32 {
        let mut hp = Biquad::highpass(SR, 100.0, 0.707);
        let mut tilt = Biquad::peaking(SR, 2_500.0, 0.7, 4.0);
        let sum: f64 = signal
            .iter()
            .map(|&x| tilt.tick(hp.tick(x)) as f64)
            .map(|y| y * y)
            .sum();
        10.0 * (sum / signal.len() as f64).log10() as f32
    }

    fn run(preset: Preset, input: &[f32]) -> Vec<f32> {
        let mut v = Voice::new(SR);
        v.set_preset(preset);
        let mut out = input.to_vec();
        v.process(&mut out);
        out
    }

    /// Switching presets should not jump the level much: each is within 1.5 dB of the dry
    /// voice at a typical speaking level (drive makes Radio and Demon level-dependent).
    #[test]
    fn presets_are_about_as_loud_as_the_dry_voice() {
        let input = vowel(4.0);
        let dry = loudness(&input[SR as usize..]);
        for p in Preset::ALL {
            let out = run(p, &input);
            let diff = loudness(&out[SR as usize..]) - dry;
            println!("{p:?}: {diff:+.2} dB");
            assert!(diff.abs() < 1.5, "{p:?} is {diff:+.2} dB against dry");
        }
    }

    /// The pitch heard: median over frames of the autocorrelation peak near `expect_hz`.
    fn measured_hz(signal: &[f32], expect_hz: f32) -> f32 {
        let period = SR / expect_hz;
        let (lo, hi) = ((period * 0.8) as usize, (period * 1.25) as usize + 1);
        let frame = 2048;
        let mut found: Vec<f32> = (0..signal.len() - frame - hi - 1)
            .step_by(frame)
            .map(|start| {
                let x = &signal[start..];
                let r = |lag: usize| {
                    let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
                    for i in 0..frame {
                        let (a, b) = (x[i] as f64, x[i + lag] as f64);
                        xy += a * b;
                        xx += a * a;
                        yy += b * b;
                    }
                    xy / (xx * yy).sqrt().max(1e-12)
                };
                let best = (lo..=hi).max_by(|&a, &b| r(a).total_cmp(&r(b))).unwrap();
                let (l, c, rr) = (r(best - 1), r(best), r(best + 1));
                let offset = 0.5 * (l - rr) / (l - 2.0 * c + rr);
                SR / (best as f32 + offset as f32)
            })
            .collect();
        found.sort_by(f32::total_cmp);
        found[found.len() / 2]
    }

    #[test]
    fn pitch_presets_hit_their_ratio() {
        use chains::{CHIPMUNK_SEMITONES, DEEP_SEMITONES, DEMON_SEMITONES};
        for (preset, semitones) in [
            (Preset::Deep, DEEP_SEMITONES),
            (Preset::Chipmunk, CHIPMUNK_SEMITONES),
            (Preset::Demon, DEMON_SEMITONES),
        ] {
            let ratio = 2f32.powf(semitones / 12.0);
            for hz in [150.0, 220.0, 330.0] {
                let input: Vec<f32> = (0..3 * SR as usize)
                    .map(|i| 0.4 * (TAU * hz * i as f32 / SR).sin())
                    .collect();
                let out = run(preset, &input);
                let got = measured_hz(&out[SR as usize / 2..], hz * ratio) / hz;
                let error = (got / ratio - 1.0).abs();
                assert!(
                    error < 0.03,
                    "{preset:?} at {hz} Hz: ratio {got:.4}, want {ratio:.4}"
                );
            }
        }
    }
}
