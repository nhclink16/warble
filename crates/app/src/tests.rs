//! The window driven as a user would: taps, drops and links, on the mock engine.

use std::path::PathBuf;

use rune::{Env, Key, KeyEvent, Mods, Runtime};
use warble_engine::{Clip, Preset};

use crate::find::YouTube;
use crate::state::{State, Tab};
use crate::ui;

fn app() -> Runtime<State> {
    let mut rt = Runtime::new(State::mock(), ui::view, (400.0, 620.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
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
