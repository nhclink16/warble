//! The phone remote: a small web server on the PC that a phone on the same Wi-Fi (or on
//! Tailscale) opens in its browser. The QR code on the Setup tab gives the phone a link that
//! holds a secret key, and every request except the page itself must carry that key. Requests
//! are read on worker threads and answered on the UI thread, by `State::api`.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

use rune::{AppHandle, TextEdit};
use serde::Deserialize;
use serde_json::json;
use warble_engine::{Preset, SoundId};

use crate::find::YouTube;
use crate::state::State;
use crate::{ui, youtube};

/// The port phones connect to. When another app has it, the system picks a free one.
pub const PORT: u16 = 47823;
const PAGE: &str = include_str!("remote.html");
/// Requests read at once; more connections are closed, and the phone tries again.
const MAX_CONNECTIONS: usize = 8;
const MAX_REQUEST: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);

/// The remote's place in the app state.
#[derive(Default)]
pub struct Remote {
    /// Listening; `None` while the remote is off, and in tests and snapshots.
    server: Option<Server>,
    pub port: u16,
    /// This PC on the local network, and on Tailscale when that's running.
    pub wifi: Option<Ipv4Addr>,
    pub tailscale: Option<Ipv4Addr>,
    /// The QR code shows the Tailscale address instead of the Wi-Fi one.
    pub use_tailscale: bool,
    /// Why the remote isn't running.
    pub problem: Option<String>,
}

impl Remote {
    /// The address the QR code shows, or `None` when this PC has no network.
    pub fn address(&self) -> Option<String> {
        let ip = if self.use_tailscale {
            self.tailscale
        } else {
            self.wifi
        }?;
        Some(format!("http://{ip}:{}", self.port))
    }

    /// Reads this PC's addresses again: networks come and go.
    pub fn find_addresses(&mut self) {
        // Connecting a UDP socket only picks the route; nothing is sent.
        let wifi = local_ip("8.8.8.8:53").filter(|ip| !is_tailscale(ip));
        let tailscale = local_ip("100.100.100.100:53").filter(is_tailscale);
        self.set_addresses(wifi, tailscale);
    }

    pub fn set_addresses(&mut self, wifi: Option<Ipv4Addr>, tailscale: Option<Ipv4Addr>) {
        (self.wifi, self.tailscale) = (wifi, tailscale);
        if tailscale.is_none() {
            self.use_tailscale = false;
        } else if wifi.is_none() {
            // A Tailscale exit node carries all traffic, so Tailscale is the only way in.
            self.use_tailscale = true;
        }
    }
}

impl State {
    /// The Setup tab's switch.
    pub fn set_remote(&mut self, on: bool) {
        self.settings.remote = on;
        self.sync_remote();
        self.changed();
    }

    /// A new key, so every phone that scanned the old code is signed out.
    pub fn forget_phones(&mut self) {
        match new_key() {
            Ok(key) => {
                self.settings.remote_key = key;
                self.changed();
            }
            Err(e) => self.notice = Some(format!("Couldn’t make a new code: {e}")),
        }
    }

    /// Starts or stops the server to match the setting. The server runs only while the app
    /// does; tests and snapshots get the card without one.
    pub fn sync_remote(&mut self) {
        if self.settings.remote && self.settings.remote_key.is_empty() {
            self.forget_phones();
        }
        let r = &mut self.remote;
        if !self.settings.remote || self.settings.remote_key.is_empty() {
            r.server = None;
            r.problem = self.settings.remote.then(|| {
                "Couldn’t make a code for your phone. Turn the remote off and on again.".into()
            });
            return;
        }
        r.find_addresses();
        if r.server.is_some() {
            return;
        }
        let Some(handle) = self.handle.clone().filter(|_| !self.headless) else {
            r.port = PORT;
            return;
        };
        let answer = on_ui_thread(handle);
        let any = |port| SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
        match listen(any(PORT), answer.clone()).or_else(|_| listen(any(0), answer)) {
            Ok(server) => {
                r.port = server.port;
                r.server = Some(server);
                r.problem = None;
            }
            Err(e) => r.problem = Some(format!("Couldn’t start the remote: {e}")),
        }
    }

    /// One phone request: its HTTP status, the JSON to send back, and whether the window
    /// shows something new.
    pub fn api(&mut self, method: &str, path: &str, key: &str, body: &[u8]) -> (u16, String, bool) {
        if !self.settings.remote {
            // Not 401: the phone's code still works once the remote is back on.
            return (503, error("The phone remote is off on your PC."), false);
        }
        if !same_key(key, &self.settings.remote_key) {
            return (401, error("Scan the code in Warble again."), false);
        }
        match (method, path) {
            ("GET", "/api/state") => {
                // The window's own polls stop while it's hidden; this keeps "playing" current.
                let changed = self.refresh_status();
                (200, self.remote_json(), changed)
            }
            ("POST", "/api/do") => {
                let done = serde_json::from_slice(body)
                    .map_err(|_| "Warble didn’t understand that. Reload the page.".to_string())
                    .and_then(|action| self.act(action));
                match done {
                    Ok(()) => (200, self.remote_json(), true),
                    Err(e) => (400, error(&e), false),
                }
            }
            _ => (404, error("Not found."), false),
        }
    }

    fn act(&mut self, action: Action) -> Result<(), String> {
        match action {
            Action::Play { id } => {
                let id = SoundId(id);
                if !self.sounds.iter().any(|s| s.id == id) {
                    return Err("That sound isn’t on the board any more.".into());
                }
                self.play(id);
            }
            Action::Stop => self.stop_sounds(),
            Action::Effect { on } => self.set_effect(on),
            Action::Preset { id } => {
                self.select_preset(Preset::from_id(&id).ok_or("Warble doesn’t have that voice.")?)
            }
            Action::MicGain { pos } => self.set_mic_gain(ui::pos_gain(pos.clamp(0.0, 1.0))),
            Action::SoundVolume { value } => self.set_sound_volume(value),
            Action::Monitor { on } => self.set_monitor(on),
            Action::MonitorVolume { value } => self.set_monitor_volume(value),
            Action::HearSounds { on } => self.set_hear_sounds(on),
            Action::Link { url } => self.link_from_phone(url.trim())?,
        }
        Ok(())
    }

    /// Opens the Find sounds page on the PC and gets the link's audio, ready to trim there. The
    /// window isn't brought up: on Windows that would take focus from a game.
    fn link_from_phone(&mut self, url: &str) -> Result<(), String> {
        if !youtube::looks_like_link(url) {
            return Err("Paste a link that starts with https://".into());
        }
        if self.trim.is_some() {
            return Err("Save or close the clip open on your PC first.".into());
        }
        if matches!(self.find.youtube, YouTube::Installing | YouTube::Fetching) {
            return Err("Warble is still getting the last link.".into());
        }
        self.open_find();
        self.find.link = TextEdit::new(url);
        self.get_audio();
        Ok(())
    }

    fn remote_json(&self) -> String {
        let s = &self.settings;
        let presets: Vec<_> = Preset::ALL
            .into_iter()
            .map(|p| json!({"id": p.id(), "name": p.name(), "icon": ui::preset_icon(p).body}))
            .collect();
        let sounds: Vec<_> = self
            .sounds
            .iter()
            .map(|x| json!({"id": x.id.0, "name": x.name, "playing": self.is_playing(x.id)}))
            .collect();
        let (link, says) = match &self.find.youtube {
            YouTube::Fetching => ("busy", "Getting the audio…"),
            YouTube::Installing => ("busy", "Downloading yt-dlp on your PC…"),
            YouTube::AskToInstall => (
                "pc",
                "Open Warble on your PC: it needs to download a free tool first.",
            ),
            YouTube::Failed(e) => ("failed", e.as_str()),
            YouTube::Idle if self.trim.is_some() => (
                "pc",
                "Ready on your PC. Open Warble there to trim and save it.",
            ),
            YouTube::Idle => ("idle", ""),
        };
        // The same words and colour as the window's status pill.
        let (status, tone) = if self.status.running {
            ("Live", "live")
        } else if self.cable_missing() {
            ("VB-CABLE missing", "warn")
        } else if let Some(e) = &self.status.error {
            (e.as_str(), "danger")
        } else {
            ("Starting…", "muted")
        };
        json!({
            "status": {"label": status, "tone": tone},
            "effect": s.effect,
            "preset": s.preset().id(),
            "presets": presets,
            "sounds": sounds,
            "sound_volume": s.sound_volume,
            "hear_sounds": s.hear_sounds,
            "mic_gain": ui::gain_pos(s.mic_gain),
            "mic_gain_label": format!("{:.0}%", s.mic_gain * 100.0),
            "monitor": s.monitor,
            "monitor_volume": s.monitor_volume,
            "link": {"state": link, "says": says},
        })
        .to_string()
    }
}

/// What a phone asks for, as `{"do": "play", "id": 3}` and so on.
#[derive(Debug, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
enum Action {
    Play {
        id: u32,
    },
    Stop,
    Effect {
        on: bool,
    },
    Preset {
        id: String,
    },
    /// The slider's position, 0..=1, as the window's mic gain slider has it.
    MicGain {
        pos: f32,
    },
    SoundVolume {
        value: f32,
    },
    Monitor {
        on: bool,
    },
    MonitorVolume {
        value: f32,
    },
    HearSounds {
        on: bool,
    },
    Link {
        url: String,
    },
}

fn error(why: &str) -> String {
    json!({ "error": why }).to_string()
}

/// Compares every byte, so how long a wrong key takes says nothing about the right one.
fn same_key(given: &str, key: &str) -> bool {
    !key.is_empty()
        && given.len() == key.len()
        && given
            .bytes()
            .zip(key.bytes())
            .fold(0, |d, (a, b)| d | (a ^ b))
            == 0
}

/// 128 random bits from the system, as 32 hex digits.
fn new_key() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    random(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
fn random(bytes: &mut [u8]) -> std::io::Result<()> {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    // SAFETY: the buffer is valid for its length; no algorithm handle with this flag.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "BCryptGenRandom failed ({status:#x})"
        )))
    }
}

#[cfg(not(windows))]
fn random(bytes: &mut [u8]) -> std::io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(bytes)
}

/// This PC's address on the route to `probe`.
fn local_ip(probe: &str) -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(probe).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// Tailscale hands out addresses in 100.64.0.0/10.
fn is_tailscale(ip: &Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

// --- The server ---

/// A request the page's script sent.
pub struct Request {
    pub method: String,
    pub path: String,
    /// The `X-Warble-Key` header.
    pub key: String,
    pub body: Vec<u8>,
}

/// Answers an API request with its HTTP status and JSON.
pub type Answer = Arc<dyn Fn(Request) -> (u16, String) + Send + Sync>;

/// Hands each request to the UI thread, where the state lives, and waits for its answer. A
/// request the phone was told failed (the UI thread was busy for 3 s) never runs late.
fn on_ui_thread(handle: AppHandle<State>) -> Answer {
    Arc::new(move |req: Request| {
        let (tx, rx) = mpsc::sync_channel(1);
        let taken = Arc::new(AtomicBool::new(false));
        let ours = taken.clone();
        handle.update_if(move |s, t| {
            if ours.swap(true, Ordering::AcqRel) {
                return false;
            }
            let (status, body, changed) = s.api(&req.method, &req.path, &req.key, &req.body);
            let _ = tx.send((status, body));
            if changed {
                // Saves are debounced through poll, which a hidden window otherwise skips.
                t.wake_after(Duration::from_millis(450));
            }
            changed
        });
        let busy = || (503, error("Warble is busy. Try again."));
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(answer) => answer,
            // It started meanwhile: wait for what it did rather than say it failed.
            Err(_) if taken.swap(true, Ordering::AcqRel) => rx.recv().unwrap_or_else(|_| busy()),
            Err(_) => busy(),
        }
    })
}

/// The listening socket's thread. Dropping it closes the port.
pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake the accept loop so it sees the flag and lets go of the port.
        let wake = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let _ = TcpStream::connect_timeout(&wake, Duration::from_millis(200));
    }
}

/// Listens on `addr`, reading each connection on its own thread.
pub fn listen(addr: SocketAddr, answer: Answer) -> std::io::Result<Server> {
    let listener = TcpListener::bind(addr)?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::Builder::new()
        .name("warble-remote".into())
        .spawn(move || {
            let open = Arc::new(AtomicUsize::new(0));
            for stream in listener.incoming() {
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                let Ok(stream) = stream else {
                    // Out of sockets, say: don't spin.
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                };
                let slot = Slot::take(&open);
                if open.load(Ordering::Acquire) > MAX_CONNECTIONS {
                    continue;
                }
                let answer = answer.clone();
                // A failed spawn drops the job, and the slot with it.
                let _ = std::thread::Builder::new().spawn(move || {
                    let _slot = slot;
                    serve(stream, &answer);
                });
            }
        })?;
    Ok(Server { port, stop })
}

/// One open connection, counted while it lives (also if its thread panics).
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(open: &Arc<AtomicUsize>) -> Slot {
        open.fetch_add(1, Ordering::AcqRel);
        Slot(open.clone())
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn serve(mut stream: TcpStream, answer: &Answer) {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    let reply = match read_request(&mut stream) {
        Ok(req) => route(req, answer),
        Err(status) => Reply::new(status, "text/plain", Vec::new()),
    };
    let _ = stream.write_all(&reply.bytes());
}

/// The page and its icons for anyone; the API through `answer`, which checks the key.
fn route(req: Request, answer: &Answer) -> Reply {
    let path = req.path.split(['?', '#']).next().unwrap_or_default();
    match (req.method.as_str(), path) {
        ("GET", "/") => Reply::new(200, "text/html; charset=utf-8", PAGE.into()),
        ("GET", "/icon.png") => Reply::new(200, "image/png", icon_png().to_vec()),
        (_, p) if p.starts_with("/api/") => {
            let (status, json) = answer(req);
            Reply::new(status, "application/json", json.into_bytes())
        }
        _ => Reply::new(404, "text/plain", b"Not found".to_vec()),
    }
}

/// Reads one request: its head and body, `MAX_REQUEST` bytes at most, within `TIMEOUT` all
/// told, so a sender can't hold a connection by trickling bytes. An error is the status to
/// answer with.
fn read_request(stream: &mut impl Read) -> Result<Request, u16> {
    let deadline = Instant::now() + TIMEOUT;
    let mut read = |chunk: &mut [u8]| -> Result<usize, u16> {
        match stream.read(chunk) {
            _ if Instant::now() > deadline => Err(408),
            Ok(0) => Err(400),
            Ok(n) => Ok(n),
            Err(_) => Err(408),
        }
    };
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let (head, len, mut req) = loop {
        let n = read(&mut chunk)?;
        buf.extend_from_slice(&chunk[..n]);
        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut parsed = httparse::Request::new(&mut headers);
        match parsed.parse(&buf) {
            Ok(httparse::Status::Complete(head)) => {
                let header = |name: &str| {
                    parsed
                        .headers
                        .iter()
                        .find(|h| h.name.eq_ignore_ascii_case(name))
                        .and_then(|h| std::str::from_utf8(h.value).ok())
                        .map(str::trim)
                };
                let len: usize = match header("content-length") {
                    Some(v) => v.parse().map_err(|_| 400u16)?,
                    None => 0,
                };
                let req = Request {
                    method: parsed.method.unwrap_or_default().to_string(),
                    path: parsed.path.unwrap_or_default().to_string(),
                    key: header("x-warble-key").unwrap_or_default().to_string(),
                    body: Vec::new(),
                };
                break (head, len, req);
            }
            Ok(httparse::Status::Partial) if buf.len() < MAX_REQUEST => {}
            _ => return Err(400),
        }
    };
    // Not `head + len`: a huge Content-Length would wrap round.
    if len > MAX_REQUEST - head {
        return Err(413);
    }
    let mut body = buf.split_off(head);
    while body.len() < len {
        let n = read(&mut chunk)?;
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    req.body = body;
    Ok(req)
}

struct Reply {
    status: u16,
    kind: &'static str,
    body: Vec<u8>,
}

impl Reply {
    fn new(status: u16, kind: &'static str, body: Vec<u8>) -> Reply {
        Reply { status, kind, body }
    }

    fn bytes(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            404 => "Not Found",
            408 => "Request Timeout",
            413 => "Content Too Large",
            _ => "Service Unavailable",
        };
        let head = format!(
            "HTTP/1.1 {} {reason}\r\n\
             Content-Type: {}\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-store\r\n\
             X-Content-Type-Options: nosniff\r\n\
             Referrer-Policy: no-referrer\r\n\
             Content-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; \
             style-src 'unsafe-inline'; frame-ancestors 'none'\r\n\
             Connection: close\r\n\r\n",
            self.status,
            self.kind,
            self.body.len()
        );
        [head.as_bytes(), &self.body].concat()
    }
}

/// Warble's icon as a PNG, for the phone's home screen (no web manifest: its start address
/// would drop the key from the link). Drawn once.
fn icon_png() -> &'static [u8] {
    const SIZE: u32 = 180;
    static PNG: OnceLock<Vec<u8>> = OnceLock::new();
    PNG.get_or_init(|| {
        let mut out = Vec::new();
        let mut png = png::Encoder::new(&mut out, SIZE, SIZE);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        let written = png
            .write_header()
            .and_then(|mut w| w.write_image_data(&crate::icon::rgba(SIZE)));
        if written.is_err() {
            out.clear();
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> State {
        let mut s = State::mock();
        s.set_remote(true);
        s
    }

    fn post(s: &mut State, body: &str) -> (u16, serde_json::Value) {
        let key = s.settings.remote_key.clone();
        let (status, json, _) = s.api("POST", "/api/do", &key, body.as_bytes());
        (status, serde_json::from_str(&json).unwrap())
    }

    #[test]
    fn a_new_key_is_128_random_bits_in_hex() {
        let (a, b) = (new_key().unwrap(), new_key().unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn only_the_key_opens_the_api() {
        let mut s = on();
        let key = s.settings.remote_key.clone();
        assert_eq!(s.api("GET", "/api/state", &key, b"").0, 200);
        assert_eq!(s.api("GET", "/api/state", "", b"").0, 401);
        let mut wrong = key.clone().into_bytes();
        wrong[31] = if wrong[31] == b'0' { b'1' } else { b'0' };
        assert_eq!(
            s.api(
                "GET",
                "/api/state",
                std::str::from_utf8(&wrong).unwrap(),
                b""
            )
            .0,
            401
        );
        s.forget_phones();
        assert_eq!(
            s.api("GET", "/api/state", &key, b"").0,
            401,
            "a new code signs phones out"
        );
        s.set_remote(false);
        let key = s.settings.remote_key.clone();
        // Off isn't a wrong code: the phone keeps its key for when it's back on.
        assert_eq!(s.api("GET", "/api/state", &key, b"").0, 503);
        assert!(!same_key("", ""), "no key never matches");
    }

    #[test]
    fn a_phone_plays_sounds_and_changes_the_voice() {
        let mut s = on();
        s.add_sounds(vec!["Air horn.mp3".into(), "Bruh.wav".into()]);
        let bruh = s.sounds.iter().find(|x| x.name == "Bruh").unwrap().id;

        let (status, json) = post(&mut s, &format!(r#"{{"do":"play","id":{}}}"#, bruh.0));
        assert_eq!(status, 200);
        assert!(s.is_playing(bruh));
        let tile = json["sounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "Bruh")
            .cloned();
        assert_eq!(
            tile.unwrap()["playing"],
            true,
            "the answer says it's playing"
        );

        post(&mut s, r#"{"do":"stop"}"#);
        assert!(s.status.playing.is_empty());
        assert_eq!(post(&mut s, r#"{"do":"play","id":999}"#).0, 400);

        post(&mut s, r#"{"do":"preset","id":"robot"}"#);
        assert_eq!(s.settings.preset(), Preset::Robot);
        let (_, json) = post(&mut s, r#"{"do":"effect","on":false}"#);
        assert!(!s.settings.effect);
        assert_eq!(json["effect"], false);
        assert_eq!(json["preset"], "robot");
        assert_eq!(json["presets"].as_array().unwrap().len(), 8);
        assert_eq!(post(&mut s, r#"{"do":"preset","id":"kazoo"}"#).0, 400);
        assert_eq!(post(&mut s, "not json").0, 400);
    }

    #[test]
    fn a_phone_sets_volumes_and_switches() {
        let mut s = on();
        post(&mut s, r#"{"do":"sound_volume","value":0.25}"#);
        post(&mut s, r#"{"do":"monitor","on":true}"#);
        post(&mut s, r#"{"do":"monitor_volume","value":7}"#);
        post(&mut s, r#"{"do":"hear_sounds","on":false}"#);
        let (_, json) = post(&mut s, r#"{"do":"mic_gain","pos":0.5}"#);
        let st = &s.settings;
        assert_eq!(
            (
                st.sound_volume,
                st.monitor,
                st.monitor_volume,
                st.hear_sounds
            ),
            (0.25, true, 1.0, false)
        );
        assert_eq!(st.mic_gain, 1.0, "the slider's middle is 100%");
        assert_eq!(json["mic_gain_label"], "100%");
        post(&mut s, r#"{"do":"mic_gain","pos":1e40}"#);
        assert_eq!(s.settings.mic_gain, 4.0);
    }

    #[test]
    fn a_link_from_the_phone_opens_on_the_pc() {
        let mut s = on();
        assert_eq!(post(&mut s, r#"{"do":"link","url":"not a link"}"#).0, 400);
        // Without yt-dlp in mock mode's folder, the PC asks before downloading it.
        let _ = std::fs::remove_file(youtube::tool_path(&s.work_dir()));
        let (status, json) = post(
            &mut s,
            r#"{"do":"link","url":"https://youtu.be/dQw4w9WgXcQ"}"#,
        );
        assert_eq!(status, 200);
        assert!(s.find.open);
        assert_eq!(s.find.link.text, "https://youtu.be/dQw4w9WgXcQ");
        assert_eq!(s.find.youtube, YouTube::AskToInstall);
        assert_eq!(json["link"]["state"], "pc");
        s.trim = Some(crate::trim::Trim::new(warble_engine::Clip::new(
            vec![0.0; 800],
            8000,
        )));
        assert_eq!(
            post(&mut s, r#"{"do":"link","url":"https://youtu.be/x1"}"#).0,
            400,
            "an open clip isn't replaced"
        );
    }

    fn request(raw: &[u8]) -> Result<Request, u16> {
        read_request(&mut std::io::Cursor::new(raw.to_vec()))
    }

    #[test]
    fn requests_are_read_with_their_key_and_body() {
        let req = request(
            b"POST /api/do HTTP/1.1\r\nHost: 192.168.1.5\r\nX-Warble-Key: abc\r\nContent-Length: 13\r\n\r\n{\"do\":\"stop\"}",
        )
        .unwrap();
        assert_eq!(
            (req.method.as_str(), req.path.as_str(), req.key.as_str()),
            ("POST", "/api/do", "abc")
        );
        assert_eq!(req.body, br#"{"do":"stop"}"#);
        assert_eq!(request(b"GET / HTTP/1.1\r\n\r\n").unwrap().path, "/");
        assert_eq!(request(b"nonsense\r\n\r\n").err(), Some(400));
        assert_eq!(
            request(b"GET / HTTP/1.1\r\nHost: x\r\n").err(),
            Some(400),
            "cut off"
        );
        let big = format!(
            "POST /api/do HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_REQUEST
        );
        assert_eq!(request(big.as_bytes()).err(), Some(413));
        let wraps = format!(
            "POST /api/do HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            usize::MAX
        );
        assert_eq!(
            request(wraps.as_bytes()).err(),
            Some(413),
            "a length that would wrap round is still too long"
        );
        let endless = [b"GET / HTTP/1.1\r\nX: ".as_slice(), &[b'a'; MAX_REQUEST]].concat();
        assert_eq!(request(&endless).err(), Some(400));
    }

    #[test]
    fn tailscale_addresses_are_told_apart() {
        assert!(is_tailscale(&Ipv4Addr::new(100, 101, 2, 3)));
        assert!(!is_tailscale(&Ipv4Addr::new(100, 128, 0, 1)));
        assert!(!is_tailscale(&Ipv4Addr::new(192, 168, 1, 20)));
        let (wifi, tailscale) = (
            Ipv4Addr::new(192, 168, 1, 20),
            Ipv4Addr::new(100, 101, 2, 3),
        );
        let mut r = Remote {
            port: PORT,
            ..Default::default()
        };
        r.set_addresses(Some(wifi), Some(tailscale));
        assert_eq!(r.address().unwrap(), "http://192.168.1.20:47823");
        r.use_tailscale = true;
        assert_eq!(r.address().unwrap(), "http://100.101.2.3:47823");
        r.set_addresses(Some(wifi), None);
        assert_eq!(
            r.address().unwrap(),
            "http://192.168.1.20:47823",
            "Tailscale went off"
        );
        // A Tailscale exit node takes the route to the internet: Tailscale is the only way.
        r.set_addresses(None, Some(tailscale));
        assert_eq!(r.address().unwrap(), "http://100.101.2.3:47823");
        r.set_addresses(None, None);
        assert_eq!(r.address(), None);
    }

    #[test]
    fn a_saved_remote_without_a_key_gets_one() {
        let mut s = State::mock();
        s.settings.remote = true;
        s.sync_remote();
        assert_eq!(s.settings.remote_key.len(), 32);
        assert_eq!(s.remote.problem, None);
    }

    /// The real server, answered by a mock app on its own thread as the UI thread would.
    /// Returns the server, the key, and the first sound's id.
    fn mock_server(addr: SocketAddr, sounds: &'static [&'static str]) -> (Server, String, u32) {
        let (to_app, requests) = mpsc::channel::<(Request, mpsc::Sender<(u16, String)>)>();
        let (key_tx, key_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut s = on();
            s.add_sounds(sounds.iter().map(Into::into).collect());
            key_tx
                .send((s.settings.remote_key.clone(), s.sounds[0].id.0))
                .unwrap();
            for (req, reply) in requests {
                let (status, json, _) = s.api(&req.method, &req.path, &req.key, &req.body);
                let _ = reply.send((status, json));
            }
        });
        let (key, id) = key_rx.recv().unwrap();
        let to_app = std::sync::Mutex::new(to_app);
        let answer: Answer = Arc::new(move |req| {
            let (tx, rx) = mpsc::channel();
            to_app.lock().unwrap().send((req, tx)).unwrap();
            rx.recv().unwrap()
        });
        (listen(addr, answer).unwrap(), key, id)
    }

    /// For a look at the phone page: serves a mock board on localhost for a few minutes.
    /// `cargo test -p warble serve_a_mock_board -- --ignored --nocapture`, then open the link.
    #[test]
    #[ignore]
    fn serve_a_mock_board() {
        let sounds = &[
            "Air horn.mp3",
            "Ba dum tss.wav",
            "Bruh.wav",
            "Crowd goes absolutely wild after the last-second winning goal.mp3",
            "Sad trombone.ogg",
            "Victory fanfare (extended stadium remix).mp3",
            "Wilhelm scream.flac",
        ];
        let (server, key, _) = mock_server((Ipv4Addr::LOCALHOST, PORT).into(), sounds);
        println!("http://127.0.0.1:{}/#k={key}", server.port);
        std::thread::sleep(Duration::from_secs(600));
    }

    /// The page, a wrong key, and a sound played, over real HTTP.
    #[test]
    fn the_server_serves_the_page_and_the_api() {
        let (server, key, id) = mock_server((Ipv4Addr::LOCALHOST, 0).into(), &["Air horn.mp3"]);
        let base = format!("http://127.0.0.1:{}", server.port);

        let page = ureq::get(&base)
            .call()
            .unwrap()
            .into_body()
            .read_to_string()
            .unwrap();
        assert!(page.contains("<title>Warble</title>"));
        let icon = ureq::get(format!("{base}/icon.png"))
            .call()
            .unwrap()
            .into_body()
            .read_to_vec()
            .unwrap();
        assert!(icon.starts_with(b"\x89PNG"));

        let denied = ureq::get(format!("{base}/api/state"))
            .header("X-Warble-Key", "nope")
            .call();
        assert!(matches!(denied, Err(ureq::Error::StatusCode(401))));

        let played = ureq::post(format!("{base}/api/do"))
            .header("X-Warble-Key", &key)
            .send(format!(r#"{{"do":"play","id":{id}}}"#))
            .unwrap()
            .into_body()
            .read_to_string()
            .unwrap();
        let played: serde_json::Value = serde_json::from_str(&played).unwrap();
        assert_eq!(played["sounds"][0]["playing"], true);

        let port = server.port;
        drop(server);
        let freed = (0..40).any(|_| {
            std::thread::sleep(Duration::from_millis(25));
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
        });
        assert!(freed, "turning it off frees the port");
    }
}
