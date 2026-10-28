//! Turns a [`WidgetDef`] plus [`Data`] into a tree of placed items: sizes from the content, free space shared by `grow`, text already formatted.
//!
//! Layout needs only text widths, so it takes a [`Metrics`]; tests use [`Fixed`] and are independent of the installed fonts.

use super::actions::Action;
use super::data::{Data, Tag, Val};
use super::dsl::{Align, ColorRef, Common, Justify, Kind, Node, Value, WidgetDef};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
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
}

pub trait Metrics {
    fn text_width(&self, s: &str, px: f32, bold: bool) -> f32;
    fn line_height(&self, px: f32) -> f32 {
        (px * 1.25).ceil()
    }
}

/// Every character is 0.55 em wide: layouts that do not depend on the machine's fonts.
pub struct Fixed;

impl Metrics for Fixed {
    fn text_width(&self, s: &str, px: f32, _bold: bool) -> f32 {
        s.chars().count() as f32 * px * 0.55
    }
}

pub struct Ctx<'a> {
    pub data: &'a Data,
    pub metrics: &'a dyn Metrics,
    /// The theme's popup radius, for `radius = "theme"`.
    pub theme_radius: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Leaf {
    None,
    Text { s: String, size: f32, bold: bool, color: ColorRef, align: Align },
    Icon { name: String, size: f32, color: ColorRef },
    Bar { frac: f32, thickness: f32, color: ColorRef, track: ColorRef },
    Ring { frac: f32, size: f32, thickness: f32, color: ColorRef, track: ColorRef },
    Graph { values: Vec<f32>, max: f32, color: ColorRef, fill: bool },
    Image { path: Option<String>, radius: f32, cover: bool },
    Dots { tags: Vec<Tag>, size: f32, gap: f32, color: ColorRef, active: ColorRef },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: &'static str,
    pub rect: Rect,
    pub leaf: Leaf,
    pub background: Option<ColorRef>,
    pub radius: f32,
    pub border: Option<ColorRef>,
    pub border_width: f32,
    pub opacity: f32,
    pub on_click: Option<Action>,
    pub on_scroll_up: Option<Action>,
    pub on_scroll_down: Option<Action>,
    pub children: Vec<Item>,
}

fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Row { .. } => "row",
        Kind::Col { .. } => "col",
        Kind::Stack => "stack",
        Kind::Spacer => "spacer",
        Kind::Button { .. } => "button",
        Kind::Text { .. } => "text",
        Kind::Icon { .. } => "icon",
        Kind::Bar { .. } => "bar",
        Kind::Ring { .. } => "ring",
        Kind::Graph { .. } => "graph",
        Kind::Image { .. } => "image",
        Kind::Workspaces { .. } => "workspaces",
    }
}

fn visible(n: &Node, cx: &Ctx) -> bool {
    n.c.show_if.as_ref().is_none_or(|p| cx.data.truthy(p))
}

fn value_of(v: &Value, cx: &Ctx) -> Option<f64> {
    match v {
        Value::Lit(n) => Some(*n),
        Value::Path(p) => cx.data.num(p),
    }
}

fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn text_of(n: &Node, cx: &Ctx) -> Option<(String, f32, bool)> {
    match &n.kind {
        Kind::Text { text, size, bold, max_chars, .. } => Some((truncate(text.eval(cx.data), *max_chars), *size, *bold)),
        _ => None,
    }
}

fn content_size(n: &Node, cx: &Ctx) -> (f32, f32) {
    let kids: Vec<&Node> = n.children.iter().filter(|c| visible(c, cx)).collect();
    let sizes: Vec<(f32, f32)> = kids.iter().map(|c| natural(c, cx)).collect();
    match &n.kind {
        Kind::Row { gap, .. } | Kind::Button { gap } => (sizes.iter().map(|s| s.0).sum::<f32>() + gap * sizes.len().saturating_sub(1) as f32, sizes.iter().map(|s| s.1).fold(0.0, f32::max)),
        Kind::Col { gap, .. } => (sizes.iter().map(|s| s.0).fold(0.0, f32::max), sizes.iter().map(|s| s.1).sum::<f32>() + gap * sizes.len().saturating_sub(1) as f32),
        Kind::Stack => (sizes.iter().map(|s| s.0).fold(0.0, f32::max), sizes.iter().map(|s| s.1).fold(0.0, f32::max)),
        Kind::Spacer => (0.0, 0.0),
        Kind::Text { .. } => {
            let (s, size, bold) = text_of(n, cx).unwrap_or_default();
            (cx.metrics.text_width(&s, size, bold), cx.metrics.line_height(size))
        }
        Kind::Icon { size, .. } => (*size, *size),
        Kind::Ring { size, .. } => (*size, *size),
        Kind::Bar { .. } => (80.0, 6.0),
        Kind::Graph { .. } => (120.0, 36.0),
        Kind::Image { .. } => (48.0, 48.0),
        Kind::Workspaces { size, gap, .. } => {
            let k = cx.data.tags.len().max(1) as f32;
            (k * size + (k - 1.0) * gap, *size)
        }
    }
}

fn natural(n: &Node, cx: &Ctx) -> (f32, f32) {
    let (w, h) = content_size(n, cx);
    let p = n.c.padding;
    (n.c.width.unwrap_or(w + p[0] + p[2]), n.c.height.unwrap_or(h + p[1] + p[3]))
}

fn leaf_of(n: &Node, cx: &Ctx, r: Rect) -> Leaf {
    match &n.kind {
        Kind::Text { color, align, .. } => {
            let (s, size, bold) = text_of(n, cx).unwrap_or_default();
            Leaf::Text { s, size, bold, color: color.clone(), align: *align }
        }
        Kind::Icon { name, size, color } => Leaf::Icon { name: name.eval(cx.data), size: *size, color: color.clone() },
        Kind::Bar { value, min, max, color, track } => {
            let v = value_of(value, cx).unwrap_or(*min);
            Leaf::Bar { frac: (((v - min) / (max - min).max(1e-9)) as f32).clamp(0.0, 1.0), thickness: n.c.height.unwrap_or(6.0).min(r.h), color: color.clone(), track: track.clone() }
        }
        Kind::Ring { value, max, size, thickness, color, track } => {
            Leaf::Ring { frac: ((value_of(value, cx).unwrap_or(0.0) / max) as f32).clamp(0.0, 1.0), size: *size, thickness: *thickness, color: color.clone(), track: track.clone() }
        }
        Kind::Graph { series, max, color, fill } => {
            let values = match cx.data.get(series) {
                Some(Val::Series(v)) => v.clone(),
                _ => Vec::new(),
            };
            let auto = values.iter().copied().fold(0.0_f32, f32::max);
            let m = max.unwrap_or(if auto <= 1.0 { 1.0 } else { auto * 1.15 });
            Leaf::Graph { values, max: m, color: color.clone(), fill: *fill }
        }
        Kind::Image { file, source, radius, cover } => {
            let path = match (file, source) {
                (Some(f), _) => Some(f.clone()),
                (_, Some(s)) => match cx.data.get(s) {
                    Some(Val::Text(t)) if !t.is_empty() => Some(t.clone()),
                    _ => None,
                },
                _ => None,
            };
            Leaf::Image { path, radius: *radius, cover: *cover }
        }
        Kind::Workspaces { size, gap, color, active } => Leaf::Dots { tags: cx.data.tags.clone(), size: *size, gap: *gap, color: color.clone(), active: active.clone() },
        _ => Leaf::None,
    }
}

fn look(c: &Common, cx: &Ctx) -> (Option<ColorRef>, f32) {
    let r = match c.radius {
        Some(r) if r < 0.0 => cx.theme_radius,
        Some(r) => r,
        None => 0.0,
    };
    (c.background.clone(), r)
}

fn arrange(n: &Node, cx: &Ctx, r: Rect) -> Item {
    let (background, radius) = look(&n.c, cx);
    let p = n.c.padding;
    let inner = Rect { x: r.x + p[0], y: r.y + p[1], w: (r.w - p[0] - p[2]).max(0.0), h: (r.h - p[1] - p[3]).max(0.0) };
    let kids: Vec<&Node> = n.children.iter().filter(|c| visible(c, cx)).collect();
    let mut children = Vec::new();
    match &n.kind {
        Kind::Row { gap, justify } => flow(&mut children, &kids, cx, inner, true, *gap, *justify, n.c.align.unwrap_or(Align::Start)),
        Kind::Button { gap } => flow(&mut children, &kids, cx, inner, true, *gap, Justify::Center, n.c.align.unwrap_or(Align::Center)),
        Kind::Col { gap, justify } => flow(&mut children, &kids, cx, inner, false, *gap, *justify, n.c.align.unwrap_or(Align::Start)),
        Kind::Stack => {
            let al = n.c.align.unwrap_or(Align::Stretch);
            for k in kids {
                let (w, h) = natural(k, cx);
                let (cw, ch) = if al == Align::Stretch { (inner.w, inner.h) } else { (w.min(inner.w), h.min(inner.h)) };
                let x = inner.x + place(al, inner.w, cw);
                let y = inner.y + place(al, inner.h, ch);
                children.push(arrange(k, cx, Rect { x, y, w: cw, h: ch }));
            }
        }
        _ => {}
    }
    Item {
        kind: kind_name(&n.kind),
        rect: r,
        leaf: leaf_of(n, cx, r),
        background,
        radius,
        border: n.c.border.clone(),
        border_width: if n.c.border.is_some() { n.c.border_width } else { 0.0 },
        opacity: n.c.opacity,
        on_click: n.c.on_click.clone(),
        on_scroll_up: n.c.on_scroll_up.clone(),
        on_scroll_down: n.c.on_scroll_down.clone(),
        children,
    }
}

fn place(al: Align, space: f32, size: f32) -> f32 {
    match al {
        Align::Start | Align::Stretch => 0.0,
        Align::Center => ((space - size) / 2.0).max(0.0),
        Align::End => (space - size).max(0.0),
    }
}

#[allow(clippy::too_many_arguments)]
fn flow(out: &mut Vec<Item>, kids: &[&Node], cx: &Ctx, inner: Rect, horizontal: bool, gap: f32, justify: Justify, cross: Align) {
    let main = |w: f32, h: f32| if horizontal { w } else { h };
    let sizes: Vec<(f32, f32)> = kids.iter().map(|k| natural(k, cx)).collect();
    let grows: Vec<f32> = kids
        .iter()
        .map(|k| {
            if k.c.grow > 0.0 {
                k.c.grow
            } else if matches!(k.kind, Kind::Spacer) {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let used: f32 = sizes.iter().map(|s| main(s.0, s.1)).sum::<f32>() + gap * kids.len().saturating_sub(1) as f32;
    let free = (main(inner.w, inner.h) - used).max(0.0);
    let total_grow: f32 = grows.iter().sum();
    let (mut pos, extra_gap) = if total_grow > 0.0 {
        (0.0, 0.0)
    } else {
        match justify {
            Justify::Start => (0.0, 0.0),
            Justify::Center => (free / 2.0, 0.0),
            Justify::End => (free, 0.0),
            Justify::Between if kids.len() > 1 => (0.0, free / (kids.len() - 1) as f32),
            Justify::Between => (0.0, 0.0),
        }
    };
    for (i, k) in kids.iter().enumerate() {
        let (nw, nh) = sizes[i];
        let m = main(nw, nh) + if total_grow > 0.0 { free * grows[i] / total_grow } else { 0.0 };
        let cross_space = if horizontal { inner.h } else { inner.w };
        let cross_nat = if horizontal { nh } else { nw };
        let al = cross;
        let cs = if al == Align::Stretch { cross_space } else { cross_nat.min(cross_space) };
        let off = place(al, cross_space, cs);
        let rect = if horizontal { Rect { x: inner.x + pos, y: inner.y + off, w: m, h: cs } } else { Rect { x: inner.x + off, y: inner.y + pos, w: cs, h: m } };
        out.push(arrange(k, cx, rect));
        pos += m + gap + extra_gap;
    }
}

/// Lays out `def` for the data in `cx`. The root gets `def.size` (or its natural size); pass `fit` to override.
pub fn layout(def: &WidgetDef, cx: &Ctx, fit: Option<(f32, f32)>) -> Item {
    let (nw, nh) = natural(&def.root, cx);
    let (w, h) = fit.or(def.size).unwrap_or((nw, nh));
    arrange(&def.root, cx, Rect { x: 0.0, y: 0.0, w, h })
}

/// The deepest item under (x, y) that has `on_click`.
pub fn hit(it: &Item, x: f32, y: f32) -> Option<&Item> {
    if !it.rect.contains(x, y) {
        return None;
    }
    it.children.iter().rev().find_map(|c| hit(c, x, y)).or_else(|| it.on_click.is_some().then_some(it))
}

/// The deepest item under (x, y) with a scroll action for `up`.
pub fn hit_scroll(it: &Item, x: f32, y: f32, up: bool) -> Option<&Item> {
    if !it.rect.contains(x, y) {
        return None;
    }
    let has = |i: &Item| if up { i.on_scroll_up.is_some() } else { i.on_scroll_down.is_some() };
    it.children.iter().rev().find_map(|c| hit_scroll(c, x, y, up)).or_else(|| has(it).then_some(it))
}

/// Which workspace dot of a `workspaces` item is at x (1-based).
pub fn dot_at(it: &Item, x: f32) -> Option<u32> {
    let Leaf::Dots { tags, size, gap, .. } = &it.leaf else { return None };
    let step = size + gap;
    let rel = x - it.rect.x - ((it.rect.w - (tags.len() as f32 * step - gap)) / 2.0).max(0.0);
    (rel >= 0.0 && !tags.is_empty()).then(|| (rel / step) as usize + 1).filter(|n| *n <= tags.len()).map(|n| n as u32)
}

/// A stable text dump, for golden tests: one line per item.
pub fn dump(it: &Item) -> String {
    fn go(it: &Item, depth: usize, out: &mut String) {
        let r = it.rect;
        out.push_str(&format!("{}{} [{:.1} {:.1} {:.1} {:.1}]", "  ".repeat(depth), it.kind, r.x, r.y, r.w, r.h));
        match &it.leaf {
            Leaf::None => {}
            Leaf::Text { s, size, bold, .. } => out.push_str(&format!(" text={s:?} size={size}{}", if *bold { " bold" } else { "" })),
            Leaf::Icon { name, size, .. } => out.push_str(&format!(" icon={name:?} size={size}")),
            Leaf::Bar { frac, thickness, .. } => out.push_str(&format!(" frac={frac:.2} thick={thickness}")),
            Leaf::Ring { frac, size, thickness, .. } => out.push_str(&format!(" frac={frac:.2} size={size} thick={thickness}")),
            Leaf::Graph { values, max, .. } => out.push_str(&format!(" points={} max={max:.2}", values.len())),
            Leaf::Image { path, .. } => out.push_str(&format!(" path={path:?}")),
            Leaf::Dots { tags, .. } => out.push_str(&format!(
                " dots={}",
                tags.iter()
                    .map(|t| if t.active {
                        'A'
                    } else if t.occupied {
                        'o'
                    } else {
                        '.'
                    })
                    .collect::<String>()
            )),
        }
        if it.background.is_some() {
            out.push_str(&format!(" bg r={}", it.radius));
        }
        if it.on_click.is_some() {
            out.push_str(" click");
        }
        out.push('\n');
        for c in &it.children {
            go(c, depth + 1, out);
        }
    }
    let mut s = String::new();
    go(it, 0, &mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::data::sample;
    use crate::widgets::{dsl, Trust};

    fn lay(src: &str) -> Item {
        let def = dsl::parse(src, Trust::Theme).unwrap();
        let d = sample();
        layout(&def, &Ctx { data: &d, metrics: &Fixed, theme_radius: 10.0 }, None)
    }

    #[test]
    fn rows_share_free_space_by_grow_and_spacers() {
        let it = lay("format = 1\nsize = [200, 40]\n[root]\ntype = \"row\"\npadding = 10\nalign = \"center\"\n[[root.children]]\ntype = \"text\"\ntext = \"ab\"\nsize = 10\n[[root.children]]\ntype = \"spacer\"\n[[root.children]]\ntype = \"text\"\ntext = \"cd\"\nsize = 10\n");
        let c = &it.children;
        assert_eq!(it.rect, Rect { x: 0.0, y: 0.0, w: 200.0, h: 40.0 });
        assert_eq!((c[0].rect.x, c[0].rect.w), (10.0, 11.0));
        assert_eq!((c[2].rect.x + c[2].rect.w).round(), 190.0, "the spacer pushes the last text to the right padding edge");
        assert_eq!(c[0].rect.y.round(), 14.0, "centred vertically: (20 - 13) / 2 + 10 rounded");
    }

    #[test]
    fn columns_stack_with_gap_and_justify() {
        let it = lay("format = 1\nsize = [100, 100]\n[root]\ntype = \"col\"\ngap = 5\njustify = \"center\"\n[[root.children]]\ntype = \"stack\"\nheight = 10\n[[root.children]]\ntype = \"stack\"\nheight = 10\n");
        // two 10px boxes with a 5px gap are 25px tall, centred in 100: they start at 37.5
        assert_eq!(it.children[0].rect.y, 37.5);
        assert_eq!(it.children[1].rect.y, 52.5);
    }

    #[test]
    fn hidden_nodes_take_no_space_and_stacks_overlay() {
        let it = lay("format = 1\nsize = [100, 50]\n[root]\ntype = \"stack\"\n[[root.children]]\ntype = \"ring\"\nvalue = 0.5\nsize = 30\n[[root.children]]\ntype = \"text\"\ntext = \"x\"\nshow_if = \"!media.playing\"\n[[root.children]]\ntype = \"text\"\ntext = \"y\"\nshow_if = \"media.playing\"\n");
        assert_eq!(it.children.len(), 2);
        assert_eq!(it.children[0].rect, it.children[1].rect, "stack children fill the same area");
    }

    #[test]
    fn hit_testing_finds_the_deepest_clickable() {
        let it = lay("format = 1\nsize = [100, 50]\n[root]\ntype = \"row\"\nalign = \"stretch\"\non_click = \"open dashboard\"\n[[root.children]]\ntype = \"button\"\nwidth = 40\non_click = \"volume mute\"\n[[root.children]]\ntype = \"spacer\"\n");
        assert_eq!(hit(&it, 10.0, 10.0).unwrap().on_click, Some(Action::Volume(None)));
        assert_eq!(hit(&it, 80.0, 10.0).unwrap().on_click, Some(Action::Open(super::super::actions::Open::Dashboard)));
        assert!(hit(&it, 120.0, 10.0).is_none());
    }

    #[test]
    fn dots_map_clicks_to_workspaces() {
        let it = lay("format = 1\nsize = [100, 20]\n[root]\ntype = \"workspaces\"\nsize = 8\ngap = 8\n");
        // 5 tags: width 5*8 + 4*8 = 72, centred in 100: starts at 14
        assert_eq!(dot_at(&it, 14.5), Some(1));
        assert_eq!(dot_at(&it, 14.0 + 16.0 * 2.0 + 1.0), Some(3));
        assert_eq!(dot_at(&it, 5.0), None);
        assert_eq!(dot_at(&it, 95.0), None);
    }
}
