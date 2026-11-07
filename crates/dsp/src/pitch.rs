//! Delay-line pitch shifter: two read taps sweep through recent input at the pitch ratio,
//! faded by sin²/cos² windows so one is at full level whenever the other jumps back.
//!
//! A plain two-tap shifter splices at an arbitrary phase, so the taps partly cancel and the
//! pitch wobbles ("phasiness"). Here a tap that jumps back lands, within `SEARCH_S`, where its
//! waveform best matches the tap that is playing, so splices line up on voiced speech.
//!
//! Latency: the heard tap sits about `window / 2 + SEARCH_S / 2` behind the input on average.

use std::f32::consts::PI;

/// Smallest delay a tap reads; the Hermite interpolator needs one newer sample.
const MIN_DELAY: f32 = 2.0;
/// How far a jumping tap may move to line up: one period of an 100 Hz voice.
const SEARCH_S: f32 = 0.010;
/// How much audio the line-up compares.
const MATCH_S: f32 = 0.010;
/// The line-up compares every n-th sample and offset, n chosen so it runs at about this rate.
const MATCH_RATE: f32 = 16_000.0;

/// Recent input, shared by every pitch shifter. `Voice` keeps it filled even while no shifter
/// runs, so a shifter that starts mid-word reads real audio straight away instead of audio
/// arriving abruptly out of a zeroed buffer (a click).
pub struct History {
    buf: Vec<f32>,
    mask: usize,
    newest: usize,
}

impl History {
    /// Holds enough for any shifter whose window is up to `max_window_s`.
    pub fn new(sample_rate: f32, max_window_s: f32) -> History {
        let need =
            ((max_window_s + SEARCH_S + MATCH_S) * sample_rate) as usize + MIN_DELAY as usize + 8;
        let len = need.next_power_of_two();
        History {
            buf: vec![0.0; len],
            mask: len - 1,
            newest: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.newest = (self.newest + 1) & self.mask;
        self.buf[self.newest] = x;
    }

    #[inline]
    fn at(&self, delay: usize) -> f32 {
        self.buf[self.newest.wrapping_sub(delay) & self.mask]
    }

    /// The input `delay` samples ago (fractional), by 4-point Hermite interpolation.
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let delay = delay.max(1.0);
        let i = delay as usize;
        let t = delay - i as f32;
        let (ym1, y0, y1, y2) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        let c1 = 0.5 * (y1 - ym1);
        let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
        ((c3 * t + c2) * t + c1) * t + y0
    }

    /// How much the audio behind delay `a` looks like the audio behind delay `b`.
    fn likeness(&self, a: usize, b: usize, len: usize, stride: usize) -> f32 {
        let (mut ab, mut aa) = (0.0, 1e-12);
        for i in (0..len).step_by(stride) {
            let (x, y) = (self.at(a + i), self.at(b + i));
            ab += x * y;
            aa += x * x;
        }
        ab / aa.sqrt()
    }
}

pub struct PitchShift {
    /// Tap 0's place in its sweep, 0..1. Tap 1 is half a sweep away.
    phase: f32,
    step: f32,
    /// Delay where a sweep starts, and how far it travels (+ for pitch down, - for up).
    start: f32,
    sweep: f32,
    /// Each tap's line-up offset from `start`, chosen when it last jumped back.
    offsets: [f32; 2],
    search: usize,
    span: usize,
    stride: usize,
}

impl PitchShift {
    pub fn new(sample_rate: f32, semitones: f32, window_s: f32) -> PitchShift {
        let ratio = 2f32.powf(semitones / 12.0);
        let window = window_s * sample_rate;
        // A tap reads at `ratio` times real time, so its delay changes by `1 - ratio` per
        // sample and it crosses the window in `window / |1 - ratio|` samples.
        let up = ratio > 1.0;
        PitchShift {
            phase: 1.0,
            step: (1.0 - ratio).abs() / window,
            start: if up { MIN_DELAY + window } else { MIN_DELAY },
            sweep: if up { -window } else { window },
            offsets: [0.0; 2],
            search: (SEARCH_S * sample_rate) as usize,
            span: (MATCH_S * sample_rate) as usize,
            stride: ((sample_rate / MATCH_RATE).round() as usize).max(1),
        }
    }

    /// Back to the start: tap 1 at full level mid-sweep, tap 0 silent and lined up on the
    /// next tick.
    pub fn reset(&mut self) {
        self.phase = 1.0;
        self.offsets = [0.0; 2];
    }

    fn delay(&self, tap: usize) -> f32 {
        let p = self.phase;
        let u = if tap == 0 {
            p
        } else if p < 0.5 {
            p + 0.5
        } else {
            p - 0.5
        };
        self.start + self.offsets[tap] + self.sweep * u
    }

    /// Tap `tap` is silent and back at the start of its sweep: move it up to `search`
    /// samples further back, to where it best matches the other tap.
    fn line_up(&mut self, tap: usize, history: &History) {
        let other = self.delay(1 - tap).round() as usize;
        let start = self.start.round() as usize;
        let mut best = (f32::MIN, 0);
        for offset in (0..=self.search).step_by(self.stride) {
            let score = history.likeness(start + offset, other, self.span, self.stride);
            if score > best.0 {
                best = (score, offset);
            }
        }
        self.offsets[tap] = best.1 as f32;
    }

    #[inline]
    pub fn tick(&mut self, history: &History) -> f32 {
        let s = (PI * self.phase).sin();
        let g0 = s * s;
        let y = g0 * history.read(self.delay(0)) + (1.0 - g0) * history.read(self.delay(1));
        let before = self.phase;
        self.phase += self.step;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.line_up(0, history);
        } else if before < 0.5 && self.phase >= 0.5 {
            self.line_up(1, history);
        }
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_stay_inside_the_history() {
        let sr = 48_000.0;
        let window_s = 0.03;
        let h_len = History::new(sr, window_s).buf.len() as f32;
        for semitones in [-12.0, -5.0, 7.5, 12.0] {
            let mut shift = PitchShift::new(sr, semitones, window_s);
            let mut h = History::new(sr, window_s);
            let max = MIN_DELAY + (window_s + SEARCH_S) * sr;
            let mut s = 1u32;
            for _ in 0..200_000 {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                h.push(s as f32 / u32::MAX as f32 - 0.5);
                shift.tick(&h);
                for tap in 0..2 {
                    let d = shift.delay(tap);
                    assert!(
                        d >= MIN_DELAY - 1e-3 && d <= max + 1.0,
                        "{semitones}: delay {d}"
                    );
                    assert!(d + (MATCH_S * sr) + 3.0 < h_len);
                }
            }
        }
    }
}
