//! The window driven as a user would: taps, drops and links, on the mock engine.

use std::path::PathBuf;

use rune::{Env, Runtime};
use warble_engine::Preset;

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
