//! Original seven-row dot alphabet; no external font or raster assets.
//! `size` is the full seven-row height. Unsupported characters show a question mark.
use tiny_skia::{Color, Pixmap};

fn glyph(c: char) -> ([u8; 7], usize) {
    let rows = match c.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 25, 21, 19, 19, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 31],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        ':' => return ([0, 2, 7, 2, 7, 2, 0], 3),
        '·' => return ([0, 0, 0, 1, 0, 0, 0], 1),
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '.' => return ([0, 0, 0, 0, 0, 0, 1], 1),
        '°' => return ([7, 5, 7, 0, 0, 0, 0], 3),
        '%' => [25, 25, 2, 4, 8, 19, 19],
        '-' | '–' | '—' => [0, 0, 0, 31, 0, 0, 0],
        ' ' => return ([0; 7], 3),
        _ => [14, 17, 1, 2, 4, 0, 4],
    };
    (rows, 5)
}

pub fn width(s: &str, size: f32) -> f32 {
    let cols: usize = s.chars().take(400).map(|c| glyph(c).1 + 1).sum();
    cols.saturating_sub(1) as f32 * size / 7.0
}

pub fn paint(pm: &mut Pixmap, s: &str, size: f32, mut x: f32, y: f32, color: Color) {
    let cell = size / 7.0;
    for c in s.chars().take(400) {
        let (rows, cols) = glyph(c);
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..cols {
                if bits & (1 << (cols - col - 1)) != 0 {
                    crate::gfx::fill_circle(pm, x + (col as f32 + 0.5) * cell, y + (row as f32 + 0.5) * cell, cell * 0.43, color);
                }
            }
        }
        x += (cols + 1) as f32 * cell;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_clock_has_separated_round_dots_and_bounded_width() {
        let mut pm = Pixmap::new(270, 70).unwrap();
        paint(&mut pm, "12:34", 70.0, 0.0, 0.0, Color::WHITE);
        assert_eq!(width("12:34", 70.0), 270.0);
        assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(pm.pixel(25, 5).unwrap().alpha(), 255);
        assert_eq!(pm.pixel(19, 5).unwrap().alpha(), 0);
        assert_eq!(width("", 70.0), 0.0);
        assert!(pm.pixel(255, 65).unwrap().alpha() > 0, "last digit must fit the measured width");
        assert_eq!(glyph('a'), glyph('A'));
        assert_ne!(glyph('A'), glyph('?'));
        assert_eq!(width("29 OCT · FRI", 14.0), 122.0);
        assert_eq!(width(&"1".repeat(500), 7.0), width(&"1".repeat(400), 7.0));
    }
}
