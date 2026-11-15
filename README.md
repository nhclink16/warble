# Warble

A lightweight voice changer and soundboard for Discord calls and game chat on Windows.
Built with Rust and [Rune](https://github.com/anomalyco/rune).

- **Voice effects:** Normal, Deep, Chipmunk, Robot, Radio, Echo, Demon, Cave. Sounds play clean unless you
  turn on *Put the effect on sounds too* on the *Voice* tab.
- **Soundboard:** drop in WAV, MP3, OGG or FLAC sounds and play them into the call.
- **Find sounds:** search Myinstants and Freesound from the *Soundboard* tab, preview a result in
  your headphones only, and add it. Freesound needs a free API key (<https://freesound.org/apiv2/apply>),
  pasted once on that page; without one, Warble searches Myinstants only.
- **From YouTube, X or Instagram:** paste a link to a video, an X post or an Instagram reel, then trim out
  the part you want (or keep it all) and save it as a sound. Posts that need a signed-in X or Instagram
  account can't be fetched.
  The first time, Warble downloads [yt-dlp](https://github.com/yt-dlp/yt-dlp) from its GitHub releases
  and lets it update itself once a day after that.
- **Trim:** any tile's scissors button, or a search result's, opens the same trim editor. So does dropping a
  video file (MP4, MOV, M4V) or an M4A on the window.
- **Push to talk:** your mic goes into the call only while you hold a key or mouse button you choose,
  and sounds always go through. See [Push to talk](#push-to-talk).
- **Hear yourself:** listen to your changed voice in your headphones.
- **Hear sounds:** soundboard sounds play in your headphones too (on by default; switch it off on the *Soundboard* tab).
- **Phone remote:** play sounds, change your voice and set the volumes from your phone's browser.
  Turn it on under *Setup → Phone remote* and scan the QR code. See [Phone remote](#phone-remote).
- **Hotkeys** that work while a game has focus, and a tray icon so it keeps running when the window is closed.

## Set it up (once per PC)

1. **Install VB-CABLE**, a free virtual audio cable: <https://vb-audio.com/Cable/>.
   Unzip it, right-click `VBCABLE_Setup_x64.exe` → *Run as administrator* → *Install Driver*, then restart the PC.
   Warble sends your changed voice into this cable. Discord and games then hear the cable as a microphone.
2. **Install Warble.** Download `Warble-Setup-<version>.exe` from the latest release
   (<https://github.com/nhclink16/warble/releases/latest>), run it and click *Install*. It puts Warble in
   `%LOCALAPPDATA%\Programs\Warble` and the Start menu, so you can open it by typing "Warble" in Start,
   PowerToys Run or Raycast. It needs no admin rights.
   Windows may say "Windows protected your PC" the first time because the installer isn't signed.
   Click *More info* → *Run anyway*.
3. **In Discord:** Settings → Voice & Video →
   - Input device: **CABLE Output (VB-Audio Virtual Cable)**.
   - Turn **off** Noise suppression (Krisp) and Echo cancellation. Otherwise Discord strips the effects out.
4. **In games:** pick **CABLE Output (VB-Audio Virtual Cable)** as the microphone in the game's voice chat settings.

Warble picks up your normal mic and VB-CABLE by itself. You can change either one on the *Setup* tab.

## Hotkeys

| Keys | Does |
|---|---|
| Ctrl+Alt+V | Voice effect on/off |
| Ctrl+Alt+1 … 8 | Pick a voice (in the order shown) |
| Ctrl+Alt+M | Hear yourself on/off |
| Ctrl+Shift+1 … 9 | Play the first nine sounds |
| Ctrl+Alt+S | Stop all sounds |

If another app already uses one of these keys, the *Setup* tab says so.

## Push to talk

With Warble's own push-to-talk key, a sound from the board plays in game chat without you holding the
game's push-to-talk key as well.

1. On the *Voice* tab, turn on *Push to talk*, then press the key or mouse button you want to hold.
   Side buttons (Mouse 4 and 5) and the middle button work. Left and right click don't.
2. In Discord or your game, turn off push to talk: choose **Voice Activity** in Discord, or **open mic**
   in the game's voice chat settings.

Your mic then goes into the call only while you hold the key, and for a moment after you let go so your
last word isn't cut off. Sounds always go through. The *Talking* light on the *Voice* tab shows when your
mic is going out. *Change key* picks another key; *Esc* cancels.

Warble only reads whether that one key is down, about 100 times a second, even while the window is
hidden or a game has focus. It never presses, sends or intercepts a key, so the game gets every key as
usual. If your game runs as administrator, run Warble as administrator too, or it can't see your key.

## Phone remote

Turn on *Setup → Phone remote*, then scan the QR code with your phone's camera. Your phone needs to be on
the same Wi-Fi as the PC (or have Tailscale on; switch the code to *Tailscale* when it shows). The page
works in any phone browser. *Add to Home Screen* keeps it one tap away.

- **Windows asks once** whether Warble may use the network: choose *Allow*. If the phone can't connect,
  check that the PC's Wi-Fi is set to *Private* in Windows settings, and that Warble is allowed in
  *Windows Security → Firewall & network protection → Allow an app through firewall*.
- **The code holds a secret.** Only phones that scanned it can use the remote. *Sign out all phones* makes a new code, so each phone has to scan again.
- **Sending a link** from the phone opens it on the PC, in the trim editor, where you trim it and add it.
- Warble listens on port 47823 (or a free port, when another app has that one), over plain HTTP on your
  own network. Use it on your home Wi-Fi or over Tailscale, not on shared or public Wi-Fi, where another
  device could read the code off the network.
- **A VPN on the PC** that sends all traffic through it can put its own address in the QR code, which the
  phone can't reach. Turn the VPN off, or use Tailscale.

## Where things live

- Settings: `%APPDATA%\Warble\settings.json`
- Sounds: `%APPDATA%\Warble\sounds\` (Warble keeps its own copies, so your original files are never touched)
- yt-dlp: `%APPDATA%\Warble\bin\yt-dlp.exe`, downloaded on first use. Delete it to have Warble fetch a fresh copy.

Closing the window keeps Warble running in the tray. Opening it again (from Start or a launcher) brings
that window back rather than starting a second copy. To quit, right-click the tray icon → *Quit Warble*.

Warble updates itself. A little after it starts, and once a day while it runs, it asks this repo's
GitHub releases for a newer version. When there is one, a line above every page offers it: *Update*
downloads that release's installer, closes Warble, installs the new one and opens it. Your sounds and
settings stay. The check sends nothing about you or your sounds.
You can also update by hand: run a newer `Warble-Setup` file.

To remove Warble: Settings → Apps → Warble → *Uninstall*. That also keeps your sounds and settings
(in `%APPDATA%\Warble`), in case you come back.

## Develop

```sh
cargo test --workspace
cargo run -p warble                                       # the app
cargo run -p warble-engine --example selftest -- 10       # engine only: lists devices, runs 10 s, plays nothing
WARBLE_MOCK=1 cargo run -p warble -- --snap snaps/voice.png --scene voice   # headless screenshot, fake engine
```

Scenes (`--list-scenes` prints them):

- Voice: `voice` (the effect on), `voice-off`, `voice-sounds` (the effect on sounds too), `ptt` (push to talk on, its key up), `ptt-talking` (the key
  held), `ptt-choose` (just turned on, waiting for a key). Push to talk is below the fold at 400x620; add
  `--size 400x1000` to see it.
- Soundboard: `soundboard` (one sound playing), `soundboard-empty` (nine empty sockets), `removed` (just after
  removing a sound, with Undo), `undo-failed` (an Undo that couldn't put the file back, the Undo still under it),
  `load-error` (three files that didn't load), `drop` (files dragged over the window).
- Find sounds and Trim: `find-start`, `find`, `find-offline`, `youtube-install`, `trim`.
- Setup: `setup`, `no-cable`, `hotkeys-failed`, `audio-problem` (the microphone can't be opened), `remote`,
  `remote-tailscale`, `remote-no-network`.
- Updates: `update` (a newer Warble on offer), `update-getting` (downloading it), `update-failed`.

The window's smallest size is 360x540 (`--size 360x540`). Setup is taller than the window; add
`--size 400x1150` to see all of the remote's card.

To check a published release: `cargo test -p warble the_live_release -- --ignored`.

To try the phone page on the mock engine: `cargo test -p warble serve_a_mock_board -- --ignored --nocapture`,
then open the link it prints.

To check that the code compiles for Windows from Linux (check only; release builds happen on Windows):

```sh
rustup target add x86_64-pc-windows-msvc
CARGO_FEATURE_PURE=1 cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
```

`CARGO_FEATURE_PURE=1` makes `blake3` (a Rune dependency) skip its MSVC-only assembly.

To install a release build from the repo on Windows: `cargo build --release -p warble`, then
`powershell -ExecutionPolicy Bypass -File scripts\install.ps1`.
To make the installer you send people, install NSIS 3 (`makensis`). On Windows, after the release build:
`makensis installer\warble.nsi`, which writes `target\Warble-Setup-<version>.exe` (about 7 MB). On Linux,
with a `warble.exe` built on Windows: `makensis -DEXE=/path/to/warble.exe -DOUT=/path/to/Warble-Setup.exe
installer/warble.nsi`. The installer holds only `warble.exe` and its icon: never your sounds, settings or
keys, which live in `%APPDATA%\Warble`. The version comes from `Cargo.toml`.

To release: bump `version` in the root `Cargo.toml`, build the installer, then publish a GitHub release
tagged `v<version>` (for example `v0.2.0`) with `Warble-Setup-<version>.exe` attached under exactly that
name. Running copies offer it within a day. Drafts and pre-releases are never offered.
The shortcut's icon is `crates/app/assets/warble.ico`, generated from `icon.rs`; after changing the
drawing, regenerate it with `WARBLE_WRITE_ICO=1 cargo test -p warble`.

Layout: `crates/dsp` (voice effects), `crates/engine` (devices, audio graph, soundboard), `crates/app` (the Rune UI).
Rune is pre-release and pinned to one commit in the root `Cargo.toml`. Bump it on purpose, not casually.

Fonts: [Inter](https://rsms.me/inter/) and [Martian Mono](https://github.com/evilmartians/mono), both under the
SIL Open Font License 1.1, are bundled into the app. Their licences are in `crates/app/assets/fonts/`.

## License

Warble is under the MIT License: see [LICENSE](LICENSE).
