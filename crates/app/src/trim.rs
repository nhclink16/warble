//! The trim editor's model: the stretch of a clip to keep, its waveform, and the 16-bit WAV it
//! saves into the sounds folder.

use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use warble_engine::{Clip, MAX_SOUND_SECONDS};

use crate::library::safe_name;
use crate::settings;

/// The longest selection: the soundboard's own limit.
pub const MAX_SECONDS: f64 = MAX_SOUND_SECONDS as f64;
/// The shortest selection.
pub const MIN_SECONDS: f64 = 0.05;
/// Bars in the waveform.
pub const BARS: usize = 90;

pub struct Trim {
    pub clip: Clip,
    /// The selection, in seconds.
    pub start: f64,
    pub end: f64,
    /// The loudest sample under each waveform bar, 0..=1 of the clip's loudest.
    pub peaks: Vec<f32>,
    pub saving: bool,
    /// The selection when a drag began, to put back if it's cancelled.
    pub grab: (f64, f64),
}

impl Trim {
    /// The whole clip selected, or its first five minutes.
    pub fn new(clip: Clip) -> Trim {
        Trim {
            peaks: peaks(clip.samples(), BARS),
            start: 0.0,
            end: clip.seconds().min(MAX_SECONDS),
            clip,
            saving: false,
            grab: (0.0, 0.0),
        }
    }

    pub fn duration(&self) -> f64 {
        self.clip.seconds()
    }

    /// Moves the start, keeping it at least `MIN_SECONDS` before the end. Past `MAX_SECONDS`
    /// from the end, the end comes along.
    pub fn set_start(&mut self, t: f64) {
        let d = self.duration();
        self.start = t.clamp(0.0, (d - MIN_SECONDS).max(0.0));
        self.end = self
            .end
            .max(self.start + MIN_SECONDS)
            .min(self.start + MAX_SECONDS)
            .min(d);
    }

    /// Moves the end, the same way.
    pub fn set_end(&mut self, t: f64) {
        let d = self.duration();
        self.end = t.clamp(MIN_SECONDS.min(d), d);
        self.start = self
            .start
            .min(self.end - MIN_SECONDS)
            .max(self.end - MAX_SECONDS)
            .max(0.0);
    }

    /// The selection, in the clip's samples.
    pub fn range(&self) -> Range<usize> {
        let (rate, len) = (self.clip.rate() as f64, self.clip.samples().len());
        let start = ((self.start * rate).round() as usize).min(len);
        let end = ((self.end * rate).round() as usize).clamp(start, len);
        start..end
    }

    /// How far an arrow key moves a handle: a hundredth of the clip, 0.05 to 1 second.
    pub fn step(&self) -> f64 {
        (self.duration() / 100.0).clamp(0.05, 1.0)
    }

    /// The selection is the whole clip.
    pub fn is_whole(&self) -> bool {
        self.range() == (0..self.clip.samples().len())
    }
}

/// "Name.wav", safe as a file name.
pub fn file_name(name: &str) -> String {
    let name = name.trim();
    format!(
        "{}.wav",
        safe_name(if name.is_empty() { "Sound" } else { name })
    )
}

/// The loudest sample in each of `bars` equal stretches of `samples`, scaled so the loudest
/// bar is 1.
pub fn peaks(samples: &[f32], bars: usize) -> Vec<f32> {
    let n = samples.len();
    if n == 0 {
        return vec![0.0; bars];
    }
    let mut out: Vec<f32> = (0..bars)
        .map(|i| {
            let a = (i * n / bars).min(n - 1);
            let b = ((i + 1) * n / bars).clamp(a + 1, n);
            samples[a..b].iter().fold(0.0f32, |m, s| m.max(s.abs()))
        })
        .collect();
    let top = out.iter().copied().fold(0.0, f32::max);
    if top > 0.0 {
        out.iter_mut().for_each(|p| *p /= top);
    }
    out
}

/// A 16-bit PCM mono WAV file.
pub fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // the format chunk's size
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes a second
    b.extend_from_slice(&2u16.to_le_bytes()); // bytes a frame
    b.extend_from_slice(&16u16.to_le_bytes()); // bits a sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Writes `range` of `clip` into `dir` as `name` (a .wav), or "name (2).wav" when that's taken.
pub fn save(dir: &Path, name: &str, clip: &Clip, range: Range<usize>) -> std::io::Result<PathBuf> {
    let (mut file, path) = settings::create_in(dir, Path::new(name))?;
    let samples = &clip.samples()[range.start.min(range.end)..range.end.min(clip.samples().len())];
    let written = file
        .write_all(&wav(samples, clip.rate()))
        .and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// Seconds as "1:05.3".
pub fn clock(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use warble_engine::{Config, Engine};

    fn clip(seconds: f64, rate: u32) -> Clip {
        let n = (seconds * rate as f64) as usize;
        Clip::new(
            (0..n).map(|i| ((i % 100) as f32 / 100.0) - 0.5).collect(),
            rate,
        )
    }

    #[test]
    fn handles_stay_in_order_and_inside_the_limits() {
        let mut t = Trim::new(clip(10.0, 1000));
        assert_eq!((t.start, t.end), (0.0, 10.0));
        assert!(t.is_whole());
        t.set_start(4.0);
        t.set_end(3.0); // can't cross the start: pushes it back instead
        assert_eq!(t.end, 3.0);
        assert!((t.start - (3.0 - MIN_SECONDS)).abs() < 1e-9);
        t.set_start(-5.0);
        t.set_end(99.0);
        assert_eq!((t.start, t.end), (0.0, 10.0));
        t.set_start(10.0);
        assert!((t.start - (10.0 - MIN_SECONDS)).abs() < 1e-9 && t.end == 10.0);
        t.set_start(2.5);
        t.set_end(7.25);
        assert_eq!(t.range(), 2500..7250);
        assert!(!t.is_whole());
        assert_eq!(t.step(), 0.1);
        assert_eq!(file_name("  Bruh "), "Bruh.wav");
        assert_eq!(file_name("what?"), "what.wav");
        assert_eq!(file_name(" "), "Sound.wav");
    }

    #[test]
    fn a_selection_never_runs_past_five_minutes() {
        let mut t = Trim::new(clip(20.0 * 60.0, 100));
        assert_eq!(
            (t.start, t.end),
            (0.0, MAX_SECONDS),
            "starts at the first five minutes"
        );
        t.set_end(900.0);
        assert_eq!(
            (t.start, t.end),
            (600.0, 900.0),
            "the start follows the end"
        );
        t.set_start(100.0);
        assert_eq!(
            (t.start, t.end),
            (100.0, 400.0),
            "and the end follows the start"
        );
        assert_eq!(t.step(), 1.0);
    }

    #[test]
    fn peaks_scale_to_the_loudest_bar() {
        let mut samples = vec![0.1; 100];
        samples[95] = -0.5;
        let p = peaks(&samples, 10);
        assert_eq!(p.len(), 10);
        assert!((p[0] - 0.2).abs() < 1e-6 && p[9] == 1.0);
        assert_eq!(peaks(&[], 4), vec![0.0; 4]);
        assert_eq!(peaks(&[0.5, 0.25], 4).len(), 4, "more bars than samples");
        assert_eq!(clock(65.27), "1:05.3");
        assert_eq!(clock(0.0), "0:00.0");
    }

    #[test]
    fn saved_wavs_load_back_in_the_engine() {
        let dir = std::env::temp_dir().join(format!("warble-trim-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = clip(2.0, 22050);
        let a = save(&dir, "Cut.wav", &source, 11025..33075).unwrap();
        let b = save(&dir, "Cut.wav", &source, 0..source.samples().len()).unwrap();
        assert_eq!(b.file_name().unwrap(), "Cut (2).wav");
        let bytes = std::fs::read(&a).unwrap();
        assert_eq!(bytes.len(), 44 + 22050 * 2);

        let back = Clip::decode(&a, 300).unwrap();
        assert_eq!((back.rate(), back.samples().len()), (22050, 22050));
        for (got, want) in back.samples().iter().zip(&source.samples()[11025..]) {
            assert!((got - want).abs() < 1.0 / 16000.0, "{got} vs {want}");
        }
        let engine = Engine::start(Config {
            input: Some("No Such Microphone".into()),
            cable: Some("No Such Cable".into()),
            monitor: Some("No Such Headphones".into()),
        });
        assert!(engine.load_sound(&b).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
