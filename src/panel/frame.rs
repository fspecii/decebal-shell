//! Axis-aware panel geometry shared by every renderer: a layout written once in the panel's own frame (`along` the
//! edge, `across` from the screen edge inwards) is mapped onto the surface of any of the four edges. Popups open on the
//! screen side of the strip; the input region keeps a path from the strip to the popup so the pointer never leaves the
//! surface on its way. Text and icons are never rotated: a renderer picks an upright form for the axis (stacked digits,
//! icon cells) instead.

use super::Edge;
use crate::app::InputRect;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn input(&self) -> InputRect {
        InputRect { x: self.x.floor() as i32, y: self.y.floor() as i32, w: (self.right() - self.x.floor()).ceil() as i32, h: (self.bottom() - self.y.floor()).ceil() as i32 }
    }
}

/// A panel surface on its edge: `w` x `h` is the whole surface (strip plus popup room), `thick` the strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub edge: Edge,
    pub w: f32,
    pub h: f32,
    pub thick: f32,
}

/// Gap between the strip and a popup opened from it, and the margin a popup keeps from the surface's ends.
pub const POP_GAP: f32 = 4.0;
pub const POP_MARGIN: f32 = 8.0;

impl Frame {
    pub fn new(edge: Edge, w: f32, h: f32, thick: f32) -> Frame {
        Frame { edge, w, h, thick }
    }
    pub fn vertical(&self) -> bool {
        self.edge.is_vertical()
    }
    /// Length of the strip along its edge.
    pub fn len(&self) -> f32 {
        if self.vertical() {
            self.h
        } else {
            self.w
        }
    }
    /// The coordinate along the strip of a surface point.
    pub fn along(&self, x: f32, y: f32) -> f32 {
        if self.vertical() {
            y
        } else {
            x
        }
    }
    /// Surface coordinate where the strip starts across.
    fn base(&self) -> f32 {
        match self.edge {
            Edge::Top | Edge::Left => 0.0,
            Edge::Bottom => self.h - self.thick,
            Edge::Right => self.w - self.thick,
        }
    }
    /// The strip: what is reserved (the strut) and takes input while nothing is open.
    pub fn strip(&self) -> Rect {
        if self.vertical() {
            Rect { x: self.base(), y: 0.0, w: self.thick, h: self.h }
        } else {
            Rect { x: 0.0, y: self.base(), w: self.w, h: self.thick }
        }
    }
    /// A rectangle of the panel frame (`a` along the strip, `c` across it from the screen edge) on the surface. The
    /// across axis is mirrored for bottom/right panels so "outer edge" stays the screen edge.
    pub fn map(&self, a: f32, c: f32, len: f32, depth: f32) -> Rect {
        match self.edge {
            Edge::Top => Rect { x: a, y: c, w: len, h: depth },
            Edge::Bottom => Rect { x: a, y: self.h - c - depth, w: len, h: depth },
            Edge::Left => Rect { x: c, y: a, w: depth, h: len },
            Edge::Right => Rect { x: self.w - c - depth, y: a, w: depth, h: len },
        }
    }
    /// Top-left of a `pw` x `ph` popup opened from `item`: beside the strip on the screen side, centred on the item
    /// along the strip (`centre`) or aligned with its start (menus), always on the surface.
    pub fn popout(&self, item: Rect, pw: f32, ph: f32, centre: bool) -> (f32, f32) {
        let s = self.strip();
        let clamp = |v: f32, size: f32, room: f32| v.min(room - size - POP_MARGIN).max(POP_MARGIN.min((room - size).max(0.0)));
        match self.edge {
            Edge::Top => (clamp(if centre { item.cx() - pw / 2.0 } else { item.x }, pw, self.w), s.bottom() + POP_GAP),
            Edge::Bottom => (clamp(if centre { item.cx() - pw / 2.0 } else { item.x }, pw, self.w), s.y - POP_GAP - ph),
            Edge::Left => (s.right() + POP_GAP, clamp(if centre { item.cy() - ph / 2.0 } else { item.y }, ph, self.h)),
            Edge::Right => (s.x - POP_GAP - pw, clamp(if centre { item.cy() - ph / 2.0 } else { item.y }, ph, self.h)),
        }
    }
    /// Input region of a popup box plus the corridor back to the strip.
    pub fn reach(&self, x: f32, y: f32, w: f32, h: f32) -> InputRect {
        let s = self.strip();
        let (x0, y0, x1, y1) = match self.edge {
            Edge::Top => (x, s.bottom(), x + w, y + h),
            Edge::Bottom => (x, y, x + w, s.y),
            Edge::Left => (s.right(), y, x + w, y + h),
            Edge::Right => (x, y, s.x, y + h),
        };
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }.input()
    }
}

/// How a bar starts a program that opens beside one of its applets (`declaunch`, `decdash`, `decmon`...): the panel's
/// edge, the applet's centre in global logical pixels and the panel's thickness. Programs that take a position read
/// `--edge E --anchor X,Y --thickness N`; see docs/universal-panels.md.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaunchAnchor {
    pub edge: Edge,
    pub x: f32,
    pub y: f32,
    pub thickness: f32,
}

impl LaunchAnchor {
    pub fn args(&self) -> Vec<String> {
        vec![
            "--edge".into(),
            self.edge.as_str().into(),
            "--anchor".into(),
            format!("{},{}", self.x.round() as i32, self.y.round() as i32),
            "--thickness".into(),
            format!("{}", self.thickness.round() as i32),
        ]
    }
    /// Reads the three flags out of an argument list (missing or malformed ones give `None`).
    pub fn parse(args: &[String]) -> Option<LaunchAnchor> {
        let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
        let edge = Edge::parse(get("--edge")?)?;
        let (x, y) = get("--anchor")?.split_once(',')?;
        let thickness = get("--thickness").and_then(|t| t.parse().ok()).unwrap_or(0.0);
        Some(LaunchAnchor { edge, x: x.trim().parse().ok()?, y: y.trim().parse().ok()?, thickness })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames() -> [Frame; 4] {
        [Frame::new(Edge::Top, 1280.0, 420.0, 26.0), Frame::new(Edge::Bottom, 1280.0, 420.0, 26.0), Frame::new(Edge::Left, 420.0, 800.0, 44.0), Frame::new(Edge::Right, 420.0, 800.0, 44.0)]
    }

    #[test]
    fn the_strip_touches_its_screen_edge() {
        for f in frames() {
            let s = f.strip();
            let touches = match f.edge {
                Edge::Top => s.y == 0.0,
                Edge::Bottom => s.bottom() == f.h,
                Edge::Left => s.x == 0.0,
                Edge::Right => s.right() == f.w,
            };
            assert!(touches, "{:?}", f.edge);
            // the frame's outer row maps onto the strip
            let r = f.map(10.0, 0.0, 20.0, f.thick);
            assert_eq!((r.w * r.h), 20.0 * f.thick);
            assert!(s.contains(r.cx(), r.cy()));
        }
    }

    #[test]
    fn popouts_open_towards_the_screen_and_stay_on_the_surface() {
        for f in frames() {
            for at in [0.0, f.len() / 2.0, f.len() - 20.0] {
                let item = f.map(at, 0.0, 20.0, f.thick);
                let (x, y) = f.popout(item, 300.0, 200.0, true);
                assert!(x >= 0.0 && y >= 0.0 && x + 300.0 <= f.w && y + 200.0 <= f.h, "{:?} at {at}: {x},{y}", f.edge);
                let s = f.strip();
                let clear = match f.edge {
                    Edge::Top => y >= s.bottom(),
                    Edge::Bottom => y + 200.0 <= s.y,
                    Edge::Left => x >= s.right(),
                    Edge::Right => x + 300.0 <= s.x,
                };
                assert!(clear, "{:?}: the popup covers the strip", f.edge);
                // the corridor joins strip and popup
                let r = f.reach(x, y, 300.0, 200.0);
                let (rx, ry) = match f.edge {
                    Edge::Top => (x + 1.0, s.bottom() + 1.0),
                    Edge::Bottom => (x + 1.0, s.y - 1.0),
                    Edge::Left => (s.right() + 1.0, y + 1.0),
                    Edge::Right => (s.x - 1.0, y + 1.0),
                };
                assert!(rx >= r.x as f32 && rx < (r.x + r.w) as f32 && ry >= r.y as f32 && ry < (r.y + r.h) as f32);
            }
        }
    }

    #[test]
    fn launch_anchor_round_trips() {
        let a = LaunchAnchor { edge: Edge::Right, x: 1890.4, y: 33.0, thickness: 44.0 };
        let mut args = vec!["toggle".to_string()];
        args.extend(a.args());
        let b = LaunchAnchor::parse(&args).unwrap();
        assert_eq!((b.edge, b.x, b.y, b.thickness), (Edge::Right, 1890.0, 33.0, 44.0));
        assert_eq!(LaunchAnchor::parse(&["--edge".into(), "up".into()]), None);
    }
}
