//! Native DCommon DClockFace variable digits. Preparation runs on a worker;
//! painting only composites a bounded, already rasterized frame.
use crate::text::{Fonts, Weight};
use crate::textface::{self, Glyph};
use crate::theme::{fontdb::FaceRef, fonts::FontPlan};
use std::sync::Arc;
mod worker;
use tiny_skia::{Color, Pixmap};
pub use worker::AsyncClock;

const DIGITS: &str = "0123456789:";
const BITMAP_BUDGET: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub width: f32,
    pub height: f32,
    pub hours: String,
    pub minutes: String,
    pub date: String,
    pub scale: f32,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Metrics {
    pub width: f32,
    pub height: f32,
    pub vertical: bool,
    pub optical_size: f32,
    pub weight: f32,
    pub roundness: f32,
    pub ink_ratio: f32,
    pub row_advance: f32,
    pub digit_size: f32,
    pub support_size: f32,
    pub support_render_size: f32,
    pub support_height: f32,
    pub row_gap: f32,
    pub block_top: f32,
    pub bitmap_bytes: usize,
}

struct Face {
    bytes: Arc<[u8]>,
    reference: FaceRef,
}
struct Frame {
    input: Input,
    metrics: Metrics,
    glyphs: Vec<(char, Glyph)>,
    ink_top: f32,
    ink_height: f32,
    support: Option<Glyph>,
    support_x: f32,
    support_y: f32,
}

#[derive(Default)]
struct ClockFace {
    plan: FontPlan,
    face: Option<Face>,
    frame: Option<Arc<Frame>>,
    shown: Option<Input>,
    text_scale: f32,
    fallback: Option<(f32, f32, f32, Arc<Glyph>)>,
    /// Diagnostic counter: steady paints never parse or rasterize fonts.
    preparations: u64,
}

impl ClockFace {
    /// Only validated package metadata is considered. Call on startup/theme
    /// generation changes, never from paint. Static UI faces remain untouched.
    fn load(plan: &FontPlan) -> Self {
        let face = plan.package_faces.iter().find(|face| face.variable && face.names.iter().any(|n| n == "Google Sans Flex")).and_then(|face| {
            let version = plan.files.iter().zip(&plan.file_versions).find(|(path, _)| **path == face.file)?.1;
            let bytes = crate::theme::fontdb::package_font_bytes(&face.file, *version)?;
            Some(Face { bytes, reference: FaceRef { file: face.file.clone(), index: face.index, variable: true, weight: 80 } })
        });
        Self { plan: plan.clone(), face, ..Self::default() }
    }
    /// Palette/reduced-motion generations reuse white coverage. Reopen font
    /// bytes only if validated font paths, versions, metadata or roles changed.
    fn sync(&mut self, plan: &FontPlan) -> bool {
        if self.plan == *plan {
            return false;
        }
        *self = Self::load(plan);
        true
    }
    fn available(&self) -> bool {
        self.frame.is_some()
    }
    fn metrics(&self) -> Option<&Metrics> {
        self.frame.as_ref().map(|f| &f.metrics)
    }
    #[cfg(test)]
    fn preparations(&self) -> u64 {
        self.preparations
    }

    /// Current README clock has no seconds and uses adaptive stacked/inline
    /// layout. Date fits down to the accessibility minimum and elides if necessary;
    /// unlike Qt HorizontalFit it is never silently shrunk below that minimum.
    fn prepare(&mut self, input: Input, fonts: &mut Fonts) -> bool {
        // Grid geometry clamps at 100%, but supporting text also follows smaller
        // accessibility sizes. Its actual scale is part of the raster cache key.
        let text_scale = crate::theme::live::font_scale();
        if self.text_scale != text_scale {
            self.text_scale = text_scale;
            self.frame = None;
            self.shown = None;
            self.fallback = None;
        }
        if self.shown.as_ref() == Some(&input) {
            return false;
        }
        if let Some(frame) = self.frame.as_mut() {
            if frame.input.width == input.width
                && frame.input.height == input.height
                && frame.input.date == input.date
                && frame.input.scale == input.scale
                && frame.input.hours.len() == input.hours.len()
                && frame.input.minutes.len() == input.minutes.len()
                && [input.hours.as_str(), input.minutes.as_str()].iter().all(|s| s.bytes().all(|b| b.is_ascii_digit()))
            {
                self.shown = Some(input);
                return true;
            }
        }
        self.frame = None;
        let Some(face) = &self.face else {
            if input.width.is_finite()
                && (1.0..=4096.0).contains(&input.width)
                && input.height.is_finite()
                && (1.0..=4096.0).contains(&input.height)
                && input.scale.is_finite()
                && (0.5..=3.0).contains(&input.scale)
                && !self.fallback.as_ref().is_some_and(|(w, h, s, _)| (*w, *h, *s) == (input.width, input.height, input.scale))
            {
                let size = 12.0;
                let text = crate::controlcenter::fit("Clock font unavailable", input.width, size, Weight::Medium, fonts);
                let height = fonts.line_height(size, Weight::Medium).ceil() as u32;
                if let Some(mut image) = Pixmap::new(input.width.ceil() as u32, height) {
                    let x = (input.width - fonts.measure(&text, size, Weight::Medium)) / 2.0;
                    fonts.draw(&mut image, &text, x, 0.0, size, Weight::Medium, Color::WHITE);
                    self.fallback = Some((
                        input.width,
                        input.height,
                        input.scale,
                        Arc::new(Glyph {
                            xmin: 0,
                            ymin: 0,
                            width: image.width() as usize,
                            height: image.height() as usize,
                            advance: 0.0,
                            bitmap: image.data().chunks_exact(4).map(|p| p[3]).collect(),
                            color: false,
                        }),
                    ));
                }
            }
            return false;
        };
        if !input.width.is_finite()
            || !input.height.is_finite()
            || !(1.0..=4096.0).contains(&input.width)
            || !(1.0..=4096.0).contains(&input.height)
            || input.date.len() > 512
            || !input.scale.is_finite()
            || !(0.5..=3.0).contains(&input.scale)
            || [input.hours.as_str(), input.minutes.as_str()].iter().any(|s| s.is_empty() || s.len() > 2 || !s.bytes().all(|b| b.is_ascii_digit()))
        {
            return false;
        }
        let valid = ttf_parser::Face::parse(&face.bytes, face.reference.index)
            .ok()
            .is_some_and(|parsed| [b"ROND", b"wght", b"opsz"].iter().all(|tag| parsed.variation_axes().into_iter().any(|axis| axis.tag == ttf_parser::Tag::from_bytes(tag))));
        if !valid {
            self.face = None;
            return self.prepare(input, fonts);
        }
        let vertical = input.height > 112.0 && input.height > input.width * 0.6;
        let optical_size = if vertical { input.height / 2.0 } else { input.height };
        let axes = [(*b"ROND", 100.0), (*b"wght", 750.0), (*b"opsz", optical_size)];
        let mut reference = Vec::with_capacity(11);
        for ch in DIGITS.chars() {
            let Some(glyph) = textface::raster_outline_axes(&face.bytes, &face.reference, ch, 100.0, &axes) else {
                self.face = None;
                return self.prepare(input, fonts);
            };
            if glyph.bitmap.is_empty() {
                self.face = None;
                return self.prepare(input, fonts);
            }
            reference.push((ch, glyph));
        }
        let glyph = |ch| &reference.iter().find(|(c, _)| *c == ch).unwrap().1;
        let widest = ('0'..='9').max_by(|a, b| glyph(*a).advance.total_cmp(&glyph(*b).advance).then_with(|| b.cmp(a))).unwrap();
        let ink_top = ('0'..='9').map(|c| -(glyph(c).height as f32) - glyph(c).ymin as f32).fold(f32::INFINITY, f32::min);
        let ink_bottom = ('0'..='9').map(|c| -(glyph(c).ymin as f32)).fold(f32::NEG_INFINITY, f32::max);
        let ink_ratio = ((ink_bottom - ink_top) / 100.0).max(0.01);
        let row_advance = if vertical {
            input.hours.len().max(input.minutes.len()) as f32 * glyph(widest).advance / 100.0
        } else {
            (input.hours.len() + input.minutes.len()) as f32 * glyph(widest).advance / 100.0 + glyph(':').advance / 100.0
        }
        .max(0.01);
        let width_size = input.width / row_advance;
        let estimate = width_size.min((if vertical { input.height / 2.0 } else { input.height }) / ink_ratio);
        let support_size = (14.0 * input.scale).max(estimate * ink_ratio * 0.25);
        // Native font ascender/descender line box, rounded like Qt's Text height.
        let line_height = fonts.line_height(support_size / input.scale, Weight::Medium).ceil();
        let support_gap = support_size * 0.3;
        let has_support = !input.date.is_empty() && input.height - line_height - support_gap >= 16.0 * input.scale;
        let support_height = if has_support { line_height + support_gap } else { 0.0 };
        let digits_height = input.height - support_height;
        let row_gap = if vertical { digits_height * 0.06 } else { 0.0 };
        let row_height = if vertical { (digits_height - row_gap) / 2.0 } else { digits_height };
        let digit_size = width_size.min(row_height / ink_ratio).max(1.0);
        // QFont rounds pixelSize; fitting/placement retains the source real value.
        let raster_size = digit_size.round().max(1.0);
        let mut glyphs = Vec::with_capacity(11);
        let mut bitmap_bytes = 0;
        for ch in DIGITS.chars() {
            let Some(g) = textface::raster_outline_axes(&face.bytes, &face.reference, ch, raster_size, &axes) else { return false };
            bitmap_bytes += g.bitmap.len();
            if bitmap_bytes > BITMAP_BUDGET {
                return false;
            }
            glyphs.push((ch, g));
        }
        let ink_height = digit_size * ink_ratio;
        let block_height = ink_height * if vertical { 2.0 } else { 1.0 } + row_gap + support_height;
        let block_top = (input.height - block_height) / 2.0;
        let mut support_render_size = support_size.round().max(1.0);
        if has_support {
            let minimum = (12.0 * input.scale).min(support_render_size);
            while support_render_size > minimum && fonts.measure(&input.date, support_render_size / input.scale, Weight::Medium) > input.width {
                support_render_size = (support_render_size - 1.0).max(minimum);
            }
        }
        let support = if has_support { crate::controlcenter::fit(&input.date, input.width, support_render_size / input.scale, Weight::Medium, fonts) } else { String::new() };
        let support_width = fonts.measure(&support, support_render_size / input.scale, Weight::Medium);
        let support_y = block_top + block_height - line_height;
        let support = if support.is_empty() {
            None
        } else {
            let width = input.width.ceil() as u32;
            let height = line_height.ceil() as u32;
            if width as usize * height as usize * 4 > BITMAP_BUDGET {
                return false;
            }
            let Some(mut image) = Pixmap::new(width, height) else {
                return false;
            };
            // Warm/render supporting text on this event, never in the paint loop.
            fonts.draw(&mut image, &support, 0.0, 0.0, support_render_size / input.scale, Weight::Medium, Color::WHITE);
            Some(Glyph { xmin: 0, ymin: 0, width: width as usize, height: height as usize, advance: support_width, bitmap: image.data().chunks_exact(4).map(|p| p[3]).collect(), color: false })
        };
        if let Some(support) = &support {
            bitmap_bytes += support.bitmap.len();
        }
        if bitmap_bytes > BITMAP_BUDGET {
            return false;
        }
        let metrics = Metrics {
            width: input.width,
            height: input.height,
            vertical,
            optical_size,
            weight: 750.0,
            roundness: 100.0,
            ink_ratio,
            row_advance,
            digit_size,
            support_size,
            support_render_size,
            support_height,
            row_gap,
            block_top,
            bitmap_bytes,
        };
        let support_x = (input.width - support_width) / 2.0;
        self.shown = Some(input.clone());
        self.frame = Some(Arc::new(Frame { input, metrics, glyphs, ink_top: ink_top * digit_size / 100.0, ink_height, support, support_x, support_y }));
        self.preparations += 1;
        true
    }

    fn pixels(&self) -> Self {
        Self { frame: self.frame.clone(), shown: self.shown.clone(), fallback: self.fallback.clone(), preparations: self.preparations, ..Default::default() }
    }
    /// No file access, font parsing, rasterization or allocation. Coordinates
    /// allow the existing dashboard viewport to scroll this cached clock.
    fn paint(&self, pm: &mut Pixmap, origin: (f32, f32), ink: Color, supporting: Color) -> bool {
        let Some(frame) = &self.frame else {
            if let Some((_, height, _, glyph)) = &self.fallback {
                crate::text::blend_glyph(
                    pm,
                    &glyph.bitmap,
                    glyph.width,
                    glyph.height,
                    origin.0.round() as i32,
                    (origin.1 + (*height - glyph.height as f32) / 2.0).round() as i32,
                    (supporting.red(), supporting.green(), supporting.blue(), supporting.alpha()),
                );
                return true;
            }
            return false;
        };
        let input = self.shown.as_ref().unwrap_or(&frame.input);
        let baseline = origin.1 + frame.metrics.block_top - frame.ink_top;
        if frame.metrics.vertical {
            frame.row(pm, &input.hours, origin.0, baseline, ink);
            frame.row(pm, &input.minutes, origin.0, baseline + frame.ink_height + frame.metrics.row_gap, ink);
        } else {
            // Avoid creating a formatted HH:MM string on every paint.
            let width = frame.width(&input.hours) + frame.width(":") + frame.width(&input.minutes);
            let x = origin.0 + (frame.input.width - width) / 2.0;
            frame.run(pm, &input.hours, x, baseline, ink);
            frame.run(pm, ":", x + frame.width(&input.hours), baseline, ink);
            frame.run(pm, &input.minutes, x + frame.width(&input.hours) + frame.width(":"), baseline, ink);
        }
        if let Some(glyph) = &frame.support {
            crate::text::blend_glyph(
                pm,
                &glyph.bitmap,
                glyph.width,
                glyph.height,
                (origin.0 + frame.support_x).round() as i32,
                (origin.1 + frame.support_y).round() as i32,
                (supporting.red(), supporting.green(), supporting.blue(), supporting.alpha()),
            );
        }
        true
    }
}
impl Frame {
    fn width(&self, value: &str) -> f32 {
        value.chars().filter_map(|ch| self.glyphs.iter().find(|(c, _)| *c == ch).map(|(_, g)| g.advance)).sum()
    }
    fn row(&self, pm: &mut Pixmap, value: &str, x: f32, baseline: f32, ink: Color) {
        self.run(pm, value, x + (self.input.width - self.width(value)) / 2.0, baseline, ink);
    }
    fn run(&self, pm: &mut Pixmap, value: &str, mut x: f32, baseline: f32, ink: Color) {
        for ch in value.chars() {
            if let Some((_, glyph)) = self.glyphs.iter().find(|(c, _)| *c == ch) {
                crate::text::blend_glyph(
                    pm,
                    &glyph.bitmap,
                    glyph.width,
                    glyph.height,
                    (x + glyph.xmin as f32).round() as i32,
                    (baseline - glyph.height as f32 - glyph.ymin as f32).round() as i32,
                    (ink.red(), ink.green(), ink.blue(), ink.alpha()),
                );
                x += glyph.advance;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_package_font_is_explicitly_unavailable() {
        let clock = ClockFace::load(&FontPlan::default());
        assert!(!clock.available());
        assert!(clock.metrics().is_none());
        assert_eq!(clock.preparations(), 0);
    }

    /// Package fonts and accessibility are global: execute alone with a private
    /// Source fixture containing the original licensed variable font as file5.
    #[test]
    #[ignore = "actual package clock evidence: run alone with DUX_CLOCK_EVIDENCE_DIR"]
    fn native_variable_clock_evidence() {
        assert!(std::env::var_os("DECEBAL_CONFIG").is_some() && std::env::var_os("DECEBAL_THEMES").is_some());
        let source = crate::theme::live::Source::discover();
        let selection = source.selection();
        let output = std::path::PathBuf::from(std::env::var_os("DUX_CLOCK_EVIDENCE_DIR").unwrap());
        std::fs::create_dir_all(&output).unwrap();
        let mut records = Vec::new();
        for scale in [1.0, 2.0] {
            for dark in [true, false] {
                let mut selection = selection.clone();
                selection.mode = if dark { crate::theme::model::Mode::Dark } else { crate::theme::model::Mode::Light };
                let (theme, error) = source.build(&selection, dark);
                assert!(error.is_none(), "{error:?}");
                let db = crate::theme::fontdb::FontDb::default().with_package_faces(&theme.fonts.package_faces);
                for (weight, name) in [(80, "400"), (100, "500"), (200, "700")] {
                    assert!(db.face("Google Sans Flex", weight).unwrap().file.to_string_lossy().contains(&format!("-{name}.ttf")), "static UI weight must remain unchanged");
                }
                let theme = crate::accessibility::Preferences { font_scale: Some(scale), ..Default::default() }.apply(theme);
                crate::theme::live::publish_theme(theme.clone());
                let mut fonts = Fonts::load().unwrap();
                let mut absent = ClockFace::default();
                assert!(!absent.prepare(Input { width: 96.0, height: 168.0, hours: "09".into(), minutes: "08".into(), date: String::new(), scale }, &mut fonts));
                assert!(!absent.available());
                let mut missing = Pixmap::new(96, 168).unwrap();
                missing.fill(theme.overlay.color(1.0));
                assert!(absent.paint(&mut missing, (0.0, 0.0), theme.accent_c(), theme.sub()));
                missing.save_png(output.join(format!("missing-{}-{}.png", if dark { "dark" } else { "light" }, (scale * 100.0) as u32))).unwrap();
                let mut clock = ClockFace::load(&theme.fonts);
                assert!(clock.face.is_some(), "original variable package asset required");
                let other = ClockFace::load(&theme.fonts);
                assert!(Arc::ptr_eq(&clock.face.as_ref().unwrap().bytes, &other.face.as_ref().unwrap().bytes), "all clocks must share Source's immutable bytes");
                let variable = &clock.face.as_ref().unwrap().reference.file;
                let version = theme.fonts.files.iter().zip(&theme.fonts.file_versions).find(|(path, _)| *path == variable).unwrap().1;
                let registered = crate::theme::fontdb::package_font_bytes(variable, *version).unwrap();
                assert!(Arc::ptr_eq(&registered, &clock.face.as_ref().unwrap().bytes));
                worker::assert_async_pipeline(&theme.fonts, Input { width: 96.0, height: 168.0, hours: "09".into(), minutes: "08".into(), date: "Friday, 30 October".into(), scale });

                // Exercise hot scale changes with identical clamped grid Input.
                // Both the same minute and a minute transition must match a new
                // clock prepared under the current actual supporting-text scale.
                let mut hot = AsyncClock::load(&theme.fonts);
                hot.set_waker(Arc::new(|| {}));
                let mut first_scale_pixels = None;
                for actual_scale in [1.0, 0.8, 0.9, 1.0] {
                    let mut scaled = theme.clone();
                    scaled.font_scale = actual_scale;
                    crate::theme::live::publish_theme(scaled);
                    for minute in ["08", "09"] {
                        let input = Input { width: 256.0, height: 168.0, hours: "09".into(), minutes: minute.into(), date: "Friday, 30 October".into(), scale: 1.0 };
                        hot.request(input.clone());
                        hot.wait_prepared();
                        let mut fresh = ClockFace::load(&theme.fonts);
                        assert!(fresh.prepare(input, &mut Fonts::load().unwrap()));
                        let mut actual = Pixmap::new(256, 168).unwrap();
                        let mut expected = Pixmap::new(256, 168).unwrap();
                        assert!(hot.paint(&mut actual, (0.0, 0.0), Color::WHITE, Color::WHITE));
                        assert!(fresh.paint(&mut expected, (0.0, 0.0), Color::WHITE, Color::WHITE));
                        assert_eq!(actual.data(), expected.data(), "hot scale {actual_scale}, minute {minute} must match fresh rasterization");
                        if minute == "08" {
                            if actual_scale == 1.0 {
                                if let Some(first) = &first_scale_pixels {
                                    assert_eq!(actual.data(), first, "returning to 100% restores the original pixels");
                                } else {
                                    first_scale_pixels = Some(actual.data().to_vec());
                                }
                            } else {
                                assert_ne!(actual.data(), first_scale_pixels.as_ref().unwrap(), "smaller text scale must rerasterize the date");
                            }
                        }
                    }
                }
                drop(hot);
                crate::theme::live::publish_theme(theme.clone());

                // Regression: this font has >32 gvar tuples; without gvar-alloc
                // even ordinary A/0 outlines fail, despite a valid cmap entry.
                let face = clock.face.as_ref().unwrap();
                let normal = textface::raster_outline_axes(&face.bytes, &face.reference, '0', 100.0, &[(*b"wght", 400.0), (*b"ROND", 20.0), (*b"opsz", 92.0)]).unwrap();
                let expressive = textface::raster_outline_axes(&face.bytes, &face.reference, '0', 100.0, &[(*b"wght", 750.0), (*b"ROND", 100.0), (*b"opsz", 92.0)]).unwrap();
                assert_ne!(normal.bitmap, expressive.bitmap, "axes must change real outlines");
                for axes in
                    [[(*b"wght", 400.0), (*b"ROND", 100.0), (*b"opsz", 92.0)], [(*b"wght", 750.0), (*b"ROND", 20.0), (*b"opsz", 92.0)], [(*b"wght", 750.0), (*b"ROND", 100.0), (*b"opsz", 200.0)]]
                {
                    let altered = DIGITS.chars().any(|ch| {
                        let changed = textface::raster_outline_axes(&face.bytes, &face.reference, ch, 100.0, &axes).unwrap();
                        let base = textface::raster_outline_axes(&face.bytes, &face.reference, ch, 100.0, &[(*b"wght", 750.0), (*b"ROND", 100.0), (*b"opsz", 92.0)]).unwrap();
                        changed.bitmap != base.bitmap
                    });
                    assert!(altered, "each axis must alter at least one clock glyph: {axes:?}");
                }
                assert!(textface::raster_outline_axes(&face.bytes, &face.reference, 'A', 100.0, &[(*b"wght", 750.0)]).is_some());
                for (width, height) in [(96, 168), (192, 336), (368, 336), (256, 168), (208, 336), (320, 80), (80, 96)] {
                    let input = Input { width: width as f32, height: height as f32, hours: "09".into(), minutes: "08".into(), date: "Friday, 30 October".into(), scale };
                    let started = std::time::Instant::now();
                    assert!(clock.prepare(input.clone(), &mut fonts));
                    let prepare_us = started.elapsed().as_micros();
                    let metrics = clock.metrics().unwrap().clone();
                    assert_eq!(metrics.vertical, height > 112 && height as f32 > width as f32 * 0.6);
                    assert_eq!((metrics.weight, metrics.roundness), (750.0, 100.0));
                    assert!(metrics.bitmap_bytes < BITMAP_BUDGET);
                    let frame = clock.frame.as_ref().unwrap();
                    for value in [&input.hours, &input.minutes] {
                        assert!(frame.width(value) <= width as f32 + 2.0, "actual digit ink/advance must fit width");
                    }
                    let mut image = Pixmap::new(width, height).unwrap();
                    image.fill(theme.overlay.color(1.0));
                    assert!(clock.paint(&mut image, (0.0, 0.0), theme.accent_c(), theme.sub()));
                    let label = format!("clock-{width}x{height}-{}-{}", if dark { "dark" } else { "light" }, (scale * 100.0) as u32);
                    image.save_png(output.join(format!("{label}.png"))).unwrap();
                    let before = image.data().to_vec();
                    let count = clock.preparations();
                    assert!(!clock.sync(&theme.fonts), "palette-only generations must retain the font and frame");
                    assert!(clock.available());
                    assert!(!clock.prepare(input.clone(), &mut fonts));
                    let paint_start = std::time::Instant::now();
                    for _ in 0..100 {
                        image.fill(theme.overlay.color(1.0));
                        clock.paint(&mut image, (0.0, 0.0), theme.accent_c(), theme.sub());
                    }
                    let paints_us = paint_start.elapsed().as_micros();
                    assert_eq!(image.data(), before);
                    assert_eq!(clock.preparations(), count);
                    let mut next = input;
                    next.minutes = "09".into();
                    assert!(clock.prepare(next, &mut fonts));
                    assert_eq!(clock.preparations(), count, "minute changes reuse digit bitmaps");
                    image.fill(theme.overlay.color(1.0));
                    clock.paint(&mut image, (0.0, 0.0), theme.accent_c(), theme.sub());
                    assert_ne!(image.data(), before, "new minute must repaint actual digits");
                    records.push(serde_json::json!({"case":label,"metrics":metrics,"prepare_us":prepare_us,"100_paints_us":paints_us}));
                }
            }
        }
        std::fs::write(output.join("native-metrics.json"), serde_json::to_vec_pretty(&records).unwrap()).unwrap();
    }
}
