//! Where a launcher style goes on the screen, and the command line the panel's launcher applet uses to say where it is.
//!
//! `declaunch [toggle|open|close] [--edge top|bottom|left|right] [--anchor X,Y] [--thickness N] [--output NAME]`
//!
//! X, Y and N are in the panel window's own coordinate space: X11 root-window coordinates, or output-local surface
//! coordinates on Wayland. [`resolve_anchor`] turns them into surface pixels of the launcher; [`placement`] then picks
//! the card rectangle. Both are pure so every style x edge x scale x screen size is unit-tested.

use super::style::{LauncherStyle, Placement};
pub use crate::window_app::Edge;

/// What the invocation asks of a running launcher (the singleton lifecycle in [`crate::single`] acts on it).
pub use crate::single::Verb;

/// A parsed `declaunch` command line. Unknown or malformed arguments are kept in `ignored`, never fatal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Invocation {
    pub verb: Verb,
    pub edge: Option<Edge>,
    /// The applet's centre (see the module docs for the coordinate space).
    pub point: Option<(f32, f32)>,
    /// The panel's thickness, same space.
    pub thickness: Option<f32>,
    pub output: Option<String>,
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Run(Invocation),
    Help,
    Version,
}

fn number(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Parses the arguments after the program name.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Parsed {
    let mut inv = Invocation::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let value = |args: &mut I::IntoIter| inline.clone().or_else(|| args.next());
        match flag.as_str() {
            "-h" | "--help" => return Parsed::Help,
            "-V" | "--version" => return Parsed::Version,
            "toggle" => inv.verb = Verb::Toggle,
            "open" => inv.verb = Verb::Open,
            "close" => inv.verb = Verb::Close,
            "--edge" => match value(&mut args) {
                Some(v) => match Edge::parse(v.trim()) {
                    Some(e) => inv.edge = Some(e),
                    None => inv.ignored.push(format!("--edge {v}")),
                },
                None => inv.ignored.push(arg),
            },
            "--anchor" => match value(&mut args) {
                Some(v) => match v.split_once(',').and_then(|(x, y)| Some((number(x)?, number(y)?))) {
                    Some(p) => inv.point = Some(p),
                    None => inv.ignored.push(format!("--anchor {v}")),
                },
                None => inv.ignored.push(arg),
            },
            "--thickness" => match value(&mut args) {
                Some(v) => match number(&v).filter(|n| *n >= 0.0) {
                    Some(n) => inv.thickness = Some(n),
                    None => inv.ignored.push(format!("--thickness {v}")),
                },
                None => inv.ignored.push(arg),
            },
            "--output" => match value(&mut args) {
                Some(v) if !v.trim().is_empty() => inv.output = Some(v.trim().to_string()),
                _ => inv.ignored.push(arg),
            },
            _ => inv.ignored.push(arg),
        }
    }
    Parsed::Run(inv)
}

/// The panel the launcher was opened from, in the launcher's surface pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub edge: Edge,
    /// The applet's centre; `None` when only the edge is known (the middle of that edge is used).
    pub point: Option<(f32, f32)>,
    /// The panel's thickness: the strip along `edge` the launcher must leave uncovered.
    pub thickness: f32,
}

/// A rectangle in surface pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// Converts the invocation's panel description into surface pixels: `origin` is the top-left of the launcher's
/// monitor in the panel's coordinate space (the X11 monitor position; zero on Wayland), `screen` its size.
/// Without an edge the nearest screen edge to the point is used; without a thickness the point is taken to be in the
/// middle of the panel (twice its distance from the edge). `None` when the invocation names no panel at all.
pub fn resolve_anchor(inv: &Invocation, origin: (f32, f32), screen: (f32, f32)) -> Option<Anchor> {
    let point = inv.point.map(|(x, y)| ((x - origin.0).clamp(0.0, screen.0), (y - origin.1).clamp(0.0, screen.1)));
    let edge = inv.edge.or_else(|| {
        let (x, y) = point?;
        let d = [(y, Edge::Top), (screen.1 - y, Edge::Bottom), (x, Edge::Left), (screen.0 - x, Edge::Right)];
        d.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, e)| e)
    })?;
    let thickness = inv.thickness.unwrap_or_else(|| {
        point.map_or(0.0, |(x, y)| {
            2.0 * match edge {
                Edge::Top => y,
                Edge::Bottom => screen.1 - y,
                Edge::Left => x,
                Edge::Right => screen.0 - x,
            }
        })
    });
    let limit = match edge {
        Edge::Top | Edge::Bottom => screen.1 / 3.0,
        Edge::Left | Edge::Right => screen.0 / 3.0,
    };
    Some(Anchor { edge, point, thickness: thickness.clamp(0.0, limit) })
}

/// The size a style would like in logical pixels, its gap to the panel and the panel it assumes without an anchor.
struct Natural {
    size: (f32, f32),
    gap: f32,
    fallback: Option<(Edge, f32)>,
}

fn natural(style: LauncherStyle) -> Natural {
    match style {
        // Windows 11: 642 x 726 epx above the 48 epx taskbar, a 12 epx gap.
        LauncherStyle::Windows => Natural { size: (642.0, 726.0), gap: 12.0, fallback: Some((Edge::Bottom, 48.0)) },
        // Plasma: Kickoff opens from a 44 px bottom panel's corner button; 655 x 516 is its size in the identified original capture
        // (docs/plasma-theme-ports/kickoff-fidelity.md), wide enough for the footer's captioned power buttons.
        LauncherStyle::Kde => Natural { size: (655.0, 516.0), gap: 4.0, fallback: Some((Edge::Bottom, 44.0)) },
        // Xfce: Whisker Menu's default menu-width 450 x menu-height 500, under the top panel's Applications button.
        LauncherStyle::Xfce => Natural { size: (450.0, 500.0), gap: 2.0, fallback: Some((Edge::Top, 28.0)) },
        LauncherStyle::Dux => Natural { size: (620.0, 620.0), gap: 0.0, fallback: None },
        LauncherStyle::Default | LauncherStyle::Gnome | LauncherStyle::BlackBerry => Natural { size: (1100.0, 720.0), gap: 0.0, fallback: None },
    }
}

/// Distance popups keep from the screen edges, logical pixels.
pub const MARGIN: f32 = 8.0;

/// The free part of the screen once the panel strut is taken off.
pub fn free_area(anchor: Option<&Anchor>, screen: (f32, f32)) -> Rect {
    let full = Rect { x: 0.0, y: 0.0, w: screen.0, h: screen.1 };
    let Some(a) = anchor else { return full };
    let t = a.thickness;
    match a.edge {
        Edge::Top => Rect { y: t, h: (screen.1 - t).max(1.0), ..full },
        Edge::Bottom => Rect { h: (screen.1 - t).max(1.0), ..full },
        Edge::Left => Rect { x: t, w: (screen.0 - t).max(1.0), ..full },
        Edge::Right => Rect { w: (screen.0 - t).max(1.0), ..full },
    }
}

/// The launcher card for `style` on a surface `screen` pixels big at UI `scale`, opened from `anchor` (or from the
/// style's natural place when `None`). The result always lies inside the screen and never covers the panel strip.
pub fn placement(style: LauncherStyle, anchor: Option<Anchor>, screen: (f32, f32), scale: f32) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let screen = (screen.0.max(1.0), screen.1.max(1.0));
    let nat = natural(style);
    match style.capabilities().placement {
        Placement::FullScreen => free_area(anchor.as_ref(), screen),
        Placement::Centered => {
            let free = free_area(anchor.as_ref(), screen);
            // The original looks keep their own sizing: Launchpad leaves 40/60 px around, Dux 50 px.
            let (w, h) = match style {
                LauncherStyle::Dux => ((free.w - 100.0).clamp(1.0, nat.size.0), (free.h - 100.0).clamp(1.0, nat.size.1)),
                _ => ((free.w - 80.0).clamp(free.w.min(300.0), nat.size.0), (free.h - 120.0).clamp(free.h.min(300.0), nat.size.1)),
            };
            Rect { x: (free.x + (free.w - w) / 2.0).round(), y: (free.y + (free.h - h) / 2.0).round(), w: w.round(), h: h.round() }
        }
        Placement::Anchored => {
            let a = anchor.or_else(|| nat.fallback.map(|(edge, t)| Anchor { edge, point: None, thickness: t * scale })).unwrap_or(Anchor { edge: Edge::Bottom, point: None, thickness: 0.0 });
            let free = free_area(Some(&a), screen);
            let m = MARGIN * scale;
            let gap = nat.gap * scale;
            // Leave the margin on the three open sides and the gap towards the panel; never below 1 px.
            let (w, h) = match a.edge {
                Edge::Top | Edge::Bottom => ((nat.size.0 * scale).min(free.w - 2.0 * m), (nat.size.1 * scale).min(free.h - gap - m)),
                Edge::Left | Edge::Right => ((nat.size.0 * scale).min(free.w - gap - m), (nat.size.1 * scale).min(free.h - 2.0 * m)),
            };
            let (w, h) = (w.max(1.0).round(), h.max(1.0).round());
            let centre = a.point.unwrap_or((screen.0 / 2.0, screen.1 / 2.0));
            // Centred on the applet along the panel, kept on screen.
            let along = |c: f32, size: f32, lo: f32, len: f32| {
                let (min, max) = (lo + m, lo + len - size - m);
                if max < min {
                    lo + (len - size) / 2.0
                } else {
                    (c - size / 2.0).clamp(min, max)
                }
            };
            let (x, y) = match a.edge {
                Edge::Bottom => (along(centre.0, w, free.x, free.w), free.bottom() - gap - h),
                Edge::Top => (along(centre.0, w, free.x, free.w), free.y + gap),
                Edge::Left => (free.x + gap, along(centre.1, h, free.y, free.h)),
                Edge::Right => (free.right() - gap - w, along(centre.1, h, free.y, free.h)),
            };
            Rect { x: x.max(free.x).round(), y: y.max(free.y).round(), w, h }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::style::ALL;

    fn args(s: &str) -> Parsed {
        parse_args(s.split_whitespace().map(String::from))
    }

    fn run(s: &str) -> Invocation {
        match args(s) {
            Parsed::Run(i) => i,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_the_panel_contract() {
        let i = run("toggle --edge bottom --anchor 960.5,1060 --thickness 48 --output DP-1");
        assert_eq!(i, Invocation { verb: Verb::Toggle, edge: Some(Edge::Bottom), point: Some((960.5, 1060.0)), thickness: Some(48.0), output: Some("DP-1".into()), ignored: vec![] });
        let i = run("open --edge=left --anchor=20,300 --thickness=40");
        assert_eq!((i.verb, i.edge, i.point, i.thickness), (Verb::Open, Some(Edge::Left), Some((20.0, 300.0)), Some(40.0)));
        assert_eq!(run("close").verb, Verb::Close);
        assert_eq!(run(""), Invocation::default(), "no arguments: toggle at the natural place");
        assert_eq!(args("--help"), Parsed::Help);
        assert_eq!(args("open -V"), Parsed::Version);
    }

    #[test]
    fn unknown_or_malformed_arguments_are_ignored_not_fatal() {
        let i = run("--frobnicate open --edge middle --anchor 1,2,3 --anchor x,y --thickness -4 --thickness nan --output");
        assert_eq!(i.verb, Verb::Open);
        assert_eq!((i.edge, i.point, i.thickness, i.output), (None, None, None, None));
        assert_eq!(i.ignored.len(), 7, "{:?}", i.ignored);
        let i = run("--anchor 5,6 --edge");
        assert_eq!(i.point, Some((5.0, 6.0)));
    }

    #[test]
    fn anchors_infer_edge_and_thickness_and_use_the_monitor_origin() {
        let screen = (1920.0, 1080.0);
        let i = run("--anchor 2944,1056");
        let a = resolve_anchor(&i, (1920.0, 0.0), screen).unwrap();
        assert_eq!((a.edge, a.point, a.thickness), (Edge::Bottom, Some((1024.0, 1056.0)), 48.0));
        let a = resolve_anchor(&run("--edge right --thickness 36"), (0.0, 0.0), screen).unwrap();
        assert_eq!((a.edge, a.point, a.thickness), (Edge::Right, None, 36.0));
        assert_eq!(resolve_anchor(&run("--thickness 30"), (0.0, 0.0), screen), None);
        let a = resolve_anchor(&run("--edge top --thickness 5000"), (0.0, 0.0), screen).unwrap();
        assert_eq!(a.thickness, 360.0, "an absurd thickness cannot swallow the screen");
    }

    const SCREENS: [(f32, f32); 5] = [(1024.0, 600.0), (800.0, 480.0), (1366.0, 768.0), (1920.0, 1080.0), (3840.0, 2160.0)];
    const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    fn strip(a: &Anchor, screen: (f32, f32)) -> Rect {
        let t = a.thickness;
        match a.edge {
            Edge::Top => Rect { x: 0.0, y: 0.0, w: screen.0, h: t },
            Edge::Bottom => Rect { x: 0.0, y: screen.1 - t, w: screen.0, h: t },
            Edge::Left => Rect { x: 0.0, y: 0.0, w: t, h: screen.1 },
            Edge::Right => Rect { x: screen.0 - t, y: 0.0, w: t, h: screen.1 },
        }
    }

    fn overlaps(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    #[test]
    fn every_style_edge_scale_and_screen_stays_on_screen_and_off_the_panel() {
        for style in ALL {
            for screen in SCREENS {
                for scale in SCALES {
                    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                        let t = 32.0 * scale;
                        // the applet near the start, the middle and the end of the panel
                        for along in [0.02, 0.5, 0.98] {
                            let point = match edge {
                                Edge::Top => (screen.0 * along, t / 2.0),
                                Edge::Bottom => (screen.0 * along, screen.1 - t / 2.0),
                                Edge::Left => (t / 2.0, screen.1 * along),
                                Edge::Right => (screen.0 - t / 2.0, screen.1 * along),
                            };
                            let a = Anchor { edge, point: Some(point), thickness: t };
                            let r = placement(style, Some(a), screen, scale);
                            let ctx = format!("{style:?} {edge:?} {screen:?} @{scale} along {along}: {r:?}");
                            assert!(r.w >= 1.0 && r.h >= 1.0, "{ctx}");
                            assert!(r.x >= 0.0 && r.y >= 0.0 && r.right() <= screen.0 + 0.01 && r.bottom() <= screen.1 + 0.01, "off screen: {ctx}");
                            assert!(!overlaps(&r, &strip(&a, screen)), "covers the panel: {ctx}");
                            match style.capabilities().placement {
                                Placement::FullScreen => assert_eq!(r, free_area(Some(&a), screen), "{ctx}"),
                                Placement::Anchored => {
                                    // opens from the panel: the side facing it sits at the style's gap
                                    let gap = natural(style).gap * scale;
                                    let d = match edge {
                                        Edge::Top => r.y - t,
                                        Edge::Bottom => screen.1 - t - r.bottom(),
                                        Edge::Left => r.x - t,
                                        Edge::Right => screen.0 - t - r.right(),
                                    };
                                    assert!((d - gap).abs() <= 1.0, "gap {d} != {gap}: {ctx}");
                                    // and as close to the applet as the screen allows, with the margin kept
                                    let m = MARGIN * scale;
                                    match edge {
                                        Edge::Top | Edge::Bottom => assert!(r.x >= m - 1.0 && r.right() <= screen.0 - m + 1.0, "{ctx}"),
                                        Edge::Left | Edge::Right => assert!(r.y >= m - 1.0 && r.bottom() <= screen.1 - m + 1.0, "{ctx}"),
                                    }
                                }
                                Placement::Centered => {
                                    let free = free_area(Some(&a), screen);
                                    assert!(((r.x + r.w / 2.0) - (free.x + free.w / 2.0)).abs() <= 1.0, "{ctx}");
                                }
                            }
                        }
                    }
                    // no anchor (keyboard shortcut, hot corner): the natural place, still on screen
                    let r = placement(style, None, screen, scale);
                    assert!(r.x >= 0.0 && r.y >= 0.0 && r.right() <= screen.0 + 0.01 && r.bottom() <= screen.1 + 0.01, "{style:?} {screen:?} @{scale}: {r:?}");
                }
            }
        }
    }

    #[test]
    fn natural_places_match_the_originals() {
        let screen = (1920.0, 1080.0);
        let r = placement(LauncherStyle::Windows, None, screen, 1.0);
        assert_eq!(r, Rect { x: 639.0, y: 1080.0 - 48.0 - 12.0 - 726.0, w: 642.0, h: 726.0 }, "centred above the taskbar");
        assert_eq!(placement(LauncherStyle::Gnome, None, screen, 1.0), Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
        let r = placement(LauncherStyle::Xfce, None, screen, 2.0);
        assert_eq!((r.y, r.w, r.h), (60.0, 900.0, 1000.0), "under the top panel, doubled");
        let r = placement(LauncherStyle::Default, None, screen, 1.0);
        assert_eq!((r.w, r.h), (1100.0, 720.0));
    }

    #[test]
    fn anchored_popups_follow_the_applet() {
        let screen = (1920.0, 1080.0);
        let at = |edge, point| Some(Anchor { edge, point: Some(point), thickness: 40.0 });
        let r = placement(LauncherStyle::Kde, at(Edge::Bottom, (24.0, 1060.0)), screen, 1.0);
        assert_eq!((r.x, r.bottom()), (8.0, 1080.0 - 40.0 - 4.0), "a corner applet pins the popup to the margin");
        let r = placement(LauncherStyle::Kde, at(Edge::Bottom, (1000.0, 1060.0)), screen, 1.0);
        // pixel-rounded: an odd card width (Kickoff's 655) centres to within half a pixel
        assert!((r.x + r.w / 2.0 - 1000.0).abs() <= 0.5, "centred on the applet: {r:?}");
        let r = placement(LauncherStyle::Xfce, at(Edge::Left, (20.0, 1070.0)), screen, 1.0);
        assert_eq!((r.x, r.bottom()), (42.0, 1072.0));
        let r = placement(LauncherStyle::Windows, at(Edge::Right, (1900.0, 30.0)), screen, 1.0);
        assert_eq!((r.right(), r.y), (1920.0 - 40.0 - 12.0, 8.0));
        let r = placement(LauncherStyle::Windows, at(Edge::Top, (960.0, 20.0)), (1024.0, 600.0), 1.0);
        assert_eq!((r.y, r.bottom()), (52.0, 592.0), "a small screen shrinks the menu to fit");
    }
}
