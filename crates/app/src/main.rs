//! Warble: a voice changer and soundboard for Discord and game chat. `WARBLE_MOCK=1` runs it
//! on the mock engine and keeps it away from the real settings folder.

// No console window behind the app in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod find;
mod find_view;
mod icon;
mod library;
mod ptt;
mod remote;
mod remote_view;
mod settings;
mod state;
mod trim;
mod ui;
mod update;
mod youtube;

use rune::tray::{Tray, TrayEvent, TrayIcon, TraySpec};
use rune::{App, AppHandle, AppSpec, Shortcut, ShortcutError, Tx};
use state::State;
use warble_engine::Preset;

fn main() {
    let headless = rune::is_headless();
    let mock = headless || std::env::var("WARBLE_MOCK").is_ok_and(|v| v == "1");
    let app = App::new(AppSpec {
        min_size: Some((360.0, 540.0)),
        scenes: ui::SCENES,
        fonts: ui::FONTS,
        poll: Some(State::poll),
        // While a push-to-talk key is being chosen, keys go to that; otherwise Escape leaves
        // Find sounds or Trim even when nothing on the page has focus.
        on_key: Some(|s, ev, tx| {
            ui::while_choosing_a_key(s, ev, tx) || find_view::escape_goes_back(s, ev, tx)
        }),
        ..AppSpec::new("Warble", (400.0, 620.0), State::mock, ui::view)
    });
    let handle = app.handle();
    #[cfg(windows)]
    if !mock && !first_copy(&handle) {
        return;
    }
    // The tray icon is added before the event loop starts, because Rune starts its GPU probe
    // with the loop. Adding it while NVIDIA's driver loads deadlocks on Windows: the driver's
    // DllMain asks the shell for a known folder, while Shell_NotifyIconW holds that folder lock
    // and waits for the loader. Once added, the shell has what it needs cached.
    let tray = if headless {
        None
    } else {
        make_tray(handle.clone())
    };
    let for_state = handle.clone();
    #[allow(unused_mut)]
    let mut app = app
        .init(move || {
            let mut s = if mock { State::mock() } else { State::load() };
            s.headless = headless;
            s.tray = tray;
            s.handle = Some(for_state);
            s.sync_tray();
            s.load_saved_sounds();
            s.sync_remote();
            s.sync_ptt();
            s.watch_for_updates();
            s
        })
        .keep_running()
        .on_focus(ui::window_focus)
        .on_close(|s: &mut State, tx: &mut Tx| {
            s.save_now();
            // With a tray icon, closing only hides the window; without one there'd be no way back.
            if s.tray.is_none() {
                tx.quit();
            }
        });
    #[cfg(windows)]
    {
        app = app.on_window(icon::set_window_icon);
    }
    // Shortcuts register once the app runs; this runs first thing on its thread. Snapshots
    // register none.
    if !headless {
        handle.update(|s, _| register_hotkeys(s));
    }
    app.run();
}

fn make_tray(handle: AppHandle<State>) -> Option<Tray> {
    let size = 32;
    let spec = TraySpec::new(
        "warble",
        "Warble",
        TrayIcon::rgba(size, size, icon::rgba(size)),
    )
    .tooltip(state::tray_tooltip(true, Preset::Normal))
    .menu(state::tray_menu(true));
    let on_event = move |event: TrayEvent| match event {
        TrayEvent::Activate { .. } if handle.is_visible() => handle.hide(),
        TrayEvent::Activate { .. } => show(&handle),
        TrayEvent::Menu(id) => match id.as_str() {
            "show" => show(&handle),
            "effect" => handle.update(|s, tx| {
                s.set_effect(!s.settings.effect);
                // The window may be hidden, so nothing else would run the debounced save.
                tx.wake_after(std::time::Duration::from_millis(450));
            }),
            "quit" => {
                let h = handle.clone();
                handle.update(move |s, _| {
                    s.save_now();
                    h.quit();
                });
            }
            _ => {}
        },
        _ => {}
    };
    match Tray::new(spec, on_event) {
        Ok(tray) => Some(tray),
        Err(e) => {
            eprintln!("warble: no tray icon: {e}");
            None
        }
    }
}

fn show(handle: &AppHandle<State>) {
    handle.open();
    handle.wake();
}

/// One Warble at a time. Launching it again (from Start, a launcher or a shortcut) shows the
/// running copy's window instead of starting a second engine; this returns false in that
/// second copy, which then exits.
#[cfg(windows)]
fn first_copy(handle: &AppHandle<State>) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::{
        CreateEventW, INFINITE, SetEvent, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

    let name: Vec<u16> = "Local\\Warble.Show\0".encode_utf16().collect();
    // SAFETY: a valid NUL-terminated name and no security attributes. The event lives as long
    // as the process.
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
    if event.is_null() {
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // We were just launched, so we may bring a window forward; let the first copy do it.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            SetEvent(event);
        }
        return false;
    }
    let handle = handle.clone();
    let event = event as usize; // a HANDLE isn't Send, but it's just a number to the kernel
    std::thread::spawn(move || {
        while unsafe { WaitForSingleObject(event as _, INFINITE) } == 0 {
            show(&handle);
        }
    });
    true
}

type Action = Box<dyn Fn(&mut State) + Send + Sync>;

/// Registers every global hotkey, keeping a plain reason for each one that fails.
fn register_hotkeys(s: &mut State) {
    let Some(handle) = s.handle.clone() else {
        return;
    };
    let mut keys: Vec<(String, Action)> = vec![
        (
            state::EFFECT_KEY.into(),
            Box::new(|s: &mut State| s.set_effect(!s.settings.effect)),
        ),
        (
            state::MONITOR_KEY.into(),
            Box::new(|s: &mut State| s.set_monitor(!s.settings.monitor)),
        ),
        (state::STOP_KEY.into(), Box::new(State::stop_sounds)),
    ];
    for (i, p) in Preset::ALL.into_iter().enumerate() {
        keys.push((
            state::preset_key(i),
            Box::new(move |s: &mut State| s.select_preset(p)),
        ));
    }
    for i in 0..9 {
        keys.push((
            state::sound_key(i),
            Box::new(move |s: &mut State| s.play_index(i)),
        ));
    }
    s.hotkey_failures.clear();
    for (chord, action) in keys {
        let result = chord.parse::<Shortcut>().and_then(|shortcut| {
            handle.register_shortcut(shortcut, move |s, tx| {
                action(s);
                // Saves are debounced through poll, which a hidden window otherwise skips.
                tx.wake_after(std::time::Duration::from_millis(450));
            })
        });
        if let Err(e) = result {
            s.hotkey_failures.push((chord, why(&e)));
        }
    }
}

/// What went wrong and what to do, as the Setup tab says it. `{keys}` stands for the hotkeys
/// that failed for this reason.
fn why(e: &ShortcutError) -> String {
    match e {
        ShortcutError::Conflict => "Another app is using {keys}. Close that app, then quit Warble \
                                    from the tray and open it again."
            .into(),
        ShortcutError::Unsupported => {
            "This desktop doesn’t let apps use hotkeys. Use Warble’s window instead.".into()
        }
        ShortcutError::PermissionDenied => "Warble wasn’t allowed to use {keys}. Quit Warble from \
                                            the tray, open it again, and allow its hotkeys."
            .into(),
        other => format!(
            "Warble couldn’t set up {{keys}} ({other}). Quit Warble from the tray and open it again."
        ),
    }
}

#[cfg(test)]
mod tests;
