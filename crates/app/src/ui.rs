//! The window: a header with the app's status, three tabs, and a drop zone around them.
//!
//! The look is a piece of gear on the desk: a graphite body, raised pads with a rim lit from
//! the top, panel lettering in Martian Mono, and one LED colour ([`led`]) that lights only
//! while audio is going into the call.

use rune::tok::TextStyle;
use rune::{
    Align, Color, Cursor, Cx, Drag, El, Family, Icon, LiveRegion, Phase, ScrollSpec, Shadow,
    Spring, TextCase, Tx, col, el, icon, oklch, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Keybind, Kit, MenuItem,
    PaletteFocus, PaletteSeed, Radius, Select, SelectAppearance, StateColors, Switch,
};
use warble_engine::{DeviceInfo, Preset};

use crate::find_view;
use crate::remote_view;
use crate::settings::DROP_EXTS;
use crate::state::{self, Picker, REMOVAL_NEWS, Removed, SEGMENTS, Sound, State, Tab};

pub const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";

// --- Look ---

pub(crate) const PAD: f32 = 16.0;
pub(crate) const GAP: f32 = 12.0;
/// The width of the printed label in front of a meter or fader.
pub(crate) const LABEL_W: f32 = 66.0;

// Whole sizes, on rune-kit's 13 (controls) and 11 (captions).
pub(crate) const TITLE: TextStyle = TextStyle::new(16.0, 650.0);
pub(crate) const HEADING: TextStyle = TextStyle::new(14.0, 600.0);
pub(crate) const BODY: TextStyle = TextStyle::new(13.0, 400.0).lh(19.0);
pub(crate) const LABEL: TextStyle = TextStyle::new(13.0, 500.0);
pub(crate) const SMALL: TextStyle = TextStyle::new(12.0, 400.0).lh(18.0);
/// What the voice effect is doing, beside its footswitch.
const DISPLAY: TextStyle = TextStyle::new(18.0, 650.0);
/// A name on a pad.
const PAD_NAME: TextStyle = TextStyle::new(14.0, 600.0).lh(18.0);

/// The bundled faces: Inter for what you read, Martian Mono for what's printed on the panel
/// (pad numbers, labels, values). Registering any font replaces Rune's built-in Inter, so
/// Inter comes along.
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/MartianMono-Variable.ttf"),
];
const PANEL: Family = Family::Named("Martian Mono");
/// Martian Mono a step narrower than its default, so labels sit like printed ones.
const PANEL_WIDTH: &str = "'wdth' 87.5";
/// Values and times: 100%, 0:02.6.
pub(crate) const NUMBER: TextStyle = TextStyle::new(12.0, 500.0)
    .family(PANEL)
    .variations(PANEL_WIDTH);
/// Panel lettering: section titles and the labels on meters and faders, drawn in capitals
/// by [`printed`].
pub(crate) const PRINT: TextStyle = TextStyle::new(10.0, 600.0)
    .family(PANEL)
    .variations(PANEL_WIDTH)
    .tracking(0.4);
/// The number on a pad, the key you press for it.
pub(crate) const DIGIT: TextStyle = TextStyle::new(18.0, 600.0)
    .family(PANEL)
    .variations(PANEL_WIDTH);

/// A pad's corners: the radius scale's `lg`, which pads use before the kit is to hand.
const PAD_RADIUS: f32 = 6.0;

/// Icons beside text, as rune-kit's buttons draw them (a 1.5 point stroke).
pub(crate) const ICON: f32 = 16.0;
/// The large glyphs printed on pads: the voices, the drop pad's plus, the logo.
pub(crate) const ICON_PAD: f32 = 24.0;

/// The graphite at lightness `l`, for the body, rims, caps and slots.
pub(crate) fn tone(l: f32) -> Color {
    oklch(l, 0.006, 70.0)
}
/// The graphite body.
pub(crate) fn bg() -> Color {
    tone(0.205)
}
/// The face of a raised pad or key, a step up from the body.
pub(crate) fn face() -> Color {
    tone(0.255)
}
/// A recess in the body: the tab tray, empty sockets, the waveform's well, the readout.
pub(crate) fn recess() -> Color {
    tone(0.17)
}
/// The edge of a control on the body, 3:1 against it: fields, menus, switch and fader slots,
/// and the footswitch's ring while it's on. Keys and latched pads take it only along their
/// lit top rim, and other pads have a dimmer one: their printed labels say what they are.
fn edge() -> Color {
    tone(0.54)
}
/// What's on or filled, in graphite: a switch's track when on, a fader's travel.
fn filled() -> Color {
    tone(0.55)
}
/// The light cap you grab: a fader's, a switch's, a Trim handle's.
pub(crate) fn cap() -> Color {
    tone(0.9)
}

/// A fader cap `w` by `h`, raised and lit from the top, with its grip line (`grip` w by h).
pub(crate) fn fader_cap(k: &Kit, (w, h): (f32, f32), grip: (f32, f32)) -> El {
    el().size(w, h)
        .radius(k.radius.xs)
        .center()
        .fill(cap())
        .gradient(Color::WHITE.alpha(0.18), Color::BLACK.alpha(0.2))
        .border_gradient(1.0, tone(0.96), tone(0.55))
        .shadow(2.0, 4.0, Color::BLACK.alpha(0.6))
        .child(el().size(grip.0, grip.1).fill(tone(0.3)))
}
/// The LED, the one colour that means "going into the call". It lights only for the mic
/// going out (the Live dot), the voice effect and the chosen voice while the changed voice
/// goes out, a sound playing into the call, and Stop all while one does. Nothing that stays on this PC (Trim, a
/// preview, Hear yourself, a selection, a sound just added) uses it.
pub(crate) fn led() -> Color {
    oklch(0.72, 0.18, 42.0)
}
pub(crate) fn warn() -> Color {
    oklch(0.83, 0.13, 80.0)
}
pub(crate) fn danger() -> Color {
    oklch(0.68, 0.18, 18.0)
}
/// The level meters' own green, below their amber and red.
fn signal() -> Color {
    oklch(0.80, 0.17, 145.0)
}

pub fn kit() -> Kit {
    let fg = oklch(0.94, 0.008, 80.0);
    // Switches and selections take a light graphite, not the LED: they send nothing into the
    // call. Focus rings are the ink colour, which reads on every surface here.
    let mut k = Kit::from_palette_focus(
        PaletteSeed {
            bg: bg(),
            fg,
            accent: tone(0.62),
        },
        PaletteFocus::Ink,
    );
    // Faint text carries hotkeys and device names, so it has to read: 5.5:1 on a pad.
    k.c.text_faint = oklch(0.68, 0.008, 70.0);
    k.c.icon_faint = k.c.text_faint;
    // A switch reads without colour: its light cap stands left in a dark slot when off and
    // right on a filled track when on (3.6:1 cap to track, 3.7:1 track to body), with a soft
    // sheen on top rather than a chrome one.
    k.c.knob = cap();
    k.c.knob_on = cap();
    k.c.layer_04 = recess();
    k.c.bg_accent = filled();
    k.elevation.switch_off = vec![Shadow {
        spread: 1.0,
        color: edge(),
        ..Shadow::default()
    }];
    k.elevation.switch_on = Vec::new();
    k.c.depth_top = Color::WHITE.alpha(0.22);
    // Fields are slots in the body (see `slot`); menus and lists have the controls' edge.
    k.c.bg_base = recess();
    k.c.border_strong = edge();
    k.ring = Some(|c| c.border_strong);
    // Keys are the pads' graphite, raised by `as_key`; the main one is a lighter key, not a
    // white pill.
    k.elevation.button_neutral = vec![Shadow {
        y: 1.0,
        blur: 2.0,
        color: Color::BLACK.alpha(0.45),
        ..Shadow::default()
    }];
    k.c.bg_button_neutral = face();
    k.c.bg_icon_button_contrast = tone(0.36);
    k.c.submit_fg = k.c.text_base;
    k.c.submit_icon = k.c.text_base;
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
    // One radius scale: keycaps and meter segments, small keys, keys and fields, pads, trays.
    k.radius = Radius {
        xs: 2.0,
        sm: 4.0,
        md: 5.0,
        lg: PAD_RADIUS,
        xl: 8.0,
    };
    k
}

/// The kit for text fields and pickers: slots in the body with an edge 3:1 against it, so
/// they read as places to type or choose.
pub(crate) fn slot(k: &Kit) -> Kit {
    let mut k = k.clone();
    k.elevation.button_neutral = vec![Shadow {
        spread: 1.0,
        color: edge(),
        ..Shadow::default()
    }];
    k
}

/// The kit for menus and the pickers' lists: the row the pointer or the keys are on lights in
/// the filled graphite, 3.3:1 against the menu, with white words on it (4.9:1).
pub(crate) fn menu(k: &Kit) -> Kit {
    let mut k = k.clone();
    k.c.hover = filled();
    k.c.text_base = Color::WHITE;
    k
}

/// A kit button as a key on the panel: raised like a pad, its rim lit from the top.
pub(crate) fn as_key(e: El) -> El {
    e.gradient(Color::WHITE.alpha(0.04), Color::BLACK.alpha(0.12))
        .border_gradient(1.0, edge(), tone(0.19))
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

/// A group of controls on the panel: no box, just a rule above it and room around it.
pub(crate) fn group(k: &Kit) -> El {
    col().w_full().pt(16.0).gap(GAP).box_shadow(Shadow {
        y: 1.0,
        color: k.c.border_base,
        inset: true,
        ..Shadow::default()
    })
}

/// The first group on a page, right under the tabs: the tray above already marks the edge, so
/// it has no rule.
pub(crate) fn first_group() -> El {
    col().w_full().gap(GAP)
}

/// Panel lettering, in capitals. The words stay as written for screen readers.
pub(crate) fn printed(k: &Kit, label: &str) -> El {
    text(label.to_string())
        .font(PRINT)
        .color(k.c.text_muted)
        .text_case(TextCase::Uppercase)
}

/// A group's title row: its printed name, and whatever is added at its right (a switch).
pub(crate) fn title_row(k: &Kit, title: &str) -> El {
    row()
        .w_full()
        .min_h(24.0)
        .gap(10.0)
        .center_y()
        .child(printed(k, title).color(k.c.text_base))
}

/// A message on the panel's readout: a dark strip set into the body, for something that
/// just happened (a file that didn't load, a sound removed or added).
pub(crate) fn readout(k: &Kit) -> El {
    row()
        .w_full()
        .min_h(40.0)
        .pl(12.0)
        .pr(6.0)
        .py(6.0)
        .gap(10.0)
        .radius(k.radius.lg)
        .fill(recess())
        .box_shadow(Shadow {
            y: 1.0,
            blur: 3.0,
            color: Color::BLACK.alpha(0.6),
            inset: true,
            ..Shadow::default()
        })
}

/// A problem on a line: its icon in warn or danger, the words in body ink.
pub(crate) fn problem_line(k: &Kit, colour: Color, font: TextStyle, says: &str) -> El {
    row()
        .w_full()
        .gap(8.0)
        .items_start()
        .child(icon(&WARNING, ICON).color(colour).mt(1.0))
        .child(
            text(says.to_string())
                .font(font)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
}

/// A section that opens and closes, built as rune-kit's Collapsible is (a `trigger` button
/// that says whether it's expanded, then the `content` region) but with a printed title.
/// Its arrow always shows, so a closed section reads as one that opens.
pub(crate) fn disclosure(
    k: &Kit,
    title: &str,
    open: bool,
    toggle: fn(&mut State),
    content: El,
) -> El {
    let arrow = if open {
        k.glyph.triangle_down
    } else {
        k.glyph.triangle_right
    };
    // The hover fill reaches past the title on both sides, so the title lines up with the
    // printed titles around it and still has room inside the fill. In a group with `pt(12)`,
    // the title and what opens under it sit where a group's title and content do.
    let trigger = row()
        .key("trigger")
        .role("button")
        .label(title.to_string())
        .expanded(open)
        .focusable(true)
        .mx(-8.0)
        .px(8.0)
        .h(32.0)
        .gap(6.0)
        .center_y()
        .radius(k.radius.md)
        .cursor(Cursor::Pointer)
        .hover(k.c.hover)
        .press(k.c.pressed)
        .on_press(toggle)
        .child(
            printed(k, title)
                .color(k.c.text_base)
                .shrink()
                .truncate()
                .a11y_hidden(),
        )
        .child(icon(arrow, ICON).color(k.c.icon_muted).a11y_hidden());
    let mut root = col().role("group").w_full().child(trigger);
    if open {
        root = root.child(
            col()
                .key("content")
                .role("region")
                .label(title.to_string())
                .w_full()
                .pt(8.0)
                .child(content.w_full()),
        );
    }
    root
}

/// A recessed tray of keys; the chosen one stands raised. The arrow keys move between them,
/// as in a segmented control.
pub(crate) fn tray(
    k: &Kit,
    label: &str,
    names: &[&str],
    current: usize,
    pick: fn(&mut State, usize),
) -> El {
    let mut tray = row()
        .role("radiogroup")
        .label(label.to_string())
        .focus_group()
        .w_full()
        .h(36.0)
        .padding(3.0)
        .gap(3.0)
        .radius(k.radius.xl)
        .fill(recess())
        .box_shadow(Shadow {
            y: 1.0,
            blur: 3.0,
            color: Color::BLACK.alpha(0.6),
            inset: true,
            ..Shadow::default()
        });
    for (i, name) in names.iter().enumerate() {
        let on = i == current;
        let key = row()
            .key(format!("segment{i}"))
            .role("radio")
            .label(name.to_string())
            .checked(on)
            .focusable(true)
            .focus_selected(on)
            .position_in_set(i + 1, names.len())
            .grow(1.0)
            .shrink()
            .h_full()
            .center()
            .on_press(move |s: &mut State| pick(s, i))
            .child(
                printed(k, name)
                    .font(TextStyle {
                        size: 11.0,
                        ..PRINT
                    })
                    .color(if on { k.c.text_base } else { k.c.text_muted })
                    .truncate()
                    .a11y_hidden(),
            );
        tray = tray.child(if on {
            raise(key, Lamp::Latched).radius(k.radius.md)
        } else {
            key.radius(k.radius.md).hover(k.c.hover).press(k.c.pressed)
        });
    }
    tray
}

/// How lit a pad is: dark, latched (chosen, but nothing is going out), or lit by the LED.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Lamp {
    Off,
    Latched,
    Lit,
}

/// `e` as a raised pad with a rim lit from the top. A lit pad glows from underneath.
pub(crate) fn raise(e: El, lamp: Lamp) -> El {
    let (fill, rim_top, rim_bottom) = match lamp {
        Lamp::Off => (face(), tone(0.38), tone(0.17)),
        Lamp::Latched => (tone(0.31), edge(), tone(0.19)),
        Lamp::Lit => (
            wash(face(), led(), 0.2),
            wash(face(), led(), 0.85),
            wash(face(), led(), 0.3),
        ),
    };
    let e = e
        .radius(PAD_RADIUS)
        .fill(fill)
        .gradient(Color::WHITE.alpha(0.03), Color::BLACK.alpha(0.14))
        .border_gradient(1.0, rim_top, rim_bottom)
        .shadow(2.0, 4.0, Color::BLACK.alpha(0.55));
    if lamp == Lamp::Lit {
        e.linear_gradient(0.0, [(0.0, led().alpha(0.34)), (1.0, Color::TRANSPARENT)])
            .shadow(0.0, 18.0, led().alpha(0.3))
    } else {
        e
    }
}

/// A small LED, lit or dark.
fn led_dot(lit: bool) -> El {
    let dot = el().size(7.0, 7.0).radius(3.5).a11y_hidden();
    if lit {
        dot.fill(wash(led(), Color::WHITE, 0.2))
            .shadow(0.0, 8.0, led().alpha(0.9))
    } else {
        dot.fill(tone(0.32))
    }
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
    // One keyboard focus ring for every control, in the panel's ink.
    cx.focus_ring(2.0, 2.0, k.c.border_focus);
    let mut layer = Vec::new();
    let body = match s.tab {
        Tab::Voice => scrolling(voice(s, cx, &k)),
        Tab::Soundboard if s.trim.is_some() => find_view::trim_page(s, cx, &k),
        Tab::Soundboard if s.find.open => find_view::find_page(s, &k),
        Tab::Soundboard => soundboard(s, cx, &k, &mut layer),
        Tab::Setup => scrolling(setup(s, cx, &k, &mut layer)),
    };
    let mut root = col()
        .key("app")
        .w_full()
        .h_full()
        .fill(k.c.page)
        .child(header(s, &k))
        .child(tabs(s, &k))
        .children(notice(s, &k))
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
            // A control brought into view stops clear of the fade, its ring whole.
            .scroll_margin(14.0)
            .child(content.w_full().px(PAD).pb(PAD).gap(GAP)),
    )
}

fn header(s: &State, k: &Kit) -> El {
    row()
        .key("header")
        .h(52.0)
        .px(PAD)
        .gap(GAP)
        .center_y()
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(
                    icon(&LOGO, ICON_PAD)
                        .key("logo")
                        .color(k.c.text_base)
                        .a11y_hidden(),
                )
                .child(text("Warble").font(TITLE).color(k.c.text_base)),
        )
        .child(el().grow(1.0))
        .child(status_pill(s, k))
}

/// Running, a lit LED printed "Live": your mic is going into the call. A problem shows its
/// icon on a key that takes you to Setup, where it's fixed, with the LED still lit beside it
/// while the mic goes out (hotkeys blocked, say). A device error is too long for the key, so
/// it says "Audio problem" and carries the rest. On Setup it only names the problem: the line
/// under the picker says the whole of it.
fn status_pill(s: &State, k: &Kit) -> El {
    let (label, tone) = s.status_label();
    let error = s.status.error.as_deref().filter(|_| tone == "danger");
    let key = tone != "live" && s.tab != Tab::Setup;
    let pill = row()
        // Its own key on Setup, so focus on the pressed key moves on into the page.
        .key(if s.tab == Tab::Setup {
            "setup-status"
        } else {
            "status"
        })
        .role("status")
        .label(match error.filter(|_| key) {
            Some(e) => format!("{label}: {e}"),
            None => label.to_string(),
        })
        .h(28.0)
        .gap(8.0)
        .center_y()
        .shrink()
        .max_w(230.0);
    let words = printed(k, label)
        .color(k.c.text_base)
        .truncate()
        .shrink()
        .a11y_hidden();
    let live = s.status.running.then(|| led_dot(true));
    let pill = match tone {
        "live" => return pill.children(live).child(words),
        "warn" => pill
            .children(live)
            .child(icon(&WARNING, ICON).color(warn()))
            .child(words),
        "danger" => pill
            .children(live)
            .child(icon(&WARNING, ICON).color(danger()))
            .child(words),
        _ => pill
            .child(led_dot(false))
            .child(words.color(k.c.text_muted)),
    };
    if !key {
        return pill;
    }
    // A key that takes you to where the problem is fixed, still read out when it changes.
    raise(pill.px(9.0), Lamp::Off)
        .role("button")
        .live_region(LiveRegion::Polite)
        .radius(k.radius.md)
        .help("Open Setup")
        .focusable(true)
        .focus_return("setup")
        .cursor(Cursor::Pointer)
        .hover(k.c.hover)
        .press(k.c.pressed)
        .on_press(|s: &mut State| s.set_tab(Tab::Setup))
}

fn tabs(s: &State, k: &Kit) -> El {
    const TABS: [Tab; 3] = [Tab::Voice, Tab::Soundboard, Tab::Setup];
    let current = TABS.iter().position(|t| *t == s.tab).unwrap_or(0);
    row().key("tabs").w_full().px(PAD).pb(12.0).child(tray(
        k,
        "Section",
        &["Voice", "Soundboard", "Setup"],
        current,
        |s, i| s.set_tab(TABS[i]),
    ))
}

/// Dropping files loads them onto the board: the app dims under one waiting pad.
fn drop_overlay(k: &Kit) -> El {
    el().key("drop")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(k.c.page.alpha(0.86))
        .center()
        .inert()
        .child(
            raise(col(), Lamp::Latched)
                .size(196.0, 132.0)
                .gap(12.0)
                .center()
                .child(icon(&PLUS, ICON_PAD).color(k.c.text_base))
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
    // Lit only while the changed voice really goes out: not while the engine is starting,
    // can't find VB-CABLE or has an audio problem.
    let live = s.voice_live();
    let detail = if on {
        format!("{} voice", s.settings.preset().name())
    } else {
        "Off: people hear your own voice".to_string()
    };
    // The line wraps rather than cut off its consequence, so the row grows to fit it. The
    // whole row takes a click, but a screen reader hears only the switch and the line under
    // it: "Voice effect" once, with its hotkey.
    row()
        .key("effect")
        .w_full()
        .min_h(70.0)
        .py(4.0)
        .gap(14.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .on_press(|s: &mut State| s.set_effect(!s.settings.effect))
        .role("none")
        .child(footswitch(k, on, live))
        .child(
            col()
                .grow(1.0)
                .shrink()
                .gap(6.0)
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(printed(k, "Voice effect").a11y_hidden())
                        .child(el().grow(1.0))
                        .child(keys(k, state::EFFECT_KEY).a11y_hidden()),
                )
                .child(
                    text(detail)
                        .font(if on {
                            DISPLAY
                        } else {
                            // On the On caption's line, so the label above stays put.
                            TextStyle {
                                weight: 500.0,
                                line_height: DISPLAY.size * 1.25,
                                ..TITLE
                            }
                        })
                        .color(if on { k.c.text_base } else { k.c.text_muted })
                        .wrap()
                        .w_full(),
                ),
        )
}

/// The voice effect's footswitch: a round stomp button with an LED ring that lights while
/// your changed voice is going into the call (`live`). On but not live, the ring is a key's
/// edge.
fn footswitch(k: &Kit, on: bool, live: bool) -> El {
    let mut ring = el()
        .key("effect-switch")
        .role("switch")
        .label("Voice effect")
        .checked(on)
        .shortcut_hint(state::keycaps(state::EFFECT_KEY).join("+"))
        .focusable(true)
        .focus_ring(2.0, 2.5, k.c.border_focus)
        .size(61.0, 61.0)
        .radius(30.5)
        .center()
        .border(
            2.0,
            if live {
                wash(led(), Color::WHITE, 0.1)
            } else if on {
                edge()
            } else {
                tone(0.32)
            },
        )
        .cursor(Cursor::Pointer)
        .hover(k.c.hover)
        .press(k.c.pressed)
        .press_scale(0.96)
        .on_press(|s: &mut State| s.set_effect(!s.settings.effect))
        .child(
            el().size(52.0, 52.0)
                .radius(26.0)
                .center()
                .fill(tone(0.24))
                .gradient(Color::WHITE.alpha(0.06), Color::BLACK.alpha(0.25))
                .border_gradient(1.0, tone(0.40), tone(0.15))
                .shadow(3.0, 6.0, Color::BLACK.alpha(0.6))
                .a11y_hidden()
                .child(
                    el().size(30.0, 30.0)
                        .radius(15.0)
                        .fill(tone(0.5))
                        .gradient(Color::WHITE.alpha(0.16), Color::BLACK.alpha(0.3))
                        .border_gradient(1.0, tone(0.72), tone(0.26))
                        .shadow(1.0, 3.0, Color::BLACK.alpha(0.5)),
                ),
        );
    if live {
        ring = ring.shadow(0.0, 14.0, led().alpha(0.45));
    }
    ring
}

fn preset_grid(s: &State, k: &Kit) -> El {
    let (live, current) = (s.voice_live(), s.settings.preset());
    let mut grid = el()
        .key("presets")
        .w_full()
        .equal_columns(4)
        .gaps(8.0, 8.0)
        .role("radiogroup")
        .label("Voice")
        .focus_group();
    for (i, p) in Preset::ALL.into_iter().enumerate() {
        grid = grid.child(preset_tile(k, p, i, p == current, live));
    }
    col().w_full().gap(10.0).child(grid).child(
        text("Ctrl+Alt+1 to 8 picks a voice.")
            .font(SMALL)
            .color(k.c.text_muted),
    )
}

/// A voice on a pad, numbered for its hotkey. The chosen one is latched, and lit while
/// the changed voice is going into the call (`live`).
fn preset_tile(k: &Kit, p: Preset, i: usize, selected: bool, live: bool) -> El {
    let lamp = match (selected, live) {
        (true, true) => Lamp::Lit,
        (true, false) => Lamp::Latched,
        _ => Lamp::Off,
    };
    let ink = if selected {
        k.c.text_base
    } else {
        k.c.text_muted
    };
    let mut tile = raise(col(), lamp)
        .key(format!("preset-{}", p.id()))
        .role("radio")
        .label(p.name())
        .checked(selected)
        // The printed number is hidden; a screen reader hears the whole hotkey instead.
        .shortcut_hint(state::keycaps(&state::preset_key(i)).join("+"))
        .focusable(true)
        .focus_selected(selected)
        .h(84.0)
        .pt(7.0)
        .px(8.0)
        .pb(9.0)
        .items_center()
        .justify_between()
        .on_press(move |s: &mut State| s.select_preset(p))
        .press_scale(0.96)
        .child(
            row()
                .w_full()
                .h(12.0)
                .center_y()
                .child(
                    text((i + 1).to_string())
                        .font(NUMBER)
                        // A lit pad's glow is lighter behind it, so its number takes the ink.
                        .color(match lamp {
                            Lamp::Lit => k.c.text_base,
                            Lamp::Latched => k.c.text_muted,
                            Lamp::Off => k.c.text_faint,
                        })
                        .a11y_hidden(),
                )
                .child(el().grow(1.0))
                .children((lamp == Lamp::Lit).then(|| led_dot(true))),
        )
        .child(icon(preset_icon(p), ICON_PAD).color(ink).a11y_hidden())
        .child(
            text(p.name())
                .font(TextStyle {
                    weight: 600.0,
                    ..LABEL
                })
                .color(ink)
                .truncate()
                .a11y_hidden(),
        );
    if !selected {
        tile = tile.hover(k.c.hover).press(k.c.pressed);
    }
    tile
}

fn levels_card(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    let gain = s.settings.mic_gain;
    group(k)
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
            signal()
        };
        bar = bar.child(
            el().grow(1.0)
                .h_full()
                .radius(k.radius.xs)
                .fill(colour)
                .live_opacity(live.smooth(Spring::new(0.09, 0.0))),
        );
    }
    row()
        .w_full()
        .h(20.0)
        .gap(GAP)
        .center_y()
        .child(printed(k, label).w(LABEL_W))
        .child(bar)
        .child(el().w(40.0))
}

fn monitor_card(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    let on = s.settings.monitor;
    group(k)
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
                        .child(
                            row()
                                .gap(10.0)
                                .center_y()
                                // The switch says both to a screen reader.
                                .child(printed(k, "Hear yourself").a11y_hidden())
                                .child(keys(k, state::MONITOR_KEY).a11y_hidden()),
                        )
                        .child(
                            text("How you sound in the call")
                                .font(LABEL)
                                .color(k.c.text_base)
                                .truncate(),
                        ),
                )
                .child(
                    Switch::new(on)
                        .label("Hear yourself")
                        .hide_label()
                        .control_size(ControlSize::Small)
                        .on_toggle(|s: &mut State| s.set_monitor(!s.settings.monitor))
                        .build(k)
                        .key("monitor-switch")
                        .shortcut_hint(state::keycaps(state::MONITOR_KEY).join("+")),
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

/// A labelled fader with its value at the end; drag, click the slot, or use the arrow keys.
fn slider(k: &Kit, cx: &mut Cx<State>, sl: Slider) -> El {
    // `knob` is the fader cap's width, which is what travels along the slot.
    let (h, knob, bar) = (22.0, 12.0, 4.0);
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
        .radius(k.radius.sm)
        .cursor(Cursor::Pointer)
        .on_step(move |s: &mut State, steps, _| {
            set(s, (get(s) + steps as f32 * 0.05).clamp(0.0, 1.0))
        })
        .on_step_limit(move |s: &mut State, end, _| set(s, if end { 1.0 } else { 0.0 }))
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
                .radius(k.radius.xs)
                .fill(tone(0.13))
                // The slot's whole length shows at 3:1, like a switch's when off.
                .border(1.0, edge()),
        )
        .child(
            el().absolute(0.0, (h - bar) / 2.0)
                .size(knob / 2.0 + pos * travel, bar)
                .radius(k.radius.xs)
                // Off, a fader still works: only its fill dims, so its text stays readable.
                .fill(if sl.enabled { filled() } else { tone(0.42) }),
        )
        .child(fader_cap(k, (knob, h - 2.0), (knob - 4.0, 2.0)).absolute(pos * travel, 1.0));
    row()
        .w_full()
        .h(h)
        .gap(GAP)
        .center_y()
        .child(printed(k, sl.label).w(LABEL_W).a11y_hidden())
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
            .child(empty_state(k));
    }
    // Stop all comes first, where a hand reaches in a panic, and its rim lights while a
    // sound is going into the call. The row wraps rather than run off a narrow window.
    let any_playing = !s.status.playing.is_empty();
    let stop = Button::new("Stop all")
        .icon(&STOP)
        .shortcut(state::STOP_KEY)
        .disabled(!any_playing)
        .on_press(|s: &mut State| s.stop_sounds())
        .build(k);
    let mut stop = as_key(stop).key("stop-all");
    if any_playing && s.cable_open() {
        stop =
            stop.border(1.0, wash(led(), Color::WHITE, 0.1))
                .shadow(0.0, 12.0, led().alpha(0.35));
    }
    // Stop all can't be pressed once nothing plays. If it had focus then, focus goes on to
    // Add sounds: an empty marker beside it hands it on as it goes. (Rune scopes a zero-size
    // marker's `focus_return` to its parent, and this one holds only Stop all.)
    let stop = row().key("stop").child(stop).children(any_playing.then(|| {
        el().key("stop-live")
            .size(0.0, 0.0)
            .focus_return("add-sounds")
    }));
    let toolbar = row()
        .key("sound-tools")
        .w_full()
        .flex_wrap()
        .gaps(8.0, 8.0)
        .center_y()
        .child(stop)
        .child(add_button(k, false))
        .child(find_button(k));
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
        .child(col().w_full().px(PAD).pb(10.0).child(toolbar))
        .child(scrolling(
            // Room for the first row's focus ring.
            col().pt(4.0).child(grid).child(
                // The pads print their numbers; this says which keys they are.
                text("The first nine play with Ctrl+Shift+1 to 9.")
                    .font(SMALL)
                    .color(k.c.text_muted),
            ),
        ))
        .child(el().w_full().h(1.0).fill(k.c.border_base))
        .child(
            col()
                .key("sound-footer")
                .w_full()
                .px(PAD)
                .py(12.0)
                .gap(10.0)
                .child(volume)
                .child(hear),
        )
}

fn find_button(k: &Kit) -> El {
    as_key(
        Button::new("Find sounds")
            .on_press(State::open_find)
            .build(k),
    )
    .key("find-sounds")
}

/// On an empty board it's the main action, the lighter key. Beside a full board it's a key
/// like Find sounds, so Stop all is the only one there with an icon.
fn add_button(k: &Kit, main: bool) -> El {
    let b = Button::new("Add sounds…");
    let b = if main {
        b.icon(&PLUS).variant(ButtonVariant::Submit)
    } else {
        b
    };
    as_key(
        b.on_press_tx(|s: &mut State, tx| s.pick_sounds(tx))
            .build(k),
    )
    .key("add-sounds")
}

/// A sound on a pad: its number (the key you press, for the first nine) and its name. While
/// it plays the pad is latched with a playing mark, and lit while the sound goes into the
/// call (see [`State::cable_open`]). Pressed, it plays. Trim and Remove show in its top corner
/// while the pointer is on it, and its context menu (right-click, the Menu key or
/// Shift+F10) offers both from the keyboard.
fn sound_tile(s: &State, k: &Kit, layer: &mut Vec<El>, i: usize, sound: &Sound) -> El {
    let (id, playing) = (sound.id, s.is_playing(sound.id));
    let lit = playing && s.cable_open();
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
        .pin(None, Some(8.0), Some(8.0), None)
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
            // A quick second click lands on the tile that slid into this one's place.
            .on_press(move |s: &mut State| {
                if !s.just_removed() {
                    s.remove_sound(id);
                }
            }),
        );
    let number = (i < 9).then(|| {
        text((i + 1).to_string())
            .font(DIGIT)
            .color(if playing {
                k.c.text_base
            } else {
                k.c.text_muted
            })
            .a11y_hidden()
    });
    // What's lit gives way to Trim and Remove under the pointer.
    let lamp = playing.then(|| {
        row()
            .gap(8.0)
            .center_y()
            .group_opacity(1.0, 0.0)
            .child(icon(&PLAYING, ICON).color(k.c.text_base))
            .children(lit.then(|| led_dot(true)))
    });
    let light = match (lit, playing) {
        (true, _) => Lamp::Lit,
        (false, true) => Lamp::Latched,
        _ => Lamp::Off,
    };
    let mut tile = raise(col(), light)
        .key(format!("sound-{}", id.0))
        .role("button")
        .label(if playing {
            format!("Play {} (playing)", sound.name)
        } else {
            format!("Play {}", sound.name)
        })
        .focusable(true)
        .w_full()
        .h(84.0)
        .pt(7.0)
        .pb(9.0)
        .px(10.0)
        .hover_group()
        .hover(k.c.hover)
        .press(k.c.pressed)
        .on_press(move |s: &mut State| {
            if !s.just_removed() {
                s.play(id);
            }
        })
        .press_scale(0.96)
        .on_secondary_press(move |s: &mut State, p, _| s.sound_menu = Some((id, p)))
        .justify_between()
        .child(
            row()
                .w_full()
                .h(22.0)
                .center_y()
                .children(number)
                .child(el().grow(1.0))
                .children(lamp),
        )
        .child(
            text(sound.name.clone())
                .font(PAD_NAME)
                .color(k.c.text_base)
                .max_lines(2)
                .w_full()
                .help_when_truncated(sound.name.clone())
                .a11y_hidden(),
        )
        .child(actions);
    // The printed number is hidden; a screen reader hears the whole hotkey instead.
    if i < 9 {
        tile = tile.shortcut_hint(state::keycaps(&state::sound_key(i)).join("+"));
    }
    let point = s.sound_menu.filter(|(m, _)| *m == id).map(|(_, p)| p);
    // Keyed by place: after Remove, the menu hands focus to the pad that slides into it.
    let pad = rune_kit::context_menu(format!("sound-menu-{i}"), tile, point)
        .item(MenuItem::new("Trim…").on_select(move |s: &mut State| {
            s.sound_menu = None;
            s.trim_sound(id);
        }))
        .item(MenuItem::new("Remove").on_select(move |s: &mut State| {
            s.sound_menu = None;
            s.remove_sound(id);
        }))
        .on_close(|s: &mut State| s.sound_menu = None)
        .build(&menu(k), layer);
    // Remove from the last pad leaves no pad in its place to take focus: then it goes to the
    // Undo. A holder around the menu does that, after the menu's own return to its place.
    if point.is_some()
        && let Some(surface) = layer.pop()
    {
        layer.push(el().focus_return("undo-remove").child(surface));
    }
    // The grid's column sets the width, however long a name's words are.
    pad.min_w(0.0)
}

/// An empty bank: nine dark sockets, numbered for the keys that will play them.
fn empty_state(k: &Kit) -> El {
    let mut sockets = el()
        .key("empty-sockets")
        .w_full()
        .equal_columns(3)
        .gaps(8.0, 8.0)
        .a11y_hidden();
    for n in 1..=9 {
        sockets = sockets.child(
            el().h(72.0)
                .pt(7.0)
                .px(10.0)
                .radius(k.radius.lg)
                .fill(recess())
                .border_gradient(1.0, tone(0.14), tone(0.26))
                .box_shadow(Shadow {
                    y: 2.0,
                    blur: 6.0,
                    color: Color::BLACK.alpha(0.45),
                    inset: true,
                    ..Shadow::default()
                })
                .child(
                    text(n.to_string())
                        .font(TextStyle {
                            weight: 500.0,
                            ..DIGIT
                        })
                        // Dim beside a full pad's, but still 4.8:1 in its socket.
                        .color(tone(0.6)),
                ),
        );
    }
    col()
        .key("empty")
        .w_full()
        .grow(1.0)
        .gap(18.0)
        .child(sockets)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .center()
                .child(text("No sounds yet").font(HEADING).color(k.c.text_base))
                .child(
                    text("Add WAV, MP3, OGG or FLAC files, or drop them anywhere on this window.")
                        .font(BODY)
                        .color(k.c.text_muted)
                        .align(Align::Center)
                        .wrap()
                        .max_w(280.0),
                )
                .child(
                    row()
                        .gap(8.0)
                        .mt(6.0)
                        .child(add_button(k, true))
                        .child(find_button(k)),
                )
                .child(
                    text("The first nine play with Ctrl+Shift+1 to 9.")
                        .font(SMALL)
                        .color(k.c.text_muted)
                        .mt(4.0),
                ),
        )
}

/// The readouts under the tabs, the same on every page: a dismissable line about something
/// that went wrong (a file that didn't load, say), and under it the Undo for a sound just
/// removed, on the Soundboard's pages only. One place, so a screen reader hears each once
/// whichever page shows; the Undo, added again on each return to the Soundboard, is read
/// only while the removal is new (see [`REMOVAL_NEWS`]).
fn notice(s: &State, k: &Kit) -> Option<El> {
    let removed = s.removed.as_ref().filter(|_| s.tab == Tab::Soundboard);
    if s.notice.is_none() && removed.is_none() {
        return None;
    }
    // Where keyboard focus goes when one is dismissed or undone: the page's first control.
    let after = match s.tab {
        Tab::Soundboard if s.trim.is_some() => "trim-name",
        Tab::Soundboard if s.find.open => "search-field",
        Tab::Soundboard if s.sounds.is_empty() => "add-sounds",
        Tab::Soundboard => "sounds",
        Tab::Voice | Tab::Setup => "page",
    };
    let problem = s.notice.as_ref().map(|message| {
        readout(k)
            .key("notice")
            .role("alert")
            .label(message.clone())
            // Dismissed above an Undo, focus goes on to the Undo.
            .focus_return(if removed.is_some() {
                "undo-remove"
            } else {
                after
            })
            .items_start()
            .child(icon(&WARNING, ICON).color(danger()).mt(6.0))
            .child(
                // One paragraph per line: each file that didn't load, say.
                col()
                    .gap(4.0)
                    .grow(1.0)
                    .shrink()
                    .py(5.0)
                    .a11y_hidden()
                    .children(message.lines().map(|line| {
                        text(line.to_string())
                            .font(SMALL)
                            .color(k.c.text_base)
                            .wrap()
                            .w_full()
                    })),
            )
            .child(dismiss(k, "dismiss-notice", |s: &mut State| s.notice = None).mt(2.0))
    });
    let removed = removed.map(|r| removed_row(k, r).focus_return(after));
    // A long list of problems scrolls in its place rather than push the page off the window.
    Some(
        col()
            .key("notices")
            .w_full()
            .max_h(200.0)
            .child(rune_kit::popover::fit_scroll(
                col()
                    .w_full()
                    .px(PAD)
                    .pb(10.0)
                    .gap(8.0)
                    .children(problem)
                    .children(removed),
            )),
    )
}

/// The readout's close button.
pub(crate) fn dismiss(k: &Kit, key: &str, on: fn(&mut State)) -> El {
    IconButton::new(&CLOSE)
        .variant(IconButtonVariant::GhostMuted)
        .label("Dismiss")
        .on_press(on)
        .build(k)
        .key(key.to_string())
}

fn removed_row(k: &Kit, r: &Removed) -> El {
    let says = format!("Removed “{}”.", r.name);
    readout(k)
        .key("removed")
        .role("status")
        .label(format!("{says} Undo is next to it."))
        .live_region(if r.at.elapsed() < REMOVAL_NEWS {
            LiveRegion::Polite
        } else {
            LiveRegion::Off
        })
        .center_y()
        .child(
            text(says)
                .font(SMALL)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
        .child(as_key(Button::new("Undo").on_press(State::undo_remove).build(k)).key("undo-remove"))
        .child(dismiss(k, "dismiss-removed", State::forget_removed))
}

// --- Setup ---

fn setup(s: &State, cx: &mut Cx<State>, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut page = col().key("setup");
    if s.cable_missing() {
        page = page.child(cable_callout(k));
    }
    let hotkeys = hotkeys_callout(s, k);
    // Under a callout, the devices take a rule; at the top of the page they don't need one.
    let first = !s.cable_missing() && hotkeys.is_none();
    page.children(hotkeys)
        .child(devices_card(s, cx, k, layer, first))
        .child(remote_view::remote_card(s, k))
        .child(discord_card(s, k))
        .child(hotkeys_card(s, k))
}

/// A problem at the top of Setup, as the VB-CABLE one: its warning, a title and what to do.
/// It stands first, above the first group's rule, with no box of its own.
fn callout(k: &Kit, key: &str, title: &str) -> El {
    col().key(key).w_full().gap(8.0).pb(6.0).child(
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
                col()
                    .role("alert")
                    .label(says.clone())
                    .w_full()
                    .pl(24.0)
                    .child(
                        text(says)
                            .font(BODY)
                            .color(k.c.text_base)
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
            .color(k.c.text_base)
            .wrap()
            .pl(24.0),
        )
        .child(
            row().pl(24.0).pt(2.0).child(as_key(
                Button::new("Get VB-CABLE")
                    .icon(&OPEN)
                    .variant(ButtonVariant::Submit)
                    .on_press_tx(|s: &mut State, tx| {
                        if tx.open_url(VB_CABLE_URL).is_err() {
                            s.notice = Some(
                                "Couldn’t open your browser. Go to vb-audio.com/Cable to get \
                                 VB-CABLE."
                                    .into(),
                            );
                        }
                    })
                    .build(k),
            ).key("get-cable")),
        )
}

fn devices_card(s: &State, cx: &mut Cx<State>, k: &Kit, layer: &mut Vec<El>, first: bool) -> El {
    let st = &s.status;
    // The engine's whole message, under the picker whose device it names, else at the foot.
    let error = st.error.as_deref().filter(|_| !s.cable_missing());
    let at = error.and_then(|e| error_picker(s, e));
    let error_line = |under: Option<Picker>| {
        error.filter(|_| at == under).map(|e| {
            problem_line(k, danger(), SMALL, e)
                .key("device-error")
                .role("alert")
                .label(e.to_string())
        })
    };
    if first { first_group() } else { group(k) }
        .key("devices")
        .gap(14.0)
        .child(title_row(k, "Devices"))
        .child(
            picker(
                s,
                cx,
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
                cx,
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
                cx,
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

#[allow(clippy::too_many_arguments)]
fn picker(
    s: &State,
    cx: &mut Cx<State>,
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
    let open = s.open_picker == Some(which);
    let select = Select::new(key, &names)
        .label(label)
        .current(Some(chosen.as_deref().unwrap_or(default)))
        .appearance(SelectAppearance::Field)
        .open(open)
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
        .build(&menu(&slot(k)), layer)
        // Only the list's rows take the menu's highlight: the field keeps the panel's hover,
        // and its faint wash while open.
        .hover(k.c.hover)
        .w_full();
    // The kit's list can be as narrow as 160 points, which cuts device names to a few
    // letters. As wide as the field (328 points in the smallest window), it shows them whole.
    if open && let Some(list) = layer.pop() {
        let w = cx
            .layout_size(&format!("{key}-trigger"))
            .map_or(328.0, |[w, _]| w);
        layer.push(list.min_w(w).max_w(w));
    }
    let select = if open {
        select.fill(k.c.bg_base).gradient(k.c.hover, k.c.hover)
    } else {
        select
    };
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
    // The steps are a real sequence, numbered as the panel prints its numbers.
    let step = |n: &str, line: &str| {
        row()
            .w_full()
            .gap(10.0)
            .items_start()
            .child(
                text(n.to_string())
                    .font(NUMBER)
                    .color(k.c.text_muted)
                    .w(14.0)
                    .mt(1.0)
                    .a11y_hidden(),
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
        .pb(4.0)
        .child(step("1", "Open Settings > Voice & Video."))
        .child(step("2", "Set Input device to “CABLE Output (VB\u{2011}Audio Virtual Cable)”."))
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
    group(k).key("discord").pt(12.0).child(disclosure(
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
    let mut c = col().gap(10.0).pb(4.0).child(
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
            line = line.child(problem_line(k, warn(), SMALL, &says).label(says.clone()));
        }
        c = c.child(line);
    }
    group(k).key("hotkeys").pt(12.0).child(disclosure(
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
    // An Undo that couldn't put the file back: the notice says why, the Undo stays under it.
    ("undo-failed", |s| {
        s.add_sounds(
            ["Air horn.mp3", "Bruh.wav"]
                .map(std::path::PathBuf::from)
                .to_vec(),
        );
        if let Some(id) = s.sounds.iter().find(|x| x.name == "Bruh").map(|x| x.id) {
            s.remove_sound(id);
        }
        s.notice = Some(
            "Couldn’t put “Bruh” back. Close any app that might be using it, then press Undo \
             again."
                .into(),
        );
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
