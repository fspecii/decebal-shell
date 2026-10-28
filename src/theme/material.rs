//! Material-You-style colour: tonal palettes, scheme variants and the dominant colour of a wallpaper.
//!
//! Material's HCT is CAM16 hue/chroma with CIE L* as the tone. This module keeps the part that
//! matters for contrast (the tone is exactly CIE L*, so tone 80 against tone 20 is always the same
//! contrast) and takes hue and chroma from OKLCH, which is a few lines instead of a CAM16 model.
//! Chroma values use the HCT scale (tonal spot: primary 36, secondary 16 ...); `CHROMA_SCALE` converts
//! them (calibrated against caelestia's default scheme, whose key colours sit at OKLCH chroma 0.095
//! for HCT 36). A colour that does not fit the sRGB gamut at its tone loses chroma, never tone.
//!
//! Everything is deterministic: the same seed, variant and mode always give the same colours.
//!
//! * [`Roles`]: the Material colour roles of one scheme (primary, surface containers, outline ...).
//! * [`generate`]: seed + [`Variant`] + dark/light -> [`Roles`].
//! * [`CAELESTIA_DARK`]: caelestia-shell's shipped default scheme (`services/Colours.qml`).
//! * [`Roles::tokens`]: maps the roles onto macshell's theme colour tokens.
//! * [`seed_from_image`]: dominant colour of a wallpaper (Material's "source colour").

use super::model::{Rgb, Tokens, V};

/// OKLCH chroma per unit of HCT chroma (calibrated on caelestia's default scheme).
pub const CHROMA_SCALE: f64 = 0.00262;
/// The seed of caelestia's default scheme (its `m3primary_paletteKeyColor`).
pub const DEFAULT_SEED: Rgb = Rgb(0xa8, 0x62, 0x7b);

// ---------------------------------------------------------------- colour science

fn to_linear(v: u8) -> f64 {
    let c = f64::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(l: f64) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// (L, C, h in degrees) of an sRGB colour in OKLCH.
pub fn oklch(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (to_linear(c.0), to_linear(c.1), to_linear(c.2));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_770_5 * b).cbrt();
    let big_l = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
    let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    (big_l, a.hypot(bb), bb.atan2(a).to_degrees().rem_euclid(360.0))
}

/// Linear sRGB (unclamped) of an OKLCH colour.
fn oklch_linear(big_l: f64, c: f64, h: f64) -> (f64, f64, f64) {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l = (big_l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (big_l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (big_l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    (
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    )
}

fn luminance_of(rgb: (f64, f64, f64)) -> f64 {
    0.2126 * rgb.0 + 0.7152 * rgb.1 + 0.0722 * rgb.2
}

/// CIE L* of a colour (Material's tone).
pub fn tone_of(c: Rgb) -> f64 {
    let y = luminance_of((to_linear(c.0), to_linear(c.1), to_linear(c.2)));
    if y <= 216.0 / 24389.0 {
        y * 24389.0 / 27.0
    } else {
        116.0 * y.cbrt() - 16.0
    }
}

fn tone_to_y(t: f64) -> f64 {
    if t > 8.0 {
        ((t + 16.0) / 116.0).powi(3)
    } else {
        t * 27.0 / 24389.0
    }
}

fn in_gamut(c: (f64, f64, f64)) -> bool {
    let ok = |v: f64| (-0.0005..=1.0005).contains(&v);
    ok(c.0) && ok(c.1) && ok(c.2)
}

/// The OKLab lightness at which (`c`, `h`) has luminance `y` (luminance rises with lightness).
fn lightness_for(y: f64, c: f64, h: f64) -> f64 {
    let (mut lo, mut hi) = (0.0, 1.2);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if luminance_of(oklch_linear(mid, c, h)) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// The colour with exactly CIE L* `tone` and hue `h` (degrees) whose OKLCH chroma is `chroma` or,
/// when the sRGB gamut does not reach that far at this tone, the largest chroma that fits.
pub fn solve(h: f64, chroma: f64, tone: f64) -> Rgb {
    if tone <= 0.0 {
        return Rgb(0, 0, 0);
    }
    if tone >= 100.0 {
        return Rgb(255, 255, 255);
    }
    let y = tone_to_y(tone);
    let fits = |c: f64| {
        let l = lightness_for(y, c, h);
        let rgb = oklch_linear(l, c, h);
        in_gamut(rgb).then_some(rgb)
    };
    let rgb = match fits(chroma) {
        Some(rgb) => rgb,
        None => {
            let (mut lo, mut hi) = (0.0, chroma);
            for _ in 0..24 {
                let mid = (lo + hi) / 2.0;
                if fits(mid).is_some() {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            fits(lo).unwrap_or_else(|| oklch_linear(lightness_for(y, 0.0, h), 0.0, h))
        }
    };
    Rgb(from_linear(rgb.0), from_linear(rgb.1), from_linear(rgb.2))
}

// ---------------------------------------------------------------- tonal palettes and variants

/// A hue and a chroma (HCT scale); [`Tonal::at`] reads a tone off it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tonal {
    pub hue: f64,
    pub chroma: f64,
}

impl Tonal {
    pub fn new(hue: f64, chroma: f64) -> Tonal {
        Tonal { hue: hue.rem_euclid(360.0), chroma }
    }
    pub fn at(self, tone: f64) -> Rgb {
        solve(self.hue, self.chroma * CHROMA_SCALE, tone)
    }
}

/// The colour schemes ("variants") macshell can generate: Material You's tonal-spot, vibrant, expressive,
/// monochrome, neutral and fidelity ("content") schemes, plus a pywal-like `Palette` that keeps the wallpaper's
/// own distinct colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    /// Material's default: a pastel palette with low chroma.
    #[default]
    TonalSpot,
    /// A high chroma palette; secondary and tertiary hues rotate away from the seed.
    Vibrant,
    /// The accent hue sits far from the seed (a +240 degree turn) with strongly rotated companions.
    Expressive,
    /// No colour at all: greys only.
    Monochrome,
    /// Close to greyscale with a hint of the seed hue.
    Neutral,
    /// Keeps the seed's own chroma (Material's "fidelity").
    Content,
    /// pywal / wallust style: the accent, secondary and tertiary colours are the wallpaper's own most distinct colours.
    Palette,
}

impl Variant {
    pub const ALL: [Variant; 7] = [Variant::TonalSpot, Variant::Vibrant, Variant::Expressive, Variant::Monochrome, Variant::Neutral, Variant::Content, Variant::Palette];

    pub fn parse(s: &str) -> Option<Variant> {
        match s.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "tonalspot" | "tonal" | "default" => Some(Variant::TonalSpot),
            "vibrant" => Some(Variant::Vibrant),
            "expressive" => Some(Variant::Expressive),
            "monochrome" | "mono" | "grey" | "gray" => Some(Variant::Monochrome),
            "neutral" => Some(Variant::Neutral),
            "content" | "fidelity" => Some(Variant::Content),
            "palette" | "pywal" | "wal" | "wallust" => Some(Variant::Palette),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Variant::TonalSpot => "tonal-spot",
            Variant::Vibrant => "vibrant",
            Variant::Expressive => "expressive",
            Variant::Monochrome => "monochrome",
            Variant::Neutral => "neutral",
            Variant::Content => "content",
            Variant::Palette => "palette",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Variant::TonalSpot => "Tonal spot",
            Variant::Vibrant => "Vibrant",
            Variant::Expressive => "Expressive",
            Variant::Monochrome => "Monochrome",
            Variant::Neutral => "Neutral",
            Variant::Content => "Content",
            Variant::Palette => "Palette",
        }
    }
    /// One line for the settings pane.
    pub fn blurb(self) -> &'static str {
        match self {
            Variant::TonalSpot => "Material's default: a soft pastel accent from the wallpaper's main colour",
            Variant::Vibrant => "A saturated accent with companions turned away from it",
            Variant::Expressive => "The accent hue is turned away from the wallpaper's: playful",
            Variant::Monochrome => "Greys only, no colour",
            Variant::Neutral => "Nearly grey with a hint of the wallpaper",
            Variant::Content => "Keeps the wallpaper's own saturation",
            Variant::Palette => "Like pywal: accents are the wallpaper's own distinct colours",
        }
    }
}

/// Hue rotation of Material's vibrant scheme: the rotation of the interval the hue falls in.
fn rotated(hue: f64, table: &[(f64, f64)]) -> f64 {
    for w in table.windows(2) {
        if (w[0].0..w[1].0).contains(&hue) {
            return (hue + w[0].1).rem_euclid(360.0);
        }
    }
    hue
}

/// Vibrant's rotation tables (hue, degrees): material-color-utilities `SchemeVibrant`.
const VIBRANT_SECONDARY: [(f64, f64); 9] = [(0.0, 18.0), (41.0, 15.0), (61.0, 10.0), (101.0, 12.0), (131.0, 15.0), (181.0, 18.0), (251.0, 15.0), (301.0, 12.0), (361.0, 20.0)];
const VIBRANT_TERTIARY: [(f64, f64); 9] = [(0.0, 35.0), (41.0, 30.0), (61.0, 20.0), (101.0, 25.0), (131.0, 30.0), (181.0, 35.0), (251.0, 30.0), (301.0, 25.0), (361.0, 40.0)];

/// The five key palettes (plus error and success) of a variant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palettes {
    pub primary: Tonal,
    pub secondary: Tonal,
    pub tertiary: Tonal,
    pub neutral: Tonal,
    pub neutral_variant: Tonal,
    pub error: Tonal,
    pub success: Tonal,
}

/// Expressive's rotation tables: `SchemeExpressive`.
const EXPRESSIVE_SECONDARY: [(f64, f64); 9] = [(0.0, 45.0), (21.0, 95.0), (51.0, 45.0), (121.0, 20.0), (151.0, 45.0), (191.0, 90.0), (271.0, 45.0), (321.0, 45.0), (361.0, 45.0)];
const EXPRESSIVE_TERTIARY: [(f64, f64); 9] = [(0.0, 120.0), (21.0, 120.0), (51.0, 20.0), (121.0, 45.0), (151.0, 20.0), (191.0, 15.0), (271.0, 20.0), (321.0, 120.0), (361.0, 120.0)];

/// [`palettes_with`] without further wallpaper colours.
pub fn palettes(seed: Rgb, variant: Variant) -> Palettes {
    palettes_with(seed, &[], variant)
}

/// The palettes of `variant`. `others` are further distinct colours of the wallpaper (most important first);
/// only [`Variant::Palette`] reads them.
pub fn palettes_with(seed: Rgb, others: &[Rgb], variant: Variant) -> Palettes {
    let (_, c, h) = oklch(seed);
    let seed_chroma = c / CHROMA_SCALE;
    let (primary, secondary, tertiary, neutral, neutral_variant) = match variant {
        Variant::TonalSpot => (Tonal::new(h, 36.0), Tonal::new(h, 16.0), Tonal::new(h + 60.0, 24.0), Tonal::new(h, 6.0), Tonal::new(h, 8.0)),
        Variant::Vibrant => (Tonal::new(h, 200.0), Tonal::new(rotated(h, &VIBRANT_SECONDARY), 24.0), Tonal::new(rotated(h, &VIBRANT_TERTIARY), 32.0), Tonal::new(h, 10.0), Tonal::new(h, 12.0)),
        Variant::Expressive => (
            Tonal::new(h + 240.0, 40.0),
            Tonal::new(rotated(h, &EXPRESSIVE_SECONDARY), 24.0),
            Tonal::new(rotated(h, &EXPRESSIVE_TERTIARY), 32.0),
            Tonal::new(h + 15.0, 8.0),
            Tonal::new(h + 15.0, 12.0),
        ),
        Variant::Monochrome => (Tonal::new(h, 0.0), Tonal::new(h, 0.0), Tonal::new(h, 0.0), Tonal::new(h, 0.0), Tonal::new(h, 0.0)),
        Variant::Neutral => (Tonal::new(h, 12.0), Tonal::new(h, 8.0), Tonal::new(h, 16.0), Tonal::new(h, 2.0), Tonal::new(h, 2.0)),
        // the seed's own chroma, with a floor so a grey seed still gives a usable accent
        Variant::Content => {
            let c = seed_chroma.max(10.0);
            (Tonal::new(h, c), Tonal::new(h, (c - 32.0).max(c * 0.5)), Tonal::new(h + 60.0, c * 0.75), Tonal::new(h, c / 8.0), Tonal::new(h, c / 8.0 + 4.0))
        }
        // the wallpaper's own colours: each companion keeps its hue and a bounded chroma
        Variant::Palette => {
            let own = |c: Option<&Rgb>, fallback_turn: f64| match c {
                Some(c) => {
                    let (_, cc, hh) = oklch(*c);
                    Tonal::new(hh, (cc / CHROMA_SCALE).clamp(20.0, 70.0))
                }
                None => Tonal::new(h + fallback_turn, 28.0),
            };
            (Tonal::new(h, seed_chroma.clamp(32.0, 90.0)), own(others.first(), 50.0), own(others.get(1), 110.0), Tonal::new(h, 5.0), Tonal::new(h, 8.0))
        }
    };
    Palettes { primary, secondary, tertiary, neutral, neutral_variant, error: Tonal::new(25.0, 84.0), success: Tonal::new(152.0, 13.4) }
}

// ---------------------------------------------------------------- roles

/// The Material colour roles of one scheme (names as in caelestia's `m3*` palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    pub dark: bool,
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub primary_container: Rgb,
    pub on_primary_container: Rgb,
    pub secondary: Rgb,
    pub on_secondary: Rgb,
    pub secondary_container: Rgb,
    pub on_secondary_container: Rgb,
    pub tertiary: Rgb,
    pub on_tertiary: Rgb,
    pub tertiary_container: Rgb,
    pub on_tertiary_container: Rgb,
    pub error: Rgb,
    pub on_error: Rgb,
    pub error_container: Rgb,
    pub on_error_container: Rgb,
    pub success: Rgb,
    pub on_success: Rgb,
    pub success_container: Rgb,
    pub on_success_container: Rgb,
    pub surface: Rgb,
    pub surface_container_lowest: Rgb,
    pub surface_container_low: Rgb,
    pub surface_container: Rgb,
    pub surface_container_high: Rgb,
    pub surface_container_highest: Rgb,
    pub on_surface: Rgb,
    pub on_surface_variant: Rgb,
    pub outline: Rgb,
    pub outline_variant: Rgb,
}

impl Roles {
    /// (foreground, background) pairs that must stay readable: every `on*` role over its own container.
    pub fn text_pairs(&self) -> Vec<(&'static str, Rgb, Rgb)> {
        vec![
            ("on_primary/primary", self.on_primary, self.primary),
            ("on_primary_container/primary_container", self.on_primary_container, self.primary_container),
            ("on_secondary/secondary", self.on_secondary, self.secondary),
            ("on_secondary_container/secondary_container", self.on_secondary_container, self.secondary_container),
            ("on_tertiary/tertiary", self.on_tertiary, self.tertiary),
            ("on_tertiary_container/tertiary_container", self.on_tertiary_container, self.tertiary_container),
            ("on_error/error", self.on_error, self.error),
            ("on_error_container/error_container", self.on_error_container, self.error_container),
            ("on_success/success", self.on_success, self.success),
            ("on_success_container/success_container", self.on_success_container, self.success_container),
            ("on_surface/surface", self.on_surface, self.surface),
            ("on_surface/container_lowest", self.on_surface, self.surface_container_lowest),
            ("on_surface/container_low", self.on_surface, self.surface_container_low),
            ("on_surface/container", self.on_surface, self.surface_container),
            ("on_surface/container_high", self.on_surface, self.surface_container_high),
            ("on_surface/container_highest", self.on_surface, self.surface_container_highest),
            ("on_surface_variant/surface", self.on_surface_variant, self.surface),
            ("on_surface_variant/container_highest", self.on_surface_variant, self.surface_container_highest),
        ]
    }

    /// The roles as macshell theme colour tokens. This is the one place the mapping lives:
    /// background = surface, cards = surface container, floating things (menus, Spotlight, the
    /// bar pill and the dock share the container tone) one step up, text = on-surface, accent = primary.
    pub fn tokens(&self) -> Tokens {
        let mut t = Tokens::new();
        for (k, c) in [
            ("background", self.surface),
            ("sidebar", self.surface_container_low),
            ("surface", self.surface_container),
            ("bar", self.surface_container),
            ("bar_text", self.on_surface),
            ("popup", self.surface_container_high),
            ("dock", self.surface_container),
            ("overlay", self.surface_container_high),
            ("titlebar", self.surface_container_high),
            ("titlebar_inactive", self.surface_container_low),
            ("border", self.outline_variant),
            ("text", self.on_surface),
            ("text_secondary", self.on_surface_variant),
            ("text_faint", self.outline),
            ("accent", self.primary),
            ("accent_text", self.on_primary),
            ("danger", self.error),
            ("ok", self.success),
            ("warn", self.tertiary),
        ] {
            t.insert(k.into(), V::S(c.hex()));
        }
        t
    }
}

/// Builds the roles of `seed` for `variant` with Material's tone mapping for dark or light schemes.
pub fn generate(seed: Rgb, variant: Variant, dark: bool) -> Roles {
    generate_with(seed, &[], variant, dark, 0.0)
}

/// Material's tone mapping at a contrast level: `contrast` runs from -1 (softer) over 0 (Material's standard)
/// to 1 (high contrast: text and accent tones move away from their surfaces). Every `on*` role keeps 4.5:1.
/// Order: accent, on-accent, container, on-container, surface, lowest, low, container, high, highest,
/// on-surface, on-surface-variant, outline, outline-variant.
fn tones(dark: bool, contrast: f64) -> [f64; 14] {
    let c = contrast.clamp(-1.0, 1.0);
    // the soft side moves less: the roles must stay readable
    let k = if c >= 0.0 { c } else { c * 0.4 };
    // a surface tone moves towards the far end of the scale as contrast rises
    let toward = |t: f64, end: f64, amount: f64| t + (end - t) * amount * k;
    if dark {
        [
            (80.0 + 12.0 * k).min(95.0),
            20.0 - 14.0 * k,
            30.0 - 10.0 * k,
            90.0 + 8.0 * k,
            toward(6.0, 0.0, 0.5),
            toward(4.0, 0.0, 0.5),
            toward(10.0, 0.0, 0.4),
            toward(12.0, 0.0, 0.4),
            toward(17.0, 0.0, 0.35),
            toward(22.0, 0.0, 0.35),
            90.0 + 10.0 * k,
            80.0 + 16.0 * k,
            60.0 + 20.0 * k,
            30.0 + 10.0 * k,
        ]
    } else {
        [
            40.0 - 14.0 * k,
            100.0,
            90.0 + 5.0 * k,
            10.0 - 8.0 * k,
            toward(98.0, 100.0, 0.4),
            100.0,
            toward(96.0, 100.0, 0.4),
            toward(94.0, 100.0, 0.4),
            toward(92.0, 100.0, 0.4),
            toward(90.0, 100.0, 0.4),
            10.0 - 8.0 * k,
            30.0 - 18.0 * k,
            50.0 - 20.0 * k,
            80.0 + 8.0 * k,
        ]
    }
}

/// [`generate`] with the wallpaper's further colours (for [`Variant::Palette`]) and a contrast level (-1..1).
pub fn generate_with(seed: Rgb, others: &[Rgb], variant: Variant, dark: bool, contrast: f64) -> Roles {
    let p = palettes_with(seed, others, variant);
    let [a, on_a, c, on_c, surf, lowest, low, mid, high, highest, on_surf, on_var, outline, outline_var] = tones(dark, contrast);
    Roles {
        dark,
        primary: p.primary.at(a),
        on_primary: p.primary.at(on_a),
        primary_container: p.primary.at(c),
        on_primary_container: p.primary.at(on_c),
        secondary: p.secondary.at(a),
        on_secondary: p.secondary.at(on_a),
        secondary_container: p.secondary.at(c),
        on_secondary_container: p.secondary.at(on_c),
        tertiary: p.tertiary.at(a),
        on_tertiary: p.tertiary.at(on_a),
        tertiary_container: p.tertiary.at(c),
        on_tertiary_container: p.tertiary.at(on_c),
        error: p.error.at(a),
        on_error: p.error.at(on_a),
        error_container: p.error.at(c),
        on_error_container: p.error.at(on_c),
        success: p.success.at(a),
        on_success: p.success.at(on_a),
        success_container: p.success.at(c),
        on_success_container: p.success.at(on_c),
        surface: p.neutral.at(surf),
        surface_container_lowest: p.neutral.at(lowest),
        surface_container_low: p.neutral.at(low),
        surface_container: p.neutral.at(mid),
        surface_container_high: p.neutral.at(high),
        surface_container_highest: p.neutral.at(highest),
        on_surface: p.neutral.at(on_surf),
        on_surface_variant: p.neutral_variant.at(on_var),
        outline: p.neutral_variant.at(outline),
        outline_variant: p.neutral_variant.at(outline_var),
    }
}

/// caelestia-shell's shipped default scheme (dark, rose), straight from `services/Colours.qml`.
pub const CAELESTIA_DARK: Roles = {
    const fn c(r: u8, g: u8, b: u8) -> Rgb {
        Rgb(r, g, b)
    }
    Roles {
        dark: true,
        primary: c(0xff, 0xb0, 0xca),
        on_primary: c(0x54, 0x1d, 0x34),
        primary_container: c(0x6f, 0x33, 0x4a),
        on_primary_container: c(0xff, 0xd9, 0xe3),
        secondary: c(0xe2, 0xbd, 0xc7),
        on_secondary: c(0x42, 0x29, 0x32),
        secondary_container: c(0x5a, 0x3f, 0x48),
        on_secondary_container: c(0xff, 0xd9, 0xe3),
        tertiary: c(0xf0, 0xbc, 0x95),
        on_tertiary: c(0x48, 0x29, 0x0c),
        tertiary_container: c(0xb5, 0x87, 0x63),
        on_tertiary_container: c(0x00, 0x00, 0x00),
        error: c(0xff, 0xb4, 0xab),
        on_error: c(0x69, 0x00, 0x05),
        error_container: c(0x93, 0x00, 0x0a),
        on_error_container: c(0xff, 0xda, 0xd6),
        success: c(0xb5, 0xcc, 0xba),
        on_success: c(0x21, 0x35, 0x28),
        success_container: c(0x37, 0x4b, 0x3e),
        on_success_container: c(0xd1, 0xe9, 0xd6),
        surface: c(0x19, 0x11, 0x14),
        surface_container_lowest: c(0x13, 0x0c, 0x0e),
        surface_container_low: c(0x22, 0x19, 0x1c),
        surface_container: c(0x26, 0x1d, 0x20),
        surface_container_high: c(0x31, 0x28, 0x2a),
        surface_container_highest: c(0x3c, 0x32, 0x35),
        on_surface: c(0xef, 0xdf, 0xe2),
        on_surface_variant: c(0xd5, 0xc2, 0xc6),
        outline: c(0x9e, 0x8c, 0x91),
        outline_variant: c(0x51, 0x43, 0x47),
    }
};

// ---------------------------------------------------------------- the seed of a wallpaper

/// Hue-bucket scoring like Material's `Score.score`: colourful pixels vote for their hue (smoothed over
/// +-15 degrees); a bin scores by the popularity of its hue and by its own chroma. Returns the best bin.
/// `pixels` are sRGB triples. A wallpaper with no colour at all gives its average (or [`DEFAULT_SEED`]).
pub fn seed_from_pixels(pixels: &[[u8; 3]]) -> Rgb {
    seed_scored(pixels).0
}

/// [`seed_from_pixels`] plus whether the image is colourless (no colour worth matching; the seed is then only a fallback).
pub fn seed_scored(pixels: &[[u8; 3]]) -> (Rgb, bool) {
    if pixels.is_empty() {
        return (DEFAULT_SEED, true);
    }
    // 4 bits per channel; each bin remembers the sum of its pixels so the colour is their average
    let mut bins: Vec<(u64, [u64; 3])> = vec![(0, [0; 3]); 4096];
    let mut total = [0u64; 3];
    for p in pixels {
        let i = ((p[0] >> 4) as usize) << 8 | ((p[1] >> 4) as usize) << 4 | (p[2] >> 4) as usize;
        bins[i].0 += 1;
        for k in 0..3 {
            bins[i].1[k] += u64::from(p[k]);
            total[k] += u64::from(p[k]);
        }
    }
    let n = pixels.len() as u64;
    let mean = Rgb((total[0] / n) as u8, (total[1] / n) as u8, (total[2] / n) as u8);
    struct Cand {
        rgb: Rgb,
        count: u64,
        chroma: f64,
        hue: f64,
    }
    let mut cands = Vec::new();
    for (count, sum) in bins.iter().filter(|b| b.0 > 0) {
        let rgb = Rgb((sum[0] / count) as u8, (sum[1] / count) as u8, (sum[2] / count) as u8);
        let (l, c, h) = oklch(rgb);
        let chroma = c / CHROMA_SCALE;
        if chroma >= 5.0 && (0.08..=0.985).contains(&l) {
            cands.push(Cand { rgb, count: *count, chroma, hue: h });
        }
    }
    let colourful: u64 = cands.iter().map(|c| c.count).sum();
    if cands.is_empty() || colourful * 100 < n {
        // (almost) grey: keep a hint of the average colour's hue, else the default
        return (if oklch(mean).1 / CHROMA_SCALE >= 1.0 { mean } else { DEFAULT_SEED }, true);
    }
    let mut hue_pop = [0f64; 360];
    for c in &cands {
        hue_pop[(c.hue.floor() as usize) % 360] += c.count as f64 / colourful as f64;
    }
    let mut best: Option<(f64, Rgb)> = None;
    for c in &cands {
        let h = (c.hue.floor() as i64).rem_euclid(360);
        let excited: f64 = (-14..=14).map(|d| hue_pop[(h + d).rem_euclid(360) as usize]).sum();
        let score = excited * 100.0 * 0.7 + (c.chroma - 48.0) * if c.chroma < 48.0 { 0.1 } else { 0.3 };
        // ties resolve towards the smaller colour value so the result never depends on bin order
        let better = match best {
            None => true,
            Some((s, rgb)) => score > s || (score == s && (c.rgb.0, c.rgb.1, c.rgb.2) < (rgb.0, rgb.1, rgb.2)),
        };
        if better {
            best = Some((score, c.rgb));
        }
    }
    (best.map(|b| b.1).unwrap_or(DEFAULT_SEED), false)
}

// ---------------------------------------------------------------- tinting an existing colour

/// The colour at OKLab lightness `l` and hue `h` whose chroma is `c`, or the largest chroma that fits sRGB.
pub fn fit_oklch(l: f64, c: f64, h: f64) -> Rgb {
    let to_rgb = |rgb: (f64, f64, f64)| Rgb(from_linear(rgb.0), from_linear(rgb.1), from_linear(rgb.2));
    let l = l.clamp(0.0, 1.0);
    let full = oklch_linear(l, c, h);
    if in_gamut(full) {
        return to_rgb(full);
    }
    let (mut lo, mut hi) = (0.0, c);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(oklch_linear(l, mid, h)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    to_rgb(oklch_linear(l, lo, h))
}

/// Smallest signed turn from hue `a` to hue `b` (degrees, -180..180).
pub fn hue_turn(a: f64, b: f64) -> f64 {
    let d = (b - a).rem_euclid(360.0);
    if d > 180.0 {
        d - 360.0
    } else {
        d
    }
}

/// `c` with its hue pulled towards `hue` and its chroma towards `goal` (OKLCH chroma), both by `amount` (0..1);
/// the lightness is untouched, so a tinted surface keeps its place on the light/dark scale and its contrast.
pub fn retint(c: Rgb, hue: f64, goal: f64, amount: f64) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    if amount == 0.0 {
        return c; // no round trip through OKLCH: the colour stays byte for byte
    }
    let (l, ch, h) = oklch(c);
    // a colour with no hue of its own (a grey) takes the target hue outright
    let h2 = if ch < 0.004 { hue } else { h + hue_turn(h, hue) * amount };
    fit_oklch(l, ch * (1.0 - amount) + goal * amount, h2.rem_euclid(360.0))
}

/// The seed colour of an image file (PNG, JPEG, WebP), looking at a 96 px thumbnail.
pub fn seed_from_image(path: &std::path::Path) -> Result<Rgb, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let small = img.thumbnail(96, 96).to_rgb8();
    let px: Vec<[u8; 3]> = small.pixels().map(|p| p.0).collect();
    Ok(seed_from_pixels(&px))
}

#[cfg(test)]
mod tests {
    use super::super::model::contrast;
    use super::*;

    #[test]
    fn tone_is_exact_cie_lstar() {
        for h in [0.0, 60.0, 130.0, 200.0, 280.0, 340.0] {
            for t in [10.0, 30.0, 50.0, 80.0, 90.0, 98.0] {
                let c = solve(h, 36.0 * CHROMA_SCALE, t);
                assert!((tone_of(c) - t).abs() < 1.2, "hue {h} tone {t}: got {} ({})", tone_of(c), c.hex());
            }
        }
        assert_eq!(solve(10.0, 0.1, 0.0), Rgb(0, 0, 0));
        assert_eq!(solve(10.0, 0.1, 100.0), Rgb(255, 255, 255));
    }

    #[test]
    fn oklch_round_trips() {
        for c in [Rgb(255, 176, 202), Rgb(25, 17, 20), Rgb(0, 128, 255), Rgb(240, 240, 10)] {
            let (l, ch, h) = oklch(c);
            let back = oklch_linear(l, ch, h);
            assert_eq!((from_linear(back.0), from_linear(back.1), from_linear(back.2)), (c.0, c.1, c.2));
        }
    }

    #[test]
    fn caelestia_default_dark_scheme_is_reproduced_closely() {
        // Our generator from caelestia's seed lands close to the shipped palette (HCT vs OKLCH hue/chroma
        // differ slightly): every role within a small RGB distance.
        let g = generate(DEFAULT_SEED, Variant::TonalSpot, true);
        let want = CAELESTIA_DARK;
        let dist = |a: Rgb, b: Rgb| (i32::from(a.0) - i32::from(b.0)).abs().max((i32::from(a.1) - i32::from(b.1)).abs()).max((i32::from(a.2) - i32::from(b.2)).abs());
        for (name, a, b) in [
            ("primary", g.primary, want.primary),
            ("on_primary", g.on_primary, want.on_primary),
            ("primary_container", g.primary_container, want.primary_container),
            ("on_primary_container", g.on_primary_container, want.on_primary_container),
            ("secondary", g.secondary, want.secondary),
            ("secondary_container", g.secondary_container, want.secondary_container),
            ("tertiary", g.tertiary, want.tertiary),
            ("error", g.error, want.error),
            ("success", g.success, want.success),
            ("surface", g.surface, want.surface),
            ("surface_container", g.surface_container, want.surface_container),
            ("surface_container_high", g.surface_container_high, want.surface_container_high),
            ("on_surface", g.on_surface, want.on_surface),
            ("on_surface_variant", g.on_surface_variant, want.on_surface_variant),
            ("outline", g.outline, want.outline),
            ("outline_variant", g.outline_variant, want.outline_variant),
        ] {
            assert!(dist(a, b) <= 14, "{name}: generated {} vs caelestia {} (distance {})", a.hex(), b.hex(), dist(a, b));
        }
    }

    #[test]
    fn generation_is_deterministic() {
        for v in Variant::ALL {
            assert_eq!(generate(Rgb(30, 120, 200), v, true), generate(Rgb(30, 120, 200), v, true));
            assert_ne!(generate(Rgb(30, 120, 200), v, true), generate(Rgb(30, 120, 200), v, false));
        }
        assert_ne!(generate(Rgb(30, 120, 200), Variant::Vibrant, true).primary, generate(Rgb(30, 120, 200), Variant::Neutral, true).primary);
    }

    #[test]
    fn variants_differ_in_colourfulness() {
        let c = |v| oklch(generate(Rgb(200, 60, 90), v, true).primary_container).1;
        assert!(c(Variant::Vibrant) > c(Variant::TonalSpot), "vibrant is more colourful than tonal spot");
        assert!(c(Variant::TonalSpot) > c(Variant::Neutral), "neutral is the greyest");
        let n = oklch(generate(Rgb(200, 60, 90), Variant::Neutral, true).surface).1;
        assert!(n < 0.01);
    }

    #[test]
    fn every_on_role_reads_on_its_container() {
        // a spread of seeds including greys, pure primaries and the awkward yellows/greens
        let seeds = [
            Rgb(168, 98, 123),
            Rgb(255, 0, 0),
            Rgb(0, 255, 0),
            Rgb(0, 0, 255),
            Rgb(255, 255, 0),
            Rgb(0, 255, 255),
            Rgb(128, 128, 128),
            Rgb(10, 10, 10),
            Rgb(250, 250, 250),
            Rgb(120, 160, 40),
            Rgb(70, 30, 140),
            Rgb(240, 130, 20),
        ];
        for seed in seeds {
            for v in Variant::ALL {
                for dark in [true, false] {
                    let r = generate(seed, v, dark);
                    for (what, fg, bg) in r.text_pairs() {
                        let ratio = contrast(fg, bg);
                        assert!(ratio >= 4.5, "{} {} {}: {what} is {ratio:.2}:1 ({} on {})", seed.hex(), v.as_str(), if dark { "dark" } else { "light" }, fg.hex(), bg.hex());
                    }
                }
            }
        }
    }

    #[test]
    fn the_shipped_default_scheme_has_readable_roles_too() {
        for (what, fg, bg) in CAELESTIA_DARK.text_pairs() {
            // caelestia's tertiary container is paired with pure black, which is fine
            assert!(contrast(fg, bg) >= 4.5, "{what}: {:.2}", contrast(fg, bg));
        }
    }

    #[test]
    fn dominant_colour_of_pixels() {
        // mostly blue sky with a little red: blue wins; grey images fall back
        let mut px = vec![[40u8, 90, 200]; 800];
        px.extend(vec![[200u8, 30, 30]; 60]);
        px.extend(vec![[128u8, 128, 128]; 140]);
        let s = seed_from_pixels(&px);
        let (_, _, h) = oklch(s);
        assert!((230.0..280.0).contains(&h), "got {} hue {h}", s.hex());
        assert_eq!(seed_from_pixels(&[[128, 128, 128]; 100]), DEFAULT_SEED);
        assert_eq!(seed_from_pixels(&[]), DEFAULT_SEED);
        // order of the pixels does not matter
        let mut rev = px.clone();
        rev.reverse();
        assert_eq!(seed_from_pixels(&rev), s);
        // a big flat colour field is the seed itself
        assert_eq!(seed_from_pixels(&[[10, 200, 60]; 50]), Rgb(10, 200, 60));
    }

    #[test]
    fn seed_from_an_image_file() {
        let dir = std::env::temp_dir().join(format!("macshell-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("w.png");
        image::RgbImage::from_fn(200, 120, |x, _| if x < 150 { image::Rgb([220, 120, 20]) } else { image::Rgb([20, 20, 30]) }).save(&p).unwrap();
        let s = seed_from_image(&p).unwrap();
        assert!((40.0..90.0).contains(&oklch(s).2), "orange hue, got {}", s.hex());
        assert!(seed_from_image(&dir.join("nope.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tokens_cover_the_theme_colours() {
        let t = CAELESTIA_DARK.tokens();
        for k in ["background", "surface", "bar", "popup", "dock", "text", "accent", "accent_text", "danger", "ok", "warn", "border", "overlay"] {
            assert!(t.contains_key(k), "{k}");
        }
        assert_eq!(t["accent"], V::S("#ffb0ca".into()));
        assert_eq!(t["background"], V::S("#191114".into()));
    }

    #[test]
    fn variant_names_parse() {
        assert_eq!(Variant::parse("Tonal Spot"), Some(Variant::TonalSpot));
        assert_eq!(Variant::parse("tonal-spot"), Some(Variant::TonalSpot));
        assert_eq!(Variant::parse("VIBRANT"), Some(Variant::Vibrant));
        assert_eq!(Variant::parse("fidelity"), Some(Variant::Content));
        assert_eq!(Variant::parse("nope"), None);
        for v in Variant::ALL {
            assert_eq!(Variant::parse(v.as_str()), Some(v));
        }
    }
}
