//! Small facts for the Overview tab: who is logged in, the avatar, date names and weather wording.

use crate::menubar::weather::symbol_for;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct User {
    /// Login name.
    pub login: String,
    /// Full name from the GECOS field, or the login name.
    pub display: String,
    pub host: String,
}

impl User {
    pub fn handle(&self) -> String {
        if self.host.is_empty() {
            self.login.clone()
        } else {
            format!("{}@{}", self.login, self.host)
        }
    }

    /// First letter for the avatar tile.
    pub fn initial(&self) -> String {
        self.display.chars().chain(self.login.chars()).find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into())
    }
}

/// (login, full name) of `uid` from `/etc/passwd` text; the GECOS field's first comma-separated part is the name.
pub fn parse_passwd(text: &str, uid: u32) -> Option<(String, String)> {
    text.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 5 && f[2].parse::<u32>().ok()? == uid).then(|| (f[0].to_string(), f[4].split(',').next().unwrap_or("").trim().to_string()))
    })
}

pub fn load_user() -> User {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let (mut login, mut full) = std::fs::read_to_string("/etc/passwd").ok().and_then(|t| parse_passwd(&t, uid)).unwrap_or_default();
    if login.is_empty() {
        login = std::env::var("USER").unwrap_or_default();
    }
    if full.is_empty() {
        full = login.clone();
    }
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").or_else(|_| std::fs::read_to_string("/etc/hostname")).unwrap_or_default().trim().to_string();
    User { login, display: full, host }
}

pub fn face_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| Path::new(&h).join(".face")).filter(|p| p.is_file())
}

/// Reads an image file as a square, premultiplied pixmap of `px` x `px` (centre-cropped).
pub fn load_square(path: &Path, px: u32) -> Option<Pixmap> {
    let img = image::open(path).ok()?.to_rgba8();
    Some(square_pixmap(&img, px))
}

pub fn square_pixmap(img: &image::RgbaImage, px: u32) -> Pixmap {
    let (w, h) = img.dimensions();
    let side = w.min(h).max(1);
    let (x0, y0) = ((w - side) / 2, (h - side) / 2);
    let crop = image::imageops::crop_imm(img, x0, y0, side, side).to_image();
    let small = image::imageops::resize(&crop, px, px, image::imageops::FilterType::Triangle);
    let mut pm = Pixmap::new(px.max(1), px.max(1)).expect("pixmap");
    for (dst, src) in pm.data_mut().chunks_exact_mut(4).zip(small.pixels()) {
        let a = u32::from(src[3]);
        dst[0] = ((u32::from(src[0]) * a + 127) / 255) as u8;
        dst[1] = ((u32::from(src[1]) * a + 127) / 255) as u8;
        dst[2] = ((u32::from(src[2]) * a + 127) / 255) as u8;
        dst[3] = src[3];
    }
    pm
}

/// A stable pleasant colour per name for letter tiles.
pub fn tile_color(name: &str) -> (u8, u8, u8) {
    let mut h: u32 = 2166136261;
    for b in name.to_lowercase().bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32;
    let (s, l) = (0.55f32, 0.45f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

/// `Tuesday, 27 October` from the C-library broken-down time.
pub fn long_date(wday: u32, mday: u32, mon0: u32) -> String {
    format!("{}, {} {}", WEEKDAYS[(wday % 7) as usize], mday, MONTHS[(mon0 % 12) as usize])
}

/// Clock face text: `15:08`, or `3:08 PM` with `hour12`.
pub fn clock_face(hour: u32, min: u32, hour12: bool) -> String {
    if hour12 {
        let h = hour % 12;
        format!("{}:{:02} {}", if h == 0 { 12 } else { h }, min, if hour >= 12 { "PM" } else { "AM" })
    } else {
        format!("{hour:02}:{min:02}")
    }
}

/// Words for a weather reading, from the same condition table the bar glyphs use.
pub fn condition_label(code: u32, night: bool) -> &'static str {
    match symbol_for(code, night) {
        "weather-clear" => "Clear",
        "weather-moon" => "Clear night",
        "weather-partly" => "Partly cloudy",
        "weather-cloud" => "Cloudy",
        "weather-fog" => "Fog",
        "weather-rain" => "Rain",
        "weather-snow" => "Snow",
        "weather-storm" => "Thunderstorm",
        _ => "Cloudy",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\nada:x:1000:1000:Ada Lovelace,,,:/home/ada:/bin/zsh\nbot:x:1001:1001::/home/bot:/bin/sh\nbroken line\n";

    #[test]
    fn passwd_gives_login_and_full_name() {
        assert_eq!(parse_passwd(PASSWD, 1000), Some(("ada".into(), "Ada Lovelace".into())));
        assert_eq!(parse_passwd(PASSWD, 1001), Some(("bot".into(), "".into())));
        assert_eq!(parse_passwd(PASSWD, 4242), None);
        assert_eq!(parse_passwd("", 0), None);
    }

    #[test]
    fn user_labels() {
        let u = User { login: "ada".into(), display: "Ada Lovelace".into(), host: "babbage".into() };
        assert_eq!((u.handle().as_str(), u.initial().as_str()), ("ada@babbage", "A"));
        let anon = User { login: "".into(), display: "".into(), host: "".into() };
        assert_eq!((anon.handle().as_str(), anon.initial().as_str()), ("", "?"));
        let lower = User { login: "zed".into(), display: "zed".into(), host: "h".into() };
        assert_eq!(lower.initial(), "Z");
    }

    #[test]
    fn dates_and_clock() {
        assert_eq!(long_date(2, 27, 9), "Tuesday, 27 October");
        assert_eq!(long_date(9, 1, 14), "Tuesday, 1 March", "out of range does not panic");
        assert_eq!(clock_face(15, 8, false), "15:08");
        assert_eq!(clock_face(15, 8, true), "3:08 PM");
        assert_eq!(clock_face(0, 5, true), "12:05 AM");
        assert_eq!(clock_face(12, 0, true), "12:00 PM");
    }

    #[test]
    fn weather_words() {
        assert_eq!(condition_label(113, false), "Clear");
        assert_eq!(condition_label(113, true), "Clear night");
        assert_eq!(condition_label(116, false), "Partly cloudy");
        assert_eq!(condition_label(296, false), "Rain");
        assert_eq!(condition_label(338, false), "Snow");
        assert_eq!(condition_label(389, false), "Thunderstorm");
        assert_eq!(condition_label(143, false), "Fog");
        assert_eq!(condition_label(9999, false), "Cloudy");
    }

    #[test]
    fn square_crop_premultiplies() {
        let mut img = image::RgbaImage::new(40, 20);
        for p in img.pixels_mut() {
            *p = image::Rgba([200, 100, 50, 128]);
        }
        let pm = square_pixmap(&img, 10);
        assert_eq!((pm.width(), pm.height()), (10, 10));
        let p = pm.pixel(5, 5).unwrap();
        assert_eq!((p.alpha(), p.red()), (128, 100), "premultiplied: 200 * 128/255");
        assert!(load_square(Path::new("/nonexistent.png"), 8).is_none());
    }

    #[test]
    fn tile_colours_are_stable_and_differ() {
        assert_eq!(tile_color("Spotify"), tile_color("spotify"));
        assert_ne!(tile_color("spotify"), tile_color("firefox"));
    }
}
