//! DLauncherV2's default full/compact Applications list, using native apps only.
//! Source: LauncherMetrics, LauncherContent, LauncherRow and SettingsSpec.
//! Icons and desktop comments are prepared on events, never from this painter.
use super::*;
use crate::menubar::symbols::SymbolCache;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub width: f32,
    pub height: f32,
    pub row: f32,
    pub rows: usize,
}
impl Layout {
    pub fn new(size: (u32, u32), scale: f32) -> Self {
        let row = 56.0_f32.max(42.0 * scale);
        let results = (size.1 as f32 - 100.0 - 32.0 - 24.0).max(0.0);
        Self { width: size.0 as f32, height: size.1 as f32, row, rows: (results / (row + 2.0)).floor().max(1.0) as usize }
    }
    pub fn cell(&self, index: usize, scroll: usize) -> (f32, f32, f32, f32) {
        (12.0, 100.0 + (index as f32 - scroll as f32) * (self.row + 2.0), (self.width - 24.0).max(1.0), self.row)
    }
    pub fn hit(&self, x: f32, y: f32, scroll: usize, shown: usize) -> Option<usize> {
        if x < 12.0 || x >= self.width - 12.0 || y < 100.0 || y >= self.height - 32.0 - 24.0 {
            return None;
        }
        let row = ((y - 100.0) / (self.row + 2.0)).floor() as usize;
        let i = scroll + row;
        (row < self.rows && i < shown && (y - 100.0) % (self.row + 2.0) < self.row).then_some(i)
    }
}

pub(super) struct State {
    pub layout: Layout,
    comments: Vec<String>,
    images: HashMap<usize, Pixmap>,
    pub symbols: SymbolCache,
    generation: (u64, u64),
    pub buffer: Option<Pixmap>,
}
fn comment(app: &AppEntry) -> String {
    let path = std::path::Path::new(&app.file);
    if path.extension().is_none_or(|ext| ext != "desktop") || !std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= 64 * 1024) {
        return String::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let mut entry = false;
    for line in text.lines() {
        if line.starts_with('[') {
            entry = line.trim() == "[Desktop Entry]";
        } else if entry {
            if let Some(value) = line.strip_prefix("Comment=") {
                return value.chars().filter(|c| !c.is_control()).take(240).collect();
            }
        }
    }
    String::new()
}
impl State {
    pub fn stale(&self) -> bool {
        self.generation != (crate::theme::generation(), crate::icons::live::generation())
    }
    pub fn new(apps: &[AppEntry], size: (u32, u32)) -> Self {
        let mut symbols = SymbolCache::default();
        symbols.prepare_material();
        Self {
            layout: Layout::new(size, crate::theme::live::font_scale()),
            comments: apps.iter().map(comment).collect(),
            images: HashMap::new(),
            symbols,
            generation: (crate::theme::generation(), crate::icons::live::generation()),
            buffer: None,
        }
    }
    pub fn prepare(&mut self, apps: &[AppEntry], shown: &[usize], scroll: usize, size: (u32, u32), icons: &mut IconCache) {
        self.layout = Layout::new(size, crate::theme::live::font_scale());
        let generation = (crate::theme::generation(), crate::icons::live::generation());
        if self.generation != generation {
            self.images.clear();
            self.symbols.prepare_material();
            self.generation = generation;
        }
        let keys: HashSet<_> = shown.iter().skip(scroll).take(self.layout.rows).copied().collect();
        self.images.retain(|index, _| keys.contains(index));
        for index in keys {
            if let Some(app) = apps.get(index) {
                self.images.entry(index).or_insert_with(|| icons.get(&app.icon, 36).clone());
            }
        }
    }
}
fn grouped_row(pm: &mut Pixmap, (x, y, w, h): (f32, f32, f32, f32), first: bool, last: bool, color: Color) {
    let top = if first { 16.0 } else { 4.0 };
    let bottom = if last { 16.0 } else { 4.0 };
    let (kt, kb) = (top * 0.552_284_8, bottom * 0.552_284_8);
    let mut path = tiny_skia::PathBuilder::new();
    path.move_to(x + top, y);
    path.line_to(x + w - top, y);
    path.cubic_to(x + w - top + kt, y, x + w, y + top - kt, x + w, y + top);
    path.line_to(x + w, y + h - bottom);
    path.cubic_to(x + w, y + h - bottom + kb, x + w - bottom + kb, y + h, x + w - bottom, y + h);
    path.line_to(x + bottom, y + h);
    path.cubic_to(x + bottom - kb, y + h, x, y + h - bottom + kb, x, y + h - bottom);
    path.line_to(x, y + top);
    path.cubic_to(x, y + top - kt, x + top - kt, y, x + top, y);
    path.close();
    if let Some(path) = path.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        pm.fill_path(&path, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    }
}

#[allow(clippy::too_many_arguments)]
fn text(fonts: &mut Fonts, pm: &mut Pixmap, value: &str, x: f32, cy: f32, size: f32, weight: Weight, ink: Color) {
    let (top, bottom) = fonts.ink_vertical(value, size, weight).unwrap_or((0.0, 0.0));
    let y = cy - (bottom - top) / 2.0 - top - fonts.ascent(size, weight);
    fonts.draw(pm, value, x, y, size, weight, ink);
}
fn fit(fonts: &Fonts, value: &str, size: f32, width: f32, weight: Weight) -> String {
    let mut out: String = value.chars().filter(|c| !c.is_control()).take(256).collect();
    if fonts.measure(&out, size, weight) <= width {
        return out;
    }
    while !out.is_empty() {
        out.pop();
        if fonts.measure(&format!("{out}…"), size, weight) <= width {
            return format!("{out}…");
        }
    }
    String::new()
}
impl Launcher {
    pub(super) fn is_dux(&self) -> bool {
        crate::theme::th().launcher_style == "dux"
    }
    pub(super) fn prepare_dux(&mut self) {
        if !self.is_dux() {
            return;
        }
        if self.dux.is_none() {
            let old = self.size;
            self.size = (old.0.min(620), old.1.min(620));
            if self.overlay {
                self.origin.0 += (old.0 - self.size.0) as f32 / 2.0;
                self.origin.1 += (old.1 - self.size.1) as f32 / 2.0;
            }
        }
        let state = self.dux.get_or_insert_with(|| State::new(&self.apps, self.size));
        state.prepare(&self.apps, &self.shown, self.scroll, self.size, &mut self.icons);
        // Glyph/backing preparation is also event-side: no lazy font file open
        // is required by the normal Dux paint path for these cached strings.
        if let Some(fonts) = self.fonts.as_ref() {
            for index in self.shown.iter().skip(self.scroll).take(state.layout.rows) {
                if let Some(app) = self.apps.get(*index) {
                    let label: String = app.name.chars().take(256).collect();
                    fonts.measure(&label, 16.0, Weight::Medium);
                }
                if let Some(comment) = state.comments.get(*index) {
                    fonts.measure(comment, 12.0, Weight::Regular);
                }
            }
            let query: String = self.field.text().chars().take(256).collect();
            fonts.measure(&query, 16.0, Weight::Regular);
            for (label, size, weight) in [
                ("Search…", 16.0, Weight::Regular),
                ("Applications…", 12.0, Weight::Medium),
                ("Apps", 12.0, Weight::Medium),
                ("↑↓ nav   Enter launch", 12.0, Weight::Regular),
                ("No results found", 14.0, Weight::Regular),
                ("×", 20.0, Weight::Regular),
                ("0123456789", 12.0, Weight::Regular),
                ("…", 16.0, Weight::Medium),
                ("…", 12.0, Weight::Regular),
            ] {
                fonts.measure(label, size, weight);
            }
        }
    }
    pub(super) fn draw_dux(&mut self, pm: &mut Pixmap) {
        let Some(state) = self.dux.as_mut() else {
            return;
        };
        let Some(fonts) = self.fonts.as_mut() else {
            return;
        };
        let th = crate::theme::th();
        let layout = state.layout;
        pm.fill(Color::TRANSPARENT);
        fill_rounded(pm, 0.0, 0.0, layout.width, layout.height, 16.0, th.background.color(1.0));
        let high = th.text.over(th.surface, 0.05);
        let field = high.color(1.0);
        fill_rounded(pm, 12.0, 12.0, layout.width - 24.0, 48.0, 24.0, field);
        stroke_rounded(pm, 12.5, 12.5, layout.width - 25.0, 47.0, 23.5, th.border.color(0.35), 1.0);
        if let Some(icon) = state.symbols.get_material("magnifyingglass", th.accent.t(), 24) {
            blit_scaled(pm, icon, 24.0, 24.0, 24.0, 24.0, 1.0);
        }
        let empty = self.field.text().is_empty();
        let value = if empty { "Search…" } else { self.field.text() };
        let query = fit(fonts, value, 16.0, (layout.width - 108.0).max(1.0), Weight::Regular);
        text(fonts, pm, &query, 60.0, 36.0, 16.0, Weight::Regular, if empty { th.text_secondary.color(1.0) } else { th.text_c() });
        if !empty {
            let cursor = (60.0 + fonts.measure(self.field.before_cursor(), 16.0, Weight::Regular)).min(layout.width - 50.0);
            let caret = (20.0 * crate::theme::live::font_scale()).min(36.0);
            crate::gfx::fill_rect(pm, cursor, 36.0 - caret / 2.0, 1.0, caret, th.accent_c());
            if let Some(icon) = state.symbols.get_material("close", th.text_secondary.t(), 16) {
                blit_scaled(pm, icon, layout.width - 40.0, 28.0, 16.0, 16.0, 1.0);
            }
        }
        if let Some(icon) = state.symbols.get_material("apps", th.accent.t(), 16) {
            blit_scaled(pm, icon, 20.0, 72.0, 16.0, 16.0, 1.0);
        }
        text(fonts, pm, "Applications", 44.0, 80.0, 12.0, Weight::Medium, th.text_secondary.color(1.0));
        for pos in self.scroll..(self.scroll + layout.rows).min(self.shown.len()) {
            let (x, y, w, h) = layout.cell(pos, self.scroll);
            let Some(index) = self.shown.get(pos).copied() else {
                continue;
            };
            let Some(app) = self.apps.get(index) else {
                continue;
            };
            let selected = pos == self.sel;
            let background = if selected { th.accent.over(high, 0.2) } else { th.surface };
            grouped_row(pm, (x, y, w, h), pos == 0, pos + 1 == self.shown.len(), background.color(1.0));
            if let Some(icon) = state.images.get(&index) {
                blit_scaled(pm, icon, x + 16.0, y + (h - 36.0) / 2.0, 36.0, 36.0, 1.0);
            }
            let label = fit(fonts, &app.name, 16.0, (w - 92.0).max(1.0), Weight::Medium);
            let subtitle = state.comments.get(index).map(String::as_str).unwrap_or("");
            if subtitle.is_empty() {
                text(fonts, pm, &label, x + 68.0, y + h / 2.0, 16.0, Weight::Medium, th.text_c());
            } else {
                let scale = crate::theme::live::font_scale();
                text(fonts, pm, &label, x + 68.0, y + h / 2.0 - 8.0 * scale, 16.0, Weight::Medium, th.text_c());
                let desc = fit(fonts, subtitle, 12.0, (w - 92.0).max(1.0), Weight::Regular);
                text(fonts, pm, &desc, x + 68.0, y + h / 2.0 + 12.0 * scale, 12.0, Weight::Regular, th.text_secondary.color(1.0));
            }
        }
        if self.shown.is_empty() {
            let cy = (100.0 + layout.height - 32.0) / 2.0;
            if let Some(icon) = state.symbols.get_material("apps", th.border.t(), 48) {
                blit_scaled(pm, icon, layout.width / 2.0 - 24.0, cy - 48.0, 48.0, 48.0, 1.0);
            }
            let label = "No results found";
            let width = fonts.measure(label, 14.0, Weight::Regular);
            text(fonts, pm, label, (layout.width - width) / 2.0, cy + 24.0, 14.0, Weight::Regular, th.text_secondary.color(1.0));
        }
        let fy = layout.height - 32.0;
        fill_rounded(pm, 12.0, fy + 4.0, 62.0, 24.0, 12.0, th.accent.over(high, 0.2).color(1.0));
        text(fonts, pm, "Apps", 26.0, fy + 16.0, 12.0, Weight::Medium, th.text_c());
        let hint = "↑↓ nav   Enter launch";
        let width = fonts.measure(hint, 12.0, Weight::Regular);
        if width < layout.width - 100.0 {
            text(fonts, pm, hint, layout.width - 16.0 - width, fy + 16.0, 12.0, Weight::Regular, th.text_secondary.color(1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_source_geometry_and_short_list_hit_targets() {
        let layout = Layout::new((620, 620), 1.0);
        assert_eq!(layout.rows, 8);
        assert_eq!(layout.cell(0, 0), (12.0, 100.0, 596.0, 56.0));
        assert_eq!(layout.hit(20.0, 110.0, 0, 20), Some(0));
        assert_eq!(layout.hit(20.0, 157.0, 0, 20), None);
        for scale in [1.0, 2.0] {
            let l = Layout::new((620, 320), scale);
            for i in 0..l.rows {
                let (_, y, _, h) = l.cell(i, 0);
                assert!(y + h <= 320.0 - 32.0 - 24.0);
            }
        }
    }
}

#[cfg(test)]
mod evidence {
    use super::*;
    #[test]
    #[ignore = "isolated real-font Dux launcher evidence"]
    fn dux_launcher_native_render_evidence() {
        let dir = std::path::PathBuf::from(std::env::var_os("DUX_LAUNCHER_EVIDENCE_DIR").expect("private evidence directory required"));
        assert!(std::env::var_os("DECEBAL_CONFIG").is_some());
        std::fs::create_dir_all(&dir).unwrap();
        let source = crate::theme::live::Source::discover();
        for mode in [crate::theme::model::Mode::Dark, crate::theme::model::Mode::Light] {
            let mut selection = source.selection();
            selection.mode = mode;
            let base = source.build(&selection, mode == crate::theme::model::Mode::Dark).0;
            for scale in [1.0, 2.0] {
                for height in [620, 320] {
                    let mut theme = base.clone();
                    theme.launcher_style = "dux".into();
                    theme.font_scale = scale;
                    crate::theme::live::publish_theme(theme);
                    let entries = (0..24).map(|i| AppEntry { name: format!("Native fixture application {i:02}"), exec: "true".into(), ..AppEntry::default() }).collect();
                    let mut launcher = Launcher::with_apps(entries);
                    assert!(launcher.popup_centered());
                    launcher.set_screen(960, height + 100);
                    assert_eq!(launcher.initial_size(), (620, height), "popup must fit monitor before native creation");
                    launcher.resize(620, height);
                    launcher.dux.as_mut().unwrap().comments.iter_mut().for_each(|s| *s = "Installed application description fixture".into());
                    let mut pixels = Pixmap::new(620, height).unwrap();
                    launcher.draw(&mut pixels);
                    let state = launcher.dux.as_ref().unwrap();
                    assert!(state.images.len() >= state.layout.rows.min(24));
                    assert_eq!(launcher.cols(), 1);
                    for pos in 0..launcher.visible_rows() {
                        let (x, y, w, h) = launcher.cell_rect(pos);
                        assert!(x >= 0.0 && x + w <= 620.0 && y + h <= height as f32 - 56.0);
                        assert_eq!(launcher.hit(x + w / 2.0, y + h / 2.0), Some(pos));
                    }
                    let fonts = launcher.fonts.as_ref().unwrap();
                    let name = "Native fixture application 00";
                    let desc = "Installed application description fixture";
                    let (nt, nb) = fonts.ink_vertical(name, 16.0, Weight::Medium).unwrap();
                    let (dt, db) = fonts.ink_vertical(desc, 12.0, Weight::Regular).unwrap();
                    let row = launcher.dux.as_ref().unwrap().layout.row;
                    let name_top = row / 2.0 - 8.0 * scale - (nb - nt) / 2.0;
                    let name_bottom = row / 2.0 - 8.0 * scale + (nb - nt) / 2.0;
                    let desc_top = row / 2.0 + 12.0 * scale - (db - dt) / 2.0;
                    let desc_bottom = row / 2.0 + 12.0 * scale + (db - dt) / 2.0;
                    assert!(name_top >= 0.0 && desc_bottom <= row, "actual text escapes row at{scale}: {name_top}..{desc_bottom}/{row}");
                    assert!(desc_top - name_bottom >= 2.0, "support text overlaps primary ink");
                    assert!(row + 2.0 + name_top - desc_bottom >= 2.0, "adjacent true ink overlaps");
                    let family = launcher.fonts.as_ref().unwrap().family_for(crate::text::Role::Ui, 'A').unwrap();
                    assert!(family.contains("Google Sans"), "{family}");
                    let mode = if mode == crate::theme::model::Mode::Dark { "dark" } else { "light" };
                    pixels.save_png(dir.join(format!("launcher-{mode}-{}-{height}.png", (scale * 100.0) as u32))).unwrap();
                    launcher.key(&KeyEvent { key: Key::PageDown, text: String::new(), ch: None, ctrl: false, shift: false, alt: false });
                    assert!(launcher.scroll > 0);
                    launcher.draw(&mut pixels);
                    pixels.save_png(dir.join(format!("launcher-{mode}-{}-{height}-scrolled.png", (scale * 100.0) as u32))).unwrap();
                }
            }
        }
    }
}
