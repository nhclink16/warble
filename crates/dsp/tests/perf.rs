//! CPU cost per preset. For real numbers run it optimised and show the output:
//! `cargo test -p warble-dsp --release --test perf -- --nocapture`

use std::time::{Duration, Instant};
use warble_dsp::{Preset, Voice};

const SR: f32 = 48_000.0;
const BLOCK: usize = 480;
const SECONDS: u32 = 10;

/// A voice-ish signal: a buzzy 130 Hz tone plus a little noise, in 3 Hz syllables.
fn signal() -> Vec<f32> {
    let mut s = 1u32;
    (0..SR as usize)
        .map(|i| {
            let t = i as f32 / SR;
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let buzz = (std::f32::consts::TAU * 130.0 * t).sin().powi(3);
            let noise = s as f32 / u32::MAX as f32 - 0.5;
            (0.4 * buzz + 0.05 * noise) * (std::f32::consts::PI * 3.0 * t).sin().powi(2)
        })
        .collect()
}

/// Average time to process one second of audio, after one second of warm-up.
fn time(voice: &mut Voice, input: &[f32]) -> Duration {
    let mut buf = input.to_vec();
    buf.chunks_mut(BLOCK).for_each(|b| voice.process(b));
    let start = Instant::now();
    for _ in 0..SECONDS {
        buf.copy_from_slice(input);
        buf.chunks_mut(BLOCK).for_each(|b| voice.process(b));
    }
    start.elapsed() / SECONDS
}

fn report(name: &str, per_second: Duration) {
    let ms = per_second.as_secs_f64() * 1e3;
    println!(
        "{name:>22}: {ms:7.3} ms per second of 48 kHz audio ({:.3}% of one core)",
        ms / 10.0
    );
    // Far above the real cost; catches pathological slowdowns such as denormal stalls.
    assert!(
        per_second < Duration::from_millis(100),
        "{name} is far too slow"
    );
}

#[test]
fn cost_per_preset() {
    let input = signal();
    for preset in Preset::ALL {
        let mut voice = Voice::new(SR);
        voice.set_preset(preset);
        report(preset.name(), time(&mut voice, &input));
    }

    // Decaying tails into silence are where denormals would bite.
    let silence = vec![0.0; SR as usize];
    for preset in [Preset::Echo, Preset::Demon, Preset::Cave] {
        let mut voice = Voice::new(SR);
        voice.set_preset(preset);
        let mut warm = input.clone();
        voice.process(&mut warm);
        report(
            &format!("{} tail, silence", preset.name()),
            time(&mut voice, &silence),
        );
    }

    // Worst case: a new preset every 30 ms, so two or more chains are always running.
    let mut voice = Voice::new(SR);
    let mut buf = input.clone();
    let start = Instant::now();
    for round in 0..SECONDS as usize {
        buf.copy_from_slice(&input);
        for (i, b) in buf.chunks_mut(BLOCK * 3).enumerate() {
            voice.set_preset(Preset::ALL[(round + i * 3) % 8]);
            b.chunks_mut(BLOCK).for_each(|b| voice.process(b));
        }
    }
    report("switching every 30 ms", start.elapsed() / SECONDS);
}
