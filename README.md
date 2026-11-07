# Warble

A lightweight voice changer and soundboard for Discord calls and game chat on Windows.
Built with Rust and [Rune](https://github.com/anomalyco/rune).

- **Voice effects:** Normal, Deep, Chipmunk, Robot, Radio, Echo, Demon, Cave.
- **Soundboard:** drop in WAV, MP3, OGG or FLAC clips and play them into the call.
- **Find sounds:** search Myinstants and Freesound from the *Soundboard* tab, preview a result in
  your headphones only, and add it. Freesound needs a free API key (<https://freesound.org/apiv2/apply>),
  pasted once on that page; without one, Warble searches Myinstants only.
- **From YouTube:** paste a link, then trim out the part you want (or keep it all) and save it as a sound.
  The first time, Warble downloads [yt-dlp](https://github.com/yt-dlp/yt-dlp) from its GitHub releases
  and lets it update itself once a day after that.
- **Trim:** any tile's scissors button, or a search result's, opens the same trim editor.
- **Hear yourself:** listen to your changed voice in your headphones.
- **Hear sounds:** soundboard clips play in your headphones too (on by default; switch it off on the *Soundboard* tab).
- **Hotkeys** that work while a game has focus, and a tray icon so it keeps running when the window is closed.

## Set it up (once per PC)

1. **Install VB-CABLE**, a free virtual audio cable: <https://vb-audio.com/Cable/>.
   Unzip it, right-click `VBCABLE_Setup_x64.exe` → *Run as administrator* → *Install Driver*, then restart the PC.
   Warble sends your changed voice into this cable. Discord and games then hear the cable as a microphone.
2. **Install Warble.** Put `warble.exe`, `warble.ico` and `install.ps1` in one folder, right-click
   `install.ps1` → *Run with PowerShell*. It copies Warble to `%LOCALAPPDATA%\Programs\Warble` and adds it
   to the Start menu, so you can open it by typing "Warble" in Start, PowerToys Run or Raycast.
   Windows may say "Windows protected your PC" the first time because the app isn't signed.
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

## Where things live

- Settings: `%APPDATA%\Warble\settings.json`
- Sounds: `%APPDATA%\Warble\sounds\` (Warble keeps its own copies, so your original files are never touched)
- yt-dlp: `%APPDATA%\Warble\bin\yt-dlp.exe`, downloaded on first use. Delete it to have Warble fetch a fresh copy.

Closing the window keeps Warble running in the tray. Opening it again (from Start or a launcher) brings
that window back rather than starting a second copy. To quit, right-click the tray icon → *Quit Warble*.

To update, quit Warble and run `install.ps1` again with the new `warble.exe`.

## Develop

```sh
cargo test --workspace
cargo run -p warble                                       # the app
cargo run -p warble-engine --example selftest -- 10       # engine only: lists devices, runs 10 s, plays nothing
WARBLE_MOCK=1 cargo run -p warble -- --snap snaps/voice.png --scene voice   # headless screenshot, fake engine
```

Scenes: `voice`, `voice-off`, `soundboard`, `soundboard-empty`, `setup`, `no-cable`, `hotkeys-failed`, `drop`, `load-error`,
`find-start`, `find`, `find-offline`, `youtube-install`, `trim`.

To check that the code compiles for Windows from Linux (check only; release builds happen on Windows):

```sh
rustup target add x86_64-pc-windows-msvc
CARGO_FEATURE_PURE=1 cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
```

`CARGO_FEATURE_PURE=1` makes `blake3` (a Rune dependency) skip its MSVC-only assembly.

To install a release build from the repo on Windows: `cargo build --release -p warble`, then
`powershell -ExecutionPolicy Bypass -File scripts\install.ps1`.
The shortcut's icon is `crates/app/assets/warble.ico`, generated from `icon.rs`; after changing the
drawing, regenerate it with `WARBLE_WRITE_ICO=1 cargo test -p warble`.

Layout: `crates/dsp` (voice effects), `crates/engine` (devices, audio graph, soundboard), `crates/app` (the Rune UI).
Rune is pre-release and pinned to one commit in the root `Cargo.toml`. Bump it on purpose, not casually.
