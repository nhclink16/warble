//! RBJ "Audio EQ Cookbook" biquads in transposed direct form II.

use crate::fx::flush;
use std::f64::consts::PI;

#[derive(Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

/// Angular frequency, with `hz` kept safely below Nyquist at any sample rate.
fn w0(sample_rate: f32, hz: f32) -> f64 {
    let sr = sample_rate as f64;
    2.0 * PI * (hz as f64).clamp(10.0, 0.45 * sr) / sr
}

impl Biquad {
    fn from(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Biquad {
        Biquad {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn lowpass(sample_rate: f32, hz: f32, q: f32) -> Biquad {
        let w = w0(sample_rate, hz);
        let (cos, alpha) = (w.cos(), w.sin() / (2.0 * q as f64));
        let b = (1.0 - cos) / 2.0;
        Biquad::from(b, 2.0 * b, b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn highpass(sample_rate: f32, hz: f32, q: f32) -> Biquad {
        let w = w0(sample_rate, hz);
        let (cos, alpha) = (w.cos(), w.sin() / (2.0 * q as f64));
        let b = (1.0 + cos) / 2.0;
        Biquad::from(b, -2.0 * b, b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    pub fn peaking(sample_rate: f32, hz: f32, q: f32, gain_db: f32) -> Biquad {
        let w = w0(sample_rate, hz);
        let (cos, alpha) = (w.cos(), w.sin() / (2.0 * q as f64));
        let a = 10f64.powf(gain_db as f64 / 40.0);
        Biquad::from(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        )
    }

    /// Shelf slope S = 1.
    pub fn low_shelf(sample_rate: f32, hz: f32, gain_db: f32) -> Biquad {
        let w = w0(sample_rate, hz);
        let a = 10f64.powf(gain_db as f64 / 40.0);
        // k = 2 * sqrt(A) * alpha, with alpha = sin(w0) / 2 * sqrt(2) for S = 1.
        let (cos, k) = (w.cos(), a.sqrt() * w.sin() * 2f64.sqrt());
        Biquad::from(
            a * ((a + 1.0) - (a - 1.0) * cos + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - k),
            (a + 1.0) + (a - 1.0) * cos + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - k,
        )
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush(self.b2 * x - self.a2 * y);
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state gain (RMS out / RMS in) for a sine at `hz`.
    fn gain(mut f: Biquad, hz: f32) -> f32 {
        let n = 48_000;
        let (mut sum_in, mut sum_out) = (0.0f64, 0.0f64);
        for i in 0..n {
            let x = (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
            let y = f.tick(x);
            if i > n / 2 {
                sum_in += (x * x) as f64;
                sum_out += (y * y) as f64;
            }
        }
        (sum_out / sum_in).sqrt() as f32
    }

    #[test]
    fn responses_have_the_right_shape() {
        let sr = 48_000.0;
        let db = |g: f32| 20.0 * g.log10();
        assert!((db(gain(Biquad::lowpass(sr, 1000.0, 0.707), 100.0))).abs() < 0.1);
        assert!(db(gain(Biquad::lowpass(sr, 1000.0, 0.707), 8000.0)) < -30.0);
        assert!((db(gain(Biquad::highpass(sr, 1000.0, 0.707), 1000.0)) + 3.0).abs() < 0.2);
        assert!(db(gain(Biquad::highpass(sr, 1000.0, 0.707), 100.0)) < -35.0);
        assert!((db(gain(Biquad::peaking(sr, 1000.0, 1.0, 6.0), 1000.0)) - 6.0).abs() < 0.1);
        assert!((db(gain(Biquad::low_shelf(sr, 300.0, 4.0), 30.0)) - 4.0).abs() < 0.2);
        assert!(db(gain(Biquad::low_shelf(sr, 300.0, 4.0), 8000.0)).abs() < 0.1);
    }
}
