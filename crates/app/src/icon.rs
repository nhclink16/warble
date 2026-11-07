//! Warble's icon, drawn in code: a mint rounded square with five dark waveform bars. The tray
//! and (on Windows) the window and taskbar use it.

const MINT: [f32; 3] = [72.0, 214.0, 164.0];
const INK: [f32; 3] = [18.0, 22.0, 28.0];
/// Bar heights as fractions of the icon, left to right.
const BARS: [f32; 5] = [0.22, 0.46, 0.64, 0.40, 0.18];

/// Straight (not premultiplied) RGBA rows, `size × size × 4` bytes.
pub fn rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut body, mut bar) = (0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if in_square(u, v) {
                        body += 1.0;
                        if in_bars(u, v) {
                            bar += 1.0;
                        }
                    }
                }
            }
            let n = (SS * SS) as f32;
            let t = if body > 0.0 { bar / body } else { 0.0 };
            for i in 0..3 {
                out.push((MINT[i] + (INK[i] - MINT[i]) * t).round() as u8);
            }
            out.push((body / n * 255.0).round() as u8);
        }
    }
    out
}

fn in_square(u: f32, v: f32) -> bool {
    let (lo, hi, r) = (0.04, 0.96, 0.24);
    let dx = (lo + r - u).max(u - (hi - r)).max(0.0);
    let dy = (lo + r - v).max(v - (hi - r)).max(0.0);
    (lo..=hi).contains(&u) && (lo..=hi).contains(&v) && dx * dx + dy * dy <= r * r
}

fn in_bars(u: f32, v: f32) -> bool {
    let (w, pitch) = (0.085, 0.145);
    BARS.iter().enumerate().any(|(i, h)| {
        let cx = 0.5 + (i as f32 - 2.0) * pitch;
        // A capsule: the bar's centre line, rounded by half its width.
        let half = (h - w).max(0.0) / 2.0;
        let dy = ((v - 0.5).abs() - half).max(0.0);
        let dx = u - cx;
        dx * dx + dy * dy <= (w / 2.0) * (w / 2.0)
    })
}

/// Sets the window's title bar and taskbar icon.
#[cfg(windows)]
pub fn set_window_icon(window: &rune::WindowHandle) {
    use rune::raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIcon, ICON_BIG, ICON_SMALL, SendMessageW, WM_SETICON,
    };

    let Ok(Some(hwnd)) = window.with_raw_handle(|h| match h.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }) else {
        return;
    };
    for (kind, size) in [(ICON_BIG, 48), (ICON_SMALL, 16)] {
        let mut bgra = rgba(size);
        let mut mask = Vec::with_capacity(bgra.len() / 4);
        for p in bgra.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
            mask.push(!p[3]);
        }
        // SAFETY: both buffers hold size × size pixels and outlive the call, which copies them.
        // The icon is never destroyed: the window keeps using it for the life of the process.
        unsafe {
            let icon = CreateIcon(
                std::ptr::null_mut(),
                size as i32,
                size as i32,
                1,
                32,
                mask.as_ptr(),
                bgra.as_ptr(),
            );
            if !icon.is_null() {
                SendMessageW(hwnd as _, WM_SETICON, kind as usize, icon as isize);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// An .ico of uncompressed 32-bit images, one per size (each under 256).
    fn ico(sizes: &[u32]) -> Vec<u8> {
        let images: Vec<Vec<u8>> = sizes
            .iter()
            .map(|&size| {
                let mut img = Vec::new();
                let mask = size.div_ceil(32) * 4 * size;
                for v in [
                    40,
                    size,
                    2 * size,
                    32 << 16 | 1,
                    0,
                    size * size * 4 + mask,
                    0,
                    0,
                    0,
                    0,
                ] {
                    img.extend_from_slice(&v.to_le_bytes());
                }
                let px = super::rgba(size);
                for row in px.chunks(size as usize * 4).rev() {
                    for p in row.chunks(4) {
                        img.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                    }
                }
                img.resize(img.len() + mask as usize, 0);
                img
            })
            .collect();
        let mut out = Vec::new();
        for v in [0u16, 1, sizes.len() as u16] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let mut offset = 6 + 16 * sizes.len() as u32;
        for (&size, img) in sizes.iter().zip(&images) {
            out.extend_from_slice(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0]);
            out.extend_from_slice(&(img.len() as u32).to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
            offset += img.len() as u32;
        }
        images.iter().for_each(|img| out.extend_from_slice(img));
        out
    }

    /// The icon `scripts/install.ps1` gives the Start menu shortcut, which is what launchers
    /// show. After changing the drawing, rewrite it with `WARBLE_WRITE_ICO=1 cargo test`.
    #[test]
    fn the_ico_file_matches_the_drawing() {
        let ico = ico(&[16, 24, 32, 48, 64, 128]);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/warble.ico");
        if std::env::var_os("WARBLE_WRITE_ICO").is_some() {
            std::fs::write(path, &ico).unwrap();
        }
        let saved = std::fs::read(path).unwrap_or_default();
        assert!(
            saved == ico,
            "assets/warble.ico is stale: rerun with WARBLE_WRITE_ICO=1"
        );
    }

    #[test]
    fn the_icon_is_opaque_inside_clear_at_the_corners_and_has_bars() {
        let px = super::rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..][..4];
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(16, 3)[3], 255);
        assert!(at(16, 16)[1] < 60, "the middle bar is dark");
    }
}
