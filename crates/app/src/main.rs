//! Warble: a voice changer and soundboard for Discord and game chat. `WARBLE_MOCK=1` runs it
//! on the mock engine and keeps it away from the real settings folder.

// No console window behind the app in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod icon;
mod settings;
mod state;
mod ui;

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
        poll: Some(State::poll),
        ..AppSpec::new("Warble", (400.0, 620.0), State::mock, ui::view)
    });
    let handle = app.handle();
    let for_state = handle.clone();
    #[allow(unused_mut)]
    let mut app = app
        .init(move || {
            let mut s = if mock { State::mock() } else { State::load() };
            s.headless = headless;
            if !headless {
                s.tray = make_tray(for_state.clone());
            }
            s.handle = Some(for_state);
            s.sync_tray();
            s.load_saved_sounds();
            s
        })
        .keep_running()
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

fn why(e: &ShortcutError) -> String {
    match e {
        ShortcutError::Conflict => "taken by another app".into(),
        ShortcutError::Unsupported => "this desktop doesn’t allow global hotkeys".into(),
        ShortcutError::PermissionDenied => "permission was declined".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests;
