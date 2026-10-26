//! SF-Symbols-style glyphs, ported from the original `MacSymbol.qml`.
//!
//! Each symbol is a small SVG on a 24x24 grid, tinted with a single colour and rasterised
//! with resvg. Rasterised glyphs are cached per (name, size, colour).

use std::collections::HashMap;
use tiny_skia::{Pixmap, Transform};

/// `#rrggbb` for an sRGB colour.
pub fn hex(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

/// Arc around (`cx`,`cy`) with radius `r` between two angles in degrees (0 = right, -90 = up).
fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> String {
    // Round to 2 decimals and drop the sign of zero so paths never contain "-0.00".
    let n = |v: f32| (v * 100.0).round() / 100.0 + 0.0;
    let (a, b) = (from.to_radians(), to.to_radians());
    format!("M{:.2} {:.2}A{r} {r} 0 0 1 {:.2} {:.2}", n(cx + r * a.cos()), n(cy + r * a.sin()), n(cx + r * b.cos()), n(cy + r * b.sin()))
}

fn stroke(d: &str, opacity: Option<f32>) -> String {
    let op = opacity.map(|o| format!(" stroke-opacity=\"{o}\"")).unwrap_or_default();
    format!("<path d=\"{d}\" fill=\"none\" stroke=\"C\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{op}/>")
}

fn fill(d: &str, opacity: Option<f32>) -> String {
    let op = opacity.map(|o| format!(" fill-opacity=\"{o}\"")).unwrap_or_default();
    format!("<path d=\"{d}\" fill=\"C\"{op}/>")
}

fn wifi(level: u32, off: bool) -> String {
    // Dot plus three arcs fanning upward from it; unlit arcs are dimmed like macOS.
    let mut out = fill("M12 20.6a1.7 1.7 0 1 0 0-3.4a1.7 1.7 0 1 0 0 3.4z", off.then_some(0.35));
    for (i, r) in [5.2f32, 9.4, 13.6].iter().enumerate() {
        let lit = !off && level > i as u32;
        out += &stroke(&arc(12.0, 19.0, *r, -137.0, -43.0), (!lit).then_some(0.3));
    }
    if off {
        out += &stroke("M4.5 4.5L19.5 19.5", None);
    }
    out
}

fn speaker(level: u32, muted: bool) -> String {
    let body = fill("M3.6 9h2.9l4.2-3.7c.6-.5 1.5-.1 1.5.7v12c0 .8-.9 1.2-1.5.7L6.5 15H3.6c-.6 0-1.1-.5-1.1-1.1V10.1c0-.6.5-1.1 1.1-1.1z", None);
    if muted {
        return body + &stroke("M15.5 9.5l5 5M20.5 9.5l-5 5", None);
    }
    let mut out = body;
    for (i, r) in [3.4f32, 6.2, 9.0].iter().enumerate() {
        if level > i as u32 {
            out += &stroke(&arc(12.0, 12.0, *r, -48.0, 48.0), None);
        }
    }
    out
}

fn bluetooth(state: &str) -> String {
    let mut rune = stroke("M7 7.6l9.6 8.6L12 20.3V3.7l4.6 4.1L7 16.4", (state == "off").then_some(0.4));
    if state == "connected" {
        rune += &fill("M4 13.2a1.3 1.3 0 1 0 0-2.6a1.3 1.3 0 1 0 0 2.6zM20 13.2a1.3 1.3 0 1 0 0-2.6a1.3 1.3 0 1 0 0 2.6z", None);
    }
    rune
}

fn sun_shape(small: bool) -> String {
    let (r, inner, outer) = if small { (3.4f32, 5.6f32, 7.6f32) } else { (4.2, 6.6, 9.0) };
    let mut out = fill(&format!("M12 {}a{r} {r} 0 1 0 0-{}a{r} {r} 0 1 0 0 {}z", 12.0 + r, 2.0 * r, 2.0 * r), None);
    for k in 0..8 {
        let t = (k as f32 * 45.0).to_radians();
        out += &stroke(&format!("M{:.2} {:.2}L{:.2} {:.2}", 12.0 + inner * t.cos(), 12.0 + inner * t.sin(), 12.0 + outer * t.cos(), 12.0 + outer * t.sin()), None);
    }
    out
}

const CLOUD: &str = "M7 18.5a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 18 10.5a4 4 0 0 1-.5 8z";

/// SVG body (without the wrapper) for a symbol name; `C` marks the tint colour.
pub fn shapes(name: &str) -> Option<String> {
    Some(match name {
        "apple" => fill("M16.365 1.43c0 1.14-.493 2.27-1.177 3.08-.744.9-1.99 1.57-2.987 1.57-.12 0-.23-.02-.3-.03-.01-.06-.04-.22-.04-.39 0-1.15.572-2.27 1.206-2.98.804-.94 2.142-1.64 3.248-1.68.03.13.05.28.05.43zm4.565 15.71c-.03.07-.463 1.58-1.518 3.12-.945 1.34-1.94 2.71-3.43 2.71-1.517 0-1.9-.88-3.63-.88-1.698 0-2.302.91-3.67.91-1.377 0-2.332-1.26-3.428-2.8-1.287-1.82-2.323-4.63-2.323-7.28 0-4.28 2.797-6.55 5.552-6.55 1.448 0 2.675.95 3.6.95.865 0 2.222-1.01 3.902-1.01.613 0 2.886.06 4.374 2.19-.13.09-2.383 1.37-2.383 4.19 0 3.26 2.854 4.42 2.955 4.45z", None),
        "wifi-0" => wifi(0, false),
        "wifi-1" => wifi(1, false),
        "wifi-2" => wifi(2, false),
        "wifi-3" => wifi(3, false),
        "wifi-off" => wifi(0, true),
        "ethernet" => stroke("M4 9h14M14.5 5.5L18 9l-3.5 3.5M20 15H6M9.5 11.5L6 15l3.5 3.5", None),
        "speaker-0" => speaker(0, false),
        "speaker-1" => speaker(1, false),
        "speaker-2" => speaker(2, false),
        "speaker-3" => speaker(3, false),
        "speaker-muted" => speaker(0, true),
        "headphones" => {
            stroke("M4 15V12a8 8 0 0 1 16 0v3", None)
                + "<rect x=\"2.6\" y=\"12.6\" width=\"4.8\" height=\"7.6\" rx=\"1.8\" fill=\"C\"/><rect x=\"16.6\" y=\"12.6\" width=\"4.8\" height=\"7.6\" rx=\"1.8\" fill=\"C\"/>"
        }
        "bluetooth" => bluetooth("on"),
        "bluetooth-connected" => bluetooth("connected"),
        "bluetooth-off" => bluetooth("off"),
        "magnifyingglass" => stroke("M10.5 17a6.5 6.5 0 1 0 0-13a6.5 6.5 0 1 0 0 13zM15.4 15.4L20.5 20.5", None),
        "switches" => {
            "<rect x=\"2.5\" y=\"3.8\" width=\"19\" height=\"7\" rx=\"3.5\" fill=\"none\" stroke=\"C\" stroke-width=\"1.8\"/>".to_string()
                + &fill("M17.9 9.3a2 2 0 1 0 0-4a2 2 0 1 0 0 4z", None)
                + "<rect x=\"2.5\" y=\"13.2\" width=\"19\" height=\"7\" rx=\"3.5\" fill=\"none\" stroke=\"C\" stroke-width=\"1.8\"/>"
                + &fill("M6.1 18.7a2 2 0 1 0 0-4a2 2 0 1 0 0 4z", None)
        }
        "moon" => fill("M19.6 14.7A8.3 8.3 0 1 1 9.3 4.4a6.6 6.6 0 0 0 10.3 10.3z", None),
        "sun" => sun_shape(false),
        "sun-small" => sun_shape(true),
        "night-shift" => fill("M7.6 16.5a4.4 4.4 0 0 1 8.8 0z", None) + &stroke("M3.5 16.5h17M12 6.8v2M5.6 9.6l1.4 1.4M18.4 9.6L17 11M6 20h12", None),
        "dark-mode" => "<circle cx=\"12\" cy=\"12\" r=\"8.2\" fill=\"none\" stroke=\"C\" stroke-width=\"2\"/>".to_string() + &fill("M12 3.8a8.2 8.2 0 0 1 0 16.4z", None),
        "cup" => stroke("M5 9h11v5a5 5 0 0 1-5 5h-1a5 5 0 0 1-5-5zM16 10.5h1.5a2.5 2.5 0 0 1 0 5H16M8 3.5v2.5M11 3.5v2.5M14 3.5v2.5", None),
        "screenshot" => stroke("M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2", None) + "<circle cx=\"12\" cy=\"12\" r=\"3.2\" fill=\"none\" stroke=\"C\" stroke-width=\"2\"/>",
        "record" => "<circle cx=\"12\" cy=\"12\" r=\"8.4\" fill=\"none\" stroke=\"C\" stroke-width=\"2\"/>".to_string() + &fill("M12 16.4a4.4 4.4 0 1 0 0-8.8a4.4 4.4 0 1 0 0 8.8z", None),
        "play" => fill("M8 5.2c0-.9 1-1.4 1.7-.9l9.6 6.8c.6.4.6 1.4 0 1.8l-9.6 6.8c-.7.5-1.7 0-1.7-.9z", None),
        "pause" => fill("M6.5 5.5c0-.6.5-1 1-1h2.3c.6 0 1 .4 1 1v13c0 .6-.4 1-1 1H7.5c-.5 0-1-.4-1-1zM13.2 5.5c0-.6.4-1 1-1h2.3c.5 0 1 .4 1 1v13c0 .6-.5 1-1 1h-2.3c-.6 0-1-.4-1-1z", None),
        "forward" => fill("M2.5 7.2c0-.8.9-1.2 1.5-.8l7.5 4.8c.6.4.6 1.2 0 1.6L4 17.6c-.6.4-1.5 0-1.5-.8zM12 7.2c0-.8.9-1.2 1.5-.8l7.5 4.8c.6.4.6 1.2 0 1.6l-7.5 4.8c-.6.4-1.5 0-1.5-.8z", None),
        "backward" => fill("M21.5 7.2c0-.8-.9-1.2-1.5-.8l-7.5 4.8c-.6.4-.6 1.2 0 1.6l7.5 4.8c.6.4 1.5 0 1.5-.8zM12 7.2c0-.8-.9-1.2-1.5-.8L3 11.2c-.6.4-.6 1.2 0 1.6l7.5 4.8c.6.4 1.5 0 1.5-.8z", None),
        "chevron-right" => stroke("M9.5 5.5L16 12l-6.5 6.5", None),
        // Weather glyphs for the bar (simple, monochrome).
        "weather-clear" => sun_shape(false),
        "weather-cloud" => fill(CLOUD, None),
        "weather-partly" => fill("M15.5 3.5a3.6 3.6 0 1 1 0 7.2a3.6 3.6 0 1 1 0-7.2z", None) + &fill("M7 20a4.2 4.2 0 0 1-.5-8.4A5.6 5.6 0 0 1 17 13.5a3.3 3.3 0 0 1-.4 6.5z", None),
        "weather-rain" => fill("M7 14.5a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 8a3.3 3.3 0 0 1-.4 6.5z", None) + &stroke("M8 17.5l-1 3M12 17.5l-1 3M16 17.5l-1 3", None),
        "weather-snow" => fill("M7 14.5a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 8a3.3 3.3 0 0 1-.4 6.5z", None) + &stroke("M8 18.5v.01M12 18.5v.01M16 18.5v.01M10 21v.01M14 21v.01", None),
        "weather-storm" => fill("M7 14a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 7.5a3.3 3.3 0 0 1-.4 6.5z", None) + &fill("M12.8 13l-3.6 5h2.8l-1 4 4.4-6h-2.9z", None),
        "weather-fog" => fill("M7 12a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 5.5a3.3 3.3 0 0 1-.4 6.5z", None) + &stroke("M5 16h14M7 20h10", None),
        _ => return None,
    })
}

/// Full SVG document for `name` tinted `color`.
pub fn svg(name: &str, color: (u8, u8, u8)) -> Option<String> {
    let body = shapes(name)?;
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\">{}</svg>", body.replace("\"C\"", &format!("\"{}\"", hex(color)))))
}

/// The Nerd Font that carries the bar's "agents" robot glyph (U+F16A3), if installed.
fn nerd_font() -> Option<&'static fontdue::Font> {
    static FONT: std::sync::OnceLock<Option<fontdue::Font>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}", "JetBrainsMono Nerd Font"]).output().ok()?;
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.to_lowercase().contains("nerd") {
            return None;
        }
        fontdue::Font::from_bytes(std::fs::read(path).ok()?, fontdue::FontSettings::default()).ok()
    })
    .as_ref()
}

/// True when the robot "agents" glyph can be drawn on this machine.
pub fn agents_available() -> bool {
    nerd_font().is_some()
}

/// Rasterises one Nerd Font glyph, centred in a `px` x `px` pixmap and tinted with `color`.
fn nerd_glyph(ch: char, color: (u8, u8, u8), px: u32) -> Option<Pixmap> {
    let font = nerd_font()?;
    let (m, cov) = font.rasterize(ch, px as f32);
    let mut pm = Pixmap::new(px.max(1), px.max(1))?;
    let (ox, oy) = ((px as i32 - m.width as i32) / 2, (px as i32 - m.height as i32) / 2);
    let data = pm.data_mut();
    for row in 0..m.height as i32 {
        for col in 0..m.width as i32 {
            let (x, y) = (ox + col, oy + row);
            if x < 0 || y < 0 || x >= px as i32 || y >= px as i32 {
                continue;
            }
            let a = cov[row as usize * m.width + col as usize] as u32;
            let i = (y as usize * px as usize + x as usize) * 4;
            data[i] = (color.0 as u32 * a / 255) as u8;
            data[i + 1] = (color.1 as u32 * a / 255) as u8;
            data[i + 2] = (color.2 as u32 * a / 255) as u8;
            data[i + 3] = a as u8;
        }
    }
    Some(pm)
}

/// Rasterises `name` at `px` x `px`.
pub fn render(name: &str, color: (u8, u8, u8), px: u32) -> Option<Pixmap> {
    match name {
        "agents" => return nerd_glyph('\u{f16a3}', color, px),
        "awake" => return nerd_glyph('\u{f0176}', color, px).or_else(|| render("cup", color, px)),
        _ => {}
    }
    let doc = svg(name, color)?;
    let tree = resvg::usvg::Tree::from_str(&doc, &resvg::usvg::Options::default()).ok()?;
    let mut pm = Pixmap::new(px.max(1), px.max(1))?;
    let scale = px as f32 / 24.0;
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pm.as_mut());
    Some(pm)
}

type SymbolKey = (String, (u8, u8, u8), u32);

/// Caches rendered symbols.
#[derive(Default)]
pub struct SymbolCache {
    map: HashMap<SymbolKey, Option<Pixmap>>,
}

impl SymbolCache {
    pub fn get(&mut self, name: &str, color: (u8, u8, u8), px: u32) -> Option<&Pixmap> {
        self.map.entry((name.to_string(), color, px)).or_insert_with(|| render(name, color, px)).as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&str] = &[
        "apple",
        "wifi-0",
        "wifi-1",
        "wifi-2",
        "wifi-3",
        "wifi-off",
        "ethernet",
        "speaker-0",
        "speaker-1",
        "speaker-2",
        "speaker-3",
        "speaker-muted",
        "headphones",
        "bluetooth",
        "bluetooth-connected",
        "bluetooth-off",
        "magnifyingglass",
        "switches",
        "moon",
        "sun",
        "sun-small",
        "night-shift",
        "dark-mode",
        "cup",
        "screenshot",
        "record",
        "play",
        "pause",
        "forward",
        "backward",
        "chevron-right",
        "weather-clear",
        "weather-cloud",
        "weather-partly",
        "weather-rain",
        "weather-snow",
        "weather-storm",
        "weather-fog",
    ];

    #[test]
    fn every_symbol_parses_and_draws_something() {
        for name in ALL {
            let pm = render(name, (255, 255, 255), 48).unwrap_or_else(|| panic!("{name} failed to render"));
            assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0), "{name} is blank");
        }
    }

    #[test]
    fn unknown_symbol_is_none() {
        assert!(svg("nope", (0, 0, 0)).is_none());
    }

    #[test]
    fn tint_is_applied() {
        assert!(svg("moon", (10, 132, 255)).unwrap().contains("#0a84ff"));
    }

    #[test]
    fn arc_endpoints() {
        assert_eq!(arc(0.0, 0.0, 10.0, 0.0, 90.0), "M10.00 0.00A10 10 0 0 1 0.00 10.00");
    }

    #[test]
    fn cache_returns_same_size() {
        let mut c = SymbolCache::default();
        assert_eq!(c.get("play", (255, 255, 255), 32).map(|p| p.width()), Some(32));
        assert!(c.get("nope", (255, 255, 255), 32).is_none());
    }
}
