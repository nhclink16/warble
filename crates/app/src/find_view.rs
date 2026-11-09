//! The Find sounds page (search, YouTube link, good sites, Freesound key) and the trim editor,
//! both shown in place of the Soundboard tab's board.

use rune::{
    Align, Color, Cursor, Cx, Drag, El, Key, KeyEvent, Phase, Shadow, Tx, col, el, icon, row, text,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Spinner, TextInput,
};

use crate::find::{Find, YouTube};
use crate::library::{FREESOUND_KEY_URL, Hit, SITES};
use crate::state::{State, Tab};
use crate::trim::{self, Trim};
use crate::ui::{
    BACK, BODY, CHECK, HEADPHONES, ICON, LABEL, NUMBER, OPEN, PAD, PLAY, PLUS, SCISSORS, SEARCH,
    SMALL, STOP, TITLE, WARNING, as_key, danger, disclosure, fader_cap, group, readout, recess,
    scrolling, slot, title_row, tone, warn,
};
use crate::youtube;

/// Escape goes back from the Find sounds and Trim pages. The pages and their text fields use
/// it; so can the window's `on_key`, for when nothing has focus.
pub fn escape_goes_back(s: &mut State, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key != Key::Escape || s.tab != Tab::Soundboard {
        return false;
    }
    if s.trim.is_some() {
        s.close_trim();
    } else if s.find.open {
        s.close_find();
    } else {
        return false;
    }
    true
}

/// A page's top line: back to the board, and what the page is.
fn page_bar(k: &Kit, title: &str, back: fn(&mut State)) -> El {
    // Equal side slots keep the title in the middle, whatever the Back button measures.
    let side = || row().w(0.0).grow(1.0).center_y();
    // The chevron's ink lines up with the page's edge and its hover fill reaches 8 into the
    // margin, as a section header's does. The ink starts 4.5 into its 16-point icon.
    row()
        .key("page-bar")
        .w_full()
        .px(PAD - 8.0)
        .pb(10.0)
        .center_y()
        .child(
            side().child(
                Button::new("Back")
                    .icon(&BACK)
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(back)
                    .build(k)
                    .key("back")
                    .pl(8.0 - 4.5)
                    .pr(8.0),
            ),
        )
        .child(text(title.to_string()).font(TITLE).color(k.c.text_base))
        .child(side())
}

/// A short line with a spinner, for work under way.
fn working(k: &Kit, key: &str, what: &str) -> El {
    row()
        .key(key.to_string())
        .role("status")
        .label(what.to_string())
        .gap(8.0)
        .center_y()
        .child(Spinner::new().size(14.0).build(k))
        .child(
            text(what.to_string())
                .font(SMALL)
                .color(k.c.text_muted)
                .a11y_hidden(),
        )
}

/// A short line about a problem: something that failed, read out at once, or (`failed` false)
/// a warning beside results, read out when the reader is idle.
fn problem(k: &Kit, key: &str, failed: bool, message: &str) -> El {
    row()
        .key(key.to_string())
        .role(if failed { "alert" } else { "status" })
        .label(message.to_string())
        .w_full()
        .gap(8.0)
        .items_start()
        .child(icon(&WARNING, ICON).color(if failed { danger() } else { warn() }))
        .child(
            text(message.to_string())
                .font(SMALL)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
}

/// A group's printed title, and a line on what it's for.
fn heading(k: &Kit, title: &str, detail: &str) -> El {
    col().w_full().gap(2.0).child(title_row(k, title)).child(
        text(detail.to_string())
            .font(SMALL)
            .color(k.c.text_muted)
            .wrap(),
    )
}

fn open_url(url: &'static str) -> impl Fn(&mut State, &mut Tx) {
    move |s: &mut State, tx: &mut Tx| {
        if tx.open_url(url).is_err() {
            let address = url.trim_start_matches("https://").trim_end_matches('/');
            s.notice = Some(format!(
                "Couldn’t open your browser. Open it and go to {address}."
            ));
        }
    }
}

// --- Find sounds ---

pub fn find_page(s: &State, k: &Kit) -> El {
    let f = &s.find;
    // Added sounds stay on this PC until played, so the check is the panel's ink, not the LED.
    let saved = f.saved.as_ref().map(|name| {
        let says = format!("Added “{name}” to your soundboard.");
        readout(k)
            .key("saved")
            .role("status")
            .label(says.clone())
            .center_y()
            .child(icon(&CHECK, ICON).color(k.c.icon_base))
            .child(
                text(says)
                    .font(SMALL)
                    .color(k.c.text_base)
                    .wrap()
                    .grow(1.0)
                    .shrink()
                    .a11y_hidden(),
            )
    });
    col()
        .key("find")
        .grow(1.0)
        .min_h(0.0)
        .on_key(escape_goes_back)
        .focus_return("find-sounds")
        .child(page_bar(k, "Find sounds", State::close_find))
        .child(scrolling(
            col()
                .key("find-body")
                // Room for the search field's edge at the top of the scroll view.
                .pt(2.0)
                .children(saved)
                .child(search_box(s, k))
                .children(results(s, f, k))
                .child(youtube_card(f, k))
                .child(sites_card(f, k))
                .children((!f.key_first).then(|| key_card(f, k))),
        ))
}

fn search_box(s: &State, k: &Kit) -> El {
    let f = &s.find;
    let field = TextInput::new(&f.query)
        .placeholder("Search for a sound")
        .label("Search for a sound")
        .leading_icon(&SEARCH)
        .width(None)
        .on_edit(|s: &mut State| &mut s.find.query)
        .on_submit(|s: &mut State, _| s.search())
        .on_key(escape_goes_back)
        .build(&slot(k))
        .key("search-field");
    let mut c = col().key("search").w_full().gap(8.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(col().grow(1.0).shrink().min_w(0.0).child(field))
            .child(
                // As tall as the field beside it, with its corners.
                as_key(
                    Button::new("Search")
                        .control_size(ControlSize::Large)
                        .disabled(f.query.text.trim().is_empty())
                        .on_press(State::search)
                        .build(k),
                )
                .radius(k.radius.md)
                .key("search-button"),
            ),
    );
    // Shown while the page offers a key, as it opened: a key typed in changes the words, so
    // the field doesn't jump.
    if f.key_first {
        let searching = if s.settings.freesound_key.is_empty() {
            "Searching Myinstants only."
        } else {
            "Searching Myinstants and Freesound."
        };
        c = c.child(text(searching).font(SMALL).color(k.c.text_faint).wrap());
        c = c.child(
            disclosure(
                k,
                "Add a Freesound key",
                f.key_open,
                |s| s.find.key_open = !s.find.key_open,
                col()
                    .gap(8.0)
                    .pb(4.0)
                    .child(text(KEY_WHY).font(SMALL).color(k.c.text_muted).wrap())
                    .child(key_field(f, k))
                    .child(get_key_button(k)),
            )
            .key("freesound"),
        );
    }
    c
}

fn results(s: &State, f: &Find, k: &Kit) -> Option<El> {
    if f.searching {
        return Some(working(k, "searching", "Searching…"));
    }
    let query = f.searched.as_ref()?;
    if f.hits.is_empty() {
        return Some(match &f.problem {
            Some(p) => problem(k, "search-problem", true, p),
            None => {
                let mut said = format!(
                    "No sounds found for “{query}”. Try a shorter word, or paste a video link \
                     below."
                );
                if s.settings.freesound_key.is_empty() {
                    said.push_str(" Add a Freesound key to search Freesound\u{a0}too.");
                }
                text(said)
                    .key("no-results")
                    .role("status")
                    .font(SMALL)
                    .color(k.c.text_muted)
                    .wrap()
            }
        });
    }
    let found = match f.hits.len() {
        1 => format!("1 sound found for “{query}”"),
        n => format!("{n} sounds found for “{query}”"),
    };
    // Rows on the panel, a hairline between each: no box around them.
    let mut list = col().key("results").label(found.clone()).w_full();
    for (i, hit) in f.hits.iter().enumerate() {
        if i > 0 {
            list = list.child(el().w_full().h(1.0).fill(k.c.border_muted));
        }
        list = list.child(hit_row(s, f, k, i, hit));
    }
    let warning = f
        .problem
        .as_ref()
        .map(|p| problem(k, "search-problem", false, p));
    Some(
        col()
            .w_full()
            .gap(8.0)
            // Read out when the results arrive; nothing to see.
            .child(
                el().key("results-status")
                    .absolute(0.0, 0.0)
                    .role("status")
                    .label(found),
            )
            .children(warning)
            .child(list),
    )
}

fn hit_row(s: &State, f: &Find, k: &Kit, i: usize, hit: &Hit) -> El {
    let busy = f.is_busy(&hit.url);
    let playing = f.previewing.as_deref() == Some(hit.url.as_str()) && s.status.preview.is_some();
    let mut meta = vec![hit.source.name().to_string()];
    if let Some(t) = hit.seconds {
        meta.push(format!("{t:.1} s"));
    }
    meta.extend(hit.licence.map(str::to_string));
    let (u1, u2, u3) = (hit.url.clone(), hit.url.clone(), hit.url.clone());
    // One width whatever it shows, so the buttons line up down the list.
    let add = el().w(72.0).h(28.0).center().child(if busy {
        Spinner::new().size(14.0).build(k)
    } else if f.is_added(&hit.url) {
        row()
            .gap(4.0)
            .center_y()
            .child(icon(&CHECK, ICON).color(k.c.icon_muted))
            .child(text("Added").font(SMALL).color(k.c.text_muted))
    } else {
        as_key(
            Button::new("Add")
                .icon(&PLUS)
                .on_press(move |s: &mut State| s.add_hit(&u3))
                .build(k),
        )
        .key(format!("add-{i}"))
        // It gives way to "Added": focus goes on to the next result that isn't downloading
        // (whose buttons can't take it), or the link field.
        .focus_return(
            (i + 1..f.hits.len())
                .find(|&j| !f.is_busy(&f.hits[j].url))
                .map_or_else(|| "link-field".into(), |j| format!("hit-{j}")),
        )
    });
    row()
        .key(format!("hit-{i}"))
        .w_full()
        .h(48.0)
        .gap(4.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(
                    text(hit.name.clone())
                        .font(LABEL)
                        .color(k.c.text_base)
                        .truncate()
                        .help_when_truncated(hit.name.clone()),
                )
                .child(
                    text(meta.join(" · "))
                        .font(SMALL)
                        .color(k.c.text_faint)
                        .truncate(),
                ),
        )
        .child(
            IconButton::new(if playing { &STOP } else { &HEADPHONES })
                .variant(IconButtonVariant::Ghost)
                .label(if playing {
                    format!("Stop {}", hit.name)
                } else {
                    format!("Preview {} in your headphones", hit.name)
                })
                .help(if playing {
                    "Stop"
                } else {
                    "Preview in your headphones"
                })
                .disabled(busy)
                .on_press(move |s: &mut State| s.preview_hit(&u1))
                .build(k)
                .key(format!("preview-{i}")),
        )
        .child(
            IconButton::new(&SCISSORS)
                .variant(IconButtonVariant::Ghost)
                .label(format!("Trim {}", hit.name))
                .help("Trim")
                .disabled(busy)
                .on_press(move |s: &mut State| s.trim_hit(&u2))
                .build(k)
                .key(format!("trim-hit-{i}")),
        )
        .child(add)
}

fn youtube_card(f: &Find, k: &Kit) -> El {
    let busy = matches!(f.youtube, YouTube::Installing | YouTube::Fetching);
    let field = TextInput::new(&f.link)
        .placeholder("Paste a YouTube or X link")
        .label("YouTube or X link")
        .width(None)
        .disabled(busy)
        .on_edit(|s: &mut State| &mut s.find.link)
        .on_submit(|s: &mut State, _| s.get_audio())
        .on_key(escape_goes_back)
        .build(&slot(k))
        .key("link-field");
    let status = match &f.youtube {
        YouTube::Idle => None,
        YouTube::AskToInstall => Some(install_notice(k)),
        YouTube::Installing => Some(working(k, "installing", "Downloading yt\u{2011}dlp…")),
        YouTube::Fetching => Some(working(k, "fetching", "Getting the audio…")),
        YouTube::Failed(e) => Some(problem(k, "youtube-problem", true, e)),
    };
    group(k)
        .key("youtube")
        .child(heading(
            k,
            "From YouTube or X",
            "Paste a link, or drop a video file on the window. Then keep the part you want.",
        ))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(col().grow(1.0).shrink().min_w(0.0).child(field))
                .child(
                    // As tall as the field beside it, with its corners.
                    as_key(
                        Button::new("Get audio")
                            .control_size(ControlSize::Large)
                            .disabled(busy || f.link.text.trim().is_empty())
                            .on_press(State::get_audio)
                            .build(k),
                    )
                    .radius(k.radius.md)
                    .key("get-audio"),
                ),
        )
        .children(status)
}

/// Said once, before yt-dlp is first downloaded: a line in the card, with its buttons under the
/// words.
fn install_notice(k: &Kit) -> El {
    let says = format!(
        "To get audio from YouTube and X, Warble downloads yt\u{2011}dlp ({}), a free tool, from \
         its official GitHub page. Warble keeps it up to date after\u{a0}that.",
        youtube::TOOL_SIZE
    );
    col()
        .key("install-notice")
        // "Not now" closes it: focus goes back to the link.
        .focus_return("link-field")
        .w_full()
        .gap(10.0)
        .child(
            row()
                .role("status")
                .label(says.clone())
                .w_full()
                .gap(8.0)
                .items_start()
                .child(icon(&WARNING, ICON).color(warn()).mt(1.0))
                .child(
                    text(says)
                        .font(BODY)
                        .color(k.c.text_base)
                        .wrap()
                        .grow(1.0)
                        .shrink()
                        .a11y_hidden(),
                ),
        )
        .child(
            row()
                .pl(24.0)
                .gap(8.0)
                .child(
                    as_key(
                        Button::new("Download yt-dlp")
                            .variant(ButtonVariant::Submit)
                            .on_press(State::install_youtube_tool)
                            .build(k),
                    )
                    .key("install-yt-dlp"),
                )
                .child(
                    Button::new("Not now")
                        .variant(ButtonVariant::Ghost)
                        .on_press(State::cancel_youtube)
                        .build(k)
                        .key("not-now"),
                ),
        )
}

fn sites_card(f: &Find, k: &Kit) -> El {
    let mut sites = col().w_full().gap(4.0).pb(4.0);
    for (i, site) in SITES.iter().enumerate() {
        let open = open_url(site.url);
        sites = sites.child(
            row()
                .key(format!("site-{i}"))
                .role("link")
                .label(site.name)
                .description(site.note)
                .focusable(true)
                // The hover fill reaches past the words and icons, which line up with the
                // page's edges (stretched, so the margins widen it on both sides).
                .mx(-8.0)
                .px(8.0)
                .py(7.0)
                .gap(10.0)
                .center_y()
                .radius(k.radius.md)
                .cursor(Cursor::Pointer)
                .hover(k.c.hover)
                .press(k.c.pressed)
                .on_press_tx(open_url(site.url))
                // Space presses it as a control; Return opens it as a link does.
                .on_key(move |s: &mut State, ev: &KeyEvent, tx: &mut Tx| {
                    let enter = ev.key == Key::Enter;
                    if enter && !tx.key_repeat() {
                        open(s, tx);
                    }
                    enter
                })
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .gap(2.0)
                        .a11y_hidden()
                        .child(text(site.name).font(LABEL).color(k.c.text_base))
                        .child(text(site.note).font(SMALL).color(k.c.text_muted).wrap()),
                )
                .child(icon(&OPEN, ICON).color(k.c.icon_muted)),
        );
    }
    // The section's trigger is its own 32-point row: less room above keeps its title where
    // a group's title sits.
    group(k).key("sites").pt(12.0).child(
        disclosure(
            k,
            "Good places to find sounds",
            f.sites_open,
            |s| s.find.sites_open = !s.find.sites_open,
            sites,
        )
        .key("sites-section"),
    )
}

const KEY_WHY: &str =
    "Freesound’s search needs a free key. Make one, then paste it here\u{a0}once.";

fn key_field(f: &Find, k: &Kit) -> El {
    TextInput::new(&f.key)
        .secure()
        .placeholder("Paste your key")
        .label("Freesound key")
        .width(None)
        .on_edit(|s: &mut State| &mut s.find.key)
        .on_change(|s: &mut State, _| s.set_freesound_key())
        .on_key(escape_goes_back)
        .build(&slot(k))
        .key("key-field")
}

fn get_key_button(k: &Kit) -> El {
    row().child(
        as_key(
            Button::new("Get a free key")
                .icon(&OPEN)
                .on_press_tx(open_url(FREESOUND_KEY_URL))
                .build(k),
        )
        .key("get-key"),
    )
}

/// The Freesound key once one is set, at the bottom of the page.
fn key_card(f: &Find, k: &Kit) -> El {
    group(k)
        .key("freesound")
        .child(heading(k, "Freesound key", KEY_WHY))
        .child(key_field(f, k))
        .child(get_key_button(k))
}

// --- Trim ---

/// The waveform's height, and the room each handle needs at the edges.
const WAVE_H: f32 = 96.0;
const EDGE: f32 = 8.0;
/// The width of a time under a handle, and the least room between the two.
const TIME_W: f32 = 56.0;
const TIME_GAP: f32 = 6.0;

pub fn trim_page(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    let Some(t) = &s.trim else { return el() };
    let playing = s.status.preview.is_some();
    let w = cx.layout_size("wave").map_or(320.0, |z| z[0]);
    let hint = if t.duration() > trim::MAX_SECONDS {
        "Plays in your headphones only. Sounds can be up to 5 minutes long."
    } else {
        "Plays in your headphones only."
    };
    let name = TextInput::new(&s.find.trim_name)
        .label("Name")
        .width(None)
        .on_edit(|s: &mut State| &mut s.find.trim_name)
        .on_submit(|s: &mut State, _| s.trim_save())
        .on_key(escape_goes_back)
        .build(&slot(k))
        .key("trim-name");
    let keeping = if t.is_whole() {
        format!("Keeping all {}", trim::clock(t.duration()))
    } else {
        format!(
            "Keeping {} of {}",
            trim::clock(t.end - t.start),
            trim::clock(t.duration())
        )
    };
    let actions = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            as_key(
                Button::new(if playing { "Stop" } else { "Play selection" })
                    .icon(if playing { &STOP } else { &PLAY })
                    .help("Plays in your headphones only")
                    .on_press(State::trim_play)
                    .build(k),
            )
            .key("trim-play"),
        )
        .child(el().grow(1.0))
        .child(
            as_key(
                Button::new(if t.saving { "Adding…" } else { "Add sound" })
                    .icon(&PLUS)
                    .variant(ButtonVariant::Submit)
                    .disabled(t.saving)
                    .on_press(State::trim_save)
                    .build(k),
            )
            .key("trim-save"),
        );
    let mut page = col()
        .key("trim")
        .grow(1.0)
        .min_h(0.0)
        .on_key(escape_goes_back);
    if let Some(opener) = &s.find.trim_opener {
        page = page.focus_return(opener.clone());
    }
    page.child(page_bar(k, "Trim", State::close_trim))
        .child(scrolling(
            col().key("trim-body").pt(2.0).child(
                col()
                    .key("trim-card")
                    .w_full()
                    .gap(14.0)
                    .child(
                        col()
                            .w_full()
                            .gap(6.0)
                            .child(text("Name").font(LABEL).color(k.c.text_muted))
                            .child(name),
                    )
                    .child(
                        col()
                            .w_full()
                            .gap(6.0)
                            .child(
                                text(keeping)
                                    .key("trim-length")
                                    .font(NUMBER)
                                    .color(k.c.text_muted),
                            )
                            .child(waveform(s, t, w, k))
                            .child(times(k, t, w)),
                    )
                    .child(actions)
                    .child(text(hint).font(SMALL).color(k.c.text_faint).wrap()),
            ),
        ))
}

/// Where `at` seconds sits across a waveform `w` points wide.
fn x_at(t: &Trim, w: f32, at: f64) -> f32 {
    let inner = (w - 2.0 * EDGE).max(1.0);
    EDGE + (at / t.duration().max(1e-9)) as f32 * inner
}

/// The start and end times, each under its handle. Visual only: the handles say them.
fn times(k: &Kit, t: &Trim, w: f32) -> El {
    let (left, right) = time_spots(w, x_at(t, w, t.start), x_at(t, w, t.end));
    let time = |at: f64, x: f32| {
        text(trim::clock(at))
            .font(NUMBER)
            .color(k.c.text_muted)
            .w(TIME_W)
            .align(Align::Center)
            .absolute(x, 0.0)
    };
    el().key("trim-times")
        .w_full()
        .h(16.0)
        .a11y_hidden()
        .child(time(t.start, left))
        .child(time(t.end, right))
}

/// The left edges of the start and end times, centred under handles at `a` and `b` on a
/// waveform `w` wide: kept inside it, and pushed apart when they'd overlap.
fn time_spots(w: f32, a: f32, b: f32) -> (f32, f32) {
    let need = TIME_W + TIME_GAP;
    let max = (w - TIME_W).max(0.0);
    let (mut left, mut right) = (a - TIME_W / 2.0, b - TIME_W / 2.0);
    if right - left < need {
        let mid = (left + right) / 2.0;
        (left, right) = (mid - need / 2.0, mid + need / 2.0);
    }
    let left = left.min(max - need).max(0.0);
    (left, right.max(left + need).min(max))
}

/// Moves the start (or with `end`, the end) handle to `at` seconds.
fn set_handle(t: &mut Trim, end: bool, at: f64) {
    if end { t.set_end(at) } else { t.set_start(at) }
}

/// The clip's waveform `w` points wide in a well set into the panel, with the part you keep
/// in the panel's ink, a handle at each end (drag, or focus and use the arrow keys), and the
/// playhead while the selection plays. Pressing the waveform moves the nearer handle there.
/// It all plays in your headphones only, so none of it takes the LED colour.
fn waveform(s: &State, t: &Trim, w: f32, k: &Kit) -> El {
    let inner = (w - 2.0 * EDGE).max(1.0);
    let d = t.duration().max(1e-9);
    let (a, b) = ((t.start / d) as f32, (t.end / d) as f32);
    let x = |frac: f32| EDGE + frac * inner;

    let mut bars = row()
        .absolute(EDGE, 0.0)
        .size(inner, WAVE_H)
        .gap(1.0)
        .center_y()
        .a11y_hidden();
    let n = t.peaks.len().max(1) as f32;
    for (i, p) in t.peaks.iter().enumerate() {
        let mid = (i as f32 + 0.5) / n;
        let lit = (a..=b).contains(&mid);
        bars = bars.child(
            el().grow(1.0)
                .h((p * (WAVE_H - 20.0)).max(2.0))
                .radius(k.radius.xs)
                .fill(if lit { tone(0.88) } else { tone(0.40) }),
        );
    }
    let selection = el()
        .absolute(x(a), 0.0)
        .size((b - a) * inner, WAVE_H)
        .fill(Color::WHITE.alpha(0.06))
        .a11y_hidden();

    let mut wave = el()
        .key("wave")
        .w_full()
        .h(WAVE_H)
        .radius(k.radius.lg)
        .fill(recess())
        .box_shadow(Shadow {
            y: 1.0,
            blur: 3.0,
            color: Color::BLACK.alpha(0.6),
            inset: true,
            ..Shadow::default()
        })
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut State, dr: &mut Drag, tx: &mut Tx| {
            let Some(t) = &mut s.trim else { return };
            let inner = (dr.w - 2.0 * EDGE).max(1.0);
            let at = ((dr.x - EDGE) / inner).clamp(0.0, 1.0) as f64 * t.duration();
            match dr.phase {
                Phase::Down => {
                    t.grab = (t.start, t.end);
                    let end = (at - t.end).abs() < (at - t.start).abs();
                    dr.data = if end { 1.0 } else { 0.0 };
                    set_handle(t, end, at);
                }
                Phase::Move => {
                    set_handle(t, dr.data > 0.5, at);
                    tx.instant();
                }
                Phase::Up => {
                    if dr.cancelled() {
                        (t.start, t.end) = t.grab;
                    }
                }
            }
        })
        .child(selection)
        .child(bars);
    if let Some(at) = s.status.preview {
        let frac = at as f32 / t.clip.samples().len().max(1) as f32;
        wave = wave.child(
            el().absolute(x(frac) - 0.75, 6.0)
                .size(1.5, WAVE_H - 12.0)
                .fill(k.c.text_base)
                .a11y_hidden(),
        );
    }
    wave.child(handle(k, t, false, x(a), inner))
        .child(handle(k, t, true, x(b), inner))
}

fn handle(k: &Kit, t: &Trim, end: bool, at: f32, inner: f32) -> El {
    let value = if end { t.end } else { t.start };
    el().key(if end { "trim-end" } else { "trim-start" })
        .role("slider")
        .label(if end { "End" } else { "Start" })
        .a11y_value(trim::clock(value))
        .range_value(value, 0.0, t.duration())
        .range_step(t.step())
        .focusable(true)
        .absolute(at - EDGE, 0.0)
        .size(2.0 * EDGE, WAVE_H)
        .center()
        .cursor(Cursor::ResizeColumn)
        .on_step(move |s: &mut State, steps, _| s.step_trim(end, steps))
        .on_step_limit(move |s: &mut State, max, _| s.trim_limit(end, max))
        .on_drag(move |s: &mut State, dr: &mut Drag, tx: &mut Tx| {
            let Some(t) = &mut s.trim else { return };
            match dr.phase {
                Phase::Down => {
                    t.grab = (t.start, t.end);
                    dr.data = if end { t.end } else { t.start } as f32;
                }
                Phase::Move => {
                    let at = dr.data as f64 + (dr.dx / inner) as f64 * t.duration();
                    set_handle(t, end, at);
                    tx.instant();
                }
                Phase::Up => {
                    if dr.cancelled() {
                        (t.start, t.end) = t.grab;
                    }
                }
            }
        })
        .child(
            el().absolute(EDGE - 1.0, 0.0)
                .size(2.0, WAVE_H)
                .fill(tone(0.88)),
        )
        // A cap like a fader's, to grab.
        .child(fader_cap(k, (10.0, 26.0), (2.0, 10.0)))
}

#[cfg(test)]
mod tests {
    use rune::{Env, Mods, Runtime};
    use warble_engine::Clip;

    use super::*;

    fn app() -> Runtime<State> {
        let mut rt = Runtime::new(
            State::mock(),
            crate::ui::view,
            (400.0, 620.0),
            Env::new(true, 1.0),
        );
        settle(&mut rt);
        rt
    }

    fn settle(rt: &mut Runtime<State>) {
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
    }

    fn press(rt: &mut Runtime<State>, key: Key) {
        rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        settle(rt);
    }

    fn focused(rt: &Runtime<State>) -> String {
        let e = rt.elements().into_iter().find(|e| e.focused);
        e.map_or_else(String::new, |e| e.path)
    }

    fn element(rt: &Runtime<State>, key: &str) -> rune::ElementInfo {
        let e = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key));
        e.unwrap_or_else(|| panic!("no element {key}"))
    }

    #[test]
    fn escape_goes_back_and_focus_returns_to_what_opened_the_page() {
        let mut rt = app();
        rt.update(|s, _| s.open_find());
        settle(&mut rt);
        assert!(focused(&rt).contains("/search-field/"), "{}", focused(&rt));

        rt.update(|s, _| scenes::results(s));
        settle(&mut rt);
        // As if result 1's scissors opened it.
        rt.update(|s, _| {
            s.open_trim(Clip::new(vec![0.1; 8000], 8000), "Bruh");
            s.find.trim_opener = Some("trim-hit-1".into());
        });
        settle(&mut rt);
        assert!(focused(&rt).contains("/trim-name/"), "{}", focused(&rt));

        press(&mut rt, Key::Escape);
        assert!(rt.state.trim.is_none() && rt.state.find.open);
        assert!(focused(&rt).ends_with("/trim-hit-1"), "{}", focused(&rt));

        press(&mut rt, Key::Escape);
        assert!(!rt.state.find.open);
        assert!(focused(&rt).ends_with("/find-sounds"), "{}", focused(&rt));
        press(&mut rt, Key::Escape);
        assert_eq!(
            rt.state.tab,
            Tab::Soundboard,
            "Escape on the board does nothing"
        );
    }

    #[test]
    fn the_sites_open_from_their_section_and_open_from_the_keyboard() {
        let mut rt = app();
        rt.update(|s, _| s.open_find());
        settle(&mut rt);
        assert!(rt.find("site-0").is_none(), "closed at first");
        let (x, y) = rt
            .find("sites-section/trigger")
            .expect("the section's trigger");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(&mut rt);
        assert!(rt.state.find.sites_open);
        // Tab from the trigger reaches each site; Return opens it.
        for _ in 0..3 {
            press(&mut rt, Key::Tab);
        }
        assert!(focused(&rt).ends_with("/site-2"), "{}", focused(&rt));
        press(&mut rt, Key::Enter);
        assert_eq!(rt.take_url_requests(), [SITES[2].url]);
    }

    #[test]
    fn results_and_failures_are_read_out() {
        let mut rt = app();
        rt.update(|s, _| scenes::results(s));
        settle(&mut rt);
        let status = element(&rt, "results-status");
        assert_eq!(status.role, "status");
        assert_eq!(status.label.as_deref(), Some("7 sounds found for “bruh”"));
        assert_eq!(
            element(&rt, "results").label.as_deref(),
            Some("7 sounds found for “bruh”")
        );

        rt.update(|s, _| {
            s.settings.freesound_key.clear();
            s.find.hits.clear();
            s.find.searched = Some("zzz".into());
        });
        settle(&mut rt);
        let none = element(&rt, "no-results");
        assert_eq!(none.role, "status");
        assert_eq!(
            none.text.as_deref(),
            Some(
                "No sounds found for “zzz”. Try a shorter word, or paste a video link below. Add \
                 a Freesound key to search Freesound\u{a0}too."
            )
        );

        rt.update(|s, _| scenes::offline(s));
        settle(&mut rt);
        assert_eq!(element(&rt, "search-problem").role, "alert");
        rt.update(|s, _| {
            s.find.youtube = YouTube::Failed(youtube::UNREADABLE.into());
        });
        settle(&mut rt);
        assert_eq!(element(&rt, "youtube-problem").role, "alert");
    }

    #[test]
    fn trim_times_stay_under_their_handles_inside_the_waveform() {
        let gap = |(l, r): (f32, f32)| r - l;
        // Far apart: each centred under its handle.
        assert_eq!(time_spots(300.0, 100.0, 200.0), (72.0, 172.0));
        // At the edges: kept inside.
        assert_eq!(time_spots(300.0, 8.0, 292.0), (0.0, 244.0));
        // Close together: pushed apart about their middle.
        let (l, r) = time_spots(300.0, 140.0, 150.0);
        assert!(gap((l, r)) >= TIME_W + TIME_GAP && (l + r) / 2.0 + TIME_W / 2.0 == 145.0);
        // Close together at an edge: pushed apart, still inside.
        assert_eq!(time_spots(300.0, 8.0, 10.0), (0.0, TIME_W + TIME_GAP));
        assert_eq!(
            time_spots(300.0, 290.0, 292.0),
            (300.0 - 2.0 * TIME_W - TIME_GAP, 300.0 - TIME_W)
        );
    }
}

// --- Scenes for headless snapshots ---

pub mod scenes {
    use warble_engine::Clip;

    use crate::find::YouTube;
    use crate::library::{Hit, Source};
    use crate::state::State;

    fn hit(name: &str, source: Source, seconds: Option<f32>, licence: Option<&'static str>) -> Hit {
        Hit {
            name: name.into(),
            source,
            url: format!("https://example.com/{name}.mp3"),
            seconds,
            licence,
        }
    }

    pub fn start(s: &mut State) {
        s.open_find();
    }

    pub fn results(s: &mut State) {
        s.settings.freesound_key = "key".into();
        s.open_find();
        let f = &mut s.find;
        f.query = rune::TextEdit::new("bruh");
        f.searched = Some("bruh".into());
        f.hits = vec![
            hit("BRUH", Source::Myinstants, None, None),
            hit(
                "Bruh, deep male voice",
                Source::Freesound,
                Some(0.9),
                Some("CC0"),
            ),
            hit("BRUH sound effect!", Source::Myinstants, None, None),
            hit(
                "bruh moment reverb",
                Source::Freesound,
                Some(2.4),
                Some("CC BY"),
            ),
            hit("omg bruh oh hell nah", Source::Myinstants, None, None),
            hit(
                "Bruh (sigh)",
                Source::Freesound,
                Some(1.3),
                Some("CC BY-NC"),
            ),
            hit("Bruh meme", Source::Myinstants, None, None),
        ];
        f.added = vec![f.hits[2].url.clone()];
        f.busy = vec![f.hits[4].url.clone()];
        let url = f.hits[0].url.clone();
        s.engine
            .preview(&Clip::new(vec![0.0; 48000], 48000), 0..48000);
        s.find.previewing = Some(url);
        s.refresh_status();
    }

    pub fn offline(s: &mut State) {
        s.open_find();
        s.find.query = rune::TextEdit::new("air horn");
        s.find.searched = Some("air horn".into());
        s.find.problem = Some("Couldn’t reach Myinstants. Check your internet.".into());
    }

    pub fn youtube_install(s: &mut State) {
        s.open_find();
        s.find.link = rune::TextEdit::new("https://www.youtube.com/watch?v=jNQXAC9IVRw");
        s.find.youtube = YouTube::AskToInstall;
    }

    /// Six seconds of talking-shaped sound, a stretch selected, and its preview playing.
    pub fn trim(s: &mut State) {
        let rate = 16000;
        let samples = (0..6 * rate)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let syllables = (t * 2.3 * std::f32::consts::PI).sin().max(0.0).powi(2);
                let swell = 0.35 + 0.65 * (t * 1.7).sin().abs();
                (t * 220.0 * std::f32::consts::TAU).sin() * syllables * swell
            })
            .collect();
        s.open_find();
        s.open_trim(Clip::new(samples, rate as u32), "Me at the zoo");
        if let Some(t) = &mut s.trim {
            t.set_start(1.3);
            t.set_end(3.9);
        }
        s.trim_play();
        for _ in 0..30 {
            s.refresh_status(); // a second into the selection
        }
    }
}
