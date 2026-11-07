//! The audio graph, free of any device API so tests can drive it with simulated clocks.
//!
//! `Graph::process` runs inside the master output callback (the virtual cable, or the monitor
//! when there is no cable). It and everything it calls (`DriftReader::read`, `RingWriter::push`,
//! the soundboard) are real-time safe: no allocation, locking, blocking or panics.
//!
//! Functions that take `now` want seconds on one clock shared by both ends of a ring (the
//! device code passes time since the session opened; tests pass simulated time).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

use rtrb::{Consumer, Producer, RingBuffer};
use warble_dsp::{Preset, Voice};

use crate::SoundId;

/// Soundboard voices that can play at once.
pub(crate) const POOL: usize = 16;
/// The longest block processed in one go; longer device buffers are split.
pub(crate) const MAX_BLOCK: usize = 2048;

/// A ring holds this much audio before its writer starts dropping samples.
const RING_SECONDS: f64 = 1.0;
/// Latency kept in a ring on top of one block from each side, to ride out scheduling jitter.
/// Each underrun adds `UNDERRUN_MARGIN_SECONDS` (up to `MAX_EXTRA_MARGIN_SECONDS`), which fades
/// over `RELEASE_SECONDS * 6` so a startup hiccup doesn't cost latency for the whole session.
const MARGIN_SECONDS: f64 = 0.003;
const UNDERRUN_MARGIN_SECONDS: f64 = 0.002;
const MAX_EXTRA_MARGIN_SECONDS: f64 = 0.017;
/// Block sizes are held at their recent peak, letting go of a one-off burst (a device's
/// first callback often fills its whole buffer) with this time constant.
const RELEASE_SECONDS: f64 = 5.0;
/// The drift controller pulls the fill back to target with this time constant...
const CONTROL_SECONDS: f64 = 2.0;
/// ...after smoothing the measured fill over this long (critically damped with the above).
const FILTER_SECONDS: f64 = 0.5;
/// The most the controller may speed up or slow down a stream (1% is a sixth of a semitone).
const MAX_CORRECTION: f64 = 0.01;
/// Meter fall-off time constant.
const METER_SECONDS: f32 = 0.15;

/// An `f32` stored as bits, for scalar parameters shared with the audio thread.
pub(crate) struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> AtomicF32 {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Relaxed))
    }
    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Relaxed)
    }
}

/// Lock-free state shared between the UI and the audio callbacks.
pub(crate) struct Controls {
    // UI → audio.
    preset: AtomicU32,
    pub effect_enabled: AtomicBool,
    pub mic_gain: AtomicF32,
    pub monitor: AtomicBool,
    pub monitor_volume: AtomicF32,
    pub sound_volume: AtomicF32,
    // Audio → UI.
    pub input_level: AtomicF32,
    pub output_level: AtomicF32,
    pub underruns: AtomicU64,
    /// The sound in each soundboard voice, 0 for none (sound ids start at 1).
    playing: [AtomicU32; POOL],
}

impl Default for Controls {
    /// Same starting values as the mock engine.
    fn default() -> Controls {
        Controls {
            preset: AtomicU32::new(0),
            effect_enabled: AtomicBool::new(true),
            mic_gain: AtomicF32::new(1.0),
            monitor: AtomicBool::new(false),
            monitor_volume: AtomicF32::new(0.8),
            sound_volume: AtomicF32::new(0.8),
            input_level: AtomicF32::new(0.0),
            output_level: AtomicF32::new(0.0),
            underruns: AtomicU64::new(0),
            playing: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl Controls {
    pub fn preset(&self) -> Preset {
        Preset::ALL
            .get(self.preset.load(Relaxed) as usize)
            .copied()
            .unwrap_or_default()
    }

    pub fn set_preset(&self, preset: Preset) {
        let index = Preset::ALL.iter().position(|p| *p == preset).unwrap_or(0);
        self.preset.store(index as u32, Relaxed);
    }

    /// Monitor gain to apply right now: the volume when monitoring is on, else silence.
    pub fn monitor_gain(&self) -> f32 {
        if self.monitor.load(Relaxed) {
            self.monitor_volume.load().clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn playing(&self) -> Vec<SoundId> {
        self.playing
            .iter()
            .map(|p| p.load(Relaxed))
            .filter(|&id| id != 0)
            .map(SoundId)
            .collect()
    }

    /// Zero the meters and playing flags while no graph is running.
    pub fn clear_outputs(&self) {
        self.input_level.store(0.0);
        self.output_level.store(0.0);
        for p in &self.playing {
            p.store(0, Relaxed);
        }
    }
}

/// A decoded clip: mono, at the file's own sample rate.
pub(crate) struct Sound {
    pub samples: Vec<f32>,
    pub rate: u32,
}

/// UI → audio soundboard commands. The UI keeps its own `Arc` of every sound it sends, so the
/// audio thread never drops the last reference (and never frees a clip).
pub(crate) enum Command {
    Play(SoundId, Arc<Sound>),
    Stop(SoundId),
    StopAll,
}

/// What the writer tells the reader besides the samples (f64s as bits).
#[derive(Default)]
struct WriterClock {
    /// The writer's recent peak block size, which sizes the reader's cushion.
    block: AtomicU64,
    /// When the last block was pushed (`now`).
    pushed_at: AtomicU64,
}

impl WriterClock {
    fn block(&self) -> f64 {
        f64::from_bits(self.block.load(Relaxed))
    }
}

/// The largest of `n` and a held peak that decays over `RELEASE_SECONDS`.
fn hold(peak: f64, n: f64, seconds: f64) -> f64 {
    n.max(peak * (-seconds / RELEASE_SECONDS).exp())
}

/// A single-producer ring between two clocks: the writer side of `ring`.
pub(crate) struct RingWriter {
    tx: Producer<f32>,
    clock: Arc<WriterClock>,
    rate: f64,
    block: f64,
}

impl RingWriter {
    /// Queue `n` samples from `samples`, which end at time `now`. Drops what doesn't fit (the
    /// reader has stalled; it skips back to its target once it runs again).
    pub fn push(&mut self, n: usize, samples: impl Iterator<Item = f32>, now: f64) {
        self.block = hold(self.block, n as f64, n as f64 / self.rate);
        self.clock.block.store(self.block.to_bits(), Relaxed);
        if let Ok(chunk) = self.tx.write_chunk_uninit(n.min(self.tx.slots())) {
            chunk.fill_from_iter(samples);
        }
        self.clock.pushed_at.store(now.to_bits(), Relaxed);
    }
}

/// A ring carrying mono audio from a stream running at `src_rate` to one at `dst_rate`, each on
/// its own clock.
pub(crate) fn ring(src_rate: u32, dst_rate: u32) -> (RingWriter, DriftReader) {
    let capacity = ((src_rate as f64 * RING_SECONDS) as usize).max(4 * MAX_BLOCK);
    let (tx, rx) = RingBuffer::new(capacity);
    let clock = Arc::new(WriterClock::default());
    let src = src_rate.max(1) as f64;
    let dst = dst_rate.max(1) as f64;
    let reader = DriftReader {
        rx,
        writer: clock.clone(),
        src_rate: src,
        dst_rate: dst,
        nominal: src / dst,
        ratio: src / dst,
        reader_block: 0.0,
        extra_margin: 0.0,
        latency_avg: 0.0,
        hist: [0.0; 4],
        frac: 0.0,
        playing: false,
    };
    (
        RingWriter {
            tx,
            clock,
            rate: src,
            block: 0.0,
        },
        reader,
    )
}

/// The reader side of `ring`: an adaptive resampler. It converts between the nominal rates and
/// absorbs clock drift by nudging its ratio so the latency through the ring stays at a small
/// target.
pub(crate) struct DriftReader {
    rx: Consumer<f32>,
    writer: Arc<WriterClock>,
    src_rate: f64,
    dst_rate: f64,
    /// `src_rate / dst_rate`: input samples per output sample if the clocks agreed exactly.
    nominal: f64,
    ratio: f64,
    /// Our recent peak block size, in input samples.
    reader_block: f64,
    /// Margin learned from underruns, in input samples.
    extra_margin: f64,
    latency_avg: f64,
    /// Last four input samples; output is interpolated between `hist[1]` and `hist[2]`.
    hist: [f32; 4],
    frac: f64,
    /// False while (re)filling to target after startup or an underrun.
    playing: bool,
}

impl DriftReader {
    /// Latency wanted through the ring, in input samples: one writer block plus one reader
    /// block plus a margin. Less, and a read that lands just before a write runs dry.
    fn target(&self) -> f64 {
        self.writer.block() + self.reader_block + MARGIN_SECONDS * self.src_rate + self.extra_margin
    }

    /// Latency through the ring right now, in input samples: what is waiting, plus what the
    /// writer has captured since its last push. Unlike the fill alone, this doesn't jump by a
    /// block whenever the two callbacks' phases slide past each other.
    fn latency(&self, fill: f64, now: f64) -> f64 {
        let since = now - f64::from_bits(self.writer.pushed_at.load(Relaxed));
        fill + (since * self.src_rate).max(0.0).min(self.writer.block())
    }

    /// Samples waiting in the ring.
    #[cfg(test)]
    pub fn fill(&self) -> usize {
        self.rx.slots()
    }

    fn skip(&mut self, n: usize) {
        if let Ok(chunk) = self.rx.read_chunk(n.min(self.rx.slots())) {
            chunk.commit_all();
        }
    }

    /// The device callback being read is `frames` long, even if it's read in `MAX_BLOCK`
    /// pieces: size the cushion for all of it, or the later pieces run dry.
    pub fn callback(&mut self, frames: usize) {
        self.reader_block = self.reader_block.max(frames as f64 * self.ratio);
    }

    /// Fill `out` (played from time `now`) with resampled audio. Returns true if the ring ran
    /// dry partway (an underrun): the rest of `out` is silence and the reader waits until the
    /// ring is back at its target. While waiting (including at startup) it outputs silence and
    /// returns false.
    pub fn read(&mut self, out: &mut [f32], now: f64) -> bool {
        let dt = out.len() as f64 / self.dst_rate;
        self.reader_block = hold(self.reader_block, out.len() as f64 * self.ratio, dt);
        self.extra_margin *= (-dt / (6.0 * RELEASE_SECONDS)).exp();
        let target = self.target();
        let mut latency = self.latency(self.rx.slots() as f64, now);
        if !self.playing {
            if latency < target {
                out.fill(0.0);
                return false;
            }
            self.playing = true;
            self.ratio = self.nominal;
            self.skip((latency - target) as usize);
            latency = target;
            self.latency_avg = target;
        }
        // Far too much (this side stalled, or the writer burst): jump back to target.
        if latency > 2.0 * target + 0.02 * self.src_rate {
            self.skip((latency - target) as usize);
            latency = target;
            self.latency_avg = target;
        }
        self.latency_avg += (latency - self.latency_avg) * (1.0 - (-dt / FILTER_SECONDS).exp());
        let correction = ((self.latency_avg - target) / (CONTROL_SECONDS * self.src_rate))
            .clamp(-MAX_CORRECTION, MAX_CORRECTION);
        self.ratio = self.nominal * (1.0 + correction);

        for i in 0..out.len() {
            while self.frac >= 1.0 {
                match self.rx.pop() {
                    Ok(s) => {
                        self.hist = [self.hist[1], self.hist[2], self.hist[3], s];
                        self.frac -= 1.0;
                    }
                    Err(_) => {
                        out[i..].fill(0.0);
                        self.playing = false;
                        self.hist = [0.0; 4];
                        self.extra_margin = (self.extra_margin
                            + UNDERRUN_MARGIN_SECONDS * self.src_rate)
                            .min(MAX_EXTRA_MARGIN_SECONDS * self.src_rate);
                        return true;
                    }
                }
            }
            out[i] = cubic(self.hist, self.frac as f32);
            self.frac += self.ratio;
        }
        false
    }
}

/// Catmull-Rom interpolation between `y[1]` (t = 0) and `y[2]` (t = 1).
#[inline]
fn cubic(y: [f32; 4], t: f32) -> f32 {
    let [y0, y1, y2, y3] = y;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// Multiply by a gain that moves linearly from `*current` to `target` across the block, so
/// slider moves and toggles don't click.
pub(crate) fn ramp(buf: &mut [f32], current: &mut f32, target: f32) {
    let step = (target - *current) / buf.len().max(1) as f32;
    for s in buf.iter_mut() {
        *current += step;
        *s *= *current;
    }
    *current = target;
}

/// Transparent below 0.8, then eases into ±1.0. Non-finite input becomes silence.
fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    if !x.is_finite() {
        return 0.0;
    }
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        (KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh()).copysign(x)
    }
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0, |m, s| m.max(s.abs()))
}

struct Playing {
    id: SoundId,
    sound: Arc<Sound>,
    /// Position in the clip's own samples.
    pos: f64,
    started: u64,
}

/// A fixed pool of soundboard voices, each resampling its clip on the fly.
pub(crate) struct Soundboard {
    slots: [Option<Playing>; POOL],
    plays: u64,
}

impl Default for Soundboard {
    fn default() -> Soundboard {
        Soundboard {
            slots: std::array::from_fn(|_| None),
            plays: 0,
        }
    }
}

impl Soundboard {
    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Play(id, sound) => {
                self.plays += 1;
                // Replay restarts the same voice; otherwise a free one; if all are busy, the
                // one that has played longest makes way.
                let slot = self
                    .slots
                    .iter()
                    .position(|s| s.as_ref().is_some_and(|p| p.id == id))
                    .or_else(|| self.slots.iter().position(Option::is_none))
                    .or_else(|| {
                        (0..POOL).min_by_key(|&i| self.slots[i].as_ref().map_or(0, |p| p.started))
                    })
                    .unwrap_or(0);
                self.slots[slot] = Some(Playing {
                    id,
                    sound,
                    pos: 0.0,
                    started: self.plays,
                });
            }
            Command::Stop(id) => {
                for s in &mut self.slots {
                    if s.as_ref().is_some_and(|p| p.id == id) {
                        *s = None;
                    }
                }
            }
            Command::StopAll => self.slots = std::array::from_fn(|_| None),
        }
    }

    /// Add every playing clip, times `volume`, into `out` (running at `rate`).
    pub fn mix(&mut self, out: &mut [f32], rate: f32, volume: f32) {
        for slot in &mut self.slots {
            let Some(p) = slot else { continue };
            let samples = &p.sound.samples;
            let len = samples.len();
            let at = |i: usize| samples.get(i).copied().unwrap_or(0.0);
            let step = p.sound.rate as f64 / rate as f64;
            for o in out.iter_mut() {
                let i = p.pos as usize;
                if i >= len {
                    break;
                }
                let t = (p.pos - i as f64) as f32;
                let before = if i == 0 { 0.0 } else { at(i - 1) };
                *o += volume * cubic([before, at(i), at(i + 1), at(i + 2)], t);
                p.pos += step;
            }
            if p.pos as usize >= len {
                *slot = None;
            }
        }
    }
}

/// Mic → gain → voice effect → + soundboard → soft limit → out, and a copy to the monitor ring.
pub(crate) struct Graph {
    rate: f32,
    controls: Arc<Controls>,
    commands: Consumer<Command>,
    mic: Option<DriftReader>,
    monitor: Option<RingWriter>,
    voice: Voice,
    preset: Preset,
    effect_enabled: bool,
    board: Soundboard,
    gain: f32,
    input_level: f32,
    output_level: f32,
}

impl Graph {
    /// Allocates (the voice effect chains); call off the audio thread.
    pub fn new(
        rate: u32,
        controls: Arc<Controls>,
        commands: Consumer<Command>,
        mic: Option<DriftReader>,
        monitor: Option<RingWriter>,
    ) -> Graph {
        let mut voice = Voice::new(rate as f32);
        let preset = controls.preset();
        let effect_enabled = controls.effect_enabled.load(Relaxed);
        voice.set_preset(preset);
        voice.set_enabled(effect_enabled);
        Graph {
            rate: rate.max(1) as f32,
            gain: controls.mic_gain.load().clamp(0.0, 4.0),
            controls,
            commands,
            mic,
            monitor,
            voice,
            preset,
            effect_enabled,
            board: Soundboard::default(),
            input_level: 0.0,
            output_level: 0.0,
        }
    }

    /// The device callback being rendered is `frames` long; see `DriftReader::callback`.
    pub fn callback(&mut self, frames: usize) {
        if let Some(mic) = &mut self.mic {
            mic.callback(frames);
        }
    }

    /// Render the next `out.len()` mono samples, played from time `now`. Real-time safe.
    pub fn process(&mut self, out: &mut [f32], now: f64) {
        self.callback(out.len());
        while let Ok(command) = self.commands.pop() {
            self.board.apply(command);
        }
        let c = &*self.controls;
        let preset = c.preset();
        if preset != self.preset {
            self.preset = preset;
            self.voice.set_preset(preset);
        }
        let enabled = c.effect_enabled.load(Relaxed);
        if enabled != self.effect_enabled {
            self.effect_enabled = enabled;
            self.voice.set_enabled(enabled);
        }
        let gain = c.mic_gain.load().clamp(0.0, 4.0);
        let sound_volume = c.sound_volume.load().clamp(0.0, 1.0);

        for block in out.chunks_mut(MAX_BLOCK) {
            match &mut self.mic {
                Some(mic) => {
                    if mic.read(block, now) {
                        c.underruns.fetch_add(1, Relaxed);
                    }
                }
                None => block.fill(0.0),
            }
            ramp(block, &mut self.gain, gain);
            let input_peak = peak(block);
            self.voice.process(block);
            self.board.mix(block, self.rate, sound_volume);
            for s in block.iter_mut() {
                *s = soft_limit(*s);
            }
            let output_peak = peak(block);
            if let Some(monitor) = &mut self.monitor {
                monitor.push(block.len(), block.iter().copied(), now);
            }
            let decay = (-(block.len() as f32) / (self.rate * METER_SECONDS)).exp();
            self.input_level = input_peak.max(self.input_level * decay);
            self.output_level = output_peak.max(self.output_level * decay);
        }

        c.input_level.store(self.input_level.min(1.0));
        c.output_level.store(self.output_level.min(1.0));
        for (flag, slot) in c.playing.iter().zip(&self.board.slots) {
            flag.store(slot.as_ref().map_or(0, |p| p.id.0), Relaxed);
        }
    }

    #[cfg(test)]
    fn mic_fill(&self) -> usize {
        self.mic.as_ref().map_or(0, DriftReader::fill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TONE: f64 = 440.0;

    /// A device callback that fires every `block` frames of its own (slightly wrong) clock,
    /// up to `jitter` seconds late. An input fires once a block has been captured; an output as
    /// its block starts playing.
    struct Clock {
        true_rate: f64,
        block: usize,
        input: bool,
        jitter: f64,
        rng: u64,
        next: f64,
        frames: u64,
    }

    impl Clock {
        fn new(true_rate: f64, block: usize, input: bool, jitter: f64) -> Clock {
            let rng = true_rate.to_bits() | 1;
            let mut clock = Clock {
                true_rate,
                block,
                input,
                jitter,
                rng,
                next: 0.0,
                frames: 0,
            };
            clock.schedule();
            clock
        }
        fn schedule(&mut self) {
            let end = self.frames + if self.input { self.block as u64 } else { 0 };
            self.rng = self
                .rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let late = (self.rng >> 11) as f64 / (1u64 << 53) as f64 * self.jitter;
            self.next = end as f64 / self.true_rate + late;
        }
        fn tick(&mut self) {
            self.frames += self.block as u64;
            self.schedule();
        }
    }

    /// Counts upward zero crossings after `from` seconds to measure a tone's frequency.
    #[derive(Default)]
    struct Tone {
        last: f32,
        crossings: u64,
        samples: u64,
    }

    impl Tone {
        fn feed(&mut self, buf: &[f32]) {
            for &s in buf {
                if self.last < 0.0 && s >= 0.0 {
                    self.crossings += 1;
                }
                self.last = s;
            }
            self.samples += buf.len() as u64;
        }
        fn hz(&self, true_rate: f64) -> f64 {
            self.crossings as f64 / (self.samples as f64 / true_rate)
        }
    }

    struct Report {
        late_underruns: u64,
        monitor_late_underruns: u64,
        /// Mic capture → cable callback latency after warm-up, in ms: (min, mean, max).
        latency_ms: (f64, f64, f64),
        /// Its mean over the second minute and over the last minute.
        early_ms: f64,
        late_ms: f64,
        /// Cable callback → monitor callback latency after warm-up, in ms: (min, max).
        monitor_ms: (f64, f64),
        cable_hz: f64,
        monitor_hz: f64,
    }

    /// Run mic → graph (clocked by the cable) → monitor ring → monitor reader for `seconds`
    /// of simulated time. Rates are (nominal, true): what the device claims and how fast it
    /// actually runs.
    fn simulate(
        mic: (u32, f64),
        cable: (u32, f64),
        monitor: (u32, f64),
        seconds: f64,
        jitter: f64,
    ) -> Report {
        let controls = Arc::new(Controls::default());
        controls.effect_enabled.store(false, Relaxed); // dry voice: don't depend on the DSP
        let (_cmd_tx, cmd_rx) = RingBuffer::new(8);
        let mut mon_pushed = 0u64; // samples the graph has sent to the monitor ring
        let (mut mic_tx, mic_rx) = ring(mic.0, cable.0);
        let (mon_tx, mut mon_rx) = ring(cable.0, monitor.0);
        let mut graph = Graph::new(
            cable.0,
            controls.clone(),
            cmd_rx,
            Some(mic_rx),
            Some(mon_tx),
        );

        let block = |rate: u32| (rate / 100) as usize; // 10 ms device periods, like WASAPI
        let mut mic_clock = Clock::new(mic.1, block(mic.0), true, jitter);
        let mut cable_clock = Clock::new(cable.1, block(cable.0), false, jitter);
        let mut mon_clock = Clock::new(monitor.1, block(monitor.0), false, jitter);
        let mut cable_buf = vec![0.0; cable_clock.block];
        let mut mon_buf = vec![0.0; mon_clock.block];

        let warmup = 5.0;
        let mut underruns_at_warmup = None;
        let mut monitor_late_underruns = 0;
        let (mut min, mut max, mut sum, mut n) = (f64::MAX, 0.0f64, 0.0, 0u64);
        let (mut early, mut late) = ((0.0, 0u64), (0.0, 0u64));
        let mut monitor_ms = (f64::MAX, 0.0f64);
        let (mut cable_tone, mut monitor_tone) = (Tone::default(), Tone::default());
        let mut mic_sample = 0u64;

        loop {
            let t = mic_clock.next.min(cable_clock.next).min(mon_clock.next);
            if t > seconds {
                break;
            }
            // How long ago the oldest sample still waiting in a ring was captured, in ms.
            let latency_ms = |pushed: u64, fill: usize, true_rate: f64| {
                (t - (pushed - fill as u64) as f64 / true_rate) * 1000.0
            };
            if t == mic_clock.next {
                // The mic samples a real-time 440 Hz tone at its true rate.
                let start = mic_sample;
                let samples = (start..start + mic_clock.block as u64).map(|k| {
                    (0.5 * (std::f64::consts::TAU * TONE * k as f64 / mic.1).sin()) as f32
                });
                mic_tx.push(mic_clock.block, samples, t);
                mic_sample += mic_clock.block as u64;
                mic_clock.tick();
            } else if t == cable_clock.next {
                let fill = latency_ms(mic_sample, graph.mic_fill(), mic.1);
                graph.process(&mut cable_buf, t);
                mon_pushed += cable_buf.len() as u64;
                if t > warmup {
                    let u = controls.underruns.load(Relaxed);
                    underruns_at_warmup.get_or_insert(u);
                    (min, max, sum, n) = (min.min(fill), max.max(fill), sum + fill, n + 1);
                    if (60.0..120.0).contains(&t) {
                        early = (early.0 + fill, early.1 + 1);
                    }
                    if t > seconds - 60.0 {
                        late = (late.0 + fill, late.1 + 1);
                        cable_tone.feed(&cable_buf);
                    }
                }
                cable_clock.tick();
            } else {
                let fill = latency_ms(mon_pushed, mon_rx.fill(), cable.1);
                let underrun = mon_rx.read(&mut mon_buf, t);
                if t > warmup {
                    monitor_late_underruns += underrun as u64;
                    monitor_ms = (monitor_ms.0.min(fill), monitor_ms.1.max(fill));
                    if t > seconds - 60.0 {
                        monitor_tone.feed(&mon_buf);
                    }
                }
                mon_clock.tick();
            }
        }
        Report {
            late_underruns: controls.underruns.load(Relaxed) - underruns_at_warmup.unwrap_or(0),
            monitor_late_underruns,
            latency_ms: (min, sum / n as f64, max),
            early_ms: early.0 / early.1 as f64,
            late_ms: late.0 / late.1 as f64,
            monitor_ms,
            cable_hz: cable_tone.hz(cable.1),
            monitor_hz: monitor_tone.hz(monitor.1),
        }
    }

    /// Target latency for 10 ms blocks on both sides: 10 + 10 + 3 ms margin, counted from
    /// when a sample was captured.
    const TARGET_MS: f64 = 23.0;
    /// The graph hands a block to the monitor ring as the block starts playing on the cable,
    /// a block (10 ms) earlier than a mic hands one over, so the monitor trails the cable by
    /// 10 ms less than the target.
    const MONITOR_MS: f64 = TARGET_MS - 10.0;

    fn check(r: Report, tolerance_ms: f64) {
        let (min, mean, max) = r.latency_ms;
        eprintln!(
            "mic latency ms min {min:.2} mean {mean:.2} max {max:.2}, minute 2 {:.2}, last minute \
             {:.2}; monitor {:.2}..{:.2} ms; cable {:.3} Hz, monitor {:.3} Hz",
            r.early_ms, r.late_ms, r.monitor_ms.0, r.monitor_ms.1, r.cable_hz, r.monitor_hz
        );
        assert_eq!(r.late_underruns, 0, "mic underruns after startup");
        assert_eq!(
            r.monitor_late_underruns, 0,
            "monitor underruns after startup"
        );
        assert!(
            min > TARGET_MS - tolerance_ms && max < TARGET_MS + tolerance_ms,
            "mic latency strayed from target: {min:.1}..{max:.1} ms"
        );
        assert!(
            (r.late_ms - r.early_ms).abs() < 1.0,
            "latency drifted over time"
        );
        assert!(
            r.monitor_ms.0 > MONITOR_MS - tolerance_ms
                && r.monitor_ms.1 < MONITOR_MS + tolerance_ms,
            "monitor latency strayed from target: {:.1}..{:.1} ms",
            r.monitor_ms.0,
            r.monitor_ms.1
        );
        assert!(
            (r.cable_hz / TONE - 1.0).abs() < 0.005,
            "cable pitch: {} Hz",
            r.cable_hz
        );
        assert!(
            (r.monitor_hz / TONE - 1.0).abs() < 0.005,
            "monitor pitch: {} Hz",
            r.monitor_hz
        );
    }

    const TEN_MINUTES: f64 = 600.0;

    #[test]
    fn fast_mic_clock_is_absorbed() {
        let r = simulate(
            (48000, 48000.0 * 1.0005),
            (48000, 48000.0),
            (48000, 48000.0 * 0.9997),
            TEN_MINUTES,
            0.0,
        );
        check(r, 3.0);
    }

    #[test]
    fn slow_mic_clock_is_absorbed() {
        let r = simulate(
            (48000, 48000.0 * 0.9995),
            (48000, 48000.0),
            (44100, 44100.0 * 1.0003),
            TEN_MINUTES,
            0.0,
        );
        check(r, 3.0);
    }

    #[test]
    fn mic_at_44100_into_cable_at_48000() {
        let r = simulate(
            (44100, 44100.0 * 1.0002),
            (48000, 48000.0),
            (48000, 48000.0 * 0.9998),
            TEN_MINUTES,
            0.0,
        );
        check(r, 3.0);
    }

    #[test]
    fn late_callbacks_dont_underrun() {
        // Every callback up to 2 ms late, as on a busy PC.
        let r = simulate(
            (44100, 44100.0 * 0.9995),
            (48000, 48000.0),
            (48000, 48000.0 * 1.0003),
            180.0,
            0.002,
        );
        check(r, 5.0);
    }

    #[test]
    fn startup_bursts_dont_cost_latency_for_good() {
        // WASAPI asks for its whole buffer on the first callback; ALSA prefills several
        // periods back to back. Both should only raise latency for a few seconds.
        let (mut tx, mut rx) = ring(48000, 48000);
        let mut out = vec![0.0; 1056];
        let mut pushed = 0u64;
        let mut latency_ms = 0.0;
        for k in 0..12_000u64 {
            // Block k is captured over [k, k + 1) × 10 ms and pushed when it ends.
            let t = (k + 1) as f64 * 0.01;
            tx.push(480, std::iter::repeat_n(0.0, 480), t);
            pushed += 480;
            let reads: &[usize] = match k {
                0 => &[1056],
                5 => &[480, 480, 480],
                _ => &[480],
            };
            for &n in reads {
                latency_ms = (t + 0.005 - (pushed - rx.fill() as u64) as f64 / 48000.0) * 1000.0;
                rx.read(&mut out[..n], t + 0.005);
            }
        }
        assert!(
            (latency_ms - TARGET_MS).abs() < 2.0,
            "settled at {latency_ms:.1} ms"
        );
    }

    #[test]
    fn big_callbacks_read_in_pieces_dont_underrun() {
        // A device asking for 4096 frames at a time gets them rendered in MAX_BLOCK pieces;
        // the cushion has to cover the whole callback, not just one piece.
        let (mut tx, mut rx) = ring(48000, 48000);
        let callback = 2 * MAX_BLOCK;
        let mut out = vec![0.0; MAX_BLOCK];
        let (mut mic_k, mut out_k, mut late_underruns) = (0u64, 0u64, 0);
        loop {
            let mic_t = (mic_k + 1) as f64 * 0.01;
            let out_t = out_k as f64 * callback as f64 / 48000.0;
            if mic_t.min(out_t) > 120.0 {
                break;
            }
            if mic_t <= out_t {
                tx.push(480, std::iter::repeat_n(0.0, 480), mic_t);
                mic_k += 1;
            } else {
                rx.callback(callback);
                for _ in 0..2 {
                    if rx.read(&mut out, out_t) && out_t > 10.0 {
                        late_underruns += 1;
                    }
                }
                out_k += 1;
            }
        }
        assert_eq!(late_underruns, 0, "underruns after startup");
    }

    fn sound(value: f32, len: usize, rate: u32) -> Arc<Sound> {
        Arc::new(Sound {
            samples: vec![value; len],
            rate,
        })
    }

    fn board_graph() -> (Graph, Producer<Command>, Arc<Controls>) {
        let controls = Arc::new(Controls::default());
        controls.effect_enabled.store(false, Relaxed);
        controls.sound_volume.store(1.0);
        let (tx, rx) = RingBuffer::new(64);
        (
            Graph::new(48000, controls.clone(), rx, None, None),
            tx,
            controls,
        )
    }

    #[test]
    fn soundboard_mixes_and_finishes() {
        let (mut graph, mut tx, controls) = board_graph();
        let a = sound(0.1, 1000, 48000);
        let b = sound(0.2, 300, 48000);
        tx.push(Command::Play(SoundId(1), a.clone())).ok();
        tx.push(Command::Play(SoundId(2), b.clone())).ok();
        let mut buf = vec![0.0; 256];
        graph.process(&mut buf, 0.0);
        assert!(
            buf.iter().all(|s| (s - 0.3).abs() < 1e-6),
            "both clips summed"
        );
        assert_eq!(controls.playing(), vec![SoundId(1), SoundId(2)]);
        graph.process(&mut buf, 0.0);
        // b (300 samples) ends 44 samples into this block; a carries on.
        assert!((buf[0] - 0.3).abs() < 1e-6 && (buf[100] - 0.1).abs() < 1e-6);
        assert_eq!(controls.playing(), vec![SoundId(1)]);
        for _ in 0..3 {
            graph.process(&mut buf, 0.0);
        }
        assert!(controls.playing().is_empty());
        assert!(buf.iter().all(|&s| s == 0.0));
        // The audio side let go of both clips; only our Arcs remain.
        assert_eq!((Arc::strong_count(&a), Arc::strong_count(&b)), (1, 1));
    }

    #[test]
    fn soundboard_replay_restarts() {
        let (mut graph, mut tx, controls) = board_graph();
        let a = Arc::new(Sound {
            samples: (0..1000).map(|i| i as f32 / 4000.0).collect(),
            rate: 48000,
        });
        tx.push(Command::Play(SoundId(7), a.clone())).ok();
        let mut buf = vec![0.0; 100];
        graph.process(&mut buf, 0.0);
        graph.process(&mut buf, 0.0);
        assert!((buf[0] - 100.0 / 4000.0).abs() < 1e-6);
        tx.push(Command::Play(SoundId(7), a.clone())).ok();
        graph.process(&mut buf, 0.0);
        assert!(
            buf[0].abs() < 1e-6 && (buf[50] - 50.0 / 4000.0).abs() < 1e-6,
            "restarted from 0"
        );
        assert_eq!(controls.playing(), vec![SoundId(7)], "still one voice");
    }

    #[test]
    fn soundboard_resamples_to_graph_rate() {
        let (mut graph, mut tx, controls) = board_graph();
        // One second at 24 kHz lasts one second at 48 kHz.
        tx.push(Command::Play(SoundId(1), sound(0.1, 24000, 24000)))
            .ok();
        let mut buf = vec![0.0; 480];
        for _ in 0..99 {
            graph.process(&mut buf, 0.0);
        }
        assert_eq!(controls.playing(), vec![SoundId(1)]);
        graph.process(&mut buf, 0.0);
        graph.process(&mut buf, 0.0);
        assert!(controls.playing().is_empty());
    }

    #[test]
    fn soundboard_stop_and_remove() {
        let (mut graph, mut tx, controls) = board_graph();
        for id in 1..=3 {
            tx.push(Command::Play(SoundId(id), sound(0.1, 48000, 48000)))
                .ok();
        }
        let mut buf = vec![0.0; 64];
        graph.process(&mut buf, 0.0);
        assert_eq!(controls.playing().len(), 3);
        tx.push(Command::Stop(SoundId(2))).ok();
        graph.process(&mut buf, 0.0);
        assert_eq!(controls.playing(), vec![SoundId(1), SoundId(3)]);
        assert!(buf.iter().all(|s| (s - 0.2).abs() < 1e-6));
        tx.push(Command::StopAll).ok();
        graph.process(&mut buf, 0.0);
        assert!(controls.playing().is_empty());
        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn soundboard_overflow_replaces_oldest() {
        let (mut graph, mut tx, controls) = board_graph();
        controls.sound_volume.store(0.01);
        for id in 1..=POOL as u32 + 1 {
            tx.push(Command::Play(SoundId(id), sound(0.5, 48000, 48000)))
                .ok();
        }
        let mut buf = vec![0.0; 64];
        graph.process(&mut buf, 0.0);
        let mut playing = controls.playing();
        playing.sort();
        let want: Vec<SoundId> = (2..=POOL as u32 + 1).map(SoundId).collect();
        assert_eq!(playing, want, "the first sound made way for the 17th");
    }

    #[test]
    fn soft_limit_is_bounded() {
        for x in [
            -100.0,
            -1.2,
            -0.5,
            0.0,
            0.79,
            1.0,
            3.0,
            f32::INFINITY,
            f32::NAN,
        ] {
            let y = soft_limit(x);
            assert!(y.abs() <= 1.0, "{x} -> {y}");
        }
        assert_eq!(soft_limit(0.5), 0.5);
    }
}
