//! SF-Symbols-style glyphs, ported from the original `MacSymbol.qml`.
//!
//! Each symbol is a small SVG on a 24x24 grid, tinted with a single colour and rasterised
//! with resvg. Rasterised glyphs are cached per (name, size, colour).

use std::collections::HashMap;
use std::sync::Arc;
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

fn mic(muted: bool) -> String {
    // Capsule on a stand; muted dims it and strikes it through like macOS.
    let capsule = fill("M12 3a3.2 3.2 0 0 0-3.2 3.2v5.6a3.2 3.2 0 0 0 6.4 0V6.2A3.2 3.2 0 0 0 12 3z", muted.then_some(0.45));
    capsule + &stroke("M6.2 11.2a5.8 5.8 0 0 0 11.6 0M12 17v3.5M8.6 20.5h6.8", muted.then_some(0.45)) + &if muted { stroke("M4.5 4.5L19.5 19.5", None) } else { String::new() }
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
        "person" => "<circle cx=\"12\" cy=\"7.5\" r=\"3.5\" fill=\"none\" stroke=\"C\" stroke-width=\"2\"/>".to_string() + &stroke("M5 21v-2a7 7 0 0 1 14 0v2z", None),
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
        "mic" => mic(false),
        "mic-muted" => mic(true),
        "headphones" => {
            stroke("M4 15V12a8 8 0 0 1 16 0v3", None)
                + "<rect x=\"2.6\" y=\"12.6\" width=\"4.8\" height=\"7.6\" rx=\"1.8\" fill=\"C\"/><rect x=\"16.6\" y=\"12.6\" width=\"4.8\" height=\"7.6\" rx=\"1.8\" fill=\"C\"/>"
        }
        "bluetooth" => bluetooth("on"),
        "bluetooth-connected" => bluetooth("connected"),
        "bluetooth-off" => bluetooth("off"),
        "magnifyingglass" => stroke("M10.5 17a6.5 6.5 0 1 0 0-13a6.5 6.5 0 1 0 0 13zM15.4 15.4L20.5 20.5", None),
        "chart" => stroke("M4.5 4v15.5H20M8.5 15.5v-4M12.5 15.5V7M16.5 15.5v-6.5", None),
        "lock" => stroke("M8 11V8a4 4 0 0 1 8 0v3", None) + &fill("M6.5 11h11a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 5 18.5v-6A1.5 1.5 0 0 1 6.5 11z", None),
        "power" => stroke("M12 3.5v8M7.1 6.6a8 8 0 1 0 9.8 0", None),
        "logout" => stroke("M10 4.5H6A1.5 1.5 0 0 0 4.5 6v12A1.5 1.5 0 0 0 6 19.5h4M15 8l4 4-4 4M19 12H9", None),
        "download" => stroke("M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14", None),
        "trash-can" => stroke("M5 7h14M9 7V4.5h6V7M7 7l1 12.5h8L17 7M10 11v5M14 11v5", None),
        "refresh" => stroke("M19 12a7 7 0 1 1-2.1-5M19 4.5V8h-3.5", None),
        "equal" => stroke("M5 9h14M5 15h14", None),
        "globe" => "<circle cx=\"12\" cy=\"12\" r=\"8.5\" fill=\"none\" stroke=\"C\" stroke-width=\"2\"/>".to_string() + &stroke("M3.5 12h17M12 3.5c3 3 3 14 0 17M12 3.5c-3 3-3 14 0 17", None),
        "grid" => fill("M5 5h5v5H5zM14 5h5v5h-5zM5 14h5v5H5zM14 14h5v5h-5z", None),
        "picture" => stroke("M4 6h16v12H4zM4 16l5-5 4 4 3-3 4 4", None),
        "doc" => stroke("M7 3.5h6.5L18 8v12.5H7zM13.5 3.5V8H18M9.5 12.5h5M9.5 15.5h5", None),
        "folder" => stroke("M3.5 6.5h6l2 2.5h9v9.5h-17z", None),
        "terminal" => stroke("M4 5.5h16v13H4zM7.5 10l3 2.5-3 2.5M12.5 15h4", None),
        "chevron-left" => stroke("M14.5 5.5L8 12l6.5 6.5", None),
        "switches" => {
            "<rect x=\"2.5\" y=\"3.8\" width=\"19\" height=\"7\" rx=\"3.5\" fill=\"none\" stroke=\"C\" stroke-width=\"1.8\"/>".to_string()
                + &fill("M17.9 9.3a2 2 0 1 0 0-4a2 2 0 1 0 0 4z", None)
                + "<rect x=\"2.5\" y=\"13.2\" width=\"19\" height=\"7\" rx=\"3.5\" fill=\"none\" stroke=\"C\" stroke-width=\"1.8\"/>"
                + &fill("M6.1 18.7a2 2 0 1 0 0-4a2 2 0 1 0 0 4z", None)
        }
        "snowflake" => stroke("M12 3.5v17M4.6 7.75l14.8 8.5M4.6 16.25l14.8-8.5M9.8 5.4L12 7.6l2.2-2.2M9.8 18.6l2.2-2.2 2.2 2.2", None),
        "gamepad" => stroke("M7.5 8h9a4.5 4.5 0 0 1 4.4 5.3l-.7 3.8a2.3 2.3 0 0 1-4 1L14.5 16h-5l-2.2 2.1a2.3 2.3 0 0 1-4-1l-.7-3.8A4.5 4.5 0 0 1 7.5 8zM8 10.5v3M6.5 12h3", None) + &fill("M15.6 11.1a.9.9 0 1 0 0 1.8a.9.9 0 1 0 0-1.8zM17.9 12.9a.9.9 0 1 0 0 1.8a.9.9 0 1 0 0-1.8z", None),
        "apps" => fill("M4 4h4v4H4zM10 4h4v4h-4zM16 4h4v4h-4zM4 10h4v4H4zM10 10h4v4h-4zM16 10h4v4h-4zM4 16h4v4H4zM10 16h4v4h-4zM16 16h4v4h-4z", None),
        "clipboard" => stroke("M9 5H6a2 2 0 0 0-2 2v13h16V7a2 2 0 0 0-2-2h-3M9 3h6v5H9zM8 12h8M8 16h6", None),
        "bell" => stroke("M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0", None),
        "bell-off" => stroke("M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0", Some(0.45)) + &stroke("M4.5 4.5L19.5 19.5", None),
        "eye" => stroke("M2.5 12s3.6-6.5 9.5-6.5 9.5 6.5 9.5 6.5-3.6 6.5-9.5 6.5S2.5 12 2.5 12z", None) + "<circle cx=\"12\" cy=\"12\" r=\"3\" fill=\"C\"/>",
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
        "weather-moon" => fill("M20 14.6A8.4 8.4 0 1 1 9.4 4a6.7 6.7 0 0 0 10.6 10.6z", None),
        "weather-cloud" => fill(CLOUD, None),
        "weather-partly" => fill("M15.5 3.5a3.6 3.6 0 1 1 0 7.2a3.6 3.6 0 1 1 0-7.2z", None) + &fill("M7 20a4.2 4.2 0 0 1-.5-8.4A5.6 5.6 0 0 1 17 13.5a3.3 3.3 0 0 1-.4 6.5z", None),
        "weather-rain" => fill("M7 14.5a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 8a3.3 3.3 0 0 1-.4 6.5z", None) + &stroke("M8 17.5l-1 3M12 17.5l-1 3M16 17.5l-1 3", None),
        "weather-snow" => fill("M7 14.5a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 8a3.3 3.3 0 0 1-.4 6.5z", None) + &stroke("M8 18.5v.01M12 18.5v.01M16 18.5v.01M10 21v.01M14 21v.01", None),
        "weather-storm" => fill("M7 14a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 7.5a3.3 3.3 0 0 1-.4 6.5z", None) + &fill("M12.8 13l-3.6 5h2.8l-1 4 4.4-6h-2.9z", None),
        "weather-fog" => fill("M7 12a4.2 4.2 0 0 1-.5-8.3A5.6 5.6 0 0 1 17 5.5a3.3 3.3 0 0 1-.4 6.5z", None) + &stroke("M5 16h14M7 20h10", None),
        // the folded application menu (three lines) and show desktop (a screen with nothing on it)
        "menu" => stroke("M4.5 7h15M4.5 12h15M4.5 17h15", None),
        "desktop" => stroke("M3.5 5.5h17v11h-17zM9 20h6M12 16.5V20", None),
        _ => return None,
    })
}

/// Full SVG document for `name` tinted `color`.
pub fn svg(name: &str, color: (u8, u8, u8)) -> Option<String> {
    let body = shapes(name)?;
    Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\">{}</svg>", body.replace("\"C\"", &format!("\"{}\"", hex(color)))))
}

/// Path of the Nerd Font that carries the bar's "agents" robot glyph (U+F16A3), if installed.
fn nerd_font_path() -> Option<&'static std::path::Path> {
    static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}", "JetBrainsMono Nerd Font"]).output().ok()?;
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (path.to_lowercase().contains("nerd") && std::path::Path::new(&path).is_file()).then(|| path.into())
    })
    .as_deref()
}

/// True when the robot "agents" glyph can be drawn on this machine.
pub fn agents_available() -> bool {
    nerd_font_path().is_some()
}

/// Rasterises one Nerd Font glyph, centred in a `px` x `px` pixmap and tinted with `color`.
fn nerd_glyph(ch: char, color: (u8, u8, u8), px: u32) -> Option<Pixmap> {
    // Parsing the face costs tens of MB, so it lives only as long as one rasterisation.
    let (m, cov) = {
        let font = fontdue::Font::from_bytes(std::fs::read(nerd_font_path()?).ok()?, fontdue::FontSettings::default()).ok()?;
        font.rasterize(ch, px as f32)
    };
    crate::mem::trim();
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

/// The original plugin's weather icon (WeatherSymbol.qml, monochrome mode) for the Nerd Font code point `g`
/// its condition table yields. Everything is drawn in the single bar colour.
pub fn weather_svg(g: u32, color: (u8, u8, u8)) -> String {
    let c = hex(color);
    let clear = matches!(g, 0xe30d | 0xe32b);
    let night = matches!(g, 0xe32b | 0xe32e);
    let partly = matches!(g, 0xe302 | 0xe32e);
    let mut body = String::new();
    if clear {
        body += if night { "<g transform=\"translate(-3 5)\">" } else { "<g transform=\"translate(5 6)\">" };
    }
    if clear || partly {
        if night {
            body += &format!("<path d=\"M18 3a8 8 0 1 0 10 10A8 8 0 0 1 18 3Z\" fill=\"{c}\"/>");
        } else {
            body += &format!(
                "<circle cx=\"12\" cy=\"11\" r=\"5\" fill=\"{c}\"/><path d=\"M12 2v2M12 18v2M3 11h2M19 11h2M5.5 4.5 7 6M17 16l1.5 1.5M5.5 17.5 7 16M17 6l1.5-1.5\" stroke=\"{c}\" stroke-width=\"1.8\" stroke-linecap=\"round\"/>"
            );
        }
    }
    if clear {
        body += "</g>";
    } else {
        body += &format!("<path d=\"M8 25a6 6 0 1 1 1-12 8 8 0 0 1 15-1 6.5 6.5 0 1 1 1 13Z\" fill=\"{c}\"/>");
    }
    if matches!(g, 0xe318 | 0xe308 | 0xe333) {
        body += &format!("<path d=\"m10 27-1 3m8-3-1 3m8-3-1 3\" stroke=\"{c}\" stroke-width=\"2\" stroke-linecap=\"round\"/>");
    }
    if matches!(g, 0xe31a | 0xe30a | 0xe327 | 0xe3ad) {
        body += &format!("<circle cx=\"10\" cy=\"29\" r=\"1.2\" fill=\"{c}\"/><circle cx=\"17\" cy=\"29\" r=\"1.2\" fill=\"{c}\"/><circle cx=\"24\" cy=\"29\" r=\"1.2\" fill=\"{c}\"/>");
    }
    if g == 0xe31d {
        body += &format!("<path d=\"m18 20-6 8h5l-2 4 9-10h-6l2-2Z\" fill=\"{c}\"/>");
    }
    if matches!(g, 0xe346 | 0xe313) {
        body += &format!("<path d=\"M6 28h21M10 31h14\" stroke=\"{c}\" stroke-width=\"1.4\" stroke-linecap=\"round\"/>");
    }
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 34 34\">{body}</svg>")
}

/// Rasterises `name` at `px` x `px`.
pub fn render(name: &str, color: (u8, u8, u8), px: u32) -> Option<Pixmap> {
    if let Some(code) = name.strip_prefix("wx:") {
        let g = u32::from_str_radix(code, 16).ok()?;
        let tree = resvg::usvg::Tree::from_str(&weather_svg(g, color), &resvg::usvg::Options::default()).ok()?;
        let mut pm = Pixmap::new(px.max(1), px.max(1))?;
        let scale = px as f32 / 34.0;
        resvg::render(&tree, Transform::from_scale(scale, scale), &mut pm.as_mut());
        return Some(pm);
    }
    if let Some(hex) = name.strip_prefix("nerd:") {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).and_then(|c| nerd_glyph(c, color, px));
    }
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

fn material_codepoint(name: &str) -> Option<char> {
    char::from_u32(match name {
        "apps" => 0xe5c3,
        "play" => 0xe037,
        "pause" => 0xe034,
        "backward" => 0xe045,
        "forward" => 0xe044,
        "bell" => 0xe7f4,
        "clipboard" => 0xe14f,
        "switches" => 0xe8b8,
        "chart" => 0xe66b,
        "wifi-0" | "wifi-1" | "wifi-2" | "wifi-3" => 0xe63e,
        "wifi-off" => 0xe648,
        "ethernet" => 0xe8be,
        "speaker-0" | "speaker-1" | "speaker-2" | "speaker-3" => 0xe050,
        "speaker-muted" => 0xe04f,
        "bluetooth" | "bluetooth-connected" => 0xe1a7,
        "bluetooth-off" => 0xe1a9,
        "power" => 0xe8ac,
        "layout" => 0xe8f1,
        "sun" | "sun-small" => 0xe1ac,
        "weather-clear" => 0xf157,
        "moon" | "dark-mode" => 0xe51c,
        "weather-moon" => 0xf159,
        "contrast" => 0xeb37,
        "do_not_disturb_on" => 0xe15c,
        "night-shift" => 0xf03d,
        "cup" => 0xe541,
        "screenshot" => 0xec08,
        "weather-partly" => 0xf172,
        "weather-cloud" => 0xf15c,
        "weather-fog" => 0xe818,
        "weather-storm" => 0xebdb,
        "weather-snow" => 0xe810,
        "weather-rain" => 0xf176,
        "gear" => 0xe8b8,
        "person" => 0xe7fd,
        "close" => 0xe5cd,
        "dashboard" => 0xe871,
        "music_note" => 0xe405,
        "wb_sunny" => 0xe430,
        "settings" => 0xe8b8,
        "device_thermostat" => 0xe1ff,
        "memory" => 0xe322,
        "developer_board" => 0xe30d,
        "cloud" => 0xe2bd,
        "sunny" => 0xe81a,
        "battery" => 0xe1a4,
        "record" => 0xf679,
        "gamepad" => 0xea28,
        "eye" => 0xe8f4,
        "magnifyingglass" => 0xe8b6,
        "lock" => 0xe897,
        "menu" => 0xe5d2,
        "desktop" => 0xe30c,
        _ => return None,
    })
}

type SymbolKey = (String, (u8, u8, u8), u32);

/// Caches rendered symbols.
#[derive(Default)]
pub struct SymbolCache {
    map: HashMap<SymbolKey, Option<Pixmap>>,
    material: Option<(Arc<[u8]>, crate::theme::fontdb::FaceRef)>,
    material_dark: bool,
    material_map: HashMap<SymbolKey, Option<Pixmap>>,
}

impl SymbolCache {
    /// Resolve licensed package Material Symbols once at startup/theme reload, outside paint.
    pub fn prepare_material(&mut self) -> bool {
        let theme = crate::theme::th();
        let db = crate::theme::fontdb::FontDb::default().with_package_faces(&theme.fonts.package_faces);
        let material = db.resolve("Material Symbols Rounded").and_then(|family| db.face(&family, crate::theme::fontdb::FC_REGULAR)).and_then(|face| {
            let version = theme.fonts.files.iter().zip(&theme.fonts.file_versions).find(|(p, _)| **p == face.file)?.1;
            crate::theme::fontdb::package_font_bytes(&face.file, *version).map(|bytes| (bytes, face))
        });
        let same = match (&self.material, &material) {
            (Some((a, af)), Some((b, bf))) => Arc::ptr_eq(a, b) && af == bf,
            (None, None) => true,
            _ => false,
        };
        if !same || self.material_dark != theme.dark {
            self.material_map.clear();
        }
        self.material_dark = theme.dark;
        self.material = material;
        self.material.is_some()
    }

    /// Actual Material Symbols outlines, never substituted with SF-style artwork.
    /// Missing package fonts or unsupported outlines return None.
    pub fn get_material(&mut self, name: &str, color: (u8, u8, u8), px: u32) -> Option<&Pixmap> {
        const MAX_ENTRIES: usize = 256;
        const MAX_BYTES: usize = 4 * 1024 * 1024;
        if !(1..=256).contains(&px) {
            return None;
        }
        let ch = material_codepoint(name)?;
        let key = (name.to_string(), color, px);
        if !self.material_map.contains_key(&key) {
            let used: usize = self.material_map.values().flatten().map(|image| image.data().len()).sum();
            if self.material_map.len() >= MAX_ENTRIES || used + px as usize * px as usize * 4 > MAX_BYTES {
                self.material_map.clear();
            }
        }
        let (bytes, face) = self.material.as_ref()?;
        self.material_map
            .entry(key)
            .or_insert_with(|| {
                let axes = [(*b"FILL", 0.0), (*b"GRAD", if crate::theme::th().dark { -25.0 } else { 0.0 }), (*b"opsz", 24.0)];
                let glyph = crate::textface::raster_outline_axes(bytes, face, ch, px as f32, &axes)?;
                if glyph.width == 0 || glyph.height == 0 {
                    return None;
                }
                let mut image = Pixmap::new(px, px)?;
                // Match centered Text's advance/em-box placement, retaining optical
                // side bearings instead of recentering each glyph's ink rectangle.
                let font = ttf_parser::Face::parse(bytes, face.index).ok()?;
                let scale = px as f32 / font.units_per_em() as f32;
                let height = (font.ascender() as f32 - font.descender() as f32) * scale;
                let baseline = ((px as f32 - height) / 2.0 + font.ascender() as f32 * scale).round() as i32;
                let left = ((px as f32 - glyph.advance) / 2.0).floor() as i32 + glyph.xmin;
                let top = baseline - glyph.ymin - glyph.height as i32;
                for y in 0..glyph.height {
                    for x in 0..glyph.width {
                        let (dx, dy) = (left + x as i32, top + y as i32);
                        if dx < 0 || dy < 0 || dx >= px as i32 || dy >= px as i32 {
                            continue;
                        }
                        let a = glyph.bitmap[y * glyph.width + x];
                        let i = (dy as usize * px as usize + dx as usize) * 4;
                        image.data_mut()[i..i + 4].copy_from_slice(&[(color.0 as u16 * a as u16 / 255) as u8, (color.1 as u16 * a as u16 / 255) as u8, (color.2 as u16 * a as u16 / 255) as u8, a]);
                    }
                }
                Some(image)
            })
            .as_ref()
    }

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
        "mic",
        "mic-muted",
        "headphones",
        "bluetooth",
        "bluetooth-connected",
        "bluetooth-off",
        "magnifyingglass",
        "chart",
        "switches",
        "person",
        "moon",
        "sun",
        "sun-small",
        "night-shift",
        "dark-mode",
        "cup",
        "snowflake",
        "gamepad",
        "bell",
        "bell-off",
        "eye",
        "screenshot",
        "record",
        "play",
        "pause",
        "forward",
        "backward",
        "chevron-right",
        "weather-clear",
        "weather-moon",
        "weather-cloud",
        "weather-partly",
        "weather-rain",
        "weather-snow",
        "weather-storm",
        "weather-fog",
    ];

    #[test]
    fn weather_icons_render_for_every_condition() {
        for g in [0xe30d, 0xe32b, 0xe302, 0xe32e, 0xe33d, 0xe313, 0xe346, 0xe318, 0xe31a, 0xe31d] {
            let pm = render(&format!("wx:{g:x}"), (255, 255, 255), 34).unwrap();
            assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0), "glyph {g:x} drew nothing");
        }
    }

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
    #[test]
    fn every_material_mapping_resolves_in_the_shipped_cmap() {
        let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/dux-lavender/fonts/MaterialSymbolsRounded.ttf")).unwrap();
        let font = ttf_parser::Face::parse(&bytes, 0).unwrap();
        // Enumerate the actual mapping arms so adding an alias cannot omit this coverage.
        let source = include_str!("symbols.rs");
        let body = source.split("fn material_codepoint").nth(1).unwrap().split("type SymbolKey").next().unwrap();
        let mut count = 0;
        for line in body.lines().filter(|line| line.contains("=> 0x")) {
            for name in line.split("=>").next().unwrap().split('|') {
                let name = name.trim().trim_matches('"');
                let cp = material_codepoint(name).unwrap();
                assert!(font.glyph_index(cp).is_some(), "{name}: U+{:04X} absent", cp as u32);
                count += 1;
            }
        }
        assert!(count >= 50);
        assert_ne!(material_codepoint("contrast"), material_codepoint("do_not_disturb_on"));
        assert_eq!(material_codepoint("cup"), char::from_u32(0xe541));
        assert_eq!(material_codepoint("weather-clear"), char::from_u32(0xf157));
        assert_eq!(material_codepoint("weather-moon"), char::from_u32(0xf159));
    }

    #[test]
    #[ignore = "requires DUX_MATERIAL_FONT licensed package asset"]
    fn package_material_icons_are_actual_cached_outlines() {
        let path = std::path::PathBuf::from(std::env::var_os("DUX_MATERIAL_FONT").expect("DUX_MATERIAL_FONT"));
        let db = crate::theme::fontdb::FontDb::default().with_package_files(std::slice::from_ref(&path));
        let family = db.resolve("Material Symbols Rounded").expect("Material family");
        let face = db.face(&family, crate::theme::fontdb::FC_REGULAR).unwrap();
        assert!(std::env::var_os("DECEBAL_CONFIG").is_some(), "explicit private config required");
        let version = crate::theme::fontdb::package_file_version(&path);
        let mut theme = (*crate::theme::th()).clone();
        theme.fonts.files = vec![path.clone()];
        theme.fonts.file_versions = vec![version];
        theme.fonts.package_faces = crate::theme::fontdb::FontDb::read_package_faces(std::slice::from_ref(&path));
        crate::theme::live::publish_theme(theme);
        let mut cache = SymbolCache::default();
        assert!(cache.prepare_material());
        let mut other = SymbolCache::default();
        assert!(other.prepare_material());
        assert!(Arc::ptr_eq(&cache.material.as_ref().unwrap().0, &other.material.as_ref().unwrap().0), "bar/overlay caches share one byte snapshot");
        let bytes = cache.material.as_ref().unwrap().0.clone();
        let names: Vec<_> = include_str!("symbols.rs")
            .split("fn material_codepoint")
            .nth(1)
            .unwrap()
            .split("type SymbolKey")
            .next()
            .unwrap()
            .lines()
            .filter(|line| line.contains("=> 0x"))
            .flat_map(|line| line.split("=>").next().unwrap().split('|').map(|name| name.trim().trim_matches('"')))
            .collect();
        for size in [14, 28] {
            for name in &names {
                let image = cache.get_material(name, (208, 188, 255), size).unwrap_or_else(|| panic!("outline unavailable: {name} at {size}px"));
                assert!(image.data().chunks_exact(4).any(|p| p[3] > 0), "missing {name}");
                assert_eq!(image.width(), size);
                if let Some(dir) = std::env::var_os("DUX_ICON_EVIDENCE") {
                    std::fs::create_dir_all(&dir).unwrap();
                    image.save_png(std::path::Path::new(&dir).join(format!("{name}-{size}.png"))).unwrap();
                }
            }
        }
        assert_eq!(cache.material_map.len(), names.len() * 2);
        assert!(cache.prepare_material());
        assert!(Arc::ptr_eq(&bytes, &cache.material.as_ref().unwrap().0));
        assert_eq!(cache.material_map.len(), names.len() * 2, "unchanged preparation preserves raster entries");
        // Source Text centers the advance/em box. For this pinned font the
        // 1056/-96 metrics at UPEM960 place the centered baseline exactly at px.
        for name in ["play", "forward"] {
            let cp = material_codepoint(name).unwrap();
            let axes = [(*b"FILL", 0.0), (*b"GRAD", if crate::theme::th().dark { -25.0 } else { 0.0 }), (*b"opsz", 24.0)];
            let glyph = crate::textface::raster_outline_axes(&bytes, &face, cp, 14.0, &axes).unwrap();
            let left = ((14.0 - glyph.advance) / 2.0).floor() as i32 + glyph.xmin;
            let top = 14 - glyph.ymin - glyph.height as i32;
            let image = cache.get_material(name, (208, 188, 255), 14).unwrap();
            for y in 0..14usize {
                for x in 0..14usize {
                    let gx = x as i32 - left;
                    let gy = y as i32 - top;
                    let expected = if gx >= 0 && gy >= 0 && gx < glyph.width as i32 && gy < glyph.height as i32 { glyph.bitmap[gy as usize * glyph.width + gx as usize] } else { 0 };
                    assert_eq!(image.data()[(y * 14 + x) * 4 + 3], expected, "{name} em-box position at{x},{y}");
                }
            }
        }
        for color in 0..300u16 {
            assert!(cache.get_material("play", (color as u8, (color >> 8) as u8, 17), 14).is_some());
            assert!(cache.material_map.len() <= 256);
            assert!(cache.material_map.values().flatten().map(|image| image.data().len()).sum::<usize>() <= 4 * 1024 * 1024);
        }
        assert!(cache.prepare_material());
        assert!(Arc::ptr_eq(&bytes, &cache.material.as_ref().unwrap().0), "palette churn must not reread font bytes");
        assert!(cache.get_material("play", (1, 2, 3), 257).is_none());
        assert!(cache.get_material("apple", (255, 255, 255), 14).is_none());
    }
}
