//! The tray icon's waiting-count badge (P7): a count, 1 to 9 or "9+", in a red disc over the
//! bottom-right of the app's mark. Drawn into an RGBA buffer from a tiny built-in bitmap font, so
//! no image decoder or font is needed. Zero means no badge.

/// Digits and "+", three columns by five rows, one bit per pixel, top row first.
const GLYPHS: [(char, [u8; 5]); 11] = [
    ('0', [0b111, 0b101, 0b101, 0b101, 0b111]),
    ('1', [0b010, 0b110, 0b010, 0b010, 0b111]),
    ('2', [0b111, 0b001, 0b111, 0b100, 0b111]),
    ('3', [0b111, 0b001, 0b011, 0b001, 0b111]),
    ('4', [0b101, 0b101, 0b111, 0b001, 0b001]),
    ('5', [0b111, 0b100, 0b111, 0b001, 0b111]),
    ('6', [0b111, 0b100, 0b111, 0b101, 0b111]),
    ('7', [0b111, 0b001, 0b010, 0b010, 0b010]),
    ('8', [0b111, 0b101, 0b111, 0b101, 0b111]),
    ('9', [0b111, 0b101, 0b111, 0b001, 0b111]),
    ('+', [0b000, 0b010, 0b111, 0b010, 0b000]),
];

const DISC: [u8; 4] = [0xd9, 0x3b, 0x3b, 0xff];
const RING: [u8; 4] = [0xff, 0xff, 0xff, 0xff];
const INK: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// The badge's text: none for zero, the count up to 9, then "9+".
pub fn label(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=9 => Some(count.to_string()),
        _ => Some("9+".into()),
    }
}

/// The tooltip beside the badge.
pub fn tooltip(count: u32) -> String {
    match count {
        0 => "Chorus".into(),
        1 => "Chorus: 1 session needs you".into(),
        n => format!("Chorus: {n} sessions need you"),
    }
}

/// `base` (RGBA, `width` × `height`) with the badge for `count` drawn over it. A buffer of the
/// wrong size, or a count of zero, comes back unchanged.
pub fn overlay(base: &[u8], width: u32, height: u32, count: u32) -> Vec<u8> {
    // About two thirds of the icon, against the bottom-right corner.
    draw(base, width, height, count, i64::from(width.min(height)) * 2 / 3)
}

/// The badge alone, filling a `size` × `size` canvas: the taskbar button's overlay icon.
pub fn alone(base: &[u8], size: u32, count: u32) -> Vec<u8> {
    draw(base, size, size, count, i64::from(size))
}

fn draw(base: &[u8], width: u32, height: u32, count: u32, diameter: i64) -> Vec<u8> {
    let mut out = base.to_vec();
    let (w, h) = (i64::from(width), i64::from(height));
    let Some(text) = label(count) else { return out };
    if w < 8 || h < 8 || base.len() as i64 != w * h * 4 {
        return out;
    }
    let mut put = |x: i64, y: i64, c: [u8; 4]| {
        if (0..w).contains(&x) && (0..h).contains(&y) {
            let i = ((y * w + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&c);
        }
    };
    // The disc, with a light ring so it reads on a dark or a light taskbar.
    let d = diameter.clamp(7, w.min(h));
    let (cx2, cy2) = (2 * w - d, 2 * h - d); // centre, doubled to stay in integers
    let r2 = d * d; // (diameter)² = (2·radius)²
    let ring = (d / 8).max(1);
    let inner2 = (d - 2 * ring) * (d - 2 * ring);
    for y in (h - d).max(0)..h {
        for x in (w - d).max(0)..w {
            let (dx, dy) = (2 * x + 1 - cx2, 2 * y + 1 - cy2);
            let dist2 = dx * dx + dy * dy;
            if dist2 <= inner2 {
                put(x, y, DISC);
            } else if dist2 <= r2 {
                put(x, y, RING);
            }
        }
    }
    // The text, as large as fits inside the disc, centred.
    let glyphs: Vec<[u8; 5]> =
        text.chars().filter_map(|c| GLYPHS.iter().find(|(g, _)| *g == c).map(|(_, rows)| *rows)).collect();
    let cols = glyphs.len() as i64 * 4 - 1;
    let inner = d - 2 * ring;
    let scale = (inner * 3 / 5 / 5).min(inner * 4 / 5 / cols).max(1);
    let (tw, th) = (cols * scale, 5 * scale);
    let (left, top) = (w - d + (d - tw) / 2, h - d + (d - th) / 2);
    for (i, rows) in glyphs.iter().enumerate() {
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..3 {
                if bits >> (2 - col) & 1 == 1 {
                    let (gx, gy) = (left + (i as i64 * 4 + col) * scale, top + row as i64 * scale);
                    for sy in 0..scale {
                        for sx in 0..scale {
                            put(gx + sx, gy + sy, INK);
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(size: u32) -> Vec<u8> {
        vec![0x20; (size * size * 4) as usize]
    }

    fn count_colour(buf: &[u8], c: [u8; 4]) -> usize {
        buf.chunks(4).filter(|p| *p == c).count()
    }

    #[test]
    fn zero_has_no_badge() {
        let base = blank(32);
        assert_eq!(overlay(&base, 32, 32, 0), base);
        assert_eq!(label(0), None);
    }

    #[test]
    fn counts_draw_a_disc_with_white_text() {
        let base = blank(32);
        let one = overlay(&base, 32, 32, 1);
        assert_ne!(one, base);
        assert!(count_colour(&one, DISC) > 50);
        assert!(count_colour(&one, INK) > 10);
        // The top-left quarter (the mark) is untouched.
        for y in 0..10 {
            for x in 0..10 {
                let i = (y * 32 + x) * 4;
                assert_eq!(&one[i..i + 4], &base[i..i + 4]);
            }
        }
    }

    #[test]
    fn nine_and_ten_differ_ten_and_ninety_nine_match() {
        let base = blank(32);
        let nine = overlay(&base, 32, 32, 9);
        let ten = overlay(&base, 32, 32, 10);
        assert_ne!(nine, ten);
        assert_eq!(ten, overlay(&base, 32, 32, 99));
        assert_eq!(label(9).as_deref(), Some("9"));
        assert_eq!(label(10).as_deref(), Some("9+"));
        assert_eq!(label(99).as_deref(), Some("9+"));
    }

    #[test]
    fn scales_to_large_icons_and_refuses_bad_buffers() {
        let big = overlay(&blank(256), 256, 256, 3);
        assert!(count_colour(&big, INK) > 400);
        let short = vec![0u8; 10];
        assert_eq!(overlay(&short, 32, 32, 3), short);
    }

    #[test]
    fn alone_fills_its_canvas() {
        let blank = vec![0u8; 32 * 32 * 4];
        let badge = alone(&blank, 32, 4);
        assert!(count_colour(&badge, DISC) > 400);
        assert_eq!(alone(&blank, 32, 0), blank);
    }

    #[test]
    fn tooltips_name_the_count() {
        assert_eq!(tooltip(0), "Chorus");
        assert_eq!(tooltip(1), "Chorus: 1 session needs you");
        assert_eq!(tooltip(2), "Chorus: 2 sessions need you");
    }
}
