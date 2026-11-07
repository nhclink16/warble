//! The Find sounds page (search, YouTube link, good sites, Freesound key) and the trim editor,
//! both shown in place of the Soundboard tab's board.

use rune::{Align, Cursor, Cx, Drag, El, Phase, Tx, col, el, icon, oklch, row, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Spinner, TextInput};

use crate::find::{Find, YouTube};
use crate::library::{FREESOUND_KEY_URL, Hit, SITES};
use crate::state::State;
use crate::trim::{self, Trim};
use crate::ui::{
    BACK, CHECK, HEADING, HEADPHONES, LABEL, NUMBER, OPEN, PAD, PLAY, PLUS, SCISSORS, SEARCH,
    SMALL, STOP, WARNING, accent, card, danger, notice, scrolling, warn, wash,
};
use crate::youtube;

/// A page's top line: back to the board, and what the page is.
fn page_bar(k: &Kit, title: &str, back: fn(&mut State)) -> El {
    row()
        .key("page-bar")
        .w_full()
        .px(PAD - 6.0)
        .pb(10.0)
        .gap(4.0)
        .center_y()
        .child(
            Button::new("Back")
                .icon(&BACK)
                .variant(ButtonVariant::GhostMuted)
                .on_press(back)
                .build(k)
                .key("back"),
        )
        .child(el().grow(1.0))
        .child(text(title.to_string()).font(HEADING).color(k.c.text_base))
        .child(el().grow(1.0))
        // Balances the back button, so the title sits in the middle.
        .child(el().w(64.0))
}

/// A short line with a spinner, for work under way.
fn working(k: &Kit, key: &str, what: &str) -> El {
    row()
        .key(key.to_string())
        .role("status")
        .gap(8.0)
        .center_y()
        .child(Spinner::new().size(14.0).build(k))
        .child(text(what.to_string()).font(SMALL).color(k.c.text_muted))
}

/// A short line about a problem.
fn problem(k: &Kit, key: &str, colour: rune::Color, message: &str) -> El {
    row()
        .key(key.to_string())
        .w_full()
        .gap(8.0)
        .items_start()
        .child(icon(&WARNING, 14.0).color(colour).mt(1.0))
        .child(
            text(message.to_string())
                .font(SMALL)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink(),
        )
}

fn heading(k: &Kit, title: &str, detail: &str) -> El {
    col()
        .gap(3.0)
        .child(text(title.to_string()).font(HEADING).color(k.c.text_base))
        .child(
            text(detail.to_string())
                .font(SMALL)
                .color(k.c.text_muted)
                .wrap(),
        )
}

fn open_url(url: &'static str) -> impl Fn(&mut State, &mut Tx) {
    move |s: &mut State, tx: &mut Tx| {
        if let Err(e) = tx.open_url(url) {
            s.notice = Some(format!("Couldn’t open the browser: {e}"));
        }
    }
}

// --- Find sounds ---

pub fn find_page(s: &State, k: &Kit) -> El {
    let f = &s.find;
    let saved = f.saved.as_ref().map(|name| {
        row()
            .key("saved")
            .role("status")
            .w_full()
            .px(12.0)
            .py(8.0)
            .gap(8.0)
            .center_y()
            .radius(10.0)
            .fill(wash(k.c.page, accent(), 0.1))
            .border(1.0, accent().alpha(0.35))
            .child(icon(&CHECK, 14.0).color(accent()))
            .child(
                text(format!("Added “{name}” to your soundboard."))
                    .font(SMALL)
                    .color(k.c.text_base)
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            )
    });
    col()
        .key("find")
        .grow(1.0)
        .min_h(0.0)
        .child(page_bar(k, "Find sounds", State::close_find))
        .child(scrolling(
            col()
                .key("find-body")
                .children(notice(s, k))
                .children(saved)
                .child(search_box(s, k))
                .children(results(s, f, k))
                .child(youtube_card(f, k))
                .child(sites_card(k))
                .child(key_card(f, k)),
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
        .build(k)
        .key("search-field");
    let mut c = col().key("search").w_full().gap(8.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(col().grow(1.0).shrink().min_w(0.0).child(field))
            .child(
                Button::new("Search")
                    .disabled(f.query.text.trim().is_empty())
                    .on_press(State::search)
                    .build(k)
                    .key("search-button"),
            ),
    );
    if s.settings.freesound_key.is_empty() {
        c = c.child(
            text("Searching Myinstants only. Add a Freesound key below for more.")
                .font(SMALL)
                .color(k.c.text_faint)
                .wrap(),
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
            Some(p) => problem(k, "search-problem", danger(), p),
            None => text(format!("No sounds found for “{query}”."))
                .key("no-results")
                .font(SMALL)
                .color(k.c.text_muted)
                .wrap(),
        });
    }
    let mut list = col()
        .key("results")
        .w_full()
        .px(12.0)
        .py(4.0)
        .radius(12.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_muted);
    for (i, hit) in f.hits.iter().enumerate() {
        if i > 0 {
            list = list.child(el().w_full().h(1.0).fill(k.c.border_muted));
        }
        list = list.child(hit_row(s, f, k, i, hit));
    }
    let warning = f
        .problem
        .as_ref()
        .map(|p| problem(k, "search-problem", warn(), p));
    Some(col().w_full().gap(8.0).children(warning).child(list))
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
            .child(icon(&CHECK, 13.0).color(accent()))
            .child(text("Added").font(SMALL).color(accent()))
    } else {
        Button::new("Add")
            .icon(&PLUS)
            .on_press(move |s: &mut State| s.add_hit(&u3))
            .build(k)
            .key(format!("add-{i}"))
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
                        .truncate(),
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
        .build(k)
        .key("link-field");
    let status = match &f.youtube {
        YouTube::Idle => None,
        YouTube::AskToInstall => Some(install_notice(k)),
        YouTube::Installing => Some(working(k, "installing", "Downloading yt-dlp…")),
        YouTube::Fetching => Some(working(k, "fetching", "Getting the audio…")),
        YouTube::Failed(e) => Some(problem(k, "youtube-problem", danger(), e)),
    };
    card(k)
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
                    Button::new("Get audio")
                        .disabled(busy || f.link.text.trim().is_empty())
                        .on_press(State::get_audio)
                        .build(k)
                        .key("get-audio"),
                ),
        )
        .children(status)
}

/// Said once, before yt-dlp is first downloaded.
fn install_notice(k: &Kit) -> El {
    col()
        .key("install-notice")
        .w_full()
        .padding(12.0)
        .gap(10.0)
        .radius(10.0)
        .fill(wash(k.c.page, warn(), 0.08))
        .border(1.0, warn().alpha(0.35))
        .child(
            text(format!(
                "To get audio from YouTube and X, Warble downloads yt-dlp ({}), a free tool, \
                 from its official GitHub page. Warble keeps it up to date after that.",
                youtube::TOOL_SIZE
            ))
            .font(SMALL)
            .color(k.c.text_base)
            .wrap(),
        )
        .child(
            row()
                .gap(8.0)
                .child(
                    Button::new("Download yt-dlp")
                        .variant(ButtonVariant::Submit)
                        .on_press(State::install_youtube_tool)
                        .build(k)
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

fn sites_card(k: &Kit) -> El {
    let mut c = card(k).key("sites").gap(4.0).child(
        text("Good places to find sounds")
            .font(HEADING)
            .color(k.c.text_base)
            .mb(4.0),
    );
    for (i, site) in SITES.iter().enumerate() {
        c = c.child(
            row()
                .key(format!("site-{i}"))
                .role("link")
                .label(site.name)
                .description(site.note)
                .w_full()
                .px(8.0)
                .py(7.0)
                .gap(10.0)
                .center_y()
                .radius(8.0)
                .cursor(Cursor::Pointer)
                .hover(k.c.hover)
                .press(k.c.pressed)
                .on_press_tx(open_url(site.url))
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .gap(2.0)
                        .a11y_hidden()
                        .child(text(site.name).font(LABEL).color(k.c.text_base))
                        .child(text(site.note).font(SMALL).color(k.c.text_muted).wrap()),
                )
                .child(icon(&OPEN, 14.0).color(k.c.icon_muted)),
        );
    }
    c
}

fn key_card(f: &Find, k: &Kit) -> El {
    card(k)
        .key("freesound")
        .child(heading(
            k,
            "Freesound key",
            "Freesound’s search needs a free key. Make one, then paste it here once.",
        ))
        .child(
            TextInput::new(&f.key)
                .secure()
                .placeholder("Paste your key")
                .label("Freesound key")
                .width(None)
                .on_edit(|s: &mut State| &mut s.find.key)
                .on_change(|s: &mut State, _| s.set_freesound_key())
                .build(k)
                .key("key-field"),
        )
        .child(
            row().child(
                Button::new("Get a free key")
                    .icon(&OPEN)
                    .on_press_tx(open_url(FREESOUND_KEY_URL))
                    .build(k)
                    .key("get-key"),
            ),
        )
}

// --- Trim ---

/// The waveform's height, and the room each handle needs at the edges.
const WAVE_H: f32 = 96.0;
const EDGE: f32 = 8.0;

pub fn trim_page(s: &State, cx: &mut Cx<State>, k: &Kit) -> El {
    let Some(t) = &s.trim else { return el() };
    let playing = s.status.preview.is_some();
    let length = t.end - t.start;
    let hint = if t.duration() > trim::MAX_SECONDS {
        "Plays in your headphones only. Sounds can be up to 5 minutes long."
    } else {
        "Plays in your headphones only."
    };
    let name = TextInput::new(&s.find.trim_name)
        .placeholder("Name")
        .label("Name")
        .width(None)
        .on_edit(|s: &mut State| &mut s.find.trim_name)
        .on_submit(|s: &mut State, _| s.trim_save())
        .build(k)
        .key("trim-name");
    let times = row()
        .key("trim-times")
        .w_full()
        .center_y()
        .child(
            text(trim::clock(t.start))
                .font(NUMBER)
                .color(k.c.text_muted)
                .w(64.0),
        )
        .child(
            text(if t.is_whole() {
                format!("Whole clip · {}", trim::clock(length))
            } else {
                format!("Keeping {}", trim::clock(length))
            })
            .font(NUMBER)
            .color(k.c.text_base)
            .grow(1.0)
            .align(Align::Center),
        )
        .child(
            text(trim::clock(t.end))
                .font(NUMBER)
                .color(k.c.text_muted)
                .w(64.0)
                .align(Align::End),
        );
    let actions = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            Button::new(if playing { "Stop" } else { "Play selection" })
                .icon(if playing { &STOP } else { &PLAY })
                .help("Plays in your headphones only")
                .on_press(State::trim_play)
                .build(k)
                .key("trim-play"),
        )
        .child(el().grow(1.0))
        .child(
            Button::new(if t.saving { "Saving…" } else { "Save" })
                .icon(&CHECK)
                .variant(ButtonVariant::Submit)
                .disabled(t.saving)
                .on_press(State::trim_save)
                .build(k)
                .key("trim-save"),
        );
    col()
        .key("trim")
        .grow(1.0)
        .min_h(0.0)
        .child(page_bar(k, "Trim", State::close_trim))
        .child(scrolling(
            col().key("trim-body").children(notice(s, k)).child(
                card(k)
                    .key("trim-card")
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
                            .gap(8.0)
                            .child(waveform(s, t, cx, k))
                            .child(times),
                    )
                    .child(actions)
                    .child(text(hint).font(SMALL).color(k.c.text_faint).wrap()),
            ),
        ))
}

/// Moves the start (or with `end`, the end) handle to `at` seconds.
fn set_handle(t: &mut Trim, end: bool, at: f64) {
    if end { t.set_end(at) } else { t.set_start(at) }
}

/// The clip's waveform with the selection lit, a handle at each end (drag, or focus and use
/// the arrow keys), and the playhead while the selection plays. Pressing the waveform moves the
/// nearer handle there.
fn waveform(s: &State, t: &Trim, cx: &mut Cx<State>, k: &Kit) -> El {
    let w = cx.layout_size("wave").map_or(320.0, |z| z[0]);
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
                .radius(1.0)
                .fill(if lit {
                    accent()
                } else {
                    k.c.text_faint.alpha(0.45)
                }),
        );
    }
    let selection = el()
        .absolute(x(a), 0.0)
        .size((b - a) * inner, WAVE_H)
        .fill(accent().alpha(0.08))
        .a11y_hidden();

    let mut wave = el()
        .key("wave")
        .w_full()
        .h(WAVE_H)
        .radius(8.0)
        .fill(k.c.layer_02)
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
                .fill(oklch(0.97, 0.0, 0.0))
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
        .focus_ring(2.0, 2.0, k.c.border_focus)
        .absolute(at - EDGE, 0.0)
        .size(2.0 * EDGE, WAVE_H)
        .center()
        .cursor(Cursor::ResizeColumn)
        .on_step(move |s: &mut State, steps, _| s.step_trim(end, steps))
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
                .fill(accent()),
        )
        .child(
            el().size(10.0, 26.0)
                .radius(5.0)
                .fill(oklch(0.97, 0.0, 0.0))
                .shadow(1.0, 3.0, rune::Color::BLACK.alpha(0.45)),
        )
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
        s.find.problem = Some("Couldn't reach Myinstants. Check your internet.".into());
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
