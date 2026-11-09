//! Renders the tray icon: a ring gauge filled to the usage level, coloured by
//! pacing status, with a tick marking where usage *should* be right now.
//! Pure std (raw RGBA), so it can be unit-tested without Tauri.

use std::f64::consts::PI;

pub const SIZE: u32 = 64;

#[derive(Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const GREEN: Rgb = Rgb(0x2e, 0xa0, 0x43);
pub const AMBER: Rgb = Rgb(0xd2, 0x99, 0x22);
pub const RED: Rgb = Rgb(0xf8, 0x51, 0x49);
pub const GRAY: Rgb = Rgb(0x8b, 0x94, 0x9e);

pub struct Gauge {
    /// 0–100, or None when unknown (renders an empty ring).
    pub used_pct: Option<f64>,
    /// 0–100, where the pace tick goes. None hides the tick.
    pub expected_pct: Option<f64>,
    pub color: Rgb,
}

/// Angle of a point measured clockwise from 12 o'clock, in [0, 2π).
fn clock_angle(dx: f64, dy: f64) -> f64 {
    let a = dx.atan2(-dy);
    if a < 0.0 {
        a + 2.0 * PI
    } else {
        a
    }
}

/// Returns `SIZE * SIZE * 4` bytes of straight (non-premultiplied) RGBA.
pub fn render(g: &Gauge) -> Vec<u8> {
    const SS: u32 = 4; // supersampling per axis
    let size = SIZE as f64;
    let c = size / 2.0;
    let r_out = size * 0.47;
    let r_in = size * 0.25;
    let tick_in = size * 0.14;
    let tick_out = size * 0.50;
    let tick_half_width = size * 0.045;

    let used = g.used_pct.map(|u| u.clamp(0.0, 100.0) / 100.0 * 2.0 * PI);
    let tick = g.expected_pct.map(|e| e.clamp(0.0, 100.0) / 100.0 * 2.0 * PI);
    let track = Rgb(0x9a, 0xa0, 0xa6);
    let tick_col = Rgb(0x6e, 0x76, 0x81);

    let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
    for py in 0..SIZE {
        for px in 0..SIZE {
            // Accumulate premultiplied colour over subsamples.
            let (mut r, mut gg, mut b, mut a) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f64 + (sx as f64 + 0.5) / SS as f64;
                    let y = py as f64 + (sy as f64 + 0.5) / SS as f64;
                    let (dx, dy) = (x - c, y - c);
                    let dist = (dx * dx + dy * dy).sqrt();
                    let ang = clock_angle(dx, dy);

                    let mut sample: Option<(Rgb, f64)> = None;
                    if dist >= r_in && dist <= r_out {
                        let filled = used.map(|u| ang <= u).unwrap_or(false);
                        sample = Some(if filled { (g.color, 1.0) } else { (track, 0.45) });
                    }
                    if let Some(t) = tick {
                        // Distance from the point to the tick's radial line.
                        let (ux, uy) = (t.sin(), -t.cos());
                        let along = dx * ux + dy * uy;
                        let perp = (dx * uy - dy * ux).abs();
                        if along >= tick_in && along <= tick_out && perp <= tick_half_width {
                            sample = Some((tick_col, 1.0));
                        }
                    }
                    if let Some((col, alpha)) = sample {
                        r += col.0 as f64 * alpha;
                        gg += col.1 as f64 * alpha;
                        b += col.2 as f64 * alpha;
                        a += alpha;
                    }
                }
            }
            let n = (SS * SS) as f64;
            let i = ((py * SIZE + px) * 4) as usize;
            if a > 0.0 {
                out[i] = (r / a).round() as u8;
                out[i + 1] = (gg / a).round() as u8;
                out[i + 2] = (b / a).round() as u8;
                out[i + 3] = (a / n * 255.0).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * SIZE + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn dimensions_and_transparency() {
        let buf = render(&Gauge { used_pct: Some(25.0), expected_pct: None, color: GREEN });
        assert_eq!(buf.len(), (SIZE * SIZE * 4) as usize);
        assert_eq!(px(&buf, 0, 0)[3], 0, "corner is transparent");
        assert_eq!(px(&buf, SIZE / 2, SIZE / 2)[3], 0, "centre is transparent");
    }

    #[test]
    fn fill_follows_usage() {
        let buf = render(&Gauge { used_pct: Some(25.0), expected_pct: None, color: RED });
        // Right side of ring (3 o'clock region just past 12) is filled.
        let ring_mid = (SIZE as f64 * 0.39) as u32;
        let right = px(&buf, SIZE / 2 + ring_mid, SIZE / 2 - 4);
        assert_eq!(&right[..3], &[RED.0, RED.1, RED.2]);
        assert_eq!(right[3], 255);
        // Left side (9 o'clock) is unfilled track — semi transparent.
        let left = px(&buf, SIZE / 2 - ring_mid, SIZE / 2);
        assert!(left[3] > 0 && left[3] < 200);
    }

    #[test]
    fn unknown_usage_renders_track_only() {
        let buf = render(&Gauge { used_pct: None, expected_pct: None, color: GRAY });
        assert!(buf.chunks(4).all(|p| p[3] < 200));
    }
}
