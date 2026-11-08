//! The window driven as a user would: taps, drops and links, on the mock engine.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use warble_engine::{Clip, Preset};

use crate::find::YouTube;
use crate::settings::HOLD_DIR;
use crate::state::{State, Tab};
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
        Some("Removed “Air horn”.")
    );

    tap(&mut rt, "undo-remove");
    let names: Vec<&str> = rt.state.sounds.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Air horn"]);
    assert!(copy.exists() && holding(&rt).is_empty(), "moved back");
    assert!(rt.find("removed").is_none(), "the row goes");
    assert!(original.exists(), "the user's own file is never touched");
}

#[test]
fn a_removal_is_for_good_once_its_undo_times_out_is_dismissed_or_followed() {
    let (mut rt, original) = app_with_folder("forget");
    let other = original.with_file_name("Bruh.wav");
    std::fs::write(&other, b"x").unwrap();
    rt.drop_files(vec![original.clone(), other], Some((200.0, 300.0)));
    settle(&mut rt);
    let (air, bruh) = (rt.state.sounds[0].id, rt.state.sounds[1].id);

    // A second removal makes the first permanent.
    tap(&mut rt, &format!("remove-{}", air.0));
    tap(&mut rt, &format!("remove-{}", bruh.0));
    assert_eq!(rt.state.removed.as_ref().unwrap().name, "Bruh");
    assert_eq!(
        holding(&rt),
        [sounds_dir(&rt).join(HOLD_DIR).join("Bruh.wav")]
    );

    // Before its time, the poll leaves it; after, the file is gone for good.
    poll(&mut rt);
    assert!(rt.state.removed.is_some() && rt.find("removed").is_some());
    rt.state.removed.as_mut().unwrap().until = Instant::now();
    poll(&mut rt);
    assert!(rt.state.removed.is_none() && rt.find("removed").is_none());
    assert!(holding(&rt).is_empty());
    assert!(std::fs::read_dir(sounds_dir(&rt)).unwrap().next().is_none());
    assert!(original.exists(), "the user's own file is never touched");

    // Dismissed, it goes at once; a notice that takes its place ends it too.
    rt.drop_files(vec![original.clone()], Some((200.0, 300.0)));
    settle(&mut rt);
    remove_first(&mut rt);
    tap(&mut rt, "dismiss-removed");
    assert!(rt.state.removed.is_none() && holding(&rt).is_empty());
    rt.drop_files(vec![original.clone()], Some((200.0, 300.0)));
    settle(&mut rt);
    remove_first(&mut rt);
    rt.update(|s, _| s.notice = Some("Something else went wrong.".into()));
    poll(&mut rt);
    assert!(rt.state.removed.is_none() && holding(&rt).is_empty());
}

#[test]
fn a_sound_outside_the_sounds_folder_is_never_deleted() {
    let mut rt = app();
    let file = std::env::temp_dir().join(format!("warble-ui-own-{}.wav", std::process::id()));
    std::fs::write(&file, b"x").unwrap();
    rt.drop_files(vec![file.clone()], Some((200.0, 300.0)));
    settle(&mut rt);
    remove_first(&mut rt);
    rt.state.removed.as_mut().unwrap().until = Instant::now();
    poll(&mut rt);
    assert!(file.exists());
    let _ = std::fs::remove_file(file);
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
    assert!(
        rt.find(&format!("sound-menu-{}-item0", id.0)).is_some(),
        "Trim…"
    );
    tap(&mut rt, &format!("sound-menu-{}-item1", id.0));
    assert!(rt.state.sounds.is_empty(), "Remove");
    assert!(rt.state.sound_menu.is_none());
}
