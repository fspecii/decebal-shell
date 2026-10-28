//! Drawing the lock screen's centre panel (clock, date, avatar, name, password field, status line) with the shared
//! theme: accent, danger and warning colours, corner radius and the display / interface fonts. The backdrop is
//! already in the pixmap when [`Painter::draw`] is called; only the panel's own pixels are touched.

use crate::gfx::{color, fill_circle, fill_rounded, stroke_rounded};
use crate::menubar::clock::Tm;
use crate::text::{Fonts, Role, Weight};
use crate::theme::Rgb;
use std::path::Path;
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap, PixmapPaint, Transform};

use super::state::{Fault, LockState, Phase};

/// Everything the panel shows, copied out of the state so drawing needs no locks and is easy to test.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub time: Option<Tm>,
    pub hour12: bool,
    pub dots: usize,
    pub phase: ViewPhase,
    pub fault: Fault,
    pub caps: bool,
    /// Horizontal offset of the field (the wrong-password shake), in unscaled pixels.
    pub shake_dx: f32,
    pub frozen_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPhase {
    Idle,
    Checking,
    Wrong,
    Frozen,
}

/// Horizontal swing of the shake at its start, unscaled pixels.
const SHAKE_AMPLITUDE: f32 = 14.0;

impl View {
    pub fn from_state(st: &LockState, time: Option<Tm>, hour12: bool, now: std::time::Instant) -> View {
        let shake_dx = st.shake(now).map_or(0.0, shake_offset);
        View {
            time,
            hour12,
            dots: st.dots(),
            phase: match st.phase() {
                Phase::Checking => ViewPhase::Checking,
                Phase::Wrong { .. } => ViewPhase::Wrong,
                Phase::Frozen { .. } => ViewPhase::Frozen,
                Phase::Idle | Phase::Unlocked => ViewPhase::Idle,
            },
            fault: st.fault(),
            caps: st.caps,
            shake_dx,
            frozen_secs: st.frozen_secs(now).unwrap_or(0),
        }
    }

    /// The line under the field: text and the colour role to draw it in.
    pub fn status(&self) -> Option<(String, StatusTone)> {
        match self.phase {
            ViewPhase::Checking => Some(("Checking\u{2026}".into(), StatusTone::Neutral)),
            ViewPhase::Wrong | ViewPhase::Frozen => {
                let base = match self.fault {
                    Fault::WrongPassword => "Wrong password",
                    Fault::AuthUnavailable => "Authentication unavailable",
                };
                Some((if self.phase == ViewPhase::Frozen { format!("{base}  \u{b7}  try again in {} s", self.frozen_secs.max(1)) } else { base.into() }, StatusTone::Bad))
            }
            ViewPhase::Idle if self.caps => Some(("Caps Lock is on".into(), StatusTone::Warn)),
            ViewPhase::Idle => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Bad,
    Warn,
}

/// Damped oscillation: `t` runs 0..1 over the shake.
pub fn shake_offset(t: f32) -> f32 {
    SHAKE_AMPLITUDE * (1.0 - t).powi(2) * (t * std::f32::consts::TAU * 3.0).sin()
}

/// Panel size in pixels at `scale` (1.0 = a 1080p screen).
pub fn panel_size(scale: f32) -> (u32, u32) {
    ((560.0 * scale).round() as u32, (540.0 * scale).round() as u32)
}

/// UI scale for a monitor of the given height.
pub fn scale_for(monitor_h: u32) -> f32 {
    (monitor_h as f32 / 1080.0).clamp(0.75, 2.5)
}

const FG: (u8, u8, u8) = (247, 247, 250);

pub struct Painter {
    fonts: Option<Fonts>,
    scale: f32,
    name: String,
    initial: char,
    avatar: Option<Pixmap>,
}

impl Painter {
    /// `face`: the user's picture (`~/.face`), if any; without one an accent-coloured disc with the initial is drawn.
    pub fn new(scale: f32, display_name: &str, face: Option<&Path>) -> Painter {
        let initial = display_name.chars().find(|c| c.is_alphanumeric()).map_or('?', |c| c.to_uppercase().next().unwrap_or(c));
        let d = (96.0 * scale).round() as u32;
        let avatar = face.and_then(|p| load_avatar(p, d));
        Painter { fonts: Fonts::load(), scale, name: display_name.to_string(), initial, avatar }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    fn measure(&self, role: Role, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure_as(role, s, px, w))
    }

    #[allow(clippy::too_many_arguments)]
    fn text_center(&mut self, pm: &mut Pixmap, role: Role, s: &str, cx: f32, y: f32, px: f32, w: Weight, c: tiny_skia::Color) {
        let tw = self.measure(role, s, px, w);
        if let Some(f) = &mut self.fonts {
            f.draw_as(role, pm, s, (cx - tw / 2.0).round(), y, px, w, c);
        }
    }

    /// Draws the panel over the backdrop already in `pm`.
    pub fn draw(&mut self, pm: &mut Pixmap, v: &View) {
        let th = crate::theme::th();
        let s = self.scale;
        let cx = pm.width() as f32 / 2.0;
        let fg = |a: f32| color(FG.0, FG.1, FG.2, a);

        // clock and date
        if let Some(tm) = v.time {
            let (time, date) = lines(&tm, v.hour12);
            self.text_center(pm, Role::Display, &time, cx, 0.0, 136.0 * s, Weight::Medium, fg(0.96));
            self.text_center(pm, Role::Ui, &date, cx, 160.0 * s, 26.0 * s, Weight::Regular, fg(0.72));
        }

        // avatar
        let d = 96.0 * s;
        let (ax, ay) = (cx - d / 2.0, 244.0 * s);
        match &self.avatar {
            Some(a) => {
                pm.draw_pixmap(ax.round() as i32, ay.round() as i32, a.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            }
            None => {
                fill_circle(pm, cx, ay + d / 2.0, d / 2.0, th.accent_c());
                let init = self.initial.to_string();
                let px = 44.0 * s;
                let col = th.accent_text_c();
                self.text_center(pm, Role::Ui, &init, cx, ay + d / 2.0 - px * 0.62, px, Weight::Medium, col);
            }
        }
        ring(pm, cx, ay + d / 2.0, d / 2.0, 2.0 * s, fg(0.35));

        // user name (ellipsised to the field width)
        let fw = 340.0 * s;
        let name = ellipsize(&self.name, |t| self.measure(Role::Ui, t, 22.0 * s, Weight::Medium), fw);
        self.text_center(pm, Role::Ui, &name, cx, 352.0 * s, 22.0 * s, Weight::Medium, fg(0.92));

        // password field
        let (fh, fy) = (54.0 * s, 398.0 * s);
        let fx = cx - fw / 2.0 + v.shake_dx * s;
        let radius = (th.r(2.0) * s).min(fh / 2.0);
        fill_rounded(pm, fx, fy, fw, fh, radius, fg(0.13));
        let bad = matches!(v.phase, ViewPhase::Wrong | ViewPhase::Frozen);
        let border = if bad { th.danger.color(0.95) } else { th.accent.color(0.9) };
        stroke_rounded(pm, fx + 1.0, fy + 1.0, fw - 2.0, fh - 2.0, (radius - 1.0).max(0.0), border, 2.0 * s);
        let mid_y = fy + fh / 2.0;
        if v.dots == 0 && v.phase != ViewPhase::Checking {
            let px = 17.0 * s;
            self.text_center(pm, Role::Ui, "Password", cx + v.shake_dx * s, mid_y - px * 0.62, px, Weight::Regular, fg(0.45));
        } else {
            let (step, r) = (18.0 * s, 5.5 * s);
            let room = ((fw - 44.0 * s) / step).floor().max(1.0) as usize;
            let shown = v.dots.min(room);
            let x0 = cx + v.shake_dx * s - (shown as f32 - 1.0) * step / 2.0;
            for i in 0..shown {
                fill_circle(pm, x0 + i as f32 * step, mid_y, r, fg(0.95));
            }
        }

        // status line
        if let Some((text, tone)) = v.status() {
            let c = match tone {
                StatusTone::Neutral => fg(0.65),
                StatusTone::Bad => readable(th.danger),
                StatusTone::Warn => readable(th.warn),
            };
            self.text_center(pm, Role::Ui, &text, cx, fy + fh + 16.0 * s, 16.0 * s, Weight::Medium, c);
        }
        if let Some(f) = &self.fonts {
            f.trim(); // fontdue faces are big: drop them between frames, the glyph cache keeps the pixels
        }
    }
}

/// A theme's danger / warning colour can be too dark for the dim backdrop: mix it towards white until it reads.
fn readable(c: Rgb) -> tiny_skia::Color {
    let bg = Rgb(12, 12, 18);
    let mut c = c;
    for _ in 0..6 {
        if crate::theme::model::contrast(c, bg) >= 4.5 {
            break;
        }
        c = c.over(Rgb(255, 255, 255), 0.18);
    }
    c.color(1.0)
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// The big time and the date line under it: `09:05` / `Wednesday, 28 October` (12-hour: `9:05`).
pub fn lines(tm: &Tm, hour12: bool) -> (String, String) {
    let time = if hour12 {
        let h = tm.hour % 12;
        format!("{}:{:02}", if h == 0 { 12 } else { h }, tm.min)
    } else {
        format!("{:02}:{:02}", tm.hour, tm.min)
    };
    (time, format!("{}, {} {}", WEEKDAYS[(tm.wday % 7) as usize], tm.mday, MONTHS[(tm.mon % 12) as usize]))
}

fn ring(pm: &mut Pixmap, cx: f32, cy: f32, r: f32, w: f32, c: tiny_skia::Color) {
    let Some(path) = PathBuilder::from_circle(cx, cy, r) else { return };
    let mut p = tiny_skia::Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    pm.stroke_path(&path, &p, &tiny_skia::Stroke { width: w, ..Default::default() }, Transform::identity(), None);
}

/// Shortens `s` with an ellipsis until `measure` fits `max`.
pub fn ellipsize(s: &str, measure: impl Fn(&str) -> f32, max: f32) -> String {
    if measure(s) <= max {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let t: String = chars.iter().collect::<String>() + "\u{2026}";
        if measure(&t) <= max {
            return t;
        }
    }
    "\u{2026}".into()
}

/// The picture at `path` as a `d` x `d` disc.
fn load_avatar(path: &Path, d: u32) -> Option<Pixmap> {
    let img = super::bg::open_image(path)?.to_rgba8();
    let (iw, ih) = img.dimensions();
    let side = iw.min(ih);
    if side == 0 {
        return None;
    }
    let sq = image::imageops::crop_imm(&img, (iw - side) / 2, (ih - side) / 2, side, side).to_image();
    let sq = image::imageops::resize(&sq, d, d, image::imageops::FilterType::Lanczos3);
    let mut src = Pixmap::new(d, d)?;
    for (dst, p) in src.data_mut().chunks_exact_mut(4).zip(sq.pixels()) {
        let a = u32::from(p.0[3]);
        for (d, s) in dst.iter_mut().zip(p.0).take(3) {
            *d = (u32::from(s) * a / 255) as u8;
        }
        dst[3] = p.0[3];
    }
    let mut mask = Mask::new(d, d)?;
    mask.fill_path(&PathBuilder::from_circle(d as f32 / 2.0, d as f32 / 2.0, d as f32 / 2.0)?, FillRule::Winding, true, Transform::identity());
    let mut out = Pixmap::new(d, d)?;
    out.draw_pixmap(0, 0, src.as_ref(), &PixmapPaint::default(), Transform::identity(), Some(&mask));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn view() -> View {
        View {
            time: Some(Tm { wday: 3, mday: 28, mon: 9, hour: 9, min: 5, sec: 0 }),
            hour12: false,
            dots: 0,
            phase: ViewPhase::Idle,
            fault: Fault::WrongPassword,
            caps: false,
            shake_dx: 0.0,
            frozen_secs: 0,
        }
    }

    fn render(v: &View) -> Pixmap {
        let mut p = Painter::new(1.0, "Alice Example", None);
        let (w, h) = panel_size(1.0);
        let mut pm = Pixmap::new(w, h).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(20, 22, 30, 255));
        p.draw(&mut pm, v);
        pm
    }

    fn differs(a: &Pixmap, b: &Pixmap) -> usize {
        a.data().iter().zip(b.data()).filter(|(x, y)| x != y).count()
    }

    #[test]
    fn clock_lines() {
        let tm = Tm { wday: 3, mday: 28, mon: 9, hour: 0, min: 5, sec: 0 };
        assert_eq!(lines(&tm, false), ("00:05".into(), "Wednesday, 28 October".into()));
        assert_eq!(lines(&tm, true).0, "12:05");
        assert_eq!(lines(&Tm { hour: 15, ..tm }, true).0, "3:05");
    }

    #[test]
    fn status_messages() {
        let mut v = view();
        assert_eq!(v.status(), None);
        v.caps = true;
        assert_eq!(v.status(), Some(("Caps Lock is on".into(), StatusTone::Warn)));
        v.phase = ViewPhase::Wrong;
        assert_eq!(v.status().unwrap().0, "Wrong password");
        v.phase = ViewPhase::Frozen;
        v.frozen_secs = 3;
        assert!(v.status().unwrap().0.ends_with("try again in 3 s"));
        v.fault = Fault::AuthUnavailable;
        assert!(v.status().unwrap().0.starts_with("Authentication unavailable"));
        v.phase = ViewPhase::Checking;
        assert_eq!(v.status().unwrap().1, StatusTone::Neutral);
    }

    #[test]
    fn shake_swings_and_dies_out() {
        assert!(shake_offset(0.0).abs() < 1e-3);
        let peak = (1..100).map(|i| shake_offset(i as f32 / 100.0).abs()).fold(0.0, f32::max);
        assert!(peak > 5.0 && peak <= SHAKE_AMPLITUDE);
        assert!(shake_offset(0.999).abs() < 0.1);
    }

    #[test]
    fn scale_is_clamped() {
        assert_eq!(scale_for(1080), 1.0);
        assert_eq!(scale_for(2160), 2.0);
        assert_eq!(scale_for(300), 0.75);
        assert_eq!(scale_for(10000), 2.5);
        assert_eq!(panel_size(2.0), (1120, 1080));
    }

    #[test]
    fn dots_wrong_state_and_caps_change_the_picture() {
        let idle = render(&view());
        let mut typed = view();
        typed.dots = 6;
        let typed_pm = render(&typed);
        assert!(differs(&idle, &typed_pm) > 1000, "dots are drawn");
        let mut more = typed.clone();
        more.dots = 7;
        assert!(differs(&typed_pm, &render(&more)) > 100, "one more dot");
        let mut wrong = view();
        wrong.phase = ViewPhase::Wrong;
        let wrong_pm = render(&wrong);
        assert!(differs(&idle, &wrong_pm) > 1000, "border and message change");
        let mut shaken = wrong.clone();
        shaken.shake_dx = 10.0;
        assert!(differs(&wrong_pm, &render(&shaken)) > 500, "the field moved");
        let mut caps = view();
        caps.caps = true;
        let caps_pm = render(&caps);
        if Fonts::load().is_some() {
            assert!(differs(&idle, &caps_pm) > 200, "the caps hint is drawn");
        }
    }

    #[test]
    fn many_dots_do_not_leave_the_field() {
        let mut v = view();
        v.dots = 400;
        let pm = render(&v);
        let (w, _) = panel_size(1.0);
        // left of the field (x < (560-340)/2 = 110) at the field's height only the backdrop is allowed
        let y = 398 + 27;
        for x in 0..100 {
            let p = pm.pixel(x, y).unwrap();
            assert_eq!((p.red(), p.green(), p.blue()), (20, 22, 30), "x={x} w={w}");
        }
    }

    #[test]
    fn ellipsize_fits() {
        let m = |s: &str| s.chars().count() as f32 * 10.0;
        assert_eq!(ellipsize("short", m, 100.0), "short");
        let e = ellipsize("a very long name indeed", m, 100.0);
        assert!(e.ends_with('\u{2026}') && m(&e) <= 100.0, "{e}");
    }

    #[test]
    fn avatar_is_cropped_to_a_disc() {
        let dir = std::env::temp_dir().join(format!("declock-face-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("face.png");
        image::RgbImage::from_pixel(60, 40, image::Rgb([200, 30, 30])).save(&f).unwrap();
        let a = load_avatar(&f, 40).unwrap();
        assert_eq!(a.pixel(0, 0).unwrap().alpha(), 0, "corner outside the disc");
        assert_eq!(a.pixel(20, 20).unwrap().alpha(), 255);
        assert!(load_avatar(&dir.join("none.png"), 40).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn view_from_state_follows_the_phase() {
        let now = Instant::now();
        let mut st = LockState::new();
        st.input(super::super::state::Input::Char('a'), now);
        let v = View::from_state(&st, None, false, now);
        assert_eq!((v.dots, v.phase), (1, ViewPhase::Idle));
        st.input(super::super::state::Input::Submit, now);
        st.verdict(Ok(false), now);
        let v = View::from_state(&st, None, false, now + Duration::from_millis(60));
        assert_eq!(v.phase, ViewPhase::Wrong);
        assert!(v.shake_dx != 0.0);
    }
}
