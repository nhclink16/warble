//! Freeverb (Jezar's public-domain design), mono: 8 damped feedback combs in parallel, then
//! 4 allpasses in series. Delay lengths are Freeverb's 44.1 kHz tunings scaled to the rate.

use crate::delay::Delay;
use crate::fx::flush;

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const INPUT_GAIN: f32 = 0.015;
const ALLPASS_FEEDBACK: f32 = 0.5;

pub struct Reverb {
    combs: [Delay; 8],
    lows: [f32; 8],
    allpasses: [Delay; 4],
    feedback: f32,
    damp: f32,
}

impl Reverb {
    /// `size` scales the delay lengths (1.0 is Freeverb's room), `room` (0..1) lengthens the
    /// decay and `damp` (0..1) darkens the tail. Returns the wet signal only.
    pub fn new(sample_rate: f32, size: f32, room: f32, damp: f32) -> Reverb {
        let scale = sample_rate / 44_100.0 * size;
        let len = |n: usize| (n as f32 * scale).round() as usize;
        Reverb {
            combs: std::array::from_fn(|i| Delay::new(len(COMB_TUNING[i]))),
            lows: [0.0; 8],
            allpasses: std::array::from_fn(|i| Delay::new(len(ALLPASS_TUNING[i]))),
            feedback: room * 0.28 + 0.7,
            damp: damp * 0.4,
        }
    }

    pub fn reset(&mut self) {
        self.combs.iter_mut().for_each(Delay::reset);
        self.allpasses.iter_mut().for_each(Delay::reset);
        self.lows = [0.0; 8];
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let input = x * INPUT_GAIN;
        let mut out = 0.0;
        for (comb, low) in self.combs.iter_mut().zip(&mut self.lows) {
            let y = comb.read();
            *low = flush(y * (1.0 - self.damp) + *low * self.damp);
            comb.write(input + *low * self.feedback);
            out += y;
        }
        for ap in &mut self.allpasses {
            let b = ap.read();
            ap.write(flush(out + b * ALLPASS_FEEDBACK));
            out = b - out;
        }
        out
    }
}
