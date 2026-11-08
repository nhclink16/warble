//! The Setup tab's Phone remote card: the switch, and while it's on, the QR code a phone scans.

use std::time::Duration;

use qrcodegen::{QrCode, QrCodeEcc};
use rune::{Color, El, Tx, col, el, icon, oklch, row, text};
use rune_kit::{Button, ControlSize, Kit, Switch};

use crate::state::State;
use crate::ui::{
    BODY, ICON, NUMBER, SMALL, WARNING, as_key, danger, group, problem_line, title_row, tray, warn,
};

/// How long a copied link, key and all, stays on the clipboard.
const COPIED_FOR: Duration = Duration::from_secs(2 * 60);

pub fn remote_card(s: &State, k: &Kit) -> El {
    let on = s.settings.remote;
    let c = group(k).key("remote").child(
        row()
            .w_full()
            .gap(10.0)
            .center_y()
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .gap(2.0)
                    .child(title_row(k, "Phone remote"))
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
        let says = "Connect this PC to Wi‑Fi or a network cable, then open this tab again.";
        return c.child(problem_line(k, warn(), BODY, says).label(says));
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
            tray(
                k,
                "Connect over",
                &["Wi‑Fi", "Tailscale"],
                usize::from(r.use_tailscale),
                |s, i| s.remote.use_tailscale = i == 1,
            )
            // A step down from the tabs: a choice within the card, not a page.
            .h(30.0)
            .key("remote-network"),
        );
    }
    c.child(
        row()
            .w_full()
            .gap(14.0)
            .items_start()
            .child(qr(k, &link))
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
                            .font(NUMBER)
                            .color(k.c.text_faint)
                            .truncate(),
                    )
                    .child(
                        row().child(
                            as_key(
                                Button::new("Copy link")
                                    .help("Copies the link with its code. Warble clears it from the clipboard after 2 minutes.")
                                    .on_press_tx(move |_: &mut State, tx: &mut Tx| {
                                        tx.copy_secret(link.clone(), COPIED_FOR);
                                    })
                                    .build(k),
                            )
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
                as_key(
                    Button::new("Sign out all phones")
                        .on_press(State::forget_phones)
                        .build(k),
                )
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
        .label(why.to_string())
        .w_full()
        .gap(8.0)
        .items_start()
        .child(icon(&WARNING, ICON).color(danger()).mt(1.0))
        .child(
            text(why.to_string())
                .font(BODY)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        )
}

/// Points per module, and the white margin around the code, in modules.
const MODULE: f32 = 4.0;
const QUIET: i32 = 2;

/// The link as a QR code: black runs on white, one rectangle per run of a row.
fn qr(k: &Kit, link: &str) -> El {
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
        .radius(k.radius.lg)
        .fill(oklch(1.0, 0.0, 0.0))
        .shadow(2.0, 4.0, Color::BLACK.alpha(0.55));
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
