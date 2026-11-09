//! Warble's own push-to-talk key. Warble only reads whether one key is down: on Windows it asks
//! `GetAsyncKeyState` about 100 times a second, on a thread of its own. It never presses, sends
//! or hooks a key, which is what anti-cheat looks for. The thread reads nothing unless push to
//! talk is on or a key is being chosen. It tells the engine first, so the mic opens within a
//! read whatever the window is doing (hidden, or behind a game), then the window.
//!
//! Elsewhere no thread reads keys. Tests run the thread's reads (`Keys::read`) on a made-up
//! keyboard, and press and release the key with `State::key_held` and `State::key_pressed`, as
//! the thread does.

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
#[cfg(any(windows, test))]
const LEFT_CTRL: Vk = 0xA2;
#[cfg(any(windows, test))]
const RIGHT_ALT: Vk = 0xA5;

/// The name Warble shows for a key it takes, or `None` for one it doesn't: the left and right
/// mouse buttons (they work the window), Escape (it cancels), the Windows keys, Pause (Windows
/// lets it go as soon as it's pressed, so it can't be held), and codes with no key. Shift, Ctrl
/// and Alt are taken by side, as "Left Ctrl". Punctuation is named as on a US keyboard.
pub fn name(vk: Vk) -> Option<String> {
    let named = match vk {
        MIDDLE_MOUSE => "Middle Mouse",
        MOUSE_4 => "Mouse 4",
        MOUSE_5 => "Mouse 5",
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x14 => "Caps Lock",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left Arrow",
        0x26 => "Up Arrow",
        0x27 => "Right Arrow",
        0x28 => "Down Arrow",
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
            // Only a weak hold on the engine, so quitting still drops it and closes the streams.
            let (shared, engine) = (self.shared.clone(), Arc::downgrade(engine));
            let handle = handle.clone();
            std::thread::spawn(move || windows::read(&shared, &engine, &handle));
        }
        #[cfg(not(windows))]
        let _ = (engine, handle);
    }
}

// Only Windows reads keys; tests read a made-up keyboard.

/// What a read of the keys has for the engine and the window.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Heard {
    /// The push-to-talk key went down (true) or came up.
    Talking(bool),
    /// While a key is being chosen, this one was pressed (see [`State::key_pressed`]).
    Pressed(Vk),
}

/// Shift, Ctrl and Alt by side, and the Windows keys.
#[cfg(any(windows, test))]
fn modifier(vk: Vk) -> bool {
    matches!(vk, 0x5B | 0x5C | 0xA0..=0xA5)
}

/// What the key reader keeps from one read to the next.
#[cfg(any(windows, test))]
struct Keys {
    /// The watch the last read was for.
    seen: Watch,
    /// The push-to-talk key is down.
    held: bool,
    /// Which codes were down at the last read, while a key is being chosen.
    was_down: [bool; 256],
    /// A Shift, Ctrl or Alt pressed on its own while a key is being chosen.
    alone: Option<Vk>,
}

#[cfg(any(windows, test))]
impl Default for Keys {
    fn default() -> Self {
        Keys {
            seen: Watch::Nothing,
            held: false,
            was_down: [false; 256],
            alone: None,
        }
    }
}

#[cfg(any(windows, test))]
impl Keys {
    /// One read for `watch`, with `down` saying whether a code is down now: what changed since
    /// the last read, if anything.
    fn read(&mut self, watch: Watch, down: impl Fn(Vk) -> bool) -> Option<Heard> {
        let mut heard = None;
        if watch != self.seen {
            self.seen = watch;
            // A key held belonged to the last watch.
            if self.held {
                self.held = false;
                heard = Some(Heard::Talking(false));
            }
            // A key already down as choosing starts (the Enter that pressed Change key)
            // doesn't count.
            if watch == Watch::Choosing {
                self.alone = None;
                for (vk, was) in self.was_down.iter_mut().enumerate() {
                    *was = down(vk as Vk);
                }
            }
        }
        match watch {
            Watch::Nothing => heard,
            Watch::Key(vk) => {
                let now = down(vk);
                if now != self.held {
                    self.held = now;
                    heard = Some(Heard::Talking(now));
                }
                heard
            }
            Watch::Choosing => self.choose(down).map(Heard::Pressed).or(heard),
        }
    }

    /// While a key is being chosen: the first one pressed that Warble takes, if any. A key
    /// pressed with Shift, Ctrl, Alt or a Windows key held is a shortcut, not the key. Shift,
    /// Ctrl and Alt count only on their own, once they come back up, so the Alt of Alt+Tab
    /// isn't taken.
    fn choose(&mut self, down: impl Fn(Vk) -> bool) -> Option<Vk> {
        // With AltGr, Windows holds a Left Ctrl down beside Right Alt. That Ctrl wasn't pressed.
        let down = |vk| down(vk) && !(vk == LEFT_CTRL && down(RIGHT_ALT));
        let (mut pressed, mut lifted) = (Vec::new(), Vec::new());
        for (vk, was) in self.was_down.iter_mut().enumerate() {
            let (vk, now) = (vk as Vk, down(vk as Vk));
            // The code for both Shifts (or Ctrls, or Alts) goes down with either one.
            if now != *was && !matches!(vk, 0x10..=0x12) {
                if now {
                    pressed.push(vk);
                } else {
                    lifted.push(vk);
                }
            }
            *was = now;
        }
        // So Right Alt going down or up flips Left Ctrl too: the Ctrl wasn't pressed or let go.
        if pressed.contains(&RIGHT_ALT) || lifted.contains(&RIGHT_ALT) {
            pressed.retain(|&vk| vk != LEFT_CTRL);
            lifted.retain(|&vk| vk != LEFT_CTRL);
        }
        let modifiers: Vec<Vk> = (0..256)
            .filter(|&m| modifier(m) && self.was_down[m as usize])
            .collect();
        match pressed[..] {
            [] => {}
            [vk] if pick(vk) != Pick::Skip && modifiers == [vk] => {
                self.alone = Some(vk);
                return None;
            }
            _ => self.alone = None,
        }
        let key = pressed.into_iter().find(|&vk| match pick(vk) {
            Pick::Cancel => true,
            Pick::Key(_) => modifiers.is_empty(),
            Pick::Skip => false,
        });
        match self.alone {
            Some(m) if key.is_none() && lifted.contains(&m) => {
                self.alone = None;
                Some(m)
            }
            _ => key,
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::Weak;
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
    pub fn read(shared: &Shared, engine: &Weak<Engine>, handle: &AppHandle<State>) {
        let mut keys = Keys::default();
        loop {
            let watch = *lock(&shared.watch);
            match keys.read(watch, down) {
                Some(Heard::Talking(now)) => {
                    // Warble has quit: there's nothing left to tell.
                    let Some(engine) = engine.upgrade() else {
                        return;
                    };
                    engine.set_talking(now);
                    handle.update(move |s, _| s.key_held(now));
                }
                Some(Heard::Pressed(vk)) => handle.update(move |s, _| s.key_pressed(vk)),
                None => {}
            }
            if watch == Watch::Nothing {
                let idle = lock(&shared.watch);
                drop(shared.changed.wait_while(idle, |w| *w == Watch::Nothing));
            } else {
                std::thread::sleep(READ_EVERY);
            }
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
            (MIDDLE_MOUSE, "Middle Mouse"),
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
        // not at all; Pause can't be held; Escape stops choosing.
        for vk in [0x01, 0x02, 0x11, 0x13, 0x5B, 0xFF] {
            assert_eq!(pick(vk), Pick::Skip, "{vk:#x}");
        }
        assert_eq!(pick(ESCAPE), Pick::Cancel);
    }

    /// One read of a made-up keyboard on which `held` are down.
    fn read(keys: &mut Keys, watch: Watch, held: &[Vk]) -> Option<Heard> {
        keys.read(watch, |vk| held.contains(&vk))
    }

    #[test]
    fn the_reader_tells_of_the_key_only_as_it_goes_down_or_up() {
        let (mut keys, key) = (Keys::default(), Watch::Key(MOUSE_4));
        assert_eq!(read(&mut keys, key, &[]), None);
        assert_eq!(read(&mut keys, key, &[MOUSE_4]), Some(Heard::Talking(true)));
        assert_eq!(read(&mut keys, key, &[MOUSE_4]), None, "still held");
        assert_eq!(read(&mut keys, key, &[MOUSE_4, 0x56]), None, "other keys");
        assert_eq!(read(&mut keys, key, &[]), Some(Heard::Talking(false)));

        // A key held as the watch changes belonged to the last one: it's let go.
        read(&mut keys, key, &[MOUSE_4]);
        let other = Watch::Key(MOUSE_5);
        assert_eq!(
            read(&mut keys, other, &[MOUSE_4]),
            Some(Heard::Talking(false))
        );
        assert_eq!(
            read(&mut keys, other, &[MOUSE_5]),
            Some(Heard::Talking(true))
        );
        for watch in [Watch::Nothing, Watch::Choosing] {
            read(&mut keys, other, &[MOUSE_5]);
            let heard = read(&mut keys, watch, &[MOUSE_5]);
            assert_eq!(heard, Some(Heard::Talking(false)), "{watch:?}");
            assert_eq!(read(&mut keys, watch, &[MOUSE_5]), None, "{watch:?}");
            assert_eq!(read(&mut keys, watch, &[]), None, "{watch:?}");
        }
    }

    #[test]
    fn choosing_takes_the_first_key_pressed_once_it_starts() {
        let mut keys = Keys::default();
        let mut read = |held: &[Vk]| read(&mut keys, Watch::Choosing, held);
        // The Enter that pressed Change key is down already: it doesn't count, nor its release.
        assert_eq!(read(&[0x0D]), None);
        assert_eq!(read(&[]), None);
        assert_eq!(read(&[0x01]), None, "left click works the window");
        let side = Some(Heard::Pressed(MOUSE_4));
        assert_eq!(
            read(&[0x01, MOUSE_4]),
            side,
            "a side button, as it goes down"
        );
        assert_eq!(read(&[MOUSE_4]), None, "once");
        assert_eq!(read(&[]), None);
        // Two in one read: the lower code, as Escape is here, which stops choosing.
        assert_eq!(read(&[0x56, ESCAPE]), Some(Heard::Pressed(ESCAPE)));
    }

    #[test]
    fn shift_ctrl_and_alt_count_only_pressed_on_their_own() {
        let (shift, ctrl, alt) = ((0x10, 0xA0), (0x11, 0xA2), (0x12, 0xA4));
        let mut keys = Keys::default();
        let mut read = |held: &[Vk]| read(&mut keys, Watch::Choosing, held);
        read(&[]);
        // Alt+Tab, to another window: neither key is taken.
        assert_eq!(
            read(&[alt.0, alt.1]),
            None,
            "Alt waits to see what comes with it"
        );
        assert_eq!(
            read(&[alt.0, alt.1, 0x09]),
            None,
            "Tab with Alt held is a shortcut"
        );
        assert_eq!(read(&[alt.0, alt.1]), None);
        assert_eq!(read(&[]), None, "nor Alt, let go after Tab");
        // Ctrl+Shift, and a Windows-key shortcut.
        assert_eq!(read(&[ctrl.0, ctrl.1]), None);
        assert_eq!(read(&[ctrl.0, ctrl.1, shift.0, shift.1]), None);
        assert_eq!(read(&[]), None);
        assert_eq!(read(&[0x5B]), None);
        assert_eq!(read(&[0x5B, 0x44]), None, "Windows+D");
        assert_eq!(read(&[]), None);
        // Left Shift on its own is taken as it comes up.
        assert_eq!(read(&[shift.0, shift.1]), None);
        assert_eq!(read(&[]), Some(Heard::Pressed(shift.1)));
    }

    #[test]
    fn altgr_is_taken_as_right_alt_not_the_left_ctrl_windows_sends_with_it() {
        let (ctrl, alt) = ([0x11, LEFT_CTRL], [0x12, RIGHT_ALT]);
        let both = [ctrl, alt].concat();
        // Windows sends that Left Ctrl just before Right Alt: in the same read, or one before.
        // Letting go, either may come up a read before the other.
        for before in [false, true] {
            for last in [&[][..], &ctrl, &alt] {
                let case = format!("Ctrl a read before: {before}; left down: {last:x?}");
                let mut keys = Keys::default();
                let mut read = |held: &[Vk]| read(&mut keys, Watch::Choosing, held);
                read(&[]);
                if before {
                    assert_eq!(read(&ctrl), None, "{case}");
                }
                assert_eq!(read(&both), None, "{case}");
                let heard: Vec<_> = [read(last), read(&[])].into_iter().flatten().collect();
                assert_eq!(heard, [Heard::Pressed(RIGHT_ALT)], "{case}");
            }
        }
    }
}
