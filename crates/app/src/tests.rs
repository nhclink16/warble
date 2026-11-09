//! The window driven as a user would: taps, drops and links, on the mock engine.

use std::path::{Path, PathBuf};

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use warble_engine::{Clip, Preset};

use crate::find::YouTube;
use crate::settings::HOLD_DIR;
use crate::state::{REMOVE_SETTLE, State, Tab};
use crate::ui;

fn app() -> Runtime<State> {
    let mut rt = Runtime::new(State::mock(), ui::view, (400.0, 620.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

/// The window on the mock engine with a real (temporary) config folder, so sounds are copied
/// in and removed files really move. Returns the app and a file outside that folder.
fn app_with_folder(name: &str) -> (Runtime<State>, PathBuf) {
    let root = std::env::temp_dir().join(format!("warble-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let outside = root.join("Downloads");
    std::fs::create_dir_all(&outside).unwrap();
    let original = outside.join("Air horn.wav");
    std::fs::write(
        &original,
        b"not really audio; the mock engine doesn't read it",
    )
    .unwrap();
    let mut s = State::mock();
    s.dir = Some(root.join("config"));
    let mut rt = Runtime::new(s, ui::view, (400.0, 620.0), Env::new(true, 1.0));
    settle(&mut rt);
    (rt, original)
}

fn remove_first(rt: &mut Runtime<State>) {
    let key = format!("remove-{}", rt.state.sounds[0].id.0);
    tap(rt, &key);
}

/// Runs the app's poll, as the event loop does about 30 times a second.
fn poll(rt: &mut Runtime<State>) {
    rt.update(|s, tx| {
        s.poll(1e6, tx);
    });
    settle(rt);
}

fn sounds_dir(rt: &Runtime<State>) -> PathBuf {
    rt.state.sounds_dir().unwrap()
}

fn holding(rt: &Runtime<State>) -> Vec<PathBuf> {
    std::fs::read_dir(sounds_dir(rt).join(HOLD_DIR))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn press(rt: &mut Runtime<State>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    settle(rt);
}

/// The path of the element with keyboard focus, or "" with none.
fn focus_path(rt: &Runtime<State>) -> String {
    let e = rt.elements().into_iter().find(|e| e.focused);
    e.map_or_else(String::new, |e| e.path)
}

/// Presses Tab until the element keyed `key`, or a field inside it, has focus.
fn tab_to(rt: &mut Runtime<State>, key: &str) {
    for _ in 0..40 {
        let at = focus_path(rt);
        if at.ends_with(&format!("/{key}")) || at.contains(&format!("/{key}/")) {
            return;
        }
        press(rt, Key::Tab);
    }
    panic!("Tab never reaches {key}");
}

/// Every node a screen reader sees, flattened.
fn a11y(rt: &Runtime<State>) -> Vec<serde_json::Value> {
    fn walk(v: &serde_json::Value, out: &mut Vec<serde_json::Value>) {
        out.push(v.clone());
        for c in v["children"].as_array().into_iter().flatten() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(&rt.accessibility_json(), &mut out);
    out
}

/// The screen reader's node for the element keyed `key`.
fn a11y_node(rt: &Runtime<State>, key: &str) -> serde_json::Value {
    let node = a11y(rt).into_iter().find(|n| n["key"] == key);
    node.unwrap_or_else(|| panic!("no node {key}"))
}

/// Runs frames until a page that just appeared has faded in and takes presses.
fn settle(rt: &mut Runtime<State>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn tap(rt: &mut Runtime<State>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    // Hover first, as a hand does: the remove button shows only while its tile is hovered.
    rt.pointer_move(x, y);
    settle(rt);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

#[test]
fn tapping_a_preset_tile_selects_it() {
    let mut rt = app();
    assert_eq!(rt.state.settings.preset(), Preset::Normal);
    tap(&mut rt, "preset-robot");
    assert_eq!(rt.state.settings.preset(), Preset::Robot);
    tap(&mut rt, "preset-cave");
    assert_eq!(rt.state.settings.preset(), Preset::Cave);
}

#[test]
fn the_effect_row_and_its_switch_both_toggle_the_effect() {
    let mut rt = app();
    assert!(rt.state.settings.effect);
    tap(&mut rt, "effect");
    assert!(!rt.state.settings.effect);
    tap(&mut rt, "effect-switch");
    assert!(rt.state.settings.effect);
}

#[test]
fn the_tabs_switch_pages() {
    let mut rt = app();
    assert!(rt.find("presets").is_some());
    tap(&mut rt, "tabs/#0/segment1");
    assert_eq!(rt.state.tab, Tab::Soundboard);
    assert!(
        rt.find("empty").is_some(),
        "no sounds yet shows the empty state"
    );
    tap(&mut rt, "tabs/#0/segment2");
    assert_eq!(rt.state.tab, Tab::Setup);
    assert!(rt.find("discord").is_some());
    tap(&mut rt, "tabs/#0/segment0");
    assert_eq!(rt.state.tab, Tab::Voice);
}

#[test]
fn dropped_audio_files_become_tiles_that_play_and_remove() {
    let mut rt = app();
    let files = ["b/Bruh.wav", "a/Air horn.MP3", "notes.txt"]
        .map(PathBuf::from)
        .to_vec();
    rt.drop_files(files, Some((200.0, 300.0)));
    settle(&mut rt);
    assert_eq!(rt.state.tab, Tab::Soundboard);
    let names: Vec<&str> = rt.state.sounds.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Air horn", "Bruh"], "audio only, sorted by name");
    let id = rt.state.sounds[0].id;
    tap(&mut rt, &format!("sound-{}", id.0));
    assert!(rt.state.is_playing(id));
    tap(&mut rt, "stop-all");
    assert!(!rt.state.is_playing(id));
    tap(&mut rt, &format!("remove-{}", id.0));
    assert_eq!(rt.state.sounds.len(), 1);
}

#[test]
fn dropping_a_file_already_on_the_board_keeps_one_tile() {
    let mut rt = app();
    let file = PathBuf::from("sounds/Bruh.wav");
    rt.drop_files(vec![file.clone()], Some((200.0, 300.0)));
    settle(&mut rt);
    rt.drop_files(vec![file], Some((200.0, 300.0)));
    settle(&mut rt);
    assert_eq!(rt.state.sounds.len(), 1);
}

#[test]
fn a_missing_cable_offers_the_download() {
    let mut rt = app();
    rt.update(|s, _| {
        s.set_tab(Tab::Setup);
        s.sim_no_cable = true;
        s.refresh_status();
    });
    settle(&mut rt);
    tap(&mut rt, "get-cable");
    assert_eq!(rt.take_url_requests(), [ui::VB_CABLE_URL]);
}

#[test]
fn find_sounds_opens_from_the_board_and_its_sites_open_in_the_browser() {
    let mut rt = app();
    tap(&mut rt, "tabs/#0/segment1");
    tap(&mut rt, "find-sounds");
    assert!(rt.state.find.open);
    assert!(rt.find("search-field").is_some() && rt.find("youtube").is_some());
    assert!(rt.find("search-button").is_some());
    tap(&mut rt, "sites-section/trigger");
    tap(&mut rt, "site-1");
    assert_eq!(rt.take_url_requests(), ["https://freesound.org/"]);
    tap(&mut rt, "back");
    assert!(!rt.state.find.open);
    assert!(rt.find("empty").is_some(), "back on the board");
}

#[test]
fn a_youtube_link_first_says_what_it_will_download() {
    let mut rt = app();
    rt.update(|s, _| {
        s.open_find();
        // Mock mode keeps yt-dlp in a temporary folder; make sure it isn't there.
        let _ = std::fs::remove_file(crate::youtube::tool_path(&s.work_dir()));
        s.find.link = rune::TextEdit::new("https://www.youtube.com/watch?v=jNQXAC9IVRw");
    });
    settle(&mut rt);
    tap(&mut rt, "get-audio");
    assert_eq!(rt.state.find.youtube, YouTube::AskToInstall);
    assert!(rt.find("install-notice").is_some());
    tap(&mut rt, "not-now");
    assert_eq!(rt.state.find.youtube, YouTube::Idle);
}

#[test]
fn the_trim_editor_plays_in_the_headphones_and_saves_a_new_tile() {
    let mut rt = app();
    let clip = Clip::new(vec![0.25; 8000 * 4], 8000);
    rt.update(move |s, _| s.open_trim(clip, "Zoo: cut"));
    settle(&mut rt);
    assert_eq!(rt.state.find.trim_name.text, "Zoo cut");
    // The arrow keys move a focused handle by a hundredth of the clip (here 0.05 s).
    tap(&mut rt, "trim-start");
    for _ in 0..4 {
        rt.key(KeyEvent {
            key: Key::Right,
            mods: Mods::default(),
        });
    }
    settle(&mut rt);
    let t = rt.state.trim.as_ref().unwrap();
    assert!((t.start - 0.2).abs() < 1e-9, "start at {}", t.start);
    assert_eq!(t.range(), 1600..32000);

    tap(&mut rt, "trim-play");
    assert_eq!(rt.state.status.preview, Some(1600));
    assert!(rt.state.status.playing.is_empty(), "not into the call");
    tap(&mut rt, "trim-play");
    assert_eq!(rt.state.status.preview, None, "pressed again, it stops");

    tap(&mut rt, "trim-save");
    assert!(rt.state.trim.is_none(), "closes once saved");
    let sound = rt.state.sounds.last().expect("a new tile");
    assert!(sound.name.starts_with("Zoo cut"), "{}", sound.name);
    let bytes = std::fs::read(&sound.path).expect("written");
    assert_eq!(bytes.len(), 44 + (32000 - 1600) * 2, "16-bit mono WAV");
    let _ = std::fs::remove_file(&sound.path);
}

#[test]
fn a_dropped_video_opens_its_audio_in_the_trim_editor() {
    let mut rt = app();
    // One second of a 440 Hz tone under a tiny H.264 picture, the way X and phones save video.
    let video = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/fixtures/tone-video.mp4"
    ));
    rt.drop_files(vec![video], Some((200.0, 300.0)));
    settle(&mut rt);
    let t = rt.state.trim.as_ref().expect("the trim editor opens");
    assert!((t.duration() - 1.0).abs() < 0.1, "{} s", t.duration());
    assert!(t.clip.samples().iter().any(|v| v.abs() > 0.1), "the tone");
    assert_eq!(rt.state.find.trim_name.text, "tone-video");
    assert!(rt.state.sounds.is_empty(), "no tile until it's saved");
}

#[test]
fn a_removed_sound_waits_aside_and_undo_puts_it_back() {
    let (mut rt, original) = app_with_folder("undo");
    rt.drop_files(vec![original.clone()], Some((200.0, 300.0)));
    settle(&mut rt);
    let copy = rt.state.sounds[0].path.clone();
    assert_eq!(copy, sounds_dir(&rt).join("Air horn.wav"));

    remove_first(&mut rt);
    assert!(rt.state.sounds.is_empty());
    assert!(!copy.exists(), "off the board's folder");
    assert_eq!(
        holding(&rt),
        [sounds_dir(&rt).join(HOLD_DIR).join("Air horn.wav")]
    );
    let row = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("removed"));
    assert_eq!(
        row.and_then(|e| e.label).as_deref(),
        Some("Removed “Air horn”. Undo is next to it.")
    );

    tap(&mut rt, "undo-remove");
    let names: Vec<&str> = rt.state.sounds.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Air horn"]);
    assert!(copy.exists() && holding(&rt).is_empty(), "moved back");
    assert!(rt.find("removed").is_none(), "the row goes");
    assert!(original.exists(), "the user's own file is never touched");
}

#[test]
fn a_removal_is_for_good_once_dismissed_or_followed_and_outlasts_a_notice() {
    let (mut rt, original) = app_with_folder("forget");
    let other = original.with_file_name("Bruh.wav");
    std::fs::write(&other, b"x").unwrap();
    rt.drop_files(vec![original.clone(), other], Some((200.0, 300.0)));
    settle(&mut rt);
    let (air, bruh) = (rt.state.sounds[0].id, rt.state.sounds[1].id);

    // A second removal makes the first permanent.
    rt.update(|s, _| s.remove_sound(air));
    rt.update(|s, _| s.remove_sound(bruh));
    assert_eq!(rt.state.removed.as_ref().unwrap().name, "Bruh");
    assert_eq!(
        holding(&rt),
        [sounds_dir(&rt).join(HOLD_DIR).join("Bruh.wav")]
    );

    // No timer: the poll leaves it, and an unrelated notice shows above it, never over it.
    poll(&mut rt);
    assert!(rt.state.removed.is_some() && rt.find("removed").is_some());
    rt.update(|s, _| s.notice = Some("Something else went wrong.".into()));
    poll(&mut rt);
    assert!(rt.state.removed.is_some() && !holding(&rt).is_empty());
    let notice = rt.find("notice").expect("the notice");
    let removed = rt.find("removed").expect("the Undo still shows");
    assert!(notice.1 < removed.1, "under the notice");
    tap(&mut rt, "dismiss-notice");
    assert!(
        rt.find("removed").is_some(),
        "and stays once the notice goes"
    );

    // Dismissed, it's gone for good.
    tap(&mut rt, "dismiss-removed");
    assert!(rt.state.removed.is_none() && holding(&rt).is_empty());
    assert!(std::fs::read_dir(sounds_dir(&rt)).unwrap().next().is_none());
    assert!(original.exists(), "the user's own file is never touched");
}

#[test]
fn a_sound_outside_the_sounds_folder_is_never_deleted() {
    let mut rt = app();
    let file = std::env::temp_dir().join(format!("warble-ui-own-{}.wav", std::process::id()));
    std::fs::write(&file, b"x").unwrap();
    rt.drop_files(vec![file.clone()], Some((200.0, 300.0)));
    settle(&mut rt);
    remove_first(&mut rt);
    tap(&mut rt, "dismiss-removed");
    assert!(file.exists());
    let _ = std::fs::remove_file(file);
}

#[test]
fn a_tile_whose_file_is_gone_still_comes_off() {
    let (mut rt, original) = app_with_folder("gone");
    rt.drop_files(vec![original], Some((200.0, 300.0)));
    settle(&mut rt);
    std::fs::remove_file(&rt.state.sounds[0].path).unwrap();
    remove_first(&mut rt);
    assert!(
        rt.state.sounds.is_empty(),
        "nothing to hold, but the tile goes"
    );
    assert!(rt.state.notice.is_none());
}

#[test]
fn a_failed_undo_keeps_the_undo() {
    let (mut rt, original) = app_with_folder("retry");
    rt.drop_files(vec![original], Some((200.0, 300.0)));
    settle(&mut rt);
    remove_first(&mut rt);
    let held = rt.state.removed.as_ref().unwrap().held.clone().unwrap();
    std::fs::remove_file(&held).unwrap();
    rt.update(|s, _| s.undo_remove());
    assert!(rt.state.removed.is_some(), "kept, to try again");
    let notice = rt.state.notice.clone().unwrap();
    assert!(notice.contains("press Undo again"), "{notice}");
}

#[test]
fn an_undo_that_works_clears_the_failed_tries_notice() {
    let (mut rt, original) = app_with_folder("retry-works");
    rt.drop_files(vec![original], Some((200.0, 300.0)));
    settle(&mut rt);
    remove_first(&mut rt);
    let held = rt.state.removed.as_ref().unwrap().held.clone().unwrap();
    std::fs::rename(&held, held.with_extension("away")).unwrap();
    tap(&mut rt, "undo-remove");
    assert!(rt.find("notice").is_some(), "the failed try says so");

    // The app that held the file lets go, and Undo is pressed again.
    std::fs::rename(held.with_extension("away"), &held).unwrap();
    tap(&mut rt, "undo-remove");
    assert_eq!(rt.state.sounds.len(), 1, "back on the board");
    assert_eq!(rt.state.notice, None);
    assert!(rt.find("notice").is_none() && rt.find("removed").is_none());
}

#[test]
fn many_failed_files_name_three_and_count_the_rest() {
    let mut rt = app();
    let errors: Vec<String> = (1..=5).map(|i| format!("File {i} failed.")).collect();
    rt.update(|s, _| s.loaded((Vec::new(), errors)));
    assert_eq!(
        rt.state.notice.as_deref(),
        Some("File 1 failed.\nFile 2 failed.\nFile 3 failed.\n2 more files didn’t load.")
    );
    let errors: Vec<String> = (1..=4).map(|i| format!("File {i} failed.")).collect();
    rt.update(|s, _| s.loaded((Vec::new(), errors)));
    assert!(
        rt.state
            .notice
            .as_deref()
            .unwrap()
            .ends_with("\n1 more file didn’t load.")
    );
}

#[test]
fn the_board_settles_for_a_moment_after_a_removal() {
    let (mut rt, original) = app_with_folder("settle");
    rt.drop_files(vec![original.clone(), original], Some((200.0, 300.0)));
    settle(&mut rt);
    assert_eq!(rt.state.sounds.len(), 2);
    remove_first(&mut rt);
    // The second tile slid under the pointer: a quick second click doesn't take it too.
    remove_first(&mut rt);
    assert_eq!(rt.state.sounds.len(), 1);
    rt.state.removed.as_mut().unwrap().at -= REMOVE_SETTLE;
    remove_first(&mut rt);
    assert!(rt.state.sounds.is_empty());
}

#[test]
fn starting_up_deletes_a_removal_left_waiting() {
    let (mut rt, _) = app_with_folder("startup");
    let dir = sounds_dir(&rt);
    std::fs::create_dir_all(dir.join(HOLD_DIR)).unwrap();
    std::fs::write(dir.join(HOLD_DIR).join("Old.wav"), b"x").unwrap();
    std::fs::write(dir.join("Keep.wav"), b"x").unwrap();
    rt.update(|s, _| s.load_saved_sounds());
    let names: Vec<&str> = rt.state.sounds.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Keep"]);
    assert!(!Path::new(&dir.join(HOLD_DIR)).exists());
}

#[test]
fn a_tiles_context_menu_trims_and_removes_from_the_keyboard() {
    let mut rt = app();
    rt.drop_files(vec![PathBuf::from("Bruh.wav")], Some((200.0, 300.0)));
    settle(&mut rt);
    let id = rt.state.sounds[0].id;
    let focused = |rt: &Runtime<State>| {
        rt.elements()
            .into_iter()
            .find(|e| e.focused)
            .and_then(|e| e.key)
    };
    for _ in 0..10 {
        if focused(&rt) == Some(format!("sound-{}", id.0)) {
            break;
        }
        rt.key(KeyEvent {
            key: Key::Tab,
            mods: Mods::default(),
        });
        settle(&mut rt);
    }
    assert_eq!(
        focused(&rt),
        Some(format!("sound-{}", id.0)),
        "Tab reaches the tile"
    );
    rt.key(KeyEvent {
        key: Key::Function(10),
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    settle(&mut rt);
    // The menu is keyed by the pad's place on the board: this is the first.
    assert!(rt.find("sound-menu-0-item0").is_some(), "Trim…");
    tap(&mut rt, "sound-menu-0-item1");
    assert!(rt.state.sounds.is_empty(), "Remove");
    assert!(rt.state.sound_menu.is_none());
}

#[test]
fn remove_from_the_menu_hands_focus_to_the_pad_that_takes_its_place() {
    let mut rt = app();
    let files = ["Air horn.wav", "Bruh.wav"].map(PathBuf::from).to_vec();
    rt.drop_files(files, Some((200.0, 300.0)));
    settle(&mut rt);
    let (air, bruh) = (rt.state.sounds[0].id, rt.state.sounds[1].id);
    tab_to(&mut rt, &format!("sound-{}", air.0));
    rt.key(KeyEvent {
        key: Key::ContextMenu,
        mods: Mods::default(),
    });
    settle(&mut rt);
    // Down to Remove, and press it.
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.sounds.len(), 1);
    let next = format!("/sound-{}", bruh.0);
    assert!(focus_path(&rt).ends_with(&next), "{}", focus_path(&rt));
}

#[test]
fn hotkey_failures_name_the_keys_and_say_what_to_do() {
    use rune::ShortcutError;
    let mut s = State::mock();
    let taken = crate::why(&ShortcutError::Conflict);
    s.hotkey_failures = vec![
        (crate::state::sound_key(0), taken.clone()),
        (crate::state::sound_key(1), taken),
        (
            crate::state::EFFECT_KEY.into(),
            crate::why(&ShortcutError::PermissionDenied),
        ),
    ];
    assert_eq!(
        ui::hotkey_problems(&s, None),
        [
            "Another app is using Ctrl+Shift+1 and Ctrl+Shift+2. Close that app, then quit \
             Warble from the tray and open it again.",
            "Warble wasn’t allowed to use Ctrl+Alt+V. Quit Warble from the tray, open it again, \
             and allow its hotkeys.",
        ]
    );
    let effect = [crate::state::EFFECT_KEY.to_string()];
    assert_eq!(ui::hotkey_problems(&s, Some(&effect)).len(), 1);
}

#[test]
fn every_file_that_fails_to_load_is_named() {
    let mut s = State::mock();
    let errors = ["Gone.wav", "Lost.mp3", "Missing.flac"]
        .map(|f| {
            warble_engine::Clip::decode(Path::new(f), 300)
                .err()
                .unwrap()
        })
        .to_vec();
    s.loaded((Vec::new(), errors));
    let notice = s.notice.unwrap();
    assert_eq!(notice.lines().count(), 3, "{notice}");
    for name in ["“Gone.wav”", "“Lost.mp3”", "“Missing.flac”"] {
        assert!(notice.contains(name), "{name} in {notice}");
    }
    assert!(!notice.contains("more"));
}

#[test]
fn blocked_hotkeys_show_in_the_pill_and_it_opens_setup() {
    let mut rt = app();
    let taken = crate::why(&rune::ShortcutError::Conflict);
    rt.update(move |s, _| s.hotkey_failures = vec![(crate::state::STOP_KEY.into(), taken)]);
    settle(&mut rt);
    assert_eq!(rt.state.status_label(), ("Hotkeys blocked", "warn"));
    tap(&mut rt, "status");
    assert_eq!(rt.state.tab, Tab::Setup);
    let callout = rt.find("hotkeys-callout").expect("the callout");
    let devices = rt.find("devices").unwrap();
    assert!(callout.1 < devices.1, "at the top of Setup");
}

#[test]
fn a_device_error_is_short_in_the_pill_and_whole_under_its_picker() {
    let mut rt = app();
    let why = "Couldn't open Blue Yeti: another app is using it";
    rt.update(move |s, _| {
        s.settings.input = Some("Blue Yeti".into());
        s.sim_error = Some(why.into());
        s.refresh_status();
    });
    settle(&mut rt);
    let pill = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status"));
    let pill = pill.unwrap();
    assert_eq!(
        pill.label.as_deref(),
        Some(format!("Audio problem: {why}").as_str())
    );
    // A key to Setup, read out when it changes, its tooltip saying where it goes.
    assert_eq!(
        (pill.role, pill.help.as_deref()),
        ("button", Some("Open Setup"))
    );
    assert!(
        a11y_node(&rt, "status")["states"]
            .as_array()
            .is_some_and(|s| s.contains(&"live-polite".into()))
    );
    tap(&mut rt, "status");
    assert_eq!(rt.state.tab, Tab::Setup);
    // On Setup it only names the problem; the line under the picker reads the whole of it.
    let here = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("setup-status"))
        .expect("the pill on Setup");
    assert_eq!(
        (here.role, here.label.as_deref(), here.help.as_deref()),
        ("status", Some("Audio problem"), None)
    );
    let error = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("device-error"));
    let error = error.expect("the whole message on Setup");
    assert_eq!((error.role, error.label.as_deref()), ("alert", Some(why)));
    let (_, mic) = rt.find("pick-input-trigger").unwrap();
    let (_, cable) = rt.find("pick-cable-trigger").unwrap();
    assert!(
        mic < error.rect[1] && error.rect[1] < cable,
        "under the microphone"
    );
}

/// Clearly the LED's colour (within 15° of its hue), solid or washed into a pad.
fn is_led(c: rune::Color) -> bool {
    let [_, a, b, alpha] = c.to_oklab();
    let hue = b.atan2(a).to_degrees();
    alpha > 0.05 && a.hypot(b) > 0.03 && (hue - 42.0).abs() < 15.0
}

/// What's lit by the LED below the header (whose Live dot is always lit while running).
fn lit(rt: &Runtime<State>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.path.contains("/header"))
        .filter(|e| is_led(e.fill) || e.text_color.is_some_and(is_led))
        .map(|e| e.path)
        .collect()
}

fn scene(rt: &mut Runtime<State>, name: &str) {
    let (_, set) = ui::SCENES.iter().find(|(n, _)| *n == name).unwrap();
    rt.update(|s, _| set(s));
    settle(rt);
}

#[test]
fn only_audio_going_into_the_call_lights_the_led() {
    let mut rt = app();
    scene(&mut rt, "voice");
    assert!(rt.state.voice_live());
    let on = lit(&rt);
    assert!(
        !on.is_empty(),
        "the chosen voice lights while the effect is on"
    );
    assert!(on.iter().all(|p| p.contains("/preset-deep")), "{on:?}");

    rt.update(|s, _| s.set_effect(false));
    settle(&mut rt);
    assert!(!rt.state.voice_live());
    assert_eq!(
        lit(&rt),
        Vec::<String>::new(),
        "nothing lights with the effect off"
    );

    let mut rt = app();
    scene(&mut rt, "soundboard");
    let playing = rt.state.status.playing[0].0;
    let on = lit(&rt);
    assert!(!on.is_empty(), "a playing sound lights");
    let pad = format!("/sound-{playing}");
    assert!(on.iter().all(|p| p.contains(&pad)), "{on:?}");

    // Previews and Trim play in your headphones only; Setup sends nothing.
    for name in ["find", "trim", "setup", "remote"] {
        let mut rt = app();
        scene(&mut rt, name);
        assert_eq!(lit(&rt), Vec::<String>::new(), "{name}");
    }

    // With the effect on but nothing going out, the chosen voice is only latched.
    for name in ["no-cable", "audio-problem", "starting"] {
        let mut rt = app();
        if name == "starting" {
            rt.update(|s, _| s.status.running = false);
        } else {
            scene(&mut rt, name);
        }
        rt.update(|s, _| s.set_tab(Tab::Voice));
        settle(&mut rt);
        assert!(rt.state.settings.effect && !rt.state.voice_live(), "{name}");
        assert_eq!(lit(&rt), Vec::<String>::new(), "{name}");
    }

    // Sounds go into the call through VB-CABLE, even while the mic has a problem. With no
    // cable they play only in the headphones: the pad keeps its playing mark and Stop all
    // still stops them, but nothing lights.
    for (name, reaches_call) in [("audio-problem", true), ("no-cable", false)] {
        let mut rt = app();
        scene(&mut rt, "soundboard");
        scene(&mut rt, name);
        let id = rt.state.sounds[0].id;
        rt.update(move |s, _| {
            s.set_tab(Tab::Soundboard);
            s.play(id);
        });
        settle(&mut rt);
        assert!(!rt.state.status.running, "{name}");
        assert_eq!(rt.state.cable_open(), reaches_call, "{name}");
        let pad = format!("/sound-{}", id.0);
        let on = lit(&rt);
        assert_eq!(!on.is_empty(), reaches_call, "{name}: {on:?}");
        assert!(on.iter().all(|p| p.contains("/sound-")), "{name}: {on:?}");
        let tile = a11y_node(&rt, &pad[1..]);
        let says = format!("Play {} (playing)", rt.state.sounds[0].name);
        assert_eq!(tile["name"], says.as_str(), "{name}");
        let marks = rt
            .elements()
            .into_iter()
            .filter(|e| e.path.contains(&format!("{pad}/")) && e.role == "icon" && e.opacity > 0.5);
        assert!(marks.count() > 0, "{name}: the playing mark shows");
        let stop = a11y_node(&rt, "stop-all");
        let disabled = stop["states"]
            .as_array()
            .is_some_and(|s| s.contains(&"disabled".into()));
        assert!(!disabled, "{name}: Stop all still stops them");
    }
}

#[test]
fn a_screen_reader_hears_each_pads_hotkey() {
    let mut rt = app();
    scene(&mut rt, "voice");
    assert_eq!(a11y_node(&rt, "preset-normal")["shortcut"], "Ctrl+Alt+1");
    assert_eq!(a11y_node(&rt, "preset-deep")["shortcut"], "Ctrl+Alt+2");
    // The effect is one switch with its hotkey, named once; the row around it only takes clicks.
    let switch = a11y_node(&rt, "effect-switch");
    assert_eq!(
        (&switch["role"], &switch["name"], &switch["shortcut"]),
        (
            &"switch".into(),
            &"Voice effect".into(),
            &"Ctrl+Alt+V".into()
        )
    );
    let named = a11y(&rt)
        .into_iter()
        .filter(|n| {
            n["name"]
                .as_str()
                .is_some_and(|n| n.contains("Voice effect"))
        })
        .count();
    assert_eq!(named, 1);
    assert!(a11y(&rt).iter().all(|n| n["key"] != "effect"));
    assert_eq!(a11y_node(&rt, "monitor-switch")["shortcut"], "Ctrl+Alt+M");
    assert!(rt.state.settings.effect);
    tap(&mut rt, "effect");
    assert!(!rt.state.settings.effect, "the row still takes a click");

    scene(&mut rt, "soundboard");
    let first = rt.state.sounds[0].id;
    let pad = a11y_node(&rt, &format!("sound-{}", first.0));
    assert_eq!(pad["shortcut"], "Ctrl+Shift+1");
}

#[test]
fn the_phone_remote_says_when_there_is_no_network() {
    let mut rt = app();
    scene(&mut rt, "remote-no-network");
    let line = rt.elements().into_iter().find(|e| {
        e.label
            .as_deref()
            .is_some_and(|l| l.starts_with("Connect this PC"))
    });
    let line = line.expect("read out as one line");
    assert_eq!(line.role, "status", "a status line, not a group");
}

#[test]
fn a_notice_shows_once_above_whichever_page_shows() {
    let mut rt = app();
    // Raised on Setup, as a failed "Sign out all phones" is.
    rt.update(|s, _| {
        s.set_tab(Tab::Setup);
        s.notice = Some("Couldn’t make a new code. Try again.".into());
    });
    settle(&mut rt);
    let path = |rt: &Runtime<State>| {
        let all: Vec<String> = rt
            .elements()
            .into_iter()
            .filter(|e| e.key.as_deref() == Some("notice") && !e.exiting)
            .map(|e| e.path)
            .collect();
        assert_eq!(all.len(), 1, "{all:?}");
        all[0].clone()
    };
    // The same element on every page, so a screen reader hears it once.
    let on_setup = path(&rt);
    for tab in [Tab::Voice, Tab::Soundboard] {
        rt.update(move |s, _| s.set_tab(tab));
        settle(&mut rt);
        assert_eq!(path(&rt), on_setup, "{tab:?}");
    }
    rt.update(|s, _| s.open_find());
    settle(&mut rt);
    assert_eq!(path(&rt), on_setup, "Find sounds");
    rt.update(|s, _| s.open_trim(Clip::new(vec![0.1; 8000], 8000), "Bruh"));
    settle(&mut rt);
    assert_eq!(path(&rt), on_setup, "Trim");

    // The Undo for a removed sound stays with the Soundboard: its pads, Find and Trim.
    let mut rt = app();
    scene(&mut rt, "removed");
    let shows = |rt: &Runtime<State>| rt.find("removed").is_some();
    assert!(shows(&rt), "the pads");
    for tab in [Tab::Voice, Tab::Setup] {
        rt.update(move |s, _| s.set_tab(tab));
        settle(&mut rt);
        assert!(!shows(&rt), "{tab:?}");
        assert!(rt.find("notices").is_none(), "no empty box on {tab:?}");
    }
    rt.update(|s, _| {
        s.set_tab(Tab::Soundboard);
        s.open_find();
    });
    settle(&mut rt);
    assert!(shows(&rt), "Find sounds");
    rt.update(|s, _| s.open_trim(Clip::new(vec![0.1; 8000], 8000), "Bruh"));
    settle(&mut rt);
    assert!(shows(&rt), "Trim");
    assert!(rt.state.removed.is_some(), "still there to undo");
}

#[test]
fn a_long_notice_scrolls_in_its_place() {
    let mut rt = Runtime::new(State::mock(), ui::view, (360.0, 540.0), Env::new(true, 1.0));
    settle(&mut rt);
    scene(&mut rt, "load-error");
    let long = "Couldn’t copy “A rather long file name from a sample pack, take 12 (final \
                mix).wav” into Warble. Copy it to your Music folder, then add it from there.";
    rt.update(move |s, _| s.notice = Some([long; 4].join("\n")));
    settle(&mut rt);
    let rect = |key: &str| {
        let e = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key));
        e.unwrap_or_else(|| panic!("no {key}")).rect
    };
    assert!(rect("notices")[3] <= 200.0, "{:?}", rect("notices"));
    let footer = rect("sound-footer");
    assert!(
        footer[1] + footer[3] <= 540.0,
        "the footer stays: {footer:?}"
    );
    let pads = rt.find("sounds").expect("the pads");
    assert!(pads.1 < footer[1], "and the pads show above it");
}

#[test]
fn dismiss_and_undo_hand_focus_to_what_comes_next() {
    let mut rt = app();
    let files = ["Air horn.wav", "Bruh.wav"].map(PathBuf::from).to_vec();
    rt.drop_files(files, Some((200.0, 300.0)));
    settle(&mut rt);
    let air = rt.state.sounds[0].id;
    rt.update(move |s, _| s.remove_sound(air));
    settle(&mut rt);
    let space = || Key::Text(" ".into());

    // Undo: the board's first pad.
    tab_to(&mut rt, "undo-remove");
    press(&mut rt, space());
    assert_eq!(rt.state.sounds.len(), 2);
    let first = format!("/sound-{}", rt.state.sounds[0].id.0);
    assert!(focus_path(&rt).ends_with(&first), "{}", focus_path(&rt));

    // A problem over an Undo: Dismiss goes on to the Undo.
    let bruh = rt.state.sounds[1].id;
    rt.update(move |s, _| {
        s.remove_sound(bruh);
        s.notice = Some("Something went wrong.".into());
    });
    settle(&mut rt);
    tab_to(&mut rt, "dismiss-notice");
    press(&mut rt, space());
    assert!(
        focus_path(&rt).ends_with("/undo-remove"),
        "{}",
        focus_path(&rt)
    );

    // The last pad's Undo dismissed: Add sounds, on the empty board.
    let last = rt.state.sounds[0].id;
    rt.update(move |s, _| s.remove_sound(last));
    settle(&mut rt);
    tab_to(&mut rt, "dismiss-removed");
    press(&mut rt, space());
    assert!(
        focus_path(&rt).ends_with("/add-sounds"),
        "{}",
        focus_path(&rt)
    );

    // On Find sounds: its search field.
    rt.update(|s, _| {
        s.open_find();
        s.notice = Some("Something went wrong.".into());
    });
    settle(&mut rt);
    tab_to(&mut rt, "dismiss-notice");
    press(&mut rt, space());
    assert!(
        focus_path(&rt).contains("/search-field"),
        "{}",
        focus_path(&rt)
    );
}

#[test]
fn the_pill_pressed_from_the_keyboard_hands_focus_to_setup() {
    let mut rt = app();
    let taken = crate::why(&rune::ShortcutError::Conflict);
    rt.update(move |s, _| s.hotkey_failures = vec![(crate::state::STOP_KEY.into(), taken)]);
    settle(&mut rt);
    tab_to(&mut rt, "status");
    press(&mut rt, Key::Text(" ".into()));
    assert_eq!(rt.state.tab, Tab::Setup);
    assert!(focus_path(&rt).contains("/setup/"), "{}", focus_path(&rt));
}

#[test]
fn status_and_alert_lines_are_read_once() {
    // How many nodes a screen reader would read `says` from.
    let read = |rt: &Runtime<State>, says: &str| {
        a11y(rt)
            .iter()
            .filter(|n| n["name"] == says || n["value"] == says)
            .count()
    };
    let mut rt = app();
    scene(&mut rt, "find-offline");
    let offline = rt.state.find.problem.clone().unwrap();
    assert_eq!(read(&rt, &offline), 1, "a failed search");

    rt.update(|s, _| s.find.youtube = YouTube::Fetching);
    settle(&mut rt);
    assert_eq!(read(&rt, "Getting the audio…"), 1, "work under way");

    rt.update(|s, _| s.find.youtube = YouTube::Failed(crate::youtube::UNREADABLE.into()));
    settle(&mut rt);
    assert_eq!(read(&rt, crate::youtube::UNREADABLE), 1, "a failed link");

    rt.update(|s, _| {
        s.find.youtube = YouTube::AskToInstall;
        s.find.saved = Some("Bruh".into());
    });
    settle(&mut rt);
    assert_eq!(
        read(&rt, "Added “Bruh” to your soundboard."),
        1,
        "a sound added"
    );
    let install = a11y(&rt)
        .into_iter()
        .filter(|n| {
            n["name"]
                .as_str()
                .is_some_and(|t| t.starts_with("To get audio"))
        })
        .count();
    assert_eq!(install, 1, "the yt-dlp notice");

    let mut rt = app();
    scene(&mut rt, "remote");
    let why = "Another app is using port 47823. Close it, then turn the remote on again.";
    rt.update(move |s, _| s.remote.problem = Some(why.into()));
    settle(&mut rt);
    assert_eq!(read(&rt, why), 1, "the remote's problem");
}

#[test]
fn home_and_end_go_to_the_ends_of_faders_and_trim_handles() {
    let mut rt = app();
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
    tab_to(&mut rt, "mic-gain");
    press(&mut rt, Key::End);
    assert!(near(rt.state.settings.mic_gain.into(), 4.0), "400%");
    press(&mut rt, Key::Home);
    assert!(near(rt.state.settings.mic_gain.into(), 0.25), "25%");

    let clip = Clip::new(vec![0.25; 8000 * 4], 8000);
    rt.update(move |s, _| {
        s.open_trim(clip, "Zoo");
        let t = s.trim.as_mut().unwrap();
        t.set_start(1.0);
        t.set_end(3.0);
    });
    settle(&mut rt);
    let at = |rt: &Runtime<State>| {
        let t = rt.state.trim.as_ref().unwrap();
        (t.start, t.end)
    };
    // The start goes as near the end handle as it can, then to the clip's start.
    tab_to(&mut rt, "trim-start");
    press(&mut rt, Key::End);
    assert!(
        near(at(&rt).0, 3.0 - crate::trim::MIN_SECONDS),
        "{:?}",
        at(&rt)
    );
    assert!(near(at(&rt).1, 3.0), "the end stays: {:?}", at(&rt));
    press(&mut rt, Key::Home);
    assert!(near(at(&rt).0, 0.0), "{:?}", at(&rt));
    // The end goes as near the start handle as it can, then to the clip's end.
    tab_to(&mut rt, "trim-end");
    press(&mut rt, Key::Home);
    assert!(near(at(&rt).1, crate::trim::MIN_SECONDS), "{:?}", at(&rt));
    press(&mut rt, Key::End);
    assert!(near(at(&rt).1, 4.0), "{:?}", at(&rt));
}

#[test]
fn the_freesound_hint_changes_its_words_not_the_layout() {
    let mut rt = app();
    rt.update(|s, _| {
        s.open_find();
        s.find.key_open = true;
    });
    settle(&mut rt);
    let says = |rt: &Runtime<State>, words: &str| {
        rt.elements()
            .into_iter()
            .any(|e| e.text.as_deref() == Some(words))
    };
    assert!(says(&rt, "Searching Myinstants only."));
    let field = rt.find("key-field").expect("the key field");
    // A key pasted in, as on_change saves it.
    rt.update(|s, _| {
        s.find.key = rune::TextEdit::new("4f9c2a7e");
        s.set_freesound_key();
    });
    settle(&mut rt);
    assert!(says(&rt, "Searching Myinstants and Freesound."));
    assert_eq!(rt.find("key-field"), Some(field), "the field stays put");
}

#[test]
fn not_now_and_add_hand_focus_on() {
    let mut rt = app();
    scene(&mut rt, "youtube-install");
    tab_to(&mut rt, "not-now");
    press(&mut rt, Key::Text(" ".into()));
    assert_eq!(rt.state.find.youtube, YouTube::Idle);
    assert!(
        focus_path(&rt).contains("/link-field"),
        "{}",
        focus_path(&rt)
    );

    // Add gives way to "Added" (here as a finished download leaves it): the next result.
    scene(&mut rt, "find");
    tab_to(&mut rt, "add-0");
    let url = rt.state.find.hits[0].url.clone();
    rt.update(move |s, _| s.find.added.push(url));
    settle(&mut rt);
    assert!(
        focus_path(&rt).ends_with("/preview-1"),
        "{}",
        focus_path(&rt)
    );

    // A result that's still downloading can't take focus: it goes on past it.
    scene(&mut rt, "find");
    let busy = rt.state.find.hits[2].url.clone();
    rt.update(move |s, _| s.find.busy.push(busy));
    settle(&mut rt);
    tab_to(&mut rt, "add-1");
    let url = rt.state.find.hits[1].url.clone();
    rt.update(move |s, _| s.find.added.push(url));
    settle(&mut rt);
    assert!(
        focus_path(&rt).ends_with("/preview-3"),
        "{}",
        focus_path(&rt)
    );
}

#[test]
fn stop_all_hands_focus_to_add_sounds_when_nothing_plays() {
    let mut rt = app();
    scene(&mut rt, "soundboard");
    assert!(!rt.state.status.playing.is_empty());
    tab_to(&mut rt, "stop-all");
    press(&mut rt, Key::Text(" ".into()));
    assert!(rt.state.status.playing.is_empty());
    assert!(
        focus_path(&rt).ends_with("/add-sounds"),
        "{}",
        focus_path(&rt)
    );
    // Focus elsewhere stays put when a sound ends.
    let id = rt.state.sounds[0].id;
    rt.update(move |s, _| s.play(id));
    settle(&mut rt);
    tab_to(&mut rt, "find-sounds");
    rt.update(|s, _| s.stop_sounds());
    settle(&mut rt);
    assert!(
        focus_path(&rt).ends_with("/find-sounds"),
        "{}",
        focus_path(&rt)
    );
}

#[test]
fn remove_from_the_last_pads_menu_hands_focus_to_undo() {
    for files in [vec!["Air horn.wav", "Bruh.wav"], vec!["Air horn.wav"]] {
        let mut rt = app();
        rt.drop_files(
            files.into_iter().map(PathBuf::from).collect(),
            Some((200.0, 300.0)),
        );
        settle(&mut rt);
        let last = rt.state.sounds.last().unwrap().id;
        tab_to(&mut rt, &format!("sound-{}", last.0));
        press(&mut rt, Key::ContextMenu);
        press(&mut rt, Key::Down);
        press(&mut rt, Key::Enter);
        assert!(rt.state.sounds.iter().all(|s| s.id != last), "removed");
        assert!(
            focus_path(&rt).ends_with("/undo-remove"),
            "{}",
            focus_path(&rt)
        );
    }
}

#[test]
fn pads_stay_inside_the_margins_at_the_smallest_window() {
    let mut rt = Runtime::new(State::mock(), ui::view, (360.0, 540.0), Env::new(true, 1.0));
    settle(&mut rt);
    let files = [
        "Air horn.wav",
        "Supercalifragilisticexpialidocious_extended_stadium_remix.wav",
        "Bruh.wav",
    ];
    rt.drop_files(files.map(PathBuf::from).to_vec(), Some((200.0, 300.0)));
    settle(&mut rt);
    for e in rt.elements() {
        let pad = e.key.as_deref().is_some_and(|k| {
            k.strip_prefix("sound-")
                .is_some_and(|n| n.parse::<u32>().is_ok())
        });
        if pad {
            let right = e.rect[0] + e.rect[2];
            assert!(right <= 360.0 - 16.0 + 0.01, "{} ends at {right}", e.path);
        }
    }
}
