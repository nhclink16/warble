//! Behaviour of `Voice` through its public API.

use std::f32::consts::TAU;
use warble_dsp::{Preset, Voice};

const SR: f32 = 48_000.0;

fn secs(s: f32) -> usize {
    (s * SR) as usize
}

/// Full-scale uniform white noise, deterministic.
fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32
        })
        .collect()
}

fn sine(n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (TAU * hz * i as f32 / SR).sin())
        .collect()
}

fn processed(voice: &mut Voice, input: &[f32]) -> Vec<f32> {
    let mut out = input.to_vec();
    voice.process(&mut out);
    out
}

fn max_jump(signal: &[f32]) -> f32 {
    signal
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn assert_bounded(out: &[f32], what: &str) {
    for (i, y) in out.iter().enumerate() {
        assert!(y.is_finite() && y.abs() <= 1.0, "{what}: sample {i} is {y}");
    }
}

#[test]
fn a_new_voice_is_normal_and_passes_audio_untouched() {
    let mut v = Voice::new(SR);
    assert_eq!(v.preset(), Preset::Normal);
    assert!(v.enabled());
    let input = noise(secs(0.1), 1);
    assert_eq!(processed(&mut v, &input), input);
}

#[test]
fn normal_is_identity_after_the_crossfade() {
    let mut v = Voice::new(SR);
    v.set_preset(Preset::Deep);
    processed(&mut v, &noise(secs(0.2), 2));
    v.set_preset(Preset::Normal);
    processed(&mut v, &noise(secs(0.021), 3));
    let input = noise(secs(0.5), 4);
    let out = processed(&mut v, &input);
    assert!(
        out.iter()
            .zip(&input)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

#[test]
fn disabled_is_identity_after_the_crossfade() {
    for preset in Preset::ALL {
        let mut v = Voice::new(SR);
        v.set_preset(preset);
        processed(&mut v, &noise(secs(0.2), 5));
        v.set_enabled(false);
        assert_eq!(v.preset(), preset);
        assert!(!v.enabled());
        processed(&mut v, &noise(secs(0.021), 6));
        let input = noise(secs(0.5), 7);
        let out = processed(&mut v, &input);
        assert!(
            out.iter()
                .zip(&input)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{preset:?}"
        );
    }
}

#[test]
fn every_preset_stays_finite_and_within_full_scale() {
    for preset in Preset::ALL {
        for (name, input) in [
            ("noise", noise(secs(3.0), 8)),
            ("100 Hz sine", sine(secs(3.0), 100.0, 1.0)),
            ("1 kHz sine", sine(secs(3.0), 1000.0, 1.0)),
            ("full-scale DC", vec![1.0; secs(1.0)]),
        ] {
            let mut v = Voice::new(SR);
            v.set_preset(preset);
            assert_bounded(&processed(&mut v, &input), &format!("{preset:?} {name}"));
        }
    }
}

#[test]
fn rapid_switching_stays_bounded() {
    // A new preset every 7 ms, so several chains fade at once.
    let mut v = Voice::new(SR);
    let input = noise(secs(4.0), 9);
    for (i, block) in input.chunks(secs(0.007)).enumerate() {
        v.set_preset(Preset::ALL[(i * 5 + i / 8) % 8]);
        if i % 11 == 0 {
            v.set_enabled(!v.enabled());
        }
        assert_bounded(&processed(&mut v, block), "rapid switching");
    }
}

#[test]
fn silence_in_gives_silence_out_once_tails_decay() {
    for preset in Preset::ALL {
        let mut v = Voice::new(SR);
        v.set_preset(preset);
        let fresh = processed(&mut v, &vec![0.0; secs(1.0)]);
        assert!(
            fresh.iter().all(|y| y.abs() < 1e-9),
            "{preset:?} not silent from the start"
        );
        processed(&mut v, &noise(secs(1.0), 10));
        let tail = processed(&mut v, &vec![0.0; secs(8.0)]);
        let end = tail[secs(7.5)..].iter().fold(0.0f32, |m, y| m.max(y.abs()));
        assert!(
            end < 1e-4,
            "{preset:?} still at {end} after 7.5 s of silence"
        );
    }
}

#[test]
fn zero_length_buffers_are_fine() {
    let mut v = Voice::new(SR);
    v.process(&mut []);
    for preset in Preset::ALL {
        v.set_preset(preset);
        v.process(&mut []);
        v.process(&mut [0.5; 3]);
        v.process(&mut []);
    }
    v.set_enabled(false);
    v.process(&mut []);
}

#[test]
fn any_sample_rate_works() {
    for sr in [
        8_000.0,
        22_050.0,
        44_100.0,
        96_000.0,
        192_000.0,
        0.0,
        -1.0,
        f32::NAN,
    ] {
        let mut v = Voice::new(sr);
        for preset in Preset::ALL {
            v.set_preset(preset);
            let mut buf = noise(4_000, 11);
            v.process(&mut buf);
            assert_bounded(&buf, &format!("{preset:?} at {sr} Hz"));
        }
    }
}

#[test]
fn output_does_not_depend_on_block_size() {
    // Switches land on a multiple of every block size, so each run sees them at the same sample.
    let switch = 61_440;
    let input: Vec<f32> = noise(switch * 3, 12)
        .iter()
        .zip(sine(switch * 3, 330.0, 0.5))
        .map(|(n, s)| 0.3 * n + s)
        .collect();
    for preset in Preset::ALL {
        let run = |block: usize| {
            let mut v = Voice::new(SR);
            v.set_preset(preset);
            let mut out = input.clone();
            for (i, part) in out.chunks_mut(switch).enumerate() {
                match i {
                    1 => v.set_preset(Preset::ALL[(preset as usize + 3) % 8]),
                    2 => v.set_enabled(false),
                    _ => {}
                }
                part.chunks_mut(block).for_each(|b| v.process(b));
            }
            out
        };
        let reference = run(1);
        for block in [64, 480, 4096] {
            let out = run(block);
            let worst = reference
                .iter()
                .zip(&out)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(
                worst <= 1e-6,
                "{preset:?}: block {block} differs by {worst}"
            );
        }
    }
}

/// A click shows up as a sample-to-sample jump far bigger than the steepest slope of the
/// input sine (a step on this sine would be about 38 times it). Chipmunk's output is 1.54
/// times steeper on its own, being pitched up, so allow 2 times.
#[test]
fn switching_presets_does_not_click() {
    let (hz, amp) = (200.0, 0.5);
    let limit = 2.0 * amp * TAU * hz / SR;
    let input = sine(secs(1.0), hz, amp);
    let (first, second) = (secs(0.6), secs(0.61));
    let check = |from: Preset, change: &dyn Fn(&mut Voice), then: Option<Preset>| {
        let mut v = Voice::new(SR);
        v.set_preset(from);
        let mut out = processed(&mut v, &input[..first]);
        change(&mut v);
        out.extend(processed(&mut v, &input[first..second]));
        if let Some(p) = then {
            v.set_preset(p);
        }
        out.extend(processed(&mut v, &input[second..]));
        let jump = max_jump(&out[first - 1..second + secs(0.025)]);
        assert!(
            jump <= limit,
            "{from:?} then {then:?}: jump {jump} > {limit}"
        );
    };
    for from in Preset::ALL {
        for to in Preset::ALL {
            check(from, &|v| v.set_preset(to), None);
            // A second change 10 ms in, while the first is still fading.
            check(
                from,
                &|v| v.set_preset(to),
                Some(Preset::ALL[(to as usize + 3) % 8]),
            );
        }
        check(from, &|v| v.set_enabled(false), None);
        check(
            from,
            &|v| v.set_enabled(false),
            Some(Preset::ALL[(from as usize + 1) % 8]),
        );
        check(
            from,
            &|v| {
                v.set_enabled(false);
                v.set_enabled(true);
            },
            None,
        );
    }
}
