//! The Setup tab's Phone remote card: the switch, and while it's on, the QR code a phone scans.

use std::time::Duration;

use qrcodegen::{QrCode, QrCodeEcc};
use rune::{Color, El, Tx, col, el, icon, oklch, row, text};
use rune_kit::{Button, ControlSize, Kit, SegmentedControl, Switch};

use crate::state::State;
use crate::ui::{BODY, HEADING, SMALL, WARNING, card, danger};

/// How long a copied link, key and all, stays on the clipboard.
const COPIED_FOR: Duration = Duration::from_secs(2 * 60);

pub fn remote_card(s: &State, k: &Kit) -> El {
    let on = s.settings.remote;
    let c = card(k).key("remote").child(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .gap(3.0)
                    .child(text("Phone remote").font(HEADING).color(k.c.text_base))
                    .child(
                        text("Play sounds and change your voice from your\u{a0}phone.")
                            .font(SMALL)
                            .color(k.c.text_muted)
                            .wrap(),
                    ),
            )
            .child(
                Switch::new(on)
                    .label("Phone remote")
                    .hide_label()
                    .control_size(ControlSize::Small)
                    .on_toggle(|s: &mut State| s.set_remote(!s.settings.remote))
                    .build(k)
                    .key("remote-switch"),
            ),
    );
    if !on {
        return c;
    }
    let r = &s.remote;
    if let Some(problem) = &r.problem {
        return c.child(warning(k, problem));
    }
    let Some(address) = r.address() else {
        return c.child(
            text("Connect this PC to Wi‑Fi or a network cable, then open this tab again.")
                .font(BODY)
                .color(k.c.text_base)
                .wrap(),
        );
    };
    let link = format!("{address}/#k={}", s.settings.remote_key);
    let network = if r.use_tailscale {
        "Your phone needs Tailscale on."
    } else {
        "Your phone needs to be on the same Wi‑Fi as this PC."
    };
    let line = |words: &str| {
        text(words.to_string())
            .font(BODY)
            .color(k.c.text_base)
            .wrap()
    };
    let mut c = c;
    if r.wifi.is_some() && r.tailscale.is_some() {
        c = c.child(
            SegmentedControl::new(&["Wi‑Fi", "Tailscale"], Some(usize::from(r.use_tailscale)))
                .label("Connect over")
                .width(None)
                .on_pick(|s: &mut State, i, _| s.remote.use_tailscale = i == 1)
                .build(k)
                .key("remote-network"),
        );
    }
    c.child(
        row()
            .w_full()
            .gap(14.0)
            .items_start()
            .child(qr(&link))
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .gap(8.0)
                    .child(line(
                        "Scan this with your phone’s camera, or copy the link and send it to \
                         your\u{a0}phone.",
                    ))
                    .child(line(network))
                    .child(
                        text(address.trim_start_matches("http://").to_string())
                            .font(SMALL)
                            .color(k.c.text_faint)
                            .truncate(),
                    )
                    .child(
                        row().child(
                            Button::new("Copy link")
                                .help("Copies the link with its code. Warble clears it from the clipboard after 2 minutes.")
                                .on_press_tx(move |_: &mut State, tx: &mut Tx| {
                                    tx.copy_secret(link.clone(), COPIED_FOR);
                                })
                                .build(k)
                                .key("copy-link"),
                        ),
                    ),
            ),
    )
    .child(
        text(
            "If Windows asks whether Warble can use the network, choose Allow. If your phone \
             still can’t open it, allow Warble in Windows Security > Firewall & network \
             protection > Allow an app through firewall.",
        )
        .font(SMALL)
        .color(k.c.text_muted)
        .wrap(),
    )
    .child(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(
                Button::new("Sign out all phones")
                    .on_press(State::forget_phones)
                    .build(k)
                    .key("new-code"),
            )
            .child(
                text("Shows a new code. Scan it again on the phones you\u{a0}keep.")
                    .font(SMALL)
                    .color(k.c.text_muted)
                    .grow(1.0)
                    .shrink()
                    .wrap(),
            ),
    )
}

fn warning(k: &Kit, why: &str) -> El {
    row()
        .key("remote-problem")
        .role("alert")
        .w_full()
        .gap(8.0)
        .items_start()
        .child(icon(&WARNING, 16.0 /* ICON */).color(danger()).mt(1.0))
        .child(
            text(why.to_string())
                .font(BODY)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink(),
        )
}

/// Points per module, and the white margin around the code, in modules.
const MODULE: f32 = 4.0;
const QUIET: i32 = 2;

/// The link as a QR code: black runs on white, one rectangle per run of a row.
fn qr(link: &str) -> El {
    let Ok(code) = QrCode::encode_text(link, QrCodeEcc::Low) else {
        return el();
    };
    let n = code.size();
    let side = (n + 2 * QUIET) as f32 * MODULE;
    let mut b = el()
        .key("remote-qr")
        .role("img")
        .label("QR code with the link for your phone")
        .size(side, side)
        .radius(8.0)
        .fill(oklch(1.0, 0.0, 0.0));
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if !code.get_module(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < n && code.get_module(x, y) {
                x += 1;
            }
            b = b.child(
                el().absolute((start + QUIET) as f32 * MODULE, (y + QUIET) as f32 * MODULE)
                    .size((x - start) as f32 * MODULE, MODULE)
                    .fill(Color::BLACK),
            );
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use rune::{Env, Runtime};

    use crate::state::State;

    fn settle(rt: &mut Runtime<State>) {
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
    }

    #[test]
    fn copy_link_copies_the_whole_link_with_its_key() {
        // Tall enough that the whole Setup tab shows.
        let mut rt = Runtime::new(
            State::mock(),
            crate::ui::view,
            (400.0, 1400.0),
            Env::new(true, 1.0),
        );
        let board = rune::clipboard::memory();
        rt.use_clipboard(&board);
        rt.update(|s, _| super::scenes::on(s));
        settle(&mut rt);
        let (x, y) = rt.find("copy-link").expect("the Copy link button");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(&mut rt);
        let copied = board.text().map(|t| t.to_string());
        assert_eq!(
            copied.as_deref(),
            Some("http://192.168.1.20:47823/#k=4f9c2a7e1b3d58c6a0e2f7419d8b3c65")
        );
        let qr = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("remote-qr"));
        assert_eq!(
            qr.and_then(|e| e.label).as_deref(),
            Some("QR code with the link for your phone")
        );
    }
}

pub mod scenes {
    use std::net::Ipv4Addr;

    use crate::state::{State, Tab};

    fn remote(s: &mut State, wifi: Option<Ipv4Addr>, tailscale: Option<Ipv4Addr>) {
        s.set_tab(Tab::Setup);
        s.set_remote(true);
        s.settings.remote_key = "4f9c2a7e1b3d58c6a0e2f7419d8b3c65".into();
        s.remote.set_addresses(wifi, tailscale);
    }

    pub fn on(s: &mut State) {
        remote(s, Some(Ipv4Addr::new(192, 168, 1, 20)), None);
    }

    pub fn tailscale(s: &mut State) {
        remote(
            s,
            Some(Ipv4Addr::new(192, 168, 1, 20)),
            Some(Ipv4Addr::new(100, 101, 2, 3)),
        );
        s.remote.use_tailscale = true;
    }

    pub fn no_network(s: &mut State) {
        remote(s, None, None);
    }
}
