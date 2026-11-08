//! The window: a header with the app's status, three tabs, and a drop zone around them.

use rune::tok::TextStyle;
use rune::{
    Align, Color, Cursor, Cx, Drag, El, Icon, Phase, ScrollSpec, Spring, Tx, col, el, icon, oklch,
    row, text,
};
use rune_kit::{
    Button, ButtonVariant, Collapsible, ControlSize, IconButton, IconButtonVariant, Keybind, Kit,
    MenuItem, PaletteSeed, SegmentedControl, Select, SelectAppearance, StateColors, Switch,
};
use warble_engine::{DeviceInfo, Preset};

use crate::find_view;
use crate::remote_view;
use crate::settings::DROP_EXTS;
use crate::state::{self, Picker, Removed, SEGMENTS, Sound, State, Tab};

pub const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";

// --- Look ---

pub(crate) const PAD: f32 = 16.0;
pub(crate) const GAP: f32 = 12.0;
pub(crate) const LABEL_W: f32 = 58.0;

// Whole sizes, on rune-kit's 13 (controls) and 11 (captions).
pub(crate) const TITLE: TextStyle = TextStyle::new(16.0, 650.0);
pub(crate) const HEADING: TextStyle = TextStyle::new(14.0, 600.0);
pub(crate) const BODY: TextStyle = TextStyle::new(13.0, 400.0).lh(19.0);
pub(crate) const LABEL: TextStyle = TextStyle::new(13.0, 500.0);
pub(crate) const SMALL: TextStyle = TextStyle::new(12.0, 400.0).lh(18.0);
pub(crate) const TINY: TextStyle = TextStyle::new(11.0, 500.0);
pub(crate) const NUMBER: TextStyle = TextStyle::new(13.0, 500.0).features("'tnum' 1");

/// Icons beside text, as rune-kit's buttons draw them (a 1.5 point stroke).
pub(crate) const ICON: f32 = 16.0;
/// The large glyphs: voice tiles, the drop overlay, the empty board.
pub(crate) const ICON_TILE: f32 = 20.0;

pub(crate) fn bg() -> Color {
    oklch(0.165, 0.01, 265.0)
}
pub(crate) fn accent() -> Color {
    oklch(0.80, 0.14, 165.0)
}
pub(crate) fn warn() -> Color {
    oklch(0.83, 0.13, 80.0)
}
pub(crate) fn danger() -> Color {
    oklch(0.70, 0.16, 25.0)
}
/// The dark ink drawn on the accent.
pub(crate) fn ink() -> Color {
    oklch(0.2, 0.02, 200.0)
}

pub fn kit() -> Kit {
    let mut k = Kit::from_palette(PaletteSeed {
        bg: bg(),
        fg: oklch(0.955, 0.005, 265.0),
        accent: accent(),
    });
    // Faint text carries hotkeys and device names, so it has to read: 6:1 on a card.
    k.c.text_faint = oklch(0.68, 0.008, 265.0);
    k.c.icon_faint = k.c.text_faint;
    // The kit's warning and danger in the app's own colours.
    let page = k.c.page;
    let status = |c: Color| StateColors {
        bg: wash(page, c, 0.12),
        fg: c,
        border: wash(page, c, 0.35),
        icon: c,
    };
    k.c.warning = status(warn());
    k.c.danger = status(danger());
    k
}

/// `colour` washed over `base`, opaque.
pub(crate) fn wash(base: Color, colour: Color, amount: f32) -> Color {
    let (a, b) = (base.to_oklab(), colour.to_oklab());
    Color::from_oklab(std::array::from_fn(|i| {
        if i == 3 {
            1.0
        } else {
            a[i] + (b[i] - a[i]) * amount
        }
    }))
}

pub(crate) fn card(k: &Kit) -> El {
    col()
        .w_full()
        .padding(14.0)
        .gap(GAP)
        .radius(12.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted)
}

fn keys(k: &Kit, chord: &str) -> El {
    Keybind::new(&state::keycaps(chord)).build(k)
}

// --- Icons (16-point view boxes) ---

macro_rules! glyph {
    ($name:ident, $body:expr) => {
        pub(crate) static $name: Icon = Icon {
            name: concat!("warble-", stringify!($name)),
            view: 16.0,
            body: $body,
        };
    };
}
macro_rules! stroke {
    () => {
        r#"stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none""#
    };
}

glyph!(
    LOGO,
    r#"<rect x="2.68" y="6.24" width="1.36" height="3.52" rx=".68" fill="currentColor"/><rect x="5" y="4.32" width="1.36" height="7.36" rx=".68" fill="currentColor"/><rect x="7.32" y="2.88" width="1.36" height="10.24" rx=".68" fill="currentColor"/><rect x="9.64" y="4.8" width="1.36" height="6.4" rx=".68" fill="currentColor"/><rect x="11.96" y="6.56" width="1.36" height="2.88" rx=".68" fill="currentColor"/>"#
);
glyph!(
    NORMAL,
    concat!(
        r#"<rect x="5.25" y="1.5" width="5.5" height="8.5" rx="2.75" "#,
        stroke!(),
        r#"/><path d="M2.75 7.75a5.25 5.25 0 0 0 10.5 0M8 13v1.5" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    DEEP,
    concat!(
        r#"<path d="M1.25 8C3 1.75 6.25 1.75 8 8s5 6.25 6.75 0" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    CHIPMUNK,
    concat!(
        r#"<path d="M1.25 8c.75-4.5 1.5-4.5 2.25 0s1.5 4.5 2.25 0 1.5-4.5 2.25 0 1.5 4.5 2.25 0 1.5-4.5 2.25 0 1.5 4.5 2.25 0" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    ROBOT,
    concat!(
        r#"<path d="M1.25 11.5h2.5v-7h3v7h3v-7h3v7h1.5" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    RADIO,
    concat!(
        r#"<rect x="4.25" y="5" width="7.5" height="9.5" rx="1.75" "#,
        stroke!(),
        r#"/><path d="M6.5 5V1.5M6.75 8.25h2.5M6.75 11h2.5" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    ECHO,
    concat!(
        r#"<circle cx="2.75" cy="8" r="1.4" fill="currentColor"/><path d="M5.5 5a4 4 0 0 1 0 6" "#,
        stroke!(),
        r#"/><path d="M8.75 3.25a6.5 6.5 0 0 1 0 9.5" stroke-opacity=".65" "#,
        stroke!(),
        r#"/><path d="M12 1.75a8.5 8.5 0 0 1 0 12.5" stroke-opacity=".35" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    DEMON,
    concat!(
        r#"<circle cx="8" cy="10" r="4.25" "#,
        stroke!(),
        r#"/><path d="M5.3 6.75C4 5.9 3.3 4.4 3.5 2.25M10.7 6.75C12 5.9 12.7 4.4 12.5 2.25" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    CAVE,
    concat!(
        r#"<path d="M1.75 14V8.5a6.25 6.25 0 0 1 12.5 0V14M5.5 14v-2.75a2.5 2.5 0 0 1 5 0V14M.75 14h14.5" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    PLAY,
    r#"<path d="M5.5 3.75v8.5l7-4.25z" fill="currentColor" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>"#
);
glyph!(
    PLAYING,
    concat!(
        r#"<path d="M3 6.5v3M6 3.5v9M9 5v6M12 7v2" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    STOP,
    r#"<rect x="4" y="4" width="8" height="8" rx="1.5" fill="currentColor"/>"#
);
glyph!(
    PLUS,
    concat!(r#"<path d="M8 3.25v9.5M3.25 8h9.5" "#, stroke!(), "/>")
);
glyph!(
    CLOSE,
    concat!(r#"<path d="M4.5 4.5l7 7M11.5 4.5l-7 7" "#, stroke!(), "/>")
);
glyph!(
    WARNING,
    concat!(
        r#"<path d="M8 2.25 14.25 13.25H1.75Z" "#,
        stroke!(),
        r#"/><path d="M8 6.5v3" "#,
        stroke!(),
        r#"/><circle cx="8" cy="11.4" r=".9" fill="currentColor"/>"#
    )
);
glyph!(
    SPEAKER,
    concat!(
        r#"<path d="M2.25 6h2.5L8.5 3v10L4.75 10h-2.5z" "#,
        stroke!(),
        r#"/><path d="M11 5.5a3.5 3.5 0 0 1 0 5M13 3.5a6.25 6.25 0 0 1 0 9" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    OPEN,
    concat!(
        r#"<path d="M6.5 3.5H3.5v9h9v-3M9.5 3.5h3v3M12.5 3.5 7.5 8.5" "#,
        stroke!(),
        "/>"
    )
);

glyph!(
    BACK,
    concat!(r#"<path d="M9.75 3.5 5.25 8l4.5 4.5" "#, stroke!(), "/>")
);
glyph!(
    SEARCH,
    concat!(
        r#"<circle cx="7" cy="7" r="4.5" "#,
        stroke!(),
        r#"/><path d="M10.5 10.5 13.75 13.75" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    HEADPHONES,
    concat!(
        r#"<path d="M2.5 10.5V8.25a5.5 5.5 0 0 1 11 0v2.25" "#,
        stroke!(),
        r#"/><rect x="1.75" y="9.25" width="3.25" height="5" rx="1.4" fill="currentColor"/><rect x="11" y="9.25" width="3.25" height="5" rx="1.4" fill="currentColor"/>"#
    )
);
glyph!(
    SCISSORS,
    concat!(
        r#"<circle cx="4.5" cy="11.5" r="2.25" "#,
        stroke!(),
        r#"/><circle cx="11.5" cy="11.5" r="2.25" "#,
        stroke!(),
        r#"/><path d="M6.1 9.9 12 2.25M9.9 9.9 4 2.25" "#,
        stroke!(),
        "/>"
    )
);
glyph!(
    CHECK,
    concat!(
        r#"<path d="M3.25 8.5 6.5 11.75 12.75 4.5" "#,
        stroke!(),
        "/>"
    )
);

pub(crate) fn preset_icon(p: Preset) -> &'static Icon {
    match p {
        Preset::Normal => &NORMAL,
        Preset::Deep => &DEEP,
        Preset::Chipmunk => &CHIPMUNK,
        Preset::Robot => &ROBOT,
        Preset::Radio => &RADIO,
        Preset::Echo => &ECHO,
        Preset::Demon => &DEMON,
        Preset::Cave => &CAVE,
    }
}

// --- The window ---

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let k = kit();
    // One keyboard focus ring for every control, in the kit's focus blue.
    cx.focus_ring(2.0, 2.0, k.c.border_focus);
    let mut layer = Vec::new();
    let body = match s.tab {
        Tab::Voice => scrolling(voice(s, cx, &k)),
        Tab::Soundboard if s.trim.is_some() => find_view::trim_page(s, cx, &k),
        Tab::Soundboard if s.find.open => find_view::find_page(s, &k),
        Tab::Soundboard => soundboard(s, cx, &k, &mut layer),
        Tab::Setup => scrolling(setup(s, &k, &mut layer)),
    };
    let mut root = col()
        .key("app")
        .w_full()
        .h_full()
        .fill(k.c.page)
        .child(header(s, &k))
        .child(tabs(s, &k))
        .child(body)
        .on_drop_files(|s: &mut State, files, _| s.add_sounds(files.to_vec()))
        .accepts_extensions(&DROP_EXTS)
        .on_drop_hover(|s: &mut State, over, _| s.drop_hover = over);
    if s.drop_hover {
        root = root.child(drop_overlay(&k));
    }
    rune_kit::root(root, layer)
}

/// A tab's content in a scroll view, for windows shorter than it.
pub(crate) fn scrolling(content: El) -> El {
    col().key("page").grow(1.0).min_h(0.0).child(
        col()
            .key("scroll")
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .scroll_edge_fade(10.0)
            .child(content.w_full().px(PAD).pb(PAD).gap(GAP)),
    )
}

fn header(s: &State, k: &Kit) -> El {
    let logo = el()
        .key("logo")
        .size(24.0, 24.0)
        .radius(6.5)
        .fill(accent())
        .center()
        .a11y_hidden()
        .child(icon(&LOGO, 24.0).color(ink()));
    row()
        .key("header")
        .h(56.0)
        .px(PAD)
        .gap(GAP)
        .center_y()
        .child(
            row()
                .gap(10.0)
                .center_y()
                .child(logo)
                .child(text("Warble").font(TITLE).color(k.c.text_base)),
        )
        .child(el().grow(1.0))
        .child(status_pill(s, k))
}

fn status_pill(s: &State, k: &Kit) -> El {
    let (label, tone) = s.status_label();
    let colour = match tone {
        "live" => accent(),
        "warn" => warn(),
        "danger" => danger(),
        _ => k.c.text_muted,
    };
    // A device error is too long for the pill: it says "Audio problem" and carries the rest.
    let error = s.status.error.as_deref().filter(|_| tone == "danger");
    let mut pill = row()
        .key("status")
        .role("status")
        .label(match error {
            Some(e) => format!("{label}: {e}"),
            None => label.to_string(),
        })
        .h(26.0)
        .px(10.0)
        .gap(7.0)
        .center_y()
        .shrink()
        .max_w(230.0)
        .radius(13.0)
        .fill(wash(k.c.page, colour, 0.12))
        .border(1.0, colour.alpha(0.3))
        .child(el().size(7.0, 7.0).radius(3.5).fill(colour))
        .child(
            text(label)
                .font(LABEL)
                .color(colour)
                .truncate()
                .shrink()
                .a11y_hidden(),
        );
    if let Some(e) = error {
        pill = pill.help(e);
    }
    // A problem's pill takes you to where it's fixed.
    if tone != "live" && s.tab != Tab::Setup {
        pill = pill
            .help(error.unwrap_or("Open Setup"))
            .focusable(true)
            .cursor(Cursor::Pointer)
            .hover(k.c.hover)
            .press(k.c.pressed)
            .on_press(|s: &mut State| s.set_tab(Tab::Setup));
    }
    pill
}

fn tabs(s: &State, k: &Kit) -> El {
    const TABS: [Tab; 3] = [Tab::Voice, Tab::Soundboard, Tab::Setup];
    let current = TABS.iter().position(|t| *t == s.tab);
    row().key("tabs").w_full().px(PAD).pb(14.0).child(
        SegmentedControl::new(&["Voice", "Soundboard", "Setup"], current)
            .label("Section")
            .width(None)
            .on_pick(|s: &mut State, i, _| s.set_tab(TABS[i]))
            .build(k),
    )
}

fn drop_overlay(k: &Kit) -> El {
    el().key("drop")
        .pin(Some(8.0), Some(8.0), Some(8.0), Some(8.0))
        .radius(14.0)
        .fill(k.c.page.alpha(0.9))
        .border(2.0, accent())
        .border_dash(8.0, 5.0)
        .center()
        .inert()
        .child(
            col()
                .gap(10.0)
                .center()
                .child(icon(&PLUS, ICON_TILE).color(accent()))
                .child(
                    text("Drop to add sounds")
                        .font(HEADING)
                        .color(k.c.text_base),
                ),
        )
}

// --- Voice ---

fn voice(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    col()
        .key("voice")
        .child(effect_card(s, k))
        .child(preset_grid(s, k))
        .child(levels_card(s, cx, k))
        .child(monitor_card(s, cx, k))
}

fn effect_card(s: &State, k: &Kit) -> El {
    let on = s.settings.effect;
    let detail = if on {
        format!("{} voice", s.settings.preset().name())
    } else {
        "Off: people hear your own voice".to_string()
    };
    // The line wraps rather than cut off its consequence, so the card grows to fit it.
    let switch = Switch::new(on)
        .label("Voice effect")
        .hide_label()
        .control_size(ControlSize::Regular)
        .on_toggle(|s: &mut State| s.set_effect(!s.settings.effect))
        .build(k)
        .key("effect-switch");
    row()
        .key("effect")
        .w_full()
        .min_h(70.0)
        .py(12.0)
        .pl(16.0)
        .pr(14.0)
        .gap(GAP)
        .center_y()
        .radius(12.0)
        .fill(if on {
            wash(k.c.layer_01, accent(), 0.09)
        } else {
            k.c.layer_01
        })
        .border(
            1.0,
            if on {
                accent().alpha(0.4)
            } else {
                k.c.border_muted
            },
        )
        .hover(k.c.hover)
        .press(k.c.pressed)
        .on_press(|s: &mut State| s.set_effect(!s.settings.effect))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .gap(6.0)
                .child(text("Voice effect").font(HEADING).color(k.c.text_base))
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(keys(k, state::EFFECT_KEY))
                        .child(
                            text(detail)
                                .font(SMALL)
                                .color(k.c.text_muted)
                                .wrap()
                                .grow(1.0)
                                .shrink(),
                        ),
                ),
        )
        .child(switch)
}

fn preset_grid(s: &State, k: &Kit) -> El {
    let (on, current) = (s.settings.effect, s.settings.preset());
    let mut grid = el()
        .key("presets")
        .w_full()
        .equal_columns(4)
        .gaps(8.0, 8.0)
        .role("radiogroup")
        .label("Voice")
        .focus_group();
    for (i, p) in Preset::ALL.into_iter().enumerate() {
        grid = grid.child(preset_tile(k, p, i, p == current, on));
    }
    grid
}

fn preset_tile(k: &Kit, p: Preset, i: usize, selected: bool, on: bool) -> El {
    let lit = selected && on;
    let (fill, border, glyph) = match (selected, on) {
        (true, true) => (wash(k.c.layer_01, accent(), 0.16), accent(), accent()),
        (true, false) => (k.c.layer_02, k.c.border_strong, k.c.text_base),
        _ => (k.c.layer_01, k.c.border_muted, k.c.icon_muted),
    };
    let mut tile = col()
        .key(format!("preset-{}", p.id()))
        .role("radio")
        .label(p.name())
        .checked(selected)
        .focusable(true)
        .focus_selected(selected)
        .h(78.0)
        .gap(5.0)
        .center()
        .radius(10.0)
        .fill(fill)
        .border(if selected { 1.5 } else { 1.0 }, border)
        .on_press(move |s: &mut State| s.select_preset(p))
        .press_scale(0.96)
        .child(icon(preset_icon(p), ICON_TILE).color(glyph).mb(1.0))
        .child(
            text(p.name())
                .font(LABEL)
                .color(if selected {
                    k.c.text_base
                } else {
                    k.c.text_muted
                })
                .truncate(),
        )
        .child(
            text(state::keycaps(&state::preset_key(i)).join("+"))
                .font(TINY)
                .color(if lit {
                    accent().alpha(0.85)
                } else {
                    k.c.text_faint
                })
                .truncate(),
        );
    if !selected {
        tile = tile.hover(k.c.hover).press(k.c.pressed);
    }
    tile
}

fn levels_card(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    let gain = s.settings.mic_gain;
    card(k)
        .key("levels")
        .child(meter_row(k, "Mic", "mic-meter", &s.meters.mic))
        .child(meter_row(k, "Output", "out-meter", &s.meters.out))
        .child(slider(
            k,
            cx,
            Slider {
                key: "mic-gain",
                label: "Mic gain",
                pos: gain_pos(gain),
                value: format!("{:.0}%", gain * 100.0),
                enabled: true,
                get: |s| gain_pos(s.settings.mic_gain),
                set: |s, p| s.set_mic_gain(pos_gain(p)),
            },
        ))
}

/// Mic gain runs 25%..400% with 100% in the middle of the slider.
pub(crate) fn gain_pos(gain: f32) -> f32 {
    (gain.log(4.0) + 1.0) / 2.0
}
pub(crate) fn pos_gain(pos: f32) -> f32 {
    let g = 4f32.powf(pos * 2.0 - 1.0);
    // Snap to 100% near the middle, where most people want it.
    if (g - 1.0).abs() < 0.04 { 1.0 } else { g }
}

fn meter_row(k: &Kit, label: &str, key: &str, segments: &[rune::Live<f32>]) -> El {
    let mut bar = row().key(key).grow(1.0).h(10.0).gap(2.0).a11y_hidden();
    for (i, live) in segments.iter().enumerate() {
        let at = i as f32 / SEGMENTS as f32;
        let colour = if at >= 0.9 {
            danger()
        } else if at >= 0.75 {
            warn()
        } else {
            accent()
        };
        bar = bar.child(
            el().grow(1.0)
                .h_full()
                .radius(1.5)
                .fill(colour)
                .live_opacity(live.smooth(Spring::new(0.09, 0.0))),
        );
    }
    row()
        .w_full()
        .h(20.0)
        .gap(GAP)
        .center_y()
        .child(text(label).font(LABEL).color(k.c.text_muted).w(LABEL_W))
        .child(bar)
        .child(el().w(40.0))
}

fn monitor_card(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    let on = s.settings.monitor;
    card(k)
        .key("monitor")
        .child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .gap(6.0)
                        .child(text("Hear yourself").font(HEADING).color(k.c.text_base))
                        .child(
                            row()
                                .gap(8.0)
                                .center_y()
                                .child(keys(k, state::MONITOR_KEY))
                                .child(
                                    text("How you sound in the call")
                                        .font(SMALL)
                                        .color(k.c.text_muted)
                                        .truncate()
                                        .shrink(),
                                ),
                        ),
                )
                .child(
                    Switch::new(on)
                        .label("Hear yourself")
                        .hide_label()
                        .control_size(ControlSize::Small)
                        .on_toggle(|s: &mut State| s.set_monitor(!s.settings.monitor))
                        .build(k)
                        .key("monitor-switch"),
                ),
        )
        .child(slider(
            k,
            cx,
            Slider {
                key: "monitor-volume",
                label: "Volume",
                pos: s.settings.monitor_volume,
                value: format!("{:.0}%", s.settings.monitor_volume * 100.0),
                enabled: on,
                get: |s| s.settings.monitor_volume,
                set: State::set_monitor_volume,
            },
        ))
}

struct Slider {
    key: &'static str,
    label: &'static str,
    /// 0..=1 along the track.
    pos: f32,
    value: String,
    enabled: bool,
    get: fn(&State) -> f32,
    set: fn(&mut State, f32),
}

/// A labelled slider with its value at the end; drag, click the track, or use the arrow keys.
fn slider(k: &Kit, cx: &mut Cx<State>, sl: Slider) -> El {
    let (h, knob, bar) = (20.0, 14.0, 4.0);
    let w = cx.layout_size(sl.key).map_or(200.0, |s| s[0]);
    let pos = sl.pos.clamp(0.0, 1.0);
    let travel = (w - knob).max(0.0);
    let (get, set) = (sl.get, sl.set);
    let to_pos = move |x: f32, w: f32| ((x - knob / 2.0) / (w - knob).max(1.0)).clamp(0.0, 1.0);
    let grabbed = move |x: f32, at: f32, w: f32| {
        (x - (knob / 2.0 + at * (w - knob))).abs() <= knob / 2.0 + 2.0
    };
    let track = el()
        .key(sl.key)
        .role("slider")
        .label(sl.label)
        .a11y_value(sl.value.clone())
        .range_value(f64::from(pos), 0.0, 1.0)
        .range_step(0.05)
        .focusable(true)
        .grow(1.0)
        .h(h)
        .cursor(Cursor::Pointer)
        .on_step(move |s: &mut State, steps, _| {
            set(s, (get(s) + steps as f32 * 0.05).clamp(0.0, 1.0))
        })
        .on_drag(
            move |s: &mut State, d: &mut Drag, tx: &mut Tx| match d.phase {
                Phase::Down => {
                    d.data = get(s);
                    if !grabbed(d.x, d.data, d.w) {
                        set(s, to_pos(d.x, d.w));
                    }
                }
                Phase::Move => {
                    let knob_x = knob / 2.0 + d.data * (d.w - knob);
                    let grab = if grabbed(d.start_x, d.data, d.w) {
                        d.start_x - knob_x
                    } else {
                        0.0
                    };
                    set(s, to_pos(d.x - grab, d.w));
                    tx.instant();
                }
                Phase::Up => {
                    if d.cancelled() {
                        set(s, d.data);
                    }
                }
            },
        )
        .child(
            el().absolute(0.0, (h - bar) / 2.0)
                .size(w, bar)
                .radius(bar / 2.0)
                .fill(k.c.layer_03),
        )
        .child(
            el().absolute(0.0, (h - bar) / 2.0)
                .size(knob / 2.0 + pos * travel, bar)
                .radius(bar / 2.0)
                .fill(if sl.enabled { accent() } else { k.c.icon_faint }),
        )
        .child(
            el().absolute(pos * travel, (h - knob) / 2.0)
                .size(knob, knob)
                .radius(knob / 2.0)
                .fill(k.c.knob)
                .shadow(1.0, 3.0, Color::BLACK.alpha(0.45)),
        );
    row()
        .w_full()
        .h(22.0)
        .gap(GAP)
        .center_y()
        .child(
            text(sl.label)
                .font(LABEL)
                .color(k.c.text_muted)
                .w(LABEL_W)
                .a11y_hidden(),
        )
        .child(track)
        .child(
            text(sl.value)
                .font(NUMBER)
                .color(k.c.text_muted)
                .w(40.0)
                .align(Align::End)
                .a11y_hidden(),
        )
}

// --- Soundboard ---

fn soundboard(s: &State, cx: &mut Cx<State>, k: &Kit, layer: &mut Vec<El>) -> El {
    if s.sounds.is_empty() {
        return col()
            .key("soundboard")
            .grow(1.0)
            .min_h(0.0)
            .px(PAD)
            .pb(PAD)
            .gap(GAP)
            .children(notice(s, k))
            .child(empty_state(k));
    }
    let any_playing = !s.status.playing.is_empty();
    // In a narrow window, Stop all wraps onto its own line rather than being cut off.
    let toolbar = row()
        .key("sound-tools")
        .w_full()
        .flex_wrap()
        .gaps(8.0, 8.0)
        .justify_between()
        .center_y()
        .child(row().gap(8.0).child(add_button(k)).child(find_button(k)))
        .child(
            Button::new("Stop all")
                .icon(&STOP)
                .shortcut(state::STOP_KEY)
                .disabled(!any_playing)
                .on_press(|s: &mut State| s.stop_sounds())
                .build(k)
                .key("stop-all"),
        );
    let mut grid = el().key("sounds").w_full().equal_columns(3).gaps(8.0, 8.0);
    for (i, sound) in s.sounds.iter().enumerate() {
        grid = grid.child(sound_tile(s, k, layer, i, sound));
    }
    let volume = slider(
        k,
        cx,
        Slider {
            key: "sound-volume",
            label: "Volume",
            pos: s.settings.sound_volume,
            value: format!("{:.0}%", s.settings.sound_volume * 100.0),
            enabled: true,
            get: |s| s.settings.sound_volume,
            set: State::set_sound_volume,
        },
    );
    let hear = row()
        .w_full()
        .h(22.0)
        .gap(GAP)
        .center_y()
        .child(
            text("Hear sounds in your headphones")
                .font(LABEL)
                .color(k.c.text_muted)
                .grow(1.0)
                .shrink()
                .truncate()
                .a11y_hidden(),
        )
        .child(
            Switch::new(s.settings.hear_sounds)
                .label("Hear sounds in your headphones")
                .hide_label()
                .control_size(ControlSize::Small)
                .on_toggle(|s: &mut State| s.set_hear_sounds(!s.settings.hear_sounds))
                .build(k)
                .key("hear-sounds-switch"),
        );
    col()
        .key("soundboard")
        .grow(1.0)
        .min_h(0.0)
        .child(
            col()
                .w_full()
                .px(PAD)
                .pb(10.0)
                .gap(10.0)
                .child(toolbar)
                .children(notice(s, k)),
        )
        .child(scrolling(col().child(grid)))
        .child(el().w_full().h(1.0).fill(k.c.border_muted))
        .child(
            col()
                .key("sound-footer")
                .w_full()
                .px(PAD)
                .py(12.0)
                .gap(10.0)
                .fill(k.c.layer_01)
                .child(volume)
                .child(hear),
        )
}

fn find_button(k: &Kit) -> El {
    Button::new("Find sounds")
        .on_press(State::open_find)
        .build(k)
        .key("find-sounds")
}

fn add_button(k: &Kit) -> El {
    Button::new("Add sounds…")
        .icon(&PLUS)
        .variant(ButtonVariant::Submit)
        .on_press_tx(|s: &mut State, tx| s.pick_sounds(tx))
        .build(k)
        .key("add-sounds")
}

/// A tile plays its sound when pressed. Trim and Remove show at its foot on hover, and its
/// context menu (right-click, the Menu key or Shift+F10) offers both from the keyboard.
fn sound_tile(s: &State, k: &Kit, layer: &mut Vec<El>, i: usize, sound: &Sound) -> El {
    let (id, playing) = (sound.id, s.is_playing(sound.id));
    let hotkey = if i < 9 {
        state::keycaps(&state::sound_key(i)).join("+")
    } else {
        String::new()
    };
    // Hover-only, so Tab skips them: the context menu is the keyboard's way in.
    let action = |glyph: &'static Icon, key: String, label: String, help: &str| {
        IconButton::new(glyph)
            .variant(IconButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .label(label)
            .help(help)
            .build(k)
            .key(key)
            .tab_index(-1)
    };
    let actions = row()
        .pin(None, None, Some(5.0), Some(5.0))
        .gap(2.0)
        .group_opacity(0.0, 1.0)
        .child(
            action(
                &SCISSORS,
                format!("trim-{}", id.0),
                format!("Trim {}", sound.name),
                "Trim",
            )
            .on_press(move |s: &mut State| s.trim_sound(id)),
        )
        .child(
            action(
                &CLOSE,
                format!("remove-{}", id.0),
                format!("Remove {}", sound.name),
                "Remove",
            )
            .on_press(move |s: &mut State| s.remove_sound(id)),
        );
    let tile = col()
        .key(format!("sound-{}", id.0))
        .role("button")
        .label(if playing {
            format!("Play {} (playing)", sound.name)
        } else {
            format!("Play {}", sound.name)
        })
        .focusable(true)
        .w_full()
        .h(72.0)
        .pt(9.0)
        .pb(8.0)
        .px(10.0)
        .radius(10.0)
        .fill(if playing {
            wash(k.c.layer_01, accent(), 0.16)
        } else {
            k.c.layer_01
        })
        .border(
            if playing { 1.5 } else { 1.0 },
            if playing { accent() } else { k.c.border_muted },
        )
        .hover_group()
        .hover(k.c.hover)
        .press(k.c.pressed)
        .on_press(move |s: &mut State| s.play(id))
        .press_scale(0.96)
        .on_secondary_press(move |s: &mut State, p, _| s.sound_menu = Some((id, p)))
        .justify_between()
        .child(
            text(sound.name.clone())
                .font(LABEL)
                .color(k.c.text_base)
                .max_lines(2)
                .w_full()
                .help_when_truncated(sound.name.clone())
                .a11y_hidden(),
        )
        .child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .group_opacity(1.0, 0.0)
                .child(
                    text(hotkey)
                        .font(TINY)
                        .color(if playing {
                            accent().alpha(0.85)
                        } else {
                            k.c.text_faint
                        })
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .child(
                    icon(if playing { &PLAYING } else { &PLAY }, ICON).color(if playing {
                        accent()
                    } else {
                        k.c.icon_faint
                    }),
                ),
        )
        .child(actions);
    let point = s.sound_menu.filter(|(m, _)| *m == id).map(|(_, p)| p);
    rune_kit::context_menu(format!("sound-menu-{}", id.0), tile, point)
        .item(MenuItem::new("Trim…").on_select(move |s: &mut State| {
            s.sound_menu = None;
            s.trim_sound(id);
        }))
        .item(MenuItem::new("Remove").on_select(move |s: &mut State| {
            s.sound_menu = None;
            s.remove_sound(id);
        }))
        .on_close(|s: &mut State| s.sound_menu = None)
        .build(k, layer)
}

fn empty_state(k: &Kit) -> El {
    col()
        .key("empty")
        .w_full()
        .grow(1.0)
        .gap(10.0)
        .center()
        .px(24.0)
        .radius(14.0)
        .border(1.5, k.c.border_muted)
        .border_dash(6.0, 5.0)
        .child(
            el().size(52.0, 52.0)
                .radius(26.0)
                .fill(k.c.layer_02)
                .center()
                .mb(4.0)
                .child(icon(&SPEAKER, ICON_TILE).color(k.c.icon_muted)),
        )
        .child(text("No sounds yet").font(HEADING).color(k.c.text_base))
        .child(
            text("Add WAV, MP3, OGG or FLAC files, or drop them anywhere on this window.")
                .font(BODY)
                .color(k.c.text_muted)
                .align(Align::Center)
                .wrap()
                .max_w(250.0),
        )
        .child(el().h(6.0))
        .child(row().gap(8.0).child(add_button(k)).child(find_button(k)))
        .child(
            text("The first nine play with Ctrl+Shift+1 to 9.")
                .font(SMALL)
                .color(k.c.text_faint)
                .mt(4.0),
        )
}

/// A dismissable line about something that went wrong, such as a file that didn't load, or
/// else the Undo for a sound just removed.
pub(crate) fn notice(s: &State, k: &Kit) -> Option<El> {
    if s.notice.is_none()
        && let Some(r) = &s.removed
    {
        return Some(removed_row(k, r));
    }
    let message = s.notice.clone()?;
    Some(
        row()
            .key("notice")
            .role("alert")
            .label(message.clone())
            .w_full()
            .pl(12.0)
            .pr(6.0)
            .py(8.0)
            .gap(8.0)
            .items_start()
            .radius(10.0)
            .fill(wash(k.c.page, danger(), 0.12))
            .border(1.0, danger().alpha(0.35))
            .child(icon(&WARNING, ICON).color(danger()).mt(1.0))
            .child(
                // One paragraph per line: each file that didn't load, say.
                col()
                    .gap(4.0)
                    .grow(1.0)
                    .shrink()
                    .mt(1.0)
                    .a11y_hidden()
                    .children(message.lines().map(|line| {
                        text(line.to_string())
                            .font(SMALL)
                            .color(k.c.text_base)
                            .wrap()
                            .w_full()
                    })),
            )
            .child(
                IconButton::new(&CLOSE)
                    .variant(IconButtonVariant::GhostMuted)
                    .control_size(ControlSize::Small)
                    .label("Dismiss")
                    .on_press(|s: &mut State| s.notice = None)
                    .build(k)
                    .key("dismiss-notice"),
            ),
    )
}

fn removed_row(k: &Kit, r: &Removed) -> El {
    let says = format!("Removed “{}”.", r.name);
    row()
        .key("removed")
        .role("status")
        .label(says.clone())
        .w_full()
        .pl(12.0)
        .pr(6.0)
        .py(6.0)
        .gap(8.0)
        .center_y()
        .radius(10.0)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_muted)
        .child(
            text(says)
                .font(SMALL)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
        .child(
            Button::new("Undo")
                .control_size(ControlSize::Small)
                .on_press(State::undo_remove)
                .build(k)
                .key("undo-remove"),
        )
        .child(
            IconButton::new(&CLOSE)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Dismiss")
                .on_press(State::forget_removed)
                .build(k)
                .key("dismiss-removed"),
        )
}

// --- Setup ---

fn setup(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut page = col().key("setup");
    if s.cable_missing() {
        page = page.child(cable_callout(k));
    }
    page.children(hotkeys_callout(s, k))
        .child(devices_card(s, k, layer))
        .child(remote_view::remote_card(s, k))
        .child(discord_card(s, k))
        .child(hotkeys_card(s, k))
}

/// The warning box at the top of Setup, as the VB-CABLE one: a title and what to do.
fn callout(k: &Kit, key: &str, title: &str) -> El {
    col()
        .key(key)
        .w_full()
        .padding(14.0)
        .gap(10.0)
        .radius(12.0)
        .fill(wash(k.c.page, warn(), 0.1))
        .border(1.0, warn().alpha(0.4))
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(icon(&WARNING, ICON).color(warn()))
                .child(text(title.to_string()).font(HEADING).color(k.c.text_base)),
        )
}

/// Hotkeys that didn't register, one line per reason, where Setup opens.
fn hotkeys_callout(s: &State, k: &Kit) -> Option<El> {
    let problems = hotkey_problems(s, None);
    if problems.is_empty() {
        return None;
    }
    Some(
        callout(k, "hotkeys-callout", "Some hotkeys don’t work").children(
            problems.into_iter().map(|says| {
                col().role("alert").label(says.clone()).w_full().child(
                    text(says)
                        .font(BODY)
                        .color(k.c.text_muted)
                        .wrap()
                        .a11y_hidden(),
                )
            }),
        ),
    )
}

fn cable_callout(k: &Kit) -> El {
    callout(k, "cable-callout", "VB-CABLE isn’t installed")
        .child(
            text(
                "Warble sends your voice to Discord and games through VB-CABLE, a free virtual audio cable. \
                 Install it, restart your PC, then open Warble again.",
            )
            .font(BODY)
            .color(k.c.text_muted)
            .wrap(),
        )
        .child(
            row().child(
                Button::new("Get VB-CABLE")
                    .icon(&OPEN)
                    .variant(ButtonVariant::Submit)
                    .on_press_tx(|s: &mut State, tx| {
                        if let Err(e) = tx.open_url(VB_CABLE_URL) {
                            s.notice = Some(format!(
                                "Couldn’t open your browser: {e}. Go to vb-audio.com/Cable to get \
                                 VB-CABLE."
                            ));
                        }
                    })
                    .build(k)
                    .key("get-cable"),
            ),
        )
}

fn devices_card(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let st = &s.status;
    // The engine's whole message, under the picker whose device it names, else at the foot.
    let error = st.error.as_deref().filter(|_| !s.cable_missing());
    let at = error.and_then(|e| error_picker(s, e));
    let error_line = |under: Option<Picker>| {
        error.filter(|_| at == under).map(|e| {
            row()
                .key("device-error")
                .role("alert")
                .label(e.to_string())
                .w_full()
                .gap(8.0)
                .items_start()
                .child(icon(&WARNING, ICON).color(danger()).mt(1.0))
                .child(
                    text(e.to_string())
                        .font(SMALL)
                        .color(k.c.text_base)
                        .wrap()
                        .grow(1.0)
                        .shrink()
                        .a11y_hidden(),
                )
        })
    };
    card(k)
        .key("devices")
        .gap(14.0)
        .child(text("Devices").font(HEADING).color(k.c.text_base))
        .child(
            picker(
                s,
                k,
                layer,
                Picker::Input,
                "Microphone",
                "System default",
                &s.devices.inputs,
                st.input_name.as_deref(),
            )
            .children(error_line(Some(Picker::Input))),
        )
        .child(
            picker(
                s,
                k,
                layer,
                Picker::Cable,
                "Virtual cable",
                "Auto-detect VB-CABLE",
                &s.devices.outputs,
                st.cable_name.as_deref(),
            )
            .children(error_line(Some(Picker::Cable))),
        )
        .child(
            picker(
                s,
                k,
                layer,
                Picker::Headphones,
                "Headphones",
                "System default",
                &s.devices.outputs,
                st.monitor_name.as_deref(),
            )
            .children(error_line(Some(Picker::Headphones))),
        )
        .children(error_line(None))
}

/// The picker a device error is about: the one whose device (chosen, or the system default)
/// the message names.
fn error_picker(s: &State, e: &str) -> Option<Picker> {
    fn default(list: &[DeviceInfo]) -> Option<&String> {
        list.iter().find(|d| d.is_default).map(|d| &d.name)
    }
    [
        (
            Picker::Input,
            s.settings.input.as_ref().or(default(&s.devices.inputs)),
        ),
        (Picker::Cable, s.settings.cable.as_ref()),
        (
            Picker::Headphones,
            s.settings
                .headphones
                .as_ref()
                .or(default(&s.devices.outputs)),
        ),
    ]
    .into_iter()
    .find(|(_, name)| name.is_some_and(|n| e.contains(n.as_str())))
    .map(|(p, _)| p)
}

/// A reference section of Setup that opens and closes, titled like the cards around it.
fn section(k: &Kit, title: &str, open: bool, toggle: fn(&mut State), content: El) -> El {
    let mut heading = k.clone();
    heading.ty.text_small_medium = HEADING;
    Collapsible::new(title, open)
        // Its arrow always shows, so a closed section reads as one that opens.
        .state(rune_kit::State::Hover)
        .on_toggle(toggle)
        .child(content.w_full().pt(6.0))
        .build(&heading)
}

#[allow(clippy::too_many_arguments)]
fn picker(
    s: &State,
    k: &Kit,
    layer: &mut Vec<El>,
    which: Picker,
    label: &'static str,
    default: &'static str,
    devices: &[DeviceInfo],
    using: Option<&str>,
) -> El {
    let chosen = match which {
        Picker::Input => &s.settings.input,
        Picker::Cable => &s.settings.cable,
        Picker::Headphones => &s.settings.headphones,
    };
    let mut names = vec![default.to_string()];
    names.extend(devices.iter().map(|d| d.name.clone()));
    // A saved device that's unplugged still shows as the choice.
    if let Some(c) = chosen
        && !names.contains(c)
    {
        names.push(c.clone());
    }
    let key = match which {
        Picker::Input => "pick-input",
        Picker::Cable => "pick-cable",
        Picker::Headphones => "pick-headphones",
    };
    let select = Select::new(key, &names)
        .label(label)
        .current(Some(chosen.as_deref().unwrap_or(default)))
        .appearance(SelectAppearance::Field)
        .open(s.open_picker == Some(which))
        .on_toggle(move |s: &mut State| {
            s.open_picker = if s.open_picker == Some(which) {
                None
            } else {
                Some(which)
            }
        })
        .on_close(|s: &mut State| s.open_picker = None)
        .on_choose(move |s: &mut State, v: &str, _| {
            s.set_device(which, (v != default).then(|| v.to_string()))
        })
        .build(k, layer)
        .w_full();
    // With the default chosen, say which device that is.
    let using = using.filter(|_| chosen.is_none()).map(|u| {
        text(if which == Picker::Cable {
            format!("Sending to {u}")
        } else {
            format!("Using {u}")
        })
        .font(SMALL)
        .color(k.c.text_faint)
        .truncate()
    });
    col()
        .w_full()
        .gap(6.0)
        .child(text(label).font(LABEL).color(k.c.text_muted))
        .child(select)
        .children(using)
}

fn discord_card(s: &State, k: &Kit) -> El {
    let step = |n: &str, line: &str| {
        row()
            .w_full()
            .gap(10.0)
            .items_start()
            .child(
                el().size(18.0, 18.0)
                    .radius(9.0)
                    .fill(k.c.layer_03)
                    .center()
                    .a11y_hidden()
                    .child(text(n.to_string()).font(TINY).color(k.c.text_base)),
            )
            .child(
                text(line.to_string())
                    .font(BODY)
                    .color(k.c.text_base)
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            )
    };
    let steps = col()
        .gap(GAP)
        .child(step("1", "Open Settings > Voice & Video."))
        .child(step("2", "Set Input device to “CABLE Output (VB-Audio Virtual Cable)”."))
        .child(step("3", "Turn off Noise suppression (Krisp) and Echo cancellation, or Discord strips the effects out."))
        .child(
            row()
                .w_full()
                .pt(4.0)
                .child(
                    text("In games, pick “CABLE Output” as the mic in the game’s voice chat settings.")
                        .font(SMALL)
                        .color(k.c.text_muted)
                        .wrap()
                        .grow(1.0)
                        .shrink(),
                ),
        );
    card(k).key("discord").child(section(
        k,
        "Set up Discord",
        s.discord_open,
        |s| s.discord_open = !s.discord_open,
        steps,
    ))
}

fn hotkeys_card(s: &State, k: &Kit) -> El {
    let presets: Vec<String> = (0..8).map(state::preset_key).collect();
    let sounds: Vec<String> = (0..9).map(state::sound_key).collect();
    let rows: [(&str, Vec<String>, Vec<String>); 5] = [
        (
            "Voice effect on or off",
            vec![state::EFFECT_KEY.into()],
            state::keycaps(state::EFFECT_KEY),
        ),
        (
            "Pick a voice",
            presets,
            vec!["Ctrl".into(), "Alt".into(), "1–8".into()],
        ),
        (
            "Hear yourself on or off",
            vec![state::MONITOR_KEY.into()],
            state::keycaps(state::MONITOR_KEY),
        ),
        (
            "Stop all sounds",
            vec![state::STOP_KEY.into()],
            state::keycaps(state::STOP_KEY),
        ),
        (
            "Play sounds 1–9",
            sounds,
            vec!["Ctrl".into(), "Shift".into(), "1–9".into()],
        ),
    ];
    let mut c = col().gap(10.0).child(
        text("These work while you’re in a game or call.")
            .font(SMALL)
            .color(k.c.text_muted),
    );
    for (what, chords, caps) in rows {
        let mut line = col().w_full().gap(4.0).child(
            row()
                .w_full()
                .h(20.0)
                .gap(8.0)
                .center_y()
                .child(
                    text(what)
                        .font(BODY)
                        .color(k.c.text_base)
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .child(Keybind::new(&caps).build(k)),
        );
        // The callout at the top of Setup is the alert; these lines say it again in place.
        for says in hotkey_problems(s, Some(&chords)) {
            line = line.child(
                row()
                    .label(says.clone())
                    .gap(6.0)
                    .items_start()
                    .child(icon(&WARNING, ICON).color(danger()).mt(1.0))
                    .child(
                        text(says)
                            .font(SMALL)
                            .color(k.c.text_base)
                            .wrap()
                            .grow(1.0)
                            .shrink()
                            .a11y_hidden(),
                    ),
            );
        }
        c = c.child(line);
    }
    card(k).key("hotkeys").child(section(
        k,
        "Hotkeys",
        s.hotkeys_open,
        |s| s.hotkeys_open = !s.hotkeys_open,
        c,
    ))
}

/// The hotkeys that didn't register (those among `chords`, or all of them) as sentences, one
/// per reason: "Another app is using Ctrl+Shift+1 and Ctrl+Shift+2. Close that app, …".
pub(crate) fn hotkey_problems(s: &State, chords: Option<&[String]>) -> Vec<String> {
    let mut reasons: Vec<(&str, Vec<String>)> = Vec::new();
    for (chord, why) in &s.hotkey_failures {
        if chords.is_some_and(|c| !c.contains(chord)) {
            continue;
        }
        let caps = state::keycaps(chord).join("+");
        match reasons.iter_mut().find(|(w, _)| w == why) {
            Some((_, keys)) => keys.push(caps),
            None => reasons.push((why, vec![caps])),
        }
    }
    reasons
        .into_iter()
        .map(|(why, keys)| {
            let list = match keys.split_last() {
                Some((last, [])) => last.clone(),
                Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
                None => String::new(),
            };
            why.replace("{keys}", &list)
        })
        .collect()
}

// --- Scenes for headless snapshots ---

type Scene = (&'static str, fn(&mut State));

pub const SCENES: &[Scene] = &[
    ("voice", |s| s.select_preset(Preset::Deep)),
    ("voice-off", |s| {
        s.select_preset(Preset::Robot);
        s.set_effect(false);
    }),
    ("soundboard", |s| {
        s.add_sounds(
            [
                "Air horn.mp3",
                "Bruh.wav",
                "Sad trombone.ogg",
                "Victory fanfare (extended stadium remix).mp3",
                "Wilhelm scream.flac",
                "Ba dum tss.wav",
                "Crowd goes absolutely wild after the last-second winning goal.mp3",
            ]
            .map(std::path::PathBuf::from)
            .to_vec(),
        );
        let id = s
            .sounds
            .iter()
            .find(|x| x.name == "Sad trombone")
            .map(|x| x.id);
        if let Some(id) = id {
            s.play(id);
        }
    }),
    ("soundboard-empty", |s| s.set_tab(Tab::Soundboard)),
    ("setup", |s| s.set_tab(Tab::Setup)),
    ("no-cable", |s| {
        s.set_tab(Tab::Setup);
        s.sim_no_cable = true;
        s.devices.inputs.retain(|d| !d.name.contains("CABLE"));
        s.devices.outputs.retain(|d| !d.name.contains("CABLE"));
        s.refresh_status();
    }),
    // Not asked for, but worth a look: a hotkey another app holds, and a file dragged over.
    ("hotkeys-failed", |s| {
        s.set_tab(Tab::Setup);
        let taken = crate::why(&rune::ShortcutError::Conflict);
        s.hotkey_failures = vec![
            (state::EFFECT_KEY.into(), taken.clone()),
            (state::sound_key(0), taken.clone()),
            (state::sound_key(1), taken),
        ];
    }),
    ("drop", |s| s.drop_hover = true),
    // The engine can't open the chosen mic: the pill says "Audio problem", Setup says why.
    ("audio-problem", |s| {
        s.set_tab(Tab::Setup);
        s.settings.input = Some("Blue Yeti".into());
        s.sim_error = Some("Couldn't open Blue Yeti: another app is using it".into());
        s.refresh_status();
    }),
    ("removed", |s| {
        s.add_sounds(
            [
                "Air horn.mp3",
                "Bruh.wav",
                "Sad trombone.ogg",
                "Wilhelm scream.flac",
            ]
            .map(std::path::PathBuf::from)
            .to_vec(),
        );
        if let Some(id) = s.sounds.iter().find(|x| x.name == "Bruh").map(|x| x.id) {
            s.remove_sound(id);
        }
    }),
    ("remote", remote_view::scenes::on),
    ("remote-tailscale", remote_view::scenes::tailscale),
    ("remote-no-network", remote_view::scenes::no_network),
    ("find-start", find_view::scenes::start),
    ("find", find_view::scenes::results),
    ("find-offline", find_view::scenes::offline),
    ("youtube-install", find_view::scenes::youtube_install),
    ("trim", find_view::scenes::trim),
    // Three files that didn't load, in the engine's words.
    ("load-error", |s| {
        s.add_sounds(
            ["Air horn.mp3", "Bruh.wav"]
                .map(std::path::PathBuf::from)
                .to_vec(),
        );
        s.loaded((
            Vec::new(),
            vec![
                "Warble can’t play “Broken clip.mp3”. It may be damaged, or renamed from another \
                 kind of file. Try another copy."
                    .into(),
                "There’s no sound in “Silence.wav”. Try another file.".into(),
                "Couldn’t find “Old take.flac”. If you moved it, add it again from its new folder."
                    .into(),
            ],
        ));
    }),
];
