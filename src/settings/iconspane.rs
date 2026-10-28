//! The "Icons" pane: installed icon themes (with an Auto entry), a live preview grid of 16 common icons for the
//! highlighted one, the icon size and symbolic tint settings, and the fallback theme.
//!
//! Click a theme to highlight it (the grid shows its icons), "Use this theme" applies it. Everything is stored in
//! `decebal.json` under `icons` (see `docs/icons.md`); running programs follow within half a second.

use super::model::{CtlVal, Model};
use super::ui::{pal, Ctl, Frame, Interact, Item, Kind, Painter, Placed, Row};
use crate::config::Stamp;
use crate::gfx::{fill_rounded, stroke_rounded};
use crate::icons::config::{IconConfig, DEFAULT_FALLBACK, SCALE_MAX, SCALE_MIN};
use crate::icons::resolve::{self, Roots, ThemeInfo};
use crate::icons::{live, preview};
use crate::text::Weight;
use std::sync::Mutex;
use tiny_skia::Pixmap;

/// Id of the "Auto" entry.
pub const AUTO: &str = "auto";

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeRow {
    /// Theme directory name, or [`AUTO`].
    pub id: String,
    pub title: String,
    pub detail: String,
    /// What is applied now.
    pub current: bool,
    /// What the preview shows.
    pub highlighted: bool,
    pub selectable: bool,
}

/// The preview grid: icons of `theme` (`Some("")` = hicolor and the fallback only).
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub theme: String,
    pub title: String,
}

static HIGHLIGHT: Mutex<Option<String>> = Mutex::new(None);

type ThemeCache = Option<(Vec<Stamp>, Vec<ThemeInfo>)>;
static THEMES: Mutex<ThemeCache> = Mutex::new(None);

/// Installed themes, rescanned only when an icon directory changed.
fn themes() -> Vec<ThemeInfo> {
    let roots = Roots::from_env();
    let stamps = roots.stamps();
    let mut g = THEMES.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        Some((s, t)) if *s == stamps => t.clone(),
        _ => {
            let t = resolve::list_themes(&roots);
            *g = Some((stamps, t.clone()));
            t
        }
    }
}

fn cfg_path(m: &Model) -> std::path::PathBuf {
    m.paths.shell_json()
}

/// The id the config selects: [`AUTO`] or an installed theme id.
fn configured(cfg: &IconConfig, themes: &[ThemeInfo]) -> String {
    if cfg.is_auto() {
        AUTO.into()
    } else {
        resolve::find_installed(themes, &cfg.theme).unwrap_or_else(|| AUTO.into())
    }
}

/// Which theme the preview grid should draw for highlight `hl`.
fn preview_theme(hl: &str, themes: &[ThemeInfo], desktop: Option<&str>) -> String {
    if hl == AUTO {
        desktop.and_then(|d| resolve::find_installed(themes, d)).unwrap_or_default()
    } else {
        hl.to_string()
    }
}

pub fn items(m: &Model) -> Vec<Item> {
    let cfg = IconConfig::read(&cfg_path(m));
    let themes = themes();
    let st = live::status();
    let cur = configured(&cfg, &themes);
    let hl = HIGHLIGHT.lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|h| h == AUTO || themes.iter().any(|t| t.id == *h)).unwrap_or_else(|| cur.clone());
    let hl_info = themes.iter().find(|t| t.id == hl);
    let hl_name = if hl == AUTO { "Auto".to_string() } else { hl_info.map(|t| t.name.clone()).unwrap_or_else(|| hl.clone()) };
    let auto_theme = st.desktop.as_deref().and_then(|d| resolve::find_installed(&themes, d));

    let mut out = Vec::new();
    if let Some(p) = &st.problem {
        out.push(Item::Note { text: format!("{p}."), warn: true });
    }
    out.push(Item::Header(format!("Preview: {hl_name}")));
    out.push(Item::IconGrid(Grid { theme: preview_theme(&hl, &themes, st.desktop.as_deref()), title: hl_name.clone() }));
    let can_use = hl != cur && (hl == AUTO || hl_info.is_some_and(|t| t.kind.selectable()));
    out.push(Item::Card(vec![Row::new(if hl == cur { "In use".to_string() } else { format!("Use {hl_name}") })
        .detail(if hl == cur {
            "This is the icon theme the dock, launchers, menu bar and notifications use".to_string()
        } else {
            "Applies to the dock, launchers, menu bar, switchers and notifications at once".to_string()
        })
        .ctl({
            let mut c = Ctl::new("ic:use", Kind::Button { label: "Use this theme".into(), danger: false });
            c.enabled = can_use;
            c
        })]));

    out.push(Item::Header("Icon themes".into()));
    let mut rows = vec![ThemeRow {
        id: AUTO.into(),
        title: "Auto".into(),
        detail: format!(
            "Follow the desktop: {}",
            st.desktop.as_deref().map(|d| if auto_theme.is_some() { d.to_string() } else { format!("{d} (not installed)") }).unwrap_or_else(|| "nothing configured, hicolor".into())
        ),
        current: cur == AUTO,
        highlighted: hl == AUTO,
        selectable: true,
    }];
    for t in &themes {
        rows.push(ThemeRow {
            id: t.id.clone(),
            title: t.name.clone(),
            detail: if t.kind.selectable() {
                if t.comment.is_empty() {
                    t.id.clone()
                } else {
                    format!("{} - {}", t.id, t.comment)
                }
            } else {
                format!("{} - {}: cannot be used for application icons", t.id, t.kind.label())
            },
            current: cur == t.id,
            highlighted: hl == t.id,
            selectable: t.kind.selectable(),
        });
    }
    out.push(Item::IconThemes(rows));

    out.push(Item::Header("Appearance".into()));
    let fb_opts: Vec<(String, String)> =
        std::iter::once(("None".to_string(), "none".to_string())).chain(themes.iter().filter(|t| t.kind.selectable()).map(|t| (t.name.clone(), t.id.clone()))).collect();
    let fb_sel = fb_opts.iter().position(|(_, v)| v.eq_ignore_ascii_case(&cfg.fallback));
    out.push(Item::Card(vec![
        Row::new("Icon size")
            .detail("How much of its square the artwork fills; themes differ in padding")
            .ctl(Ctl::new("ic:scale", Kind::Slider { v: f64::from(cfg.size_scale) * 100.0, min: f64::from(SCALE_MIN) * 100.0, max: f64::from(SCALE_MAX) * 100.0, step: 5.0, unit: " %".into() })),
        Row::new("Tint symbolic icons").detail("Repaint monochrome (symbolic) icons in the theme's text colour").ctl(Ctl::new("ic:tint", Kind::Toggle(cfg.monochrome_tint))),
        Row::new("Fallback theme").detail("Tried after the theme, its parents and hicolor, for icons they lack").ctl(Ctl::new("ic:fallback", Kind::Popup(fb_opts, fb_sel))),
        Row::new("Reset icon settings").detail("Automatic theme, full size, no tint").ctl(Ctl::new("ic:reset", Kind::Button { label: "Reset".into(), danger: false })),
    ]));
    out.push(Item::Note {
        text: "Icon themes are read from ~/.local/share/icons, ~/.icons and the system's icons folders (see docs/icons.md for Arch packages such as papirus-icon-theme). decthemectl icons list, set and status do the same from a terminal.".into(),
        warn: false,
    });
    out
}

fn write(m: &mut Model, cfg: &IconConfig, msg: String) {
    match cfg.write(&cfg_path(m)) {
        Ok(()) => {
            live::poll_now();
            m.dirty = true;
            m.say(msg, false);
        }
        Err(e) => m.say(format!("Not saved: {e}"), true),
    }
}

pub fn apply(m: &mut Model, parts: &[&str], v: CtlVal) {
    let path = cfg_path(m);
    let mut cfg = IconConfig::read(&path);
    match (parts, v) {
        (["l", "icons", "pick", id], CtlVal::Click) => {
            *HIGHLIGHT.lock().unwrap_or_else(|e| e.into_inner()) = Some((*id).to_string());
            m.dirty = true;
        }
        (["ic", "use"], CtlVal::Click) => {
            let hl = HIGHLIGHT.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let Some(hl) = hl else { return };
            let list = themes();
            if hl == AUTO {
                cfg.theme = AUTO.into();
            } else if let Some(t) = list.iter().find(|t| t.id == hl).filter(|t| t.kind.selectable()) {
                cfg.theme = t.id.clone();
            } else {
                m.say("That theme has no application icons", true);
                return;
            }
            let msg = format!("Icon theme: {}", if cfg.is_auto() { "Auto" } else { &cfg.theme });
            write(m, &cfg, msg);
        }
        (["ic", "scale"], CtlVal::Num(n)) => {
            cfg.size_scale = (n / 100.0).clamp(f64::from(SCALE_MIN), f64::from(SCALE_MAX)) as f32;
            let msg = format!("Icon size: {:.0} %", n.round());
            write(m, &cfg, msg);
        }
        (["ic", "scale"], CtlVal::Reset) => {
            cfg.size_scale = 1.0;
            write(m, &cfg, "Icon size: 100 %".into());
        }
        (["ic", "tint"], CtlVal::Bool(b)) => {
            cfg.monochrome_tint = b;
            write(m, &cfg, format!("Symbolic icon tint: {}", if b { "on" } else { "off" }));
        }
        (["ic", "fallback"], CtlVal::Str(s)) => {
            cfg.fallback = s;
            let msg = format!("Fallback theme: {}", cfg.fallback);
            write(m, &cfg, msg);
        }
        (["ic", "fallback"], CtlVal::Reset) => {
            cfg.fallback = DEFAULT_FALLBACK.into();
            write(m, &cfg, format!("Fallback theme: {DEFAULT_FALLBACK}"));
        }
        (["ic", "reset"], CtlVal::Click) => match IconConfig::reset(&path) {
            Ok(()) => {
                *HIGHLIGHT.lock().unwrap_or_else(|e| e.into_inner()) = None;
                live::poll_now();
                m.dirty = true;
                m.say("Icon settings reset", false);
            }
            Err(e) => m.say(format!("Not saved: {e}"), true),
        },
        _ => {}
    }
}

// ---------------------------------------------------------------- drawing (called from ui::draw_items)

const ROW_H: f32 = 46.0;

/// The list of themes: one card, a row each; returns the y below it.
pub fn draw_list(pm: &mut Pixmap, p: &mut Painter, rows: &[ThemeRow], x0: f32, y: f32, cw: f32, it: &Interact, fr: &mut Frame) -> f32 {
    let s = p.s;
    let u = |v: f32| v * s;
    let h = rows.len() as f32 * u(ROW_H);
    fill_rounded(pm, x0, y, cw, h, u(10.0), pal::card());
    for (i, r) in rows.iter().enumerate() {
        let ry = y + i as f32 * u(ROW_H);
        let id = format!("l:icons:pick:{}", r.id);
        let rect = (x0, ry, cw, u(ROW_H));
        if r.highlighted {
            crate::gfx::fill_rect(pm, x0 + u(2.0), ry, cw - u(4.0), u(ROW_H), crate::theme::th().accent.color(0.22));
        } else if it.hover.as_deref() == Some(id.as_str()) {
            crate::gfx::fill_rect(pm, x0 + u(2.0), ry, cw - u(4.0), u(ROW_H), crate::theme::th().ink(0.06));
        }
        if i > 0 {
            crate::gfx::fill_rect(pm, x0 + u(14.0), ry, cw - u(14.0), u(1.0).max(1.0), pal::sep());
        }
        let status = if r.current {
            "In use"
        } else if !r.selectable {
            "Not selectable"
        } else {
            ""
        };
        let right = x0 + cw - u(14.0);
        let sw = p.measure(status, u(12.0), Weight::Medium);
        if !status.is_empty() {
            let c = if r.current { pal::accent() } else { pal::faint() };
            p.text_right(pm, status, right, ry + u(ROW_H) / 2.0, u(12.0), Weight::Medium, c);
        }
        let avail = (right - sw - x0 - u(34.0)).max(u(60.0));
        let (tc, dc) = if r.selectable { (pal::text(), pal::sub()) } else { (pal::faint(), pal::faint()) };
        let t = p.fit(&r.title, avail, u(13.0), Weight::Regular);
        p.text(pm, &t, x0 + u(14.0), ry + u(16.0), u(13.0), Weight::Regular, tc);
        let d = p.fit(&r.detail, avail, u(11.0), Weight::Regular);
        p.text(pm, &d, x0 + u(14.0), ry + u(32.0), u(11.0), Weight::Regular, dc);
        if it.focus.as_deref() == Some(id.as_str()) && it.ring {
            stroke_rounded(pm, x0 + u(1.0), ry + u(1.0), cw - u(2.0), u(ROW_H) - u(2.0), u(6.0), crate::theme::th().accent.color(0.85), u(2.0));
        }
        fr.layouts.push((rect, id.clone()));
        fr.placed.push(Placed { rect, ctl: Ctl::new(id, Kind::Button { label: r.title.clone(), danger: false }), parts: vec![], row_rect: rect, label: r.title.clone() });
    }
    y + h + u(18.0)
}

/// The 4 x 4 grid of sample icons; returns the y below it.
pub fn draw_grid(pm: &mut Pixmap, p: &mut Painter, g: &Grid, x0: f32, y: f32, cw: f32) -> f32 {
    let s = p.s;
    let u = |v: f32| v * s;
    let cols = 4usize;
    let px = (48.0 * s).round().max(16.0) as u32;
    let cell_w = cw / cols as f32;
    let cell_h = px as f32 + u(30.0);
    let pad = u(10.0);
    let rows = preview::SAMPLES.len().div_ceil(cols);
    let h = rows as f32 * cell_h + 2.0 * pad;
    fill_rounded(pm, x0, y, cw, h, u(10.0), pal::card());
    for (i, (caption, name)) in preview::SAMPLES.iter().enumerate() {
        let (col, row) = ((i % cols) as f32, (i / cols) as f32);
        let cx = x0 + col * cell_w + cell_w / 2.0;
        let top = y + pad + row * cell_h + u(4.0);
        let left = (cx - px as f32 / 2.0).round();
        let top = top.round();
        match preview::icon(Some(&g.theme), name, px) {
            Some(icon) => pm.draw_pixmap(left as i32, top as i32, icon.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None),
            None => {
                let m = px as f32 * 0.14;
                fill_rounded(pm, left + m, top + m, px as f32 - 2.0 * m, px as f32 - 2.0 * m, u(8.0), crate::theme::th().ink(0.07));
                p.text_center(pm, "-", cx, top + px as f32 / 2.0, u(16.0), Weight::Regular, pal::faint());
            }
        }
        p.text_center(pm, caption, cx, top + px as f32 + u(14.0), u(11.0), Weight::Regular, pal::sub());
    }
    y + h + u(14.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::model::tests_support::model_with;
    use std::sync::Mutex as StdMutex;

    // The pane keeps its highlight in a process-wide slot and writes the real decebal.json of the model's
    // sandbox: the tests of this module take turns.
    static LOCK: StdMutex<()> = StdMutex::new(());

    fn card_ctl<'a>(items: &'a [Item], id: &str) -> Option<&'a Ctl> {
        items.iter().find_map(|i| if let Item::Card(rows) = i { rows.iter().flat_map(|r| r.ctls.iter()).find(|c| c.id == id) } else { None })
    }

    #[test]
    fn pane_lists_auto_first_and_controls_exist() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (m, _) = model_with("");
        let its = items(&m);
        let Some(Item::IconThemes(rows)) = its.iter().find(|i| matches!(i, Item::IconThemes(_))) else { panic!("no list") };
        assert_eq!(rows[0].id, AUTO);
        assert!(rows[0].current && rows[0].highlighted, "a fresh config is Auto");
        for id in ["ic:use", "ic:scale", "ic:tint", "ic:fallback", "ic:reset"] {
            assert!(card_ctl(&its, id).is_some(), "{id}");
        }
        assert!(its.iter().any(|i| matches!(i, Item::IconGrid(_))));
        assert!(!card_ctl(&its, "ic:use").unwrap().enabled, "nothing to apply while the highlighted one is in use");
    }

    #[test]
    fn controls_write_the_icons_object_and_reset_clears_it() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut m, _) = model_with("");
        let path = m.paths.shell_json();
        m.apply("ic:scale", CtlVal::Num(85.0));
        m.apply("ic:tint", CtlVal::Bool(true));
        m.apply("ic:fallback", CtlVal::Str("none".into()));
        let c = IconConfig::read(&path);
        assert_eq!((c.size_scale, c.monochrome_tint, c.fallback.as_str()), (0.85, true, "none"));
        m.apply("ic:scale", CtlVal::Num(500.0));
        assert_eq!(IconConfig::read(&path).size_scale, SCALE_MAX, "clamped");
        // the slider reflects the file
        let its = items(&m);
        assert!(matches!(&card_ctl(&its, "ic:tint").unwrap().kind, Kind::Toggle(true)));
        m.apply("ic:reset", CtlVal::Click);
        assert_eq!(IconConfig::read(&path), IconConfig::default());
        let doc = crate::settings::io::json_load(&path);
        assert!(doc.get("icons").is_none());
    }

    #[test]
    fn picking_highlights_and_use_applies_only_selectable_themes() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut m, _) = model_with("");
        let list = themes();
        let Some(good) = list.iter().find(|t| t.kind.selectable()) else { return }; // a machine without any icon theme
        m.apply("ic:reset", CtlVal::Click);
        m.apply(&format!("l:icons:pick:{}", good.id), CtlVal::Click);
        let its = items(&m);
        let Some(Item::IconThemes(rows)) = its.iter().find(|i| matches!(i, Item::IconThemes(_))) else { panic!() };
        assert!(rows.iter().find(|r| r.id == good.id).unwrap().highlighted && rows[0].current);
        assert!(card_ctl(&its, "ic:use").unwrap().enabled);
        m.apply("ic:use", CtlVal::Click);
        assert_eq!(IconConfig::read(&m.paths.shell_json()).theme, good.id);
        if let Some(bad) = list.iter().find(|t| !t.kind.selectable()) {
            m.apply(&format!("l:icons:pick:{}", bad.id), CtlVal::Click);
            let its = items(&m);
            assert!(!card_ctl(&its, "ic:use").unwrap().enabled, "{} cannot be used", bad.id);
            m.apply("ic:use", CtlVal::Click);
            assert_eq!(IconConfig::read(&m.paths.shell_json()).theme, good.id, "refused");
        }
        m.apply("ic:reset", CtlVal::Click);
    }
}
