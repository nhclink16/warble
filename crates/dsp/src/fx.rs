//! Small shapers and helpers: soft clipper, output limiter, ring modulator, noise, envelope.

use std::f32::consts::TAU;

/// Zero out values far below hearing (-360 dB) so recursive state never goes denormal.
/// Denormal maths is very slow on x86, and Windows does not flush them by default.
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-18 { 0.0 } else { x }
}

/// tanh-shaped saturation: about `x` for small `x`, smooth, exactly ±1 from |x| >= 3.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    (x * (27.0 + x * x) / (27.0 + 9.0 * x * x)).clamp(-1.0, 1.0)
}

/// Where the output limiter starts to bend.
const KNEE: f32 = 0.8;

/// The last stage of every effect chain: untouched below the knee, then a smooth bend that
/// never passes ±1 (and maps NaN to a finite value rather than passing it on).
#[inline]
pub fn limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let y = KNEE + (1.0 - KNEE) * soft_clip((a - KNEE) / (1.0 - KNEE));
    y.min(1.0).copysign(x)
}

/// Multiplies the signal by a sine carrier.
pub struct RingMod {
    phase: f32,
    inc: f32,
}

impl RingMod {
    pub fn new(sample_rate: f32, hz: f32) -> RingMod {
        RingMod {
            phase: 0.0,
            inc: hz / sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let y = x * (TAU * self.phase).sin();
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        y
    }
}

/// White noise in [-1, 1) from xorshift32: cheap and deterministic.
pub struct Noise(u32);

const NOISE_SEED: u32 = 0x9E37_79B9;

impl Noise {
    pub fn new() -> Noise {
        Noise(NOISE_SEED)
    }

    pub fn reset(&mut self) {
        self.0 = NOISE_SEED;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        (s as i32) as f32 * (1.0 / 2_147_483_648.0)
    }
}

/// Peak follower with separate attack and release times.
pub struct Envelope {
    env: f32,
    attack: f32,
    release: f32,
}

impl Envelope {
    pub fn new(sample_rate: f32, attack_s: f32, release_s: f32) -> Envelope {
        let coef = |t: f32| 1.0 - (-1.0 / (t * sample_rate)).exp();
        Envelope {
            env: 0.0,
            attack: coef(attack_s),
            release: coef(release_s),
        }
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let a = x.abs();
        let c = if a > self.env {
            self.attack
        } else {
            self.release
        };
        self.env = flush(self.env + (a - self.env) * c);
        self.env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_is_transparent_below_knee_and_bounded_above() {
        for i in -800..=800 {
            let x = i as f32 / 1000.0;
            assert_eq!(limit(x), x);
        }
        let mut last = 0.0;
        for i in 0..=100_000 {
            let x = i as f32 / 1000.0;
            let y = limit(x);
            assert!(y <= 1.0 && y >= last, "limit({x}) = {y}");
            assert_eq!(limit(-x), -y);
            last = y;
        }
        assert!(limit(f32::NAN).is_finite());
        assert_eq!(limit(f32::INFINITY), 1.0);
    }

    #[test]
    fn soft_clip_is_bounded_and_odd() {
        for i in -10_000..=10_000 {
            let x = i as f32 / 1000.0;
            let y = soft_clip(x);
            assert!(y.abs() <= 1.0);
            assert_eq!(soft_clip(-x), -y);
        }
        assert!((soft_clip(0.01) - 0.01).abs() < 1e-5);
    }

    #[test]
    fn noise_stays_in_range() {
        let mut n = Noise::new();
        for _ in 0..100_000 {
            let v = n.next();
            assert!((-1.0..1.0).contains(&v));
        }
    }
}
