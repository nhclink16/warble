// No console window behind the app in release builds on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use rune::tok::{self, type_};
use rune::{AppSpec, Cx, El, col, text};

struct State {
    _engine: warble_engine::Engine,
}

fn view(_s: &State, _: &mut Cx<State>) -> El {
    let t = tok::theme(true);
    col().w_full().h_full().center().fill(t.bg).child(text("Warble").font(type_::TITLE).color(t.text))
}

fn main() {
    rune::run(AppSpec::<State> {
        ..AppSpec::new("Warble", (420.0, 600.0), || State { _engine: warble_engine::Engine::mock() }, view)
    });
}
