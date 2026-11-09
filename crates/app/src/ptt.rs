//! Warble's own push-to-talk key. Warble only reads whether one key is down: on Windows it asks
//! `GetAsyncKeyState` about 100 times a second, on a thread of its own. It never presses, sends
//! or hooks a key, which is what anti-cheat looks for. The thread reads nothing unless push to
//! talk is on or a key is being chosen. It tells the engine first, so the mic opens within a
//! read whatever the window is doing (hidden, or behind a game), then the window.
//!
//! Elsewhere no thread reads keys. Tests press and release the key with `State::key_held` and
//! `State::key_pressed`, as the thread does.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use rune::AppHandle;
use warble_engine::Engine;

use crate::state::State;

/// A Windows virtual-key code: a key or a mouse button.
pub type Vk = u16;

pub const MIDDLE_MOUSE: Vk = 0x04;
/// The side buttons: back (X1) and forward (X2).
pub const MOUSE_4: Vk = 0x05;
pub const MOUSE_5: Vk = 0x06;
pub const ESCAPE: Vk = 0x1B;

/// The name Warble shows for a key it takes, or `None` for one it doesn't: the left and right
/// mouse buttons (they work the window), Escape (it cancels), the Windows keys, and codes with
/// no key. Shift, Ctrl and Alt are taken by side, as "Left Ctrl". Punctuation is named as on a
/// US keyboard.
pub fn name(vk: Vk) -> Option<String> {
    let named = match vk {
        MIDDLE_MOUSE => "Middle mouse",
        MOUSE_4 => "Mouse 4",
        MOUSE_5 => "Mouse 5",
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x13 => "Pause",
        0x14 => "Caps Lock",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left arrow",
        0x26 => "Up arrow",
        0x27 => "Right arrow",
        0x28 => "Down arrow",
        0x2D => "Insert",
        0x2E => "Delete",
        0x30..=0x39 | 0x41..=0x5A => return Some(char::from(vk as u8).to_string()),
        0x60..=0x69 => return Some(format!("Num {}", vk - 0x60)),
        0x6A => "Num *",
        0x6B => "Num +",
        0x6D => "Num -",
        0x6E => "Num .",
        0x6F => "Num /",
        0x70..=0x87 => return Some(format!("F{}", vk - 0x6F)),
        0x90 => "Num Lock",
        0x91 => "Scroll Lock",
        0xA0 => "Left Shift",
        0xA1 => "Right Shift",
        0xA2 => "Left Ctrl",
        0xA3 => "Right Ctrl",
        0xA4 => "Left Alt",
        0xA5 => "Right Alt",
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => return None,
    };
    Some(named.to_string())
}

/// What a key or button pressed while one is being chosen does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// It becomes the push-to-talk key.
    Key(Vk),
    /// Escape: stop choosing.
    Cancel,
    /// Not one Warble takes (see [`name`]): waiting goes on.
    Skip,
}

pub fn pick(vk: Vk) -> Pick {
    if vk == ESCAPE {
        Pick::Cancel
    } else if name(vk).is_some() {
        Pick::Key(vk)
    } else {
        Pick::Skip
    }
}

/// What the key reader watches for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Watch {
    #[default]
    Nothing,
    /// The push-to-talk key, going down and coming up.
    Key(Vk),
    /// The first key or button to go down, while one is being chosen.
    Choosing,
}

/// The window's end of the key reader.
#[derive(Default)]
pub struct Reader {
    shared: Arc<Shared>,
    #[cfg(windows)]
    started: bool,
}

#[derive(Default)]
struct Shared {
    watch: Mutex<Watch>,
    changed: Condvar,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Reader {
    pub fn watch(&self) -> Watch {
        *lock(&self.shared.watch)
    }

    /// Watch for `watch` from now on. The first time there's something to watch, in a real
    /// window on Windows (`handle`), this starts the thread that reads keys.
    pub fn set(&mut self, watch: Watch, engine: &Arc<Engine>, handle: Option<&AppHandle<State>>) {
        *lock(&self.shared.watch) = watch;
        self.shared.changed.notify_all();
        #[cfg(windows)]
        if !self.started
            && watch != Watch::Nothing
            && let Some(handle) = handle
        {
            self.started = true;
            let (shared, engine, handle) = (self.shared.clone(), engine.clone(), handle.clone());
            std::thread::spawn(move || windows::read(&shared, &engine, &handle));
        }
        #[cfg(not(windows))]
        let _ = (engine, handle);
    }
}

#[cfg(windows)]
mod windows {
    use std::time::Duration;

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    use super::*;

    /// About 100 reads a second: the mic opens within 10 ms of the key going down.
    const READ_EVERY: Duration = Duration::from_millis(10);

    fn down(vk: Vk) -> bool {
        // SAFETY: it only reads a key's state, and takes any code. The top bit set (a negative
        // number) means the key is down now.
        unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
    }

    /// Reads keys for as long as Warble runs, waiting while there's nothing to watch.
    pub fn read(shared: &Shared, engine: &Engine, handle: &AppHandle<State>) {
        let (mut seen, mut held) = (Watch::Nothing, false);
        let mut was_down = [false; 256];
        loop {
            let watch = *lock(&shared.watch);
            if watch != seen {
                seen = watch;
                // A key held belonged to the last watch.
                if held {
                    held = false;
                    engine.set_talking(false);
                }
                // A key already down as choosing starts (the Enter that pressed Change key)
                // doesn't count.
                if watch == Watch::Choosing {
                    for (vk, was) in was_down.iter_mut().enumerate() {
                        *was = down(vk as Vk);
                    }
                }
            }
            match watch {
                Watch::Nothing => {
                    let idle = lock(&shared.watch);
                    drop(shared.changed.wait_while(idle, |w| *w == Watch::Nothing));
                    continue;
                }
                Watch::Key(vk) => {
                    let now = down(vk);
                    if now != held {
                        held = now;
                        engine.set_talking(now);
                        handle.update(move |s, _| s.key_held(now));
                    }
                }
                Watch::Choosing => {
                    for (vk, was) in was_down.iter_mut().enumerate() {
                        let vk = vk as Vk;
                        let now = down(vk);
                        if now && !*was && pick(vk) != Pick::Skip {
                            handle.update(move |s, _| s.key_pressed(vk));
                        }
                        *was = now;
                    }
                }
            }
            std::thread::sleep(READ_EVERY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_and_middle_buttons_and_keys_can_be_the_key_but_not_left_or_right_click() {
        for (vk, says) in [
            (MOUSE_4, "Mouse 4"),
            (MOUSE_5, "Mouse 5"),
            (MIDDLE_MOUSE, "Middle mouse"),
            (0x56, "V"),
            (0x14, "Caps Lock"),
            (0x37, "7"),
            (0x74, "F5"),
            (0x63, "Num 3"),
            (0xA4, "Left Alt"),
            (0xDE, "'"),
        ] {
            assert_eq!(pick(vk), Pick::Key(vk), "{says}");
            assert_eq!(name(vk).as_deref(), Some(says));
        }
        // Left and right click work the window; Ctrl and the Windows key are taken by side or
        // not at all; Escape stops choosing.
        for vk in [0x01, 0x02, 0x11, 0x5B, 0xFF] {
            assert_eq!(pick(vk), Pick::Skip, "{vk:#x}");
        }
        assert_eq!(pick(ESCAPE), Pick::Cancel);
    }
}
