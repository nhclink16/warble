//! Everything that touches cpal: listing devices, the device thread that owns the streams, and
//! `Real`, the engine handle the UI talks to.
//!
//! The device thread opens a *session*: the master output stream (VB-CABLE, or the monitor when
//! there is no cable) running a `Graph`, the mic input feeding it through a ring, and the monitor
//! output fed from the graph through a second ring. Any stream error drops the whole session
//! and it is reopened (at most every `RETRY`). A session missing a device polls every `RETRY`
//! and reopens when that device shows up.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, ErrorKind, FromSample, Host, I24, Sample, SampleFormat, SizedSample,
    Stream, StreamConfig, SupportedBufferSize, U24,
};
use rtrb::{Producer, RingBuffer};

use crate::graph::{self, Command, Controls, DriftReader, Graph, RingWriter, Sound};
use crate::{Config, DeviceInfo, Devices, SoundId, Status, is_cable_input, lock, pick_cable};

/// How often a failed or incomplete session is retried.
const RETRY: Duration = Duration::from_secs(2);
/// How often the device thread checks for stream errors.
const TICK: Duration = Duration::from_millis(100);
/// How long a backend may take to open one stream.
const OPEN_TIMEOUT: Duration = Duration::from_secs(3);
/// Queued soundboard commands; more than this between two audio callbacks are dropped.
const COMMANDS: usize = 64;

const NO_CABLE: &str = "VB-CABLE isn't installed";
const NO_MIC: &str = "No microphone found";
const NO_MONITOR: &str = "No headphones or speakers found";
const MIC_IS_CABLE: &str =
    "Your microphone is set to VB-CABLE. Pick your real microphone in settings.";
const MONITOR_IS_CABLE: &str = "Monitoring is set to VB-CABLE. Pick your headphones in settings.";
const LOST: &str = "Lost an audio device. Reconnecting…";

pub(crate) fn list_devices() -> Devices {
    let host = cpal::default_host();
    let default_in = host.default_input_device().and_then(named).map(|(_, n)| n);
    let default_out = host.default_output_device().and_then(named).map(|(_, n)| n);
    Devices {
        inputs: infos(host.input_devices().into_iter().flatten(), default_in),
        outputs: infos(host.output_devices().into_iter().flatten(), default_out),
    }
}

fn infos(devices: impl Iterator<Item = Device>, default: Option<String>) -> Vec<DeviceInfo> {
    let mut out: Vec<DeviceInfo> = Vec::new();
    for (_, name) in devices.filter_map(named) {
        if !out.iter().any(|d| d.name == name) {
            out.push(DeviceInfo {
                is_default: default.as_deref() == Some(name.as_str()),
                name,
            });
        }
    }
    out
}

/// A device and its name. cpal 0.18 dropped `Device::name()`; the OS name (WASAPI's
/// FriendlyName, e.g. "CABLE Input (VB-Audio Virtual Cable)") is `description().name()`.
/// (`to_string()` would panic if the device vanished, so it isn't used.)
fn named(device: Device) -> Option<(Device, String)> {
    let name = device.description().ok()?.name().to_string();
    Some((device, name))
}

#[derive(Clone, Debug, Default)]
struct Info {
    running: bool,
    input: Option<String>,
    cable: Option<String>,
    monitor: Option<String>,
    error: Option<String>,
    /// Shown only while monitoring is on.
    monitor_error: Option<String>,
}

struct Shared {
    controls: Arc<Controls>,
    /// Written only by the device thread.
    info: Mutex<Info>,
    /// The current graph's command queue; replaced by the device thread on every session.
    commands: Mutex<Option<Producer<Command>>>,
}

impl Shared {
    /// After a session's streams are dropped: nothing is playing, and nobody reads commands.
    fn stopped(&self) {
        self.controls.clear_outputs();
        *lock(&self.commands) = None;
    }
}

enum Msg {
    Restart(Config),
    Shutdown,
}

#[derive(Default)]
struct Library {
    last_id: u32,
    sounds: HashMap<SoundId, Arc<Sound>>,
    /// The clip last sent to preview, kept so the audio side never holds its last reference.
    preview: Option<Arc<Sound>>,
    /// Removed sounds the audio side may still hold. Dropped here, never on the audio thread,
    /// once ours is the last reference.
    retired: Vec<Arc<Sound>>,
}

/// The real engine handle.
pub(crate) struct Real {
    shared: Arc<Shared>,
    library: Mutex<Library>,
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Real {
    /// Spawns the device thread and waits (a few seconds at most) for its first attempt to
    /// open the devices, so the first `status()` already says what happened.
    pub fn start(config: Config) -> Real {
        let shared = Arc::new(Shared {
            controls: Arc::new(Controls::default()),
            info: Mutex::default(),
            commands: Mutex::new(None),
        });
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("warble-devices".into())
            .spawn({
                let shared = shared.clone();
                move || run(config, &shared, rx, ready_tx)
            });
        let thread = match thread {
            Ok(thread) => {
                let _ = ready_rx.recv_timeout(Duration::from_secs(5));
                Some(thread)
            }
            Err(_) => {
                lock(&shared.info).error = Some("Couldn't start audio".into());
                None
            }
        };
        Real {
            shared,
            library: Mutex::default(),
            tx,
            thread,
        }
    }

    pub fn controls(&self) -> &Controls {
        &self.shared.controls
    }

    pub fn restart(&self, config: Config) {
        let _ = self.tx.send(Msg::Restart(config));
    }

    pub fn load_sound(&self, path: &Path) -> Result<SoundId, String> {
        let sound = Arc::new(crate::sound::decode(path)?);
        let mut library = lock(&self.library);
        library.last_id = library.last_id.wrapping_add(1).max(1);
        let id = SoundId(library.last_id);
        library.sounds.insert(id, sound);
        Ok(id)
    }

    pub fn remove_sound(&self, id: SoundId) {
        let mut library = lock(&self.library);
        if let Some(sound) = library.sounds.remove(&id) {
            library.retired.push(sound);
        }
        drop(library);
        self.send(Command::Stop(id));
    }

    pub fn play_sound(&self, id: SoundId) {
        let sound = lock(&self.library).sounds.get(&id).cloned();
        if let Some(sound) = sound {
            self.send(Command::Play(id, sound));
        }
    }

    pub fn stop_sounds(&self) {
        self.send(Command::StopAll);
    }

    pub fn preview(&self, sound: Arc<Sound>, start: usize, end: usize) {
        self.retire_preview(Some(sound.clone()));
        self.send(Command::Preview(sound, start, end));
    }

    pub fn stop_preview(&self) {
        self.retire_preview(None);
        self.send(Command::StopPreview);
    }

    fn retire_preview(&self, next: Option<Arc<Sound>>) {
        let mut library = lock(&self.library);
        if let Some(old) = std::mem::replace(&mut library.preview, next) {
            library.retired.push(old);
        }
    }

    /// Dropped (queue full, or no audio running) commands are freed here, on the UI thread.
    fn send(&self, command: Command) {
        if let Some(tx) = lock(&self.shared.commands).as_mut() {
            let _ = tx.push(command);
        }
    }

    pub fn status(&self) -> Status {
        lock(&self.library)
            .retired
            .retain(|s| Arc::strong_count(s) > 1);
        let info = lock(&self.shared.info).clone();
        let c = &self.shared.controls;
        let monitor_error = if c.monitor_in_use() {
            info.monitor_error
        } else {
            None
        };
        Status {
            running: info.running,
            input_name: info.input,
            cable_name: info.cable,
            monitor_name: info.monitor,
            error: info.error.or(monitor_error),
            input_level: c.input_level.load(),
            output_level: c.output_level.load(),
            playing: c.playing(),
            preview: c.preview_at(),
            underruns: c.underruns.load(Relaxed),
        }
    }
}

impl Drop for Real {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Open streams, kept alive until dropped.
struct Session {
    _streams: Vec<Stream>,
    /// Set by the error callback of any stream the call needs.
    failed: Arc<AtomicBool>,
    /// Set by the side monitor's error callback (when the cable is the master). With
    /// "hear yourself" and "hear sounds" both off it's left dead, so a headphone or
    /// default-speaker change can't cut the call; turning either on reopens the session.
    monitor_failed: Arc<AtomicBool>,
    /// The devices this session resolved to (input, cable, monitor), to notice new ones.
    names: [Option<String>; 3],
    /// Every wanted device is open.
    complete: bool,
    /// The cable or mic (or the monitor, standing in for a missing cable) was there but
    /// wouldn't open (busy, odd format...), so the call has no voice: retry every `RETRY` even
    /// though nothing changed. A monitor failing on the side doesn't count, so it can't make a
    /// working call glitch every few seconds; it is retried when the devices change.
    build_failed: bool,
}

fn run(mut config: Config, shared: &Shared, rx: Receiver<Msg>, ready: SyncSender<()>) {
    let mut session: Option<Session> = None;
    let mut reopen = true;
    let mut next_try = Instant::now();
    // Small fixed buffers first; if a device refuses to open, alternate with its defaults.
    let mut small_buffers = true;
    loop {
        if reopen && Instant::now() >= next_try {
            if session.take().is_some_and(|s| s.build_failed) {
                small_buffers = !small_buffers;
            }
            shared.stopped();
            session = Some(open_guarded(&config, shared, small_buffers));
            next_try = Instant::now() + RETRY;
            reopen = false;
            let _ = ready.try_send(());
        }
        match rx.recv_timeout(TICK) {
            Ok(Msg::Restart(c)) => {
                config = c;
                reopen = true;
                next_try = Instant::now();
                continue;
            }
            Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let Some(s) = &session else { continue };
        let monitor_lost = s.monitor_failed.load(Relaxed) && shared.controls.monitor_in_use();
        if s.failed.load(Relaxed) || monitor_lost {
            // Drop the dead streams now; reopen once `next_try` comes round.
            session = None;
            shared.stopped();
            *lock(&shared.info) = Info {
                error: Some(LOST.into()),
                ..Info::default()
            };
            reopen = true;
        } else if !s.complete && Instant::now() >= next_try {
            let host = cpal::default_host();
            if s.build_failed || resolve(&host, &config).names() != s.names {
                reopen = true;
            } else {
                next_try = Instant::now() + RETRY;
            }
        }
    }
    drop(session);
    shared.stopped();
}

/// `open`, but a panic inside a backend becomes an error and a retry instead of killing audio.
fn open_guarded(config: &Config, shared: &Shared, small_buffers: bool) -> Session {
    catch_unwind(AssertUnwindSafe(|| open(config, shared, small_buffers))).unwrap_or_else(|_| {
        *lock(&shared.info) = Info {
            error: Some("Audio device error. Retrying…".into()),
            ..Info::default()
        };
        Session {
            _streams: Vec::new(),
            failed: Arc::default(),
            monitor_failed: Arc::default(),
            names: Default::default(),
            complete: false,
            build_failed: true,
        }
    })
}

type Found = Result<(Device, String), String>;

struct Resolved {
    input: Found,
    cable: Found,
    monitor: Found,
}

impl Resolved {
    fn names(&self) -> [Option<String>; 3] {
        let name = |f: &Found| f.as_ref().ok().map(|(_, n)| n.clone());
        [name(&self.input), name(&self.cable), name(&self.monitor)]
    }
}

/// Which devices `config` means right now.
fn resolve(host: &Host, config: &Config) -> Resolved {
    let find = |name: &str, output: bool| -> Found {
        let devices = if output {
            host.output_devices()
        } else {
            host.input_devices()
        };
        devices
            .into_iter()
            .flatten()
            .filter_map(named)
            .find(|(_, n)| n == name)
            .ok_or_else(|| format!("Can't find \"{name}\". Is it plugged in?"))
    };
    let cable = match &config.cable {
        Some(name) => find(name, true),
        None => {
            let outputs: Vec<_> = host
                .output_devices()
                .into_iter()
                .flatten()
                .filter_map(named)
                .collect();
            let name = pick_cable(outputs.iter().map(|(_, n)| n.as_str())).map(str::to_owned);
            outputs
                .into_iter()
                .find(|(_, n)| Some(n) == name.as_ref())
                .ok_or_else(|| NO_CABLE.to_string())
        }
    };
    // Recording from the cable's other end, or monitoring into the cable, would loop the
    // call's audio back on itself. Windows sometimes makes VB-CABLE the default after install.
    let input = match &config.input {
        Some(name) => find(name, false),
        None => host
            .default_input_device()
            .and_then(named)
            .ok_or_else(|| NO_MIC.to_string()),
    }
    .and_then(|(d, n)| {
        if n.to_lowercase().contains("cable output") {
            Err(MIC_IS_CABLE.into())
        } else {
            Ok((d, n))
        }
    });
    let cable_name = cable.as_ref().ok().map(|(_, n)| n.clone());
    let monitor = match &config.monitor {
        Some(name) => find(name, true),
        None => host
            .default_output_device()
            .and_then(named)
            .ok_or_else(|| NO_MONITOR.to_string()),
    }
    .and_then(|(d, n)| {
        if is_cable_input(&n) || cable_name.as_ref() == Some(&n) {
            Err(MONITOR_IS_CABLE.into())
        } else {
            Ok((d, n))
        }
    });
    Resolved {
        input,
        cable,
        monitor,
    }
}

/// A device ready to open: its name and the stream format we'll ask for.
struct Endpoint {
    device: Device,
    name: String,
    config: StreamConfig,
    format: SampleFormat,
}

fn open(config: &Config, shared: &Shared, small_buffers: bool) -> Session {
    let host = cpal::default_host();
    let resolved = resolve(&host, config);
    let names = resolved.names();
    let failed = Arc::new(AtomicBool::new(false));
    let monitor_failed = Arc::new(AtomicBool::new(false));
    let mut build_failed = false;
    let (mut cable_err, mut mic_err, mut monitor_err) = (None, None, None);

    let mut endpoint = |found: Found, input: bool, essential: bool, err: &mut Option<String>| {
        let (device, name) = found.map_err(|e| *err = Some(e)).ok()?;
        match stream_config(&device, input, small_buffers) {
            Ok((config, format)) => Some(Endpoint {
                device,
                name,
                config,
                format,
            }),
            Err(e) => {
                *err = Some(couldnt_open(&name, &e));
                build_failed |= essential;
                None
            }
        }
    };
    let cable = endpoint(resolved.cable, false, true, &mut cable_err);
    let mic = endpoint(resolved.input, true, true, &mut mic_err);
    let monitor = endpoint(resolved.monitor, false, cable.is_none(), &mut monitor_err);

    let mut info = Info::default();
    let mut streams = Vec::new();
    let mut opened = None;
    if let Some(cable) = &cable {
        match open_set(
            shared,
            &failed,
            &monitor_failed,
            cable,
            mic.as_ref(),
            monitor.as_ref(),
            false,
        ) {
            Ok(set) => {
                info.cable = Some(cable.name.clone());
                opened = Some(set);
            }
            Err(e) => {
                cable_err = Some(couldnt_open(&cable.name, &e));
                build_failed = true;
            }
        }
    }
    if opened.is_none() {
        // No cable: run the graph on the monitor so effects can still be previewed.
        if let Some(monitor) = &monitor {
            match open_set(shared, &failed, &failed, monitor, mic.as_ref(), None, true) {
                Ok(set) => opened = Some(set),
                Err(e) => {
                    monitor_err = Some(couldnt_open(&monitor.name, &e));
                    build_failed = true;
                }
            }
        }
    }
    if let Some(set) = opened {
        streams = set.streams;
        for (result, endpoint, essential, err, name) in [
            (set.mic, &mic, true, &mut mic_err, &mut info.input),
            (
                set.monitor,
                &monitor,
                false,
                &mut monitor_err,
                &mut info.monitor,
            ),
        ] {
            if let Some(endpoint) = endpoint {
                match result {
                    Ok(()) => *name = Some(endpoint.name.clone()),
                    Err(e) => {
                        *err = Some(couldnt_open(&endpoint.name, &e));
                        build_failed |= essential;
                    }
                }
            }
        }
    }
    info.running = info.cable.is_some() && info.input.is_some();
    let complete = cable_err.is_none() && mic_err.is_none() && monitor_err.is_none();
    info.error = cable_err.or(mic_err);
    info.monitor_error = monitor_err;
    *lock(&shared.info) = info;
    Session {
        _streams: streams,
        failed,
        monitor_failed,
        names,
        complete,
        build_failed,
    }
}

/// The streams of one session. `mic` and `monitor` are `Ok` when opened or not wanted.
struct Set {
    streams: Vec<Stream>,
    mic: Result<(), cpal::Error>,
    monitor: Result<(), cpal::Error>,
}

/// Open `master` running the graph, plus the mic and (when `master` is the cable) the monitor.
/// Fails only if the master stream fails; mic and monitor failures are reported in `Set`.
/// `failed` flags the master and mic streams; `monitor_failed` the side monitor.
fn open_set(
    shared: &Shared,
    failed: &Arc<AtomicBool>,
    monitor_failed: &Arc<AtomicBool>,
    master: &Endpoint,
    mic: Option<&Endpoint>,
    monitor: Option<&Endpoint>,
    master_is_monitor: bool,
) -> Result<Set, cpal::Error> {
    let rate = master.config.sample_rate;
    let (mic_writer, mic_reader) = mic.map(|m| graph::ring(m.config.sample_rate, rate)).unzip();
    let (monitor_writer, monitor_reader) = monitor
        .map(|m| graph::ring(rate, m.config.sample_rate))
        .unzip();
    let (commands, command_rx) = RingBuffer::new(COMMANDS);
    let mut graph = Graph::new(
        rate,
        shared.controls.clone(),
        command_rx,
        mic_reader,
        monitor_writer,
        master_is_monitor,
    );
    // The clock both ends of each ring read.
    let epoch = Instant::now();

    let master_stream = build_output(master, failed, move |buf, frames| {
        graph.callback(frames);
        graph.process(buf, epoch.elapsed().as_secs_f64())
    })?;
    master_stream.play()?;
    *lock(&shared.commands) = Some(commands);
    let mut streams = vec![master_stream];

    let mut start = |stream: Result<Stream, cpal::Error>| -> Result<(), cpal::Error> {
        let stream = stream?;
        stream.play()?;
        streams.push(stream);
        Ok(())
    };
    let mic = match mic.zip(mic_writer) {
        Some((m, writer)) => start(build_input(m, failed, writer, epoch)),
        None => Ok(()),
    };
    let monitor = match monitor.zip(monitor_reader) {
        Some((m, reader)) => start(monitor_stream(m, monitor_failed, reader, epoch)),
        None => Ok(()),
    };
    Ok(Set {
        streams,
        mic,
        monitor,
    })
}

fn monitor_stream(
    endpoint: &Endpoint,
    failed: &Arc<AtomicBool>,
    mut reader: DriftReader,
    epoch: Instant,
) -> Result<Stream, cpal::Error> {
    // The graph already applied the headphone gains to what it put in the ring.
    build_output(endpoint, failed, move |buf, frames| {
        reader.callback(frames);
        reader.read(buf, epoch.elapsed().as_secs_f64());
    })
}

/// The device's own format, with ~10 ms buffers when it reports a fixed-size range.
fn stream_config(
    device: &Device,
    input: bool,
    small: bool,
) -> Result<(StreamConfig, SampleFormat), cpal::Error> {
    let supported = if input {
        device.default_input_config()
    } else {
        device.default_output_config()
    }?;
    let mut config = supported.config();
    if let (true, SupportedBufferSize::Range { min, max }) = (small, supported.buffer_size()) {
        config.buffer_size = BufferSize::Fixed((config.sample_rate / 100).max(*min).min(*max));
    }
    Ok((config, supported.sample_format()))
}

/// Any error but an xrun or a refused priority boost means the stream needs reopening.
fn on_error(failed: &Arc<AtomicBool>) -> impl FnMut(cpal::Error) + Send + 'static {
    let failed = failed.clone();
    move |e| {
        if !matches!(e.kind(), ErrorKind::Xrun | ErrorKind::RealtimeDenied) {
            failed.store(true, Relaxed);
        }
    }
}

fn couldnt_open(name: &str, e: &cpal::Error) -> String {
    let why = match e.kind() {
        ErrorKind::DeviceBusy => "another app is using it",
        ErrorKind::DeviceNotAvailable => "it isn't available. Is it unplugged or disabled?",
        ErrorKind::PermissionDenied => "Windows blocked it. Check Settings > Privacy > Microphone.",
        ErrorKind::UnsupportedConfig | ErrorKind::InvalidInput => {
            "its audio format isn't supported"
        }
        _ => "the device reported an error",
    };
    format!("Couldn't open {name}: {why}")
}

/// Calls `$f::<T>(args)` with `T` the sample type for `$format`.
macro_rules! with_sample_type {
    ($format:expr, $f:ident($($arg:expr),*)) => {
        match $format {
            SampleFormat::F32 => $f::<f32>($($arg),*),
            SampleFormat::F64 => $f::<f64>($($arg),*),
            SampleFormat::I8 => $f::<i8>($($arg),*),
            SampleFormat::I16 => $f::<i16>($($arg),*),
            SampleFormat::I24 => $f::<I24>($($arg),*),
            SampleFormat::I32 => $f::<i32>($($arg),*),
            SampleFormat::I64 => $f::<i64>($($arg),*),
            SampleFormat::U8 => $f::<u8>($($arg),*),
            SampleFormat::U16 => $f::<u16>($($arg),*),
            SampleFormat::U24 => $f::<U24>($($arg),*),
            SampleFormat::U32 => $f::<u32>($($arg),*),
            SampleFormat::U64 => $f::<u64>($($arg),*),
            _ => Err(ErrorKind::UnsupportedConfig.into()),
        }
    };
}

/// An output stream that renders mono with `render` and copies it to every channel. `render`
/// gets at most `MAX_BLOCK` frames at a time, plus the length of the whole device callback.
fn build_output(
    endpoint: &Endpoint,
    failed: &Arc<AtomicBool>,
    render: impl FnMut(&mut [f32], usize) + Send + 'static,
) -> Result<Stream, cpal::Error> {
    with_sample_type!(endpoint.format, output_typed(endpoint, failed, render))
}

fn output_typed<T: SizedSample + FromSample<f32>>(
    endpoint: &Endpoint,
    failed: &Arc<AtomicBool>,
    mut render: impl FnMut(&mut [f32], usize) + Send + 'static,
) -> Result<Stream, cpal::Error> {
    let channels = usize::from(endpoint.config.channels.max(1));
    let mut mono = vec![0.0f32; graph::MAX_BLOCK];
    endpoint.device.build_output_stream(
        endpoint.config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels;
            for chunk in data.chunks_mut(graph::MAX_BLOCK * channels) {
                let mono = &mut mono[..chunk.len() / channels];
                render(mono, frames);
                for (frame, &s) in chunk.chunks_mut(channels).zip(mono.iter()) {
                    frame.fill(T::from_sample(s));
                }
            }
        },
        on_error(failed),
        Some(OPEN_TIMEOUT),
    )
}

/// An input stream that downmixes to mono into `ring`.
fn build_input(
    endpoint: &Endpoint,
    failed: &Arc<AtomicBool>,
    ring: RingWriter,
    epoch: Instant,
) -> Result<Stream, cpal::Error> {
    with_sample_type!(endpoint.format, input_typed(endpoint, failed, ring, epoch))
}

fn input_typed<T: SizedSample>(
    endpoint: &Endpoint,
    failed: &Arc<AtomicBool>,
    mut ring: RingWriter,
    epoch: Instant,
) -> Result<Stream, cpal::Error>
where
    f32: FromSample<T>,
{
    let channels = usize::from(endpoint.config.channels.max(1));
    let scale = 1.0 / channels as f32;
    endpoint.device.build_input_stream(
        endpoint.config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let frames = data.chunks_exact(channels);
            let n = frames.len();
            let mono = frames.map(|f| f.iter().map(|&s| f32::from_sample(s)).sum::<f32>() * scale);
            ring.push(n, mono, epoch.elapsed().as_secs_f64());
        },
        on_error(failed),
        Some(OPEN_TIMEOUT),
    )
}
