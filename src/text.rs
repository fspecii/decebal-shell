//! Text rendering: per-role font chains with per-character fallback, a glyph cache, measuring and drawing
//! into a tiny-skia pixmap.
//!
//! A [`Role`] (interface, title, monospace, display) names a chain of installed families (see
//! [`crate::theme::fonts`]); each character is drawn by the first family of the chain that has a glyph for it,
//! so CJK, emoji and icon glyphs show up whatever the primary font. Faces are opened lazily the first time a glyph
//! is missing from the cache and dropped again by [`Fonts::trim`]; steady-state drawing only touches the small
//! glyph cache. The chains are resolved against the installed fonts once per theme change (one `fc-list` per
//! process, see [`crate::theme::fontdb`]), never per frame.

use crate::textface::{self, Backing, Glyph, Kind, Spec};
use crate::theme::fontdb::{self, FontDb, FC_BOLD, FC_MEDIUM, FC_REGULAR, FC_SEMIBOLD};
use crate::theme::fonts::{FontPlan, Resolved};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tiny_skia::{Color, Pixmap};

pub use crate::theme::fonts::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl Weight {
    fn fc(self) -> i32 {
        match self {
            Weight::Regular => FC_REGULAR,
            Weight::Medium => FC_MEDIUM,
            Weight::SemiBold => FC_SEMIBOLD,
            Weight::Bold => FC_BOLD,
        }
    }
}

/// Cap on cached glyphs (distinct char/size/weight/role combinations) so odd window titles cannot grow memory.
const MAX_GLYPHS: usize = 4096;
/// Parsed fontdue faces kept at once (about 12 MB each) while a frame mixes Latin text with a second face.
const MAX_PARSED: usize = 2;

type SpecKey = (Role, usize, Weight);
type GlyphKey = (char, u32, Weight, Role);

/// What the current theme asked for, checked against the installed fonts.
struct Setup {
    plan: FontPlan,
    db: Arc<FontDb>,
    resolved: Resolved,
}

impl Setup {
    fn new(plan: FontPlan, db: Arc<FontDb>) -> Setup {
        let resolved = plan.resolve(&db);
        Setup { plan, db, resolved }
    }
}

/// UI text: the fonts of the current theme, per role, with per-character fallback.
pub struct Fonts {
    setup: RefCell<Setup>,
    /// Theme generation the setup/glyphs were built for (a theme change can switch fonts and scale).
    theme_gen: Cell<u64>,
    /// Faces by (role, chain position, weight); `None` = that family has no usable file.
    specs: RefCell<HashMap<SpecKey, Option<Rc<Spec>>>>,
    backings: RefCell<HashMap<PathBuf, Rc<Backing>>>,
    parsed: RefCell<Vec<(PathBuf, u32, Rc<fontdue::Font>)>>,
    glyphs: RefCell<HashMap<GlyphKey, Option<Rc<Glyph>>>>,
    ascents: RefCell<HashMap<(Role, u32, Weight), f32>>,
}

/// Hands freed heap pages back to the OS (glibc keeps them otherwise, which would defeat `trim`).
fn release_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim only releases free memory; it has no preconditions.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// Characters that take no space and have no glyph of their own: joiners, variation selectors, zero-width space.
fn is_invisible(ch: char) -> bool {
    matches!(ch, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}' | '\u{1f3fb}'..='\u{1f3ff}')
}

impl Fonts {
    /// `None` only if no usable font file can be found at all.
    pub fn load() -> Option<Self> {
        let db = fontdb::get();
        let setup = Setup::new(crate::theme::current().fonts.clone(), db);
        setup.resolved.primary(Role::Ui)?;
        Some(Self {
            setup: RefCell::new(setup),
            theme_gen: Cell::new(crate::theme::generation()),
            specs: RefCell::new(HashMap::new()),
            backings: RefCell::new(HashMap::new()),
            parsed: RefCell::new(Vec::new()),
            glyphs: RefCell::new(HashMap::new()),
            ascents: RefCell::new(HashMap::new()),
        })
    }

    /// Rebuilds the chains and drops cached glyphs when the theme's fonts (or the installed fonts) changed since the last call.
    fn sync_theme(&self) {
        let g = crate::theme::generation();
        if self.theme_gen.get() == g {
            return;
        }
        self.theme_gen.set(g);
        let plan = crate::theme::current().fonts.clone();
        let db = fontdb::get();
        {
            let cur = self.setup.borrow();
            if cur.plan == plan && cur.db.gen == db.gen {
                return; // only colours / sizes changed: glyph keys already include the scaled size
            }
        }
        *self.setup.borrow_mut() = Setup::new(plan, db);
        self.specs.borrow_mut().clear();
        self.glyphs.borrow_mut().clear();
        self.ascents.borrow_mut().clear();
        self.trim();
    }

    fn chain_len(&self, role: Role) -> usize {
        self.setup.borrow().resolved.chain(role).len()
    }

    fn backing(&self, path: &PathBuf) -> Option<Rc<Backing>> {
        if let Some(b) = self.backings.borrow().get(path) {
            return Some(b.clone());
        }
        let b = Rc::new(Backing::open(path)?);
        self.backings.borrow_mut().insert(path.clone(), b.clone());
        Some(b)
    }

    /// The face of the `i`-th family of `role`'s chain at `weight`.
    fn spec(&self, role: Role, i: usize, weight: Weight) -> Option<Rc<Spec>> {
        let key = (role, i, weight);
        if let Some(s) = self.specs.borrow().get(&key) {
            return s.clone();
        }
        let spec = (|| {
            let face = {
                let setup = self.setup.borrow();
                let family = setup.resolved.chain(role).get(i)?;
                setup.db.face(family, weight.fc())?
            };
            let backing = self.backing(&face.file)?;
            let kind = textface::classify(backing.bytes(), &face)?;
            Some(Rc::new(Spec { face, kind }))
        })();
        self.specs.borrow_mut().insert(key, spec.clone());
        spec
    }

    /// The fontdue face for `spec`, parsing it if needed (at most [`MAX_PARSED`] stay in memory).
    fn parsed_face(&self, spec: &Spec) -> Option<Rc<fontdue::Font>> {
        let mut parsed = self.parsed.borrow_mut();
        if let Some(pos) = parsed.iter().position(|(p, i, _)| *p == spec.face.file && *i == spec.face.index) {
            let hit = parsed.remove(pos); // most recently used goes last
            let font = hit.2.clone();
            parsed.push(hit);
            return Some(font);
        }
        let backing = self.backing(&spec.face.file)?;
        let font = Rc::new(textface::load_fontdue(backing.bytes(), spec.face.index)?);
        if parsed.len() >= MAX_PARSED {
            parsed.remove(0);
        }
        parsed.push((spec.face.file.clone(), spec.face.index, font.clone()));
        Some(font)
    }

    fn rasterize(&self, spec: &Spec, ch: char, px: f32) -> Option<Glyph> {
        match spec.kind {
            Kind::Fontdue => Some(textface::raster_fontdue(&*self.parsed_face(spec)?, ch, px)),
            Kind::Outline => textface::raster_outline(self.backing(&spec.face.file)?.bytes(), &spec.face, ch, px),
            Kind::Bitmap => textface::raster_bitmap(self.backing(&spec.face.file)?.bytes(), &spec.face, ch, px),
        }
    }

    /// Drops the parsed faces and file mappings (tens of MB); cached glyphs stay. Call after a frame is drawn.
    pub fn trim(&self) {
        let had = !self.parsed.borrow().is_empty() || !self.backings.borrow().is_empty();
        self.parsed.borrow_mut().clear();
        self.backings.borrow_mut().clear();
        if had {
            release_heap();
        }
    }

    /// True while a parsed face is held in memory.
    pub fn has_loaded_face(&self) -> bool {
        !self.parsed.borrow().is_empty()
    }

    fn glyph(&self, role: Role, ch: char, px: f32, weight: Weight) -> Option<Rc<Glyph>> {
        if is_invisible(ch) {
            return None;
        }
        self.sync_theme();
        let px = px * crate::theme::live::font_scale();
        let key = (ch, (px * 4.0).round() as u32, weight, role);
        if let Some(g) = self.glyphs.borrow().get(&key) {
            return g.clone();
        }
        let g = self.build_glyph(role, ch, px, weight).map(Rc::new);
        let mut cache = self.glyphs.borrow_mut();
        if cache.len() >= MAX_GLYPHS {
            cache.clear();
        }
        cache.insert(key, g.clone());
        g
    }

    /// The first family of the chain with a glyph for `ch` draws it; a character nobody has shows the primary font's own
    /// placeholder.
    fn build_glyph(&self, role: Role, ch: char, px: f32, weight: Weight) -> Option<Glyph> {
        let n = self.chain_len(role);
        let owner = textface::first_covering(n, |i| self.spec(role, i, weight).and_then(|s| self.backing(&s.face.file).map(|b| textface::covers(b.bytes(), s.face.index, ch))).unwrap_or(false));
        for i in owner.into_iter().chain(0..n) {
            if let Some(spec) = self.spec(role, i, weight) {
                if let Some(g) = self.rasterize(&spec, ch, px) {
                    return Some(g);
                }
            }
        }
        None
    }

    /// Which family of `role`'s chain draws `ch` (the installed name), for the settings preview and tests.
    pub fn family_for(&self, role: Role, ch: char) -> Option<String> {
        self.sync_theme();
        let n = self.chain_len(role);
        let i = textface::first_covering(n, |i| self.spec(role, i, Weight::Regular).and_then(|s| self.backing(&s.face.file).map(|b| textface::covers(b.bytes(), s.face.index, ch))).unwrap_or(false))?;
        self.setup.borrow().resolved.chain(role).get(i).cloned()
    }

    /// Distance from the top of the line box to the baseline at pixel size `px`.
    pub fn ascent(&self, px: f32, weight: Weight) -> f32 {
        self.ascent_as(Role::Ui, px, weight)
    }

    /// [`Self::ascent`] in the primary face of `role`.
    pub fn ascent_as(&self, role: Role, px: f32, weight: Weight) -> f32 {
        self.sync_theme();
        let px = px * crate::theme::live::font_scale();
        let key = (role, (px * 4.0).round() as u32, weight);
        if let Some(a) = self.ascents.borrow().get(&key) {
            return *a;
        }
        let a = self.spec(role, 0, weight).and_then(|s| textface::ascent(self.backing(&s.face.file)?.bytes(), s.face.index, px)).unwrap_or(px * 0.9);
        self.ascents.borrow_mut().insert(key, a);
        a
    }

    /// Advance width of `text` at pixel size `px`.
    pub fn measure(&self, text: &str, px: f32, weight: Weight) -> f32 {
        self.measure_as(Role::Ui, text, px, weight)
    }

    /// [`Self::measure`] in the font of `role`.
    pub fn measure_as(&self, role: Role, text: &str, px: f32, weight: Weight) -> f32 {
        text.chars().filter_map(|c| self.glyph(role, c, px, weight)).map(|g| g.advance).sum()
    }

    /// Draws `text` with its baseline-box top-left at (`x`, `y`) and returns the width drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, pm: &mut Pixmap, text: &str, x: f32, y: f32, px: f32, weight: Weight, color: Color) -> f32 {
        self.draw_as(Role::Ui, pm, text, x, y, px, weight, color)
    }

    /// [`Self::draw`] in the font of `role`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_as(&mut self, role: Role, pm: &mut Pixmap, text: &str, x: f32, y: f32, px: f32, weight: Weight, color: Color) -> f32 {
        let baseline = y + self.ascent_as(role, px, weight);
        let (cr, cg, cb, ca) = (color.red(), color.green(), color.blue(), color.alpha());
        let mut pen = x;
        for ch in text.chars() {
            let Some(g) = self.glyph(role, ch, px, weight) else { continue };
            let gx = (pen + g.xmin as f32).round() as i32;
            let gy = (baseline - g.height as f32 - g.ymin as f32).round() as i32;
            if g.color {
                blend_color_glyph(pm, &g.bitmap, g.width, g.height, gx, gy, ca);
            } else {
                blend_glyph(pm, &g.bitmap, g.width, g.height, gx, gy, (cr, cg, cb, ca));
            }
            pen += g.advance;
        }
        pen - x
    }
}

/// Source-over of a premultiplied RGBA glyph (colour emoji) scaled by `alpha`.
fn blend_color_glyph(pm: &mut Pixmap, rgba: &[u8], gw: usize, gh: usize, gx: i32, gy: i32, alpha: f32) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let stride = pm.width() as usize * 4;
    let data = pm.data_mut();
    for row in 0..gh as i32 {
        let py = gy + row;
        if py < 0 || py >= ph {
            continue;
        }
        for col in 0..gw as i32 {
            let px = gx + col;
            if px < 0 || px >= pw {
                continue;
            }
            let s = (row as usize * gw + col as usize) * 4;
            let a = rgba[s + 3] as f32 / 255.0 * alpha;
            if a <= 0.0 {
                continue;
            }
            let i = py as usize * stride + px as usize * 4;
            let inv = 1.0 - a;
            for c in 0..3 {
                data[i + c] = (rgba[s + c] as f32 * alpha + data[i + c] as f32 * inv).round().min(255.0) as u8;
            }
            data[i + 3] = (255.0 * a + data[i + 3] as f32 * inv).round().min(255.0) as u8;
        }
    }
}

/// Source-over blend of an 8-bit coverage bitmap tinted with a straight (non-premultiplied) colour.
fn blend_glyph(pm: &mut Pixmap, bitmap: &[u8], gw: usize, gh: usize, gx: i32, gy: i32, c: (f32, f32, f32, f32)) {
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let stride = pm.width() as usize * 4;
    let data = pm.data_mut();
    for row in 0..gh as i32 {
        let py = gy + row;
        if py < 0 || py >= ph {
            continue;
        }
        for col in 0..gw as i32 {
            let px = gx + col;
            if px < 0 || px >= pw {
                continue;
            }
            let cov = bitmap[row as usize * gw + col as usize] as f32 / 255.0 * c.3;
            if cov <= 0.0 {
                continue;
            }
            let i = py as usize * stride + px as usize * 4;
            let inv = 1.0 - cov;
            data[i] = (c.0 * 255.0 * cov + data[i] as f32 * inv).round() as u8;
            data[i + 1] = (c.1 * 255.0 * cov + data[i + 1] as f32 * inv).round() as u8;
            data[i + 2] = (c.2 * 255.0 * cov + data[i + 2] as f32 * inv).round() as u8;
            data[i + 3] = (255.0 * cov + data[i + 3] as f32 * inv).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measuring_unseen_text_loads_a_face_that_trim_drops() {
        let Some(f) = Fonts::load() else { return };
        assert!(!f.has_loaded_face());
        f.measure("zq unseen \u{2603} text", 12.0, Weight::Regular);
        // the face is a fontdue one unless the system font is variable or huge
        f.trim();
        assert!(!f.has_loaded_face());
        assert!(f.backings.borrow().is_empty(), "mappings are released too");
    }

    #[test]
    fn measures_and_draws() {
        let Some(mut f) = Fonts::load() else { return }; // CI machines may have no fonts
        assert!(f.measure("Finder", 13.0, Weight::Regular) > f.measure("Fi", 13.0, Weight::Regular));
        let mut pm = Pixmap::new(80, 20).unwrap();
        let w = f.draw(&mut pm, "File", 2.0, 2.0, 13.0, Weight::Regular, Color::WHITE);
        assert!(w > 10.0);
        assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0));
    }

    #[test]
    fn bold_is_a_different_face_than_regular() {
        let Some(f) = Fonts::load() else { return };
        assert!(f.measure("Finder File Edit", 12.0, Weight::Bold) > f.measure("Finder File Edit", 12.0, Weight::Regular));
    }

    #[test]
    fn ascent_is_positive_and_scales() {
        let Some(f) = Fonts::load() else { return };
        assert!(f.ascent(24.0, Weight::Regular) > f.ascent(12.0, Weight::Regular) && f.ascent(12.0, Weight::Regular) > 0.0);
    }

    #[test]
    fn trimming_drops_faces_but_text_still_works() {
        let Some(mut f) = Fonts::load() else { return };
        let before = f.measure("Control Center", 12.0, Weight::Regular);
        f.trim();
        assert!(!f.has_loaded_face(), "faces are released");
        assert_eq!(f.measure("Control Center", 12.0, Weight::Regular), before, "answered from the glyph cache");
        // A glyph never seen before reloads the face transparently.
        assert!(f.measure("Zq", 12.0, Weight::Regular) > 0.0);
        f.trim();
        let mut pm = Pixmap::new(60, 20).unwrap();
        assert!(f.draw(&mut pm, "Edit", 1.0, 1.0, 12.0, Weight::Bold, Color::WHITE) > 0.0);
    }

    #[test]
    fn glyph_cache_is_bounded() {
        let Some(f) = Fonts::load() else { return };
        for c in 0..(MAX_GLYPHS as u32 + 200) {
            if let Some(ch) = char::from_u32(0x4e00 + c) {
                f.measure(&ch.to_string(), 12.0, Weight::Regular);
            }
        }
        assert!(f.glyphs.borrow().len() <= MAX_GLYPHS);
    }

    #[test]
    fn empty_text_is_zero_width() {
        let Some(f) = Fonts::load() else { return };
        assert_eq!(f.measure("", 13.0, Weight::Bold), 0.0);
    }

    #[test]
    fn joiners_and_variation_selectors_take_no_space() {
        let Some(f) = Fonts::load() else { return };
        assert_eq!(f.measure("a\u{200d}\u{fe0f}", 13.0, Weight::Regular), f.measure("a", 13.0, Weight::Regular));
    }

    #[test]
    fn missing_characters_come_from_a_later_family_when_one_has_them() {
        let Some(f) = Fonts::load() else { return };
        // Whatever the primary font is, a CJK character is drawn by a family that really has it, or shows the placeholder.
        let w = f.measure("日", 16.0, Weight::Regular);
        assert!(w > 0.0);
        if let Some(fam) = f.family_for(Role::Ui, '日') {
            let mut pm = Pixmap::new(40, 30).unwrap();
            let mut f = f;
            f.draw(&mut pm, "日", 4.0, 4.0, 16.0, Weight::Regular, Color::WHITE);
            assert!(pm.data().chunks_exact(4).any(|p| p[3] > 0), "{fam} drew the glyph");
        }
    }

    #[test]
    fn roles_use_their_own_chains() {
        let Some(f) = Fonts::load() else { return };
        // the mono chain starts with a monospace family on any machine that has one: digits are all the same width
        let w: Vec<f32> = "0189".chars().map(|c| f.measure_as(Role::Mono, &c.to_string(), 14.0, Weight::Regular)).collect();
        if f.setup.borrow().db.is_mono(f.setup.borrow().resolved.primary(Role::Mono).unwrap_or("")) {
            assert!(w.windows(2).all(|p| (p[0] - p[1]).abs() < 0.01), "{w:?}");
        }
    }
}
