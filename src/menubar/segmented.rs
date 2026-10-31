//! Native Dux DBar sections: three independently placed tonal containers.
//!
//! Source contract: DBar/BarSection.qml and Theme.qml (default band38, face28,
//! text12, icons14, spacing4); BarWidgetDefaults.js
//! canonical item IDs. Layout is shared by paint, pointer input and material geometry.
//! Data below is a cached snapshot; system sampling and MPRIS live on workers.
use super::*;
use crate::dash::mpris::{self, Cmd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Tray(Tray),
    Media,
    Previous,
    Toggle,
    Next,
    Notifications,
    Clipboard,
    Cpu,
    Memory,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct Item {
    pub kind: Kind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
#[derive(Debug, Clone)]
pub(super) struct Group {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}
#[derive(Default)]
pub(super) struct Layout {
    pub items: Vec<Item>,
    pub groups: Vec<Group>,
    /// What did not fit (the overflow cell lists it).
    pub hidden: Vec<Kind>,
    /// A cluster (`bar.cluster`): one face per group, the groups adjacent and centred as one run.
    pub cluster: bool,
}
impl Layout {
    pub fn faces(&self) -> Vec<Group> {
        if self.cluster {
            return self.groups.clone();
        }
        let category = |kind| match kind {
            Kind::Previous | Kind::Toggle | Kind::Next | Kind::Media => 1,
            Kind::Tray(Tray::Indicator(_) | Tray::Indicators) => 2,
            _ => 0,
        };
        let mut faces = Vec::new();
        let mut index = 0;
        while index < self.items.len() {
            let first = self.items[index];
            let mut last = first;
            let key = category(first.kind);
            index += 1;
            while key != 0 && index < self.items.len() && category(self.items[index].kind) == key {
                last = self.items[index];
                index += 1;
            }
            faces.push(Group { x: first.x, y: first.y, w: last.x + last.w - first.x, h: first.h, radius: self.groups.first().map_or(first.h / 2.0, |g| g.radius) });
        }
        faces
    }
    pub fn hit(&self, x: f32, y: f32) -> Option<&Item> {
        self.items.iter().find(|i| x >= i.x && x < i.x + i.w && y >= i.y && y < i.y + i.h)
    }
}

/// Place center on the monitor's center, never the free space's center. On crowded
/// monitors remove optional wide content first, retaining both left and right controls.
/// `cluster` = Some((gap between group faces, padding inside a face)): the groups are placed adjacent, in order, as ONE run centred on the bar and sized by
/// its content (the three-island taskbar of UKUI's panel); `None` keeps the stock start / centre / end placement.
// Six independent geometry values plus content and the optional cluster contract.
#[allow(clippy::too_many_arguments)]
fn place(width: f32, margin: f32, gap: f32, height: f32, top: f32, radius: f32, mut groups: [Vec<(Kind, f32)>; 3], cluster: Option<(f32, f32)>) -> Layout {
    let available = (width - margin * 2.0).max(0.0);
    // Reserve an overflow target before decorative padding on very short outputs.
    let pad = cluster.map_or(0.0, |c| c.1.min(((available - 56.0 - gap) / 2.0).max(0.0)));
    let gg = cluster.map_or(gap, |c| c.0);
    let size = |v: &Vec<(Kind, f32)>| if v.is_empty() { 0.0 } else { v.iter().map(|(_, w)| w).sum::<f32>() + gap * v.len().saturating_sub(1) as f32 + 2.0 * pad };
    // the space the faces' own gaps take: the stock layout counts two gaps; a cluster counts one between each pair of faces that exist
    let between = |groups: &[Vec<(Kind, f32)>; 3]| if cluster.is_some() { gg * groups.iter().filter(|g| !g.is_empty()).count().saturating_sub(1) as f32 } else { 2.0 * gap };
    let mut hidden: Vec<Kind> = Vec::new();
    // Remove detail before interaction. Never paint/hit overlapping groups.
    for optional in [
        Kind::Tray(Tray::Window),
        Kind::Media,
        Kind::Tray(Tray::Weather),
        Kind::Cpu,
        Kind::Memory,
        Kind::Clipboard,
        Kind::Previous,
        Kind::Next,
        Kind::Tray(Tray::Battery),
        Kind::Tray(Tray::Brightness),
        Kind::Tray(Tray::Agents),
        Kind::Tray(Tray::Spotlight),
        Kind::Tray(Tray::Dash),
        Kind::Notifications,
        Kind::Tray(Tray::ShowDesktop),
        Kind::Tray(Tray::Lock),
        Kind::Tray(Tray::AppMenu),
    ] {
        let total = groups.iter().map(size).sum::<f32>() + between(&groups);
        let side = size(&groups[0]).max(size(&groups[2]));
        if total <= available && (cluster.is_some() || side * 2.0 + size(&groups[1]) + 2.0 * gap <= available) {
            break;
        }
        for group in &mut groups {
            group.retain(|(kind, _)| {
                let gone = *kind == optional || (optional == Kind::Tray(Tray::Window) && matches!(kind, Kind::Tray(Tray::Task(_))));
                if gone {
                    hidden.push(*kind);
                }
                !gone
            });
        }
    }
    let drop_where = |group: &mut Vec<(Kind, f32)>, keep: &dyn Fn(Kind) -> bool, hidden: &mut Vec<Kind>| {
        group.retain(|(kind, _)| {
            let ok = keep(*kind);
            if !ok {
                hidden.push(*kind);
            }
            ok
        });
    };
    // At extreme widths an omitted group is preferable to inaccessible painted overlap.
    if cluster.is_some() {
        // a cluster that still does not fit gives up whole groups, the middle one (the applications) first, never painting overlapping faces
        for index in [1usize, 0] {
            if groups.iter().map(size).sum::<f32>() + between(&groups) > available {
                drop_where(&mut groups[index], &|_| false, &mut hidden);
            }
        }
        if size(&groups[2]) > available {
            drop_where(&mut groups[2], &|kind| matches!(kind, Kind::Tray(Tray::Control | Tray::More)), &mut hidden);
        }
        if size(&groups[2]) > available && groups[2].iter().any(|(k, _)| *k == Kind::Tray(Tray::More)) {
            drop_where(&mut groups[2], &|kind| kind == Kind::Tray(Tray::More), &mut hidden);
        }
    } else {
        if groups.iter().map(size).sum::<f32>() + 2.0 * gap > available {
            drop_where(&mut groups[1], &|_| false, &mut hidden);
        }
        let lw = size(&groups[0]);
        let rw = size(&groups[2]);
        if lw + rw + gap > available {
            drop_where(&mut groups[0], &|kind| kind == Kind::Tray(Tray::Logo), &mut hidden);
        }
        if size(&groups[0]) + size(&groups[2]) + gap > available {
            drop_where(&mut groups[0], &|_| false, &mut hidden);
        }
        if size(&groups[2]) > available {
            drop_where(&mut groups[2], &|kind| matches!(kind, Kind::Tray(Tray::Control | Tray::More)), &mut hidden);
        }
    }
    let sizes = groups.each_ref().map(size);
    // Upstream CenterLayout's default index centering keeps the configured
    // middle clock on the output centre, even when the preceding music is absent.
    let centered_start = |items: &Vec<(Kind, f32)>| {
        if let Some(index) = items.iter().position(|(kind, _)| *kind == Kind::Tray(Tray::Clock)) {
            width / 2.0 - items[..index].iter().map(|(_, w)| *w).sum::<f32>() - gap * index as f32 - items[index].1 / 2.0
        } else {
            (width - size(items)) / 2.0
        }
    };
    let cx = centered_start(&groups[1]);
    let center_fits = cluster.is_some() || (cx >= margin + sizes[0] + gap && cx + sizes[1] <= width - margin - sizes[2] - gap);
    if !center_fits {
        hidden.extend(groups[1].drain(..).map(|(kind, _)| kind));
    }
    let sizes = groups.each_ref().map(size);
    let starts = if cluster.is_some() {
        // one content-sized run, centred on the bar: each existing face follows the previous one by `gg`
        let run = sizes.iter().sum::<f32>() + between(&groups);
        let mut x = (width - run) / 2.0;
        let mut out = [0.0f32; 3];
        for (index, group) in groups.iter().enumerate() {
            out[index] = x;
            if !group.is_empty() {
                x += sizes[index] + gg;
            }
        }
        out
    } else {
        [margin, centered_start(&groups[1]), width - margin - sizes[2]]
    };
    // the transport buttons are part of the media item, not applets of their own
    hidden.retain(|kind| !matches!(kind, Kind::Previous | Kind::Toggle | Kind::Next));
    let mut result = Layout { hidden, cluster: cluster.is_some(), ..Layout::default() };
    for (index, contents) in groups.into_iter().enumerate() {
        if contents.is_empty() || starts[index] < 0.0 || sizes[index] > available {
            continue;
        }
        result.groups.push(Group { x: starts[index], y: top, w: sizes[index], h: height, radius: radius.min(height / 2.0) });
        let mut x = starts[index] + pad;
        for (kind, w) in contents {
            result.items.push(Item { kind, x, y: top, w, h: height });
            x += w + gap;
        }
    }
    result
}

/// Dux expressiveEffects cubic-bezier(.34,.8,.34,1), 200ms.
fn effects(progress: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..12 {
        let t = (lo + hi) / 2.0;
        let x = 3.0 * (1.0 - t).powi(2) * t * 0.34 + 3.0 * (1.0 - t) * t * t * 0.34 + t * t * t;
        if x < progress {
            lo = t;
        } else {
            hi = t;
        }
    }
    let t = (lo + hi) / 2.0;
    3.0 * (1.0 - t).powi(2) * t * 0.8 + 3.0 * (1.0 - t) * t * t + t * t * t
}

impl DecBar {
    pub(super) fn dux_animating(&self) -> bool {
        self.dux() && !self.dux_reduce_motion && self.dux_effect.is_some_and(|(_, at, _)| at.elapsed() < Duration::from_millis(200))
    }
    fn dux_hover_opacity(&self, kind: Kind) -> f32 {
        if self.dux_reduce_motion {
            return if self.dux_hover == Some(kind) { 0.08 } else { 0.0 };
        }
        if let Some((target, at, entering)) = self.dux_effect.filter(|(target, _, _)| *target == kind) {
            let _ = target;
            let progress = (at.elapsed().as_secs_f32() / 0.2).clamp(0.0, 1.0);
            let value = effects(progress);
            return 0.08 * if entering { value } else { 1.0 - value };
        }
        if self.dux_hover == Some(kind) {
            0.08
        } else {
            0.0
        }
    }
    pub(super) fn dux(&self) -> bool {
        #[cfg(test)]
        if super::applets::TEST_DUX.with(|f| f.get()) {
            return !self.embedded;
        }
        !self.embedded && crate::theme::th().bar_style == "dux"
    }
    pub(super) fn bar_bottom(&self) -> f32 {
        crate::theme::bar_bottom_for(&crate::theme::th(), true)
    }
    pub(super) fn dux_player(&self) -> Option<mpris::Player> {
        #[cfg(test)]
        if let Some(p) = super::applets::TEST_PLAYER.with(|p| p.borrow().clone()) {
            return Some(p);
        }
        let snap = self.dux_mpris.as_ref()?.snapshot();
        mpris::pick_active(&snap.players, None).cloned()
    }
    fn dux_window_title(&self) -> &str {
        self.v_win.as_ref().map_or("", |(id, title)| if title.is_empty() { id.as_str() } else { title.as_str() })
    }
    fn dux_measure(&self, text: &str, size: f32) -> f32 {
        self.fonts.as_ref().map_or(text.chars().count() as f32 * size * 0.6, |f| f.measure(text, size, Weight::Regular))
    }
    fn dux_workspace_width(&self) -> f32 {
        let scale = 1.0;
        let face = (crate::theme::th().bar_height - 10.0).max(16.0) * scale;
        self.workspaces.iter().map(|w| face * if w.active { 1.05 } else { 0.7 }).sum::<f32>() + 8.0 * scale * self.workspaces.len().saturating_sub(1) as f32 + 2.0 * (8.0 * face / 30.0).round()
    }
    fn dux_workspace_cells(&self, item: Item) -> Vec<(usize, f32, f32)> {
        let scale = 1.0;
        let mut x = item.x + (8.0 * item.h / 30.0).round();
        self.workspaces
            .iter()
            .enumerate()
            .map(|(index, ws)| {
                let w = item.h * if ws.active { 1.05 } else { 0.7 };
                let result = (index, x, w);
                x += w + 8.0 * scale;
                result
            })
            .collect()
    }
    pub(super) fn dux_regions(&self) -> Vec<crate::glass::Region> {
        let th = crate::theme::th();
        let scale = 1.0;
        if th.bar_layout.background == "band" {
            let margin = th.bar_layout.margin * scale;
            let height = th.bar_height * scale;
            vec![crate::glass::Region { x: margin, y: margin, w: (self.width as f32 - margin * 2.0).max(0.0), h: height, radius: th.bar_layout.band_radius.min(height / 2.0) }]
        } else {
            self.dux_layout().faces().into_iter().map(|g| crate::glass::Region { x: g.x, y: g.y, w: g.w, h: g.h, radius: g.radius }).collect()
        }
    }
    /// The applets of each Dux group: the theme's item lists in their order (only those that are on), then the other
    /// enabled applets in the group they belong to.
    pub(super) fn dux_groups(&self) -> [Vec<crate::panel::AppletId>; 3] {
        use crate::panel::AppletId as A;
        let th = crate::theme::th();
        let settings = &th.bar_layout;
        let p = self.panel();
        let mut groups: [Vec<A>; 3] = Default::default();
        let mut seen = Vec::new();
        for (out, ids) in groups.iter_mut().zip([&settings.left, &settings.center, &settings.right]) {
            for id in ids.iter().filter_map(|k| A::parse(k)) {
                if p.shows(id) && !seen.contains(&id) {
                    seen.push(id);
                    out.push(id);
                }
            }
        }
        for id in p.applets.iter() {
            if !groups.iter().any(|g| g.contains(&id)) {
                groups[super::applets::dux_group(id)].push(id);
            }
        }
        groups
    }

    fn dux_items_of(&self, id: crate::panel::AppletId, out: &mut Vec<(Kind, f32)>) {
        use crate::panel::AppletId as A;
        let th = crate::theme::th();
        let scale = 1.0;
        let cell = 28.0 * scale;
        let snap = self.snapshot();
        let p = self.panel();
        let item = match id {
            A::Launcher => match &th.branding.launcher_label {
                Some(l) => (Kind::Tray(Tray::Logo), self.dux_measure(l, 12.0) + 14.0 * scale + if matches!(th.branding.logo, crate::panel::branding::Logo::None) { 0.0 } else { 18.0 }),
                None => (Kind::Tray(Tray::Logo), cell),
            },
            A::Workspaces if !self.workspaces.is_empty() && self.show_workspaces => (Kind::Tray(Tray::Workspaces), self.dux_workspace_width()),
            A::WindowTitle if self.v_win.is_some() => (Kind::Tray(Tray::Window), self.dux_measure(self.dux_window_title(), 12.0).min(160.0 * scale) + 14.0 * scale),
            A::Media if self.dux_player().is_some() => {
                out.extend([(Kind::Previous, cell), (Kind::Toggle, cell), (Kind::Next, cell)]);
                (Kind::Media, 114.0 * scale)
            }
            A::Clock => (Kind::Tray(Tray::Clock), self.dux_measure(&self.clock_text, 12.0) + 14.0 * scale),
            A::Weather => {
                let weather = self.shared.weather.lock().ok().and_then(|v| v.clone());
                (Kind::Tray(Tray::Weather), weather.as_ref().map_or(self.dux_measure("--°C", 12.0) + 32.0 * scale, |w| self.dux_measure(&format!("{}°C", w.temp_c), 12.0) + 32.0 * scale))
            }
            A::Battery if snap.battery.is_some() => (Kind::Tray(Tray::Battery), 64.0 * scale),
            A::Tray => {
                out.extend(self.indicator_kinds(6).into_iter().map(|k| (Kind::Tray(k), cell)));
                return;
            }
            A::Tasks => {
                out.extend(self.tasks.iter().map(|t| (Kind::Tray(Tray::Task(t.key())), cell)));
                return;
            }
            A::Clipboard => (Kind::Clipboard, cell),
            A::Cpu | A::Memory => {
                let perf = self.dux_perf.lock().ok().and_then(|p| *p);
                let has = perf.and_then(|p| if id == A::Cpu { p.0 } else { p.1 }).is_some();
                if !has {
                    return;
                }
                (if id == A::Cpu { Kind::Cpu } else { Kind::Memory }, self.dux_measure("100%", 12.0) + 32.0 * scale)
            }
            A::Notifications => (Kind::Notifications, cell),
            A::ControlCenter if th.control_center.enabled => (Kind::Tray(Tray::Control), cell),
            // the Dux bar always shows its network and audio items, as upstream
            A::Network => (Kind::Tray(Tray::Network), cell),
            A::Volume => (Kind::Tray(Tray::Audio), cell),
            A::Bluetooth if snap.bluetooth.is_some() => (Kind::Tray(Tray::Bluetooth), cell),
            A::Dashboard => (Kind::Tray(Tray::Dash), cell),
            A::Power => (Kind::Tray(Tray::Power), cell),
            A::Layout => (Kind::Tray(Tray::Layout), cell),
            other => {
                // the applets added with the registry: one face each, while their need is met
                if !self.applet_ready(other, &p, &snap) {
                    return;
                }
                let Some(kind) = super::applets::single(other) else { return };
                (Kind::Tray(kind), cell)
            }
        };
        out.push(item);
    }

    pub(super) fn dux_layout(&self) -> Layout {
        let th = crate::theme::th();
        let settings = &th.bar_layout;
        let scale = 1.0;
        let mut groups: [Vec<(Kind, f32)>; 3] = Default::default();
        for (out, ids) in groups.iter_mut().zip(self.dux_groups()) {
            for id in ids {
                self.dux_items_of(id, out);
            }
        }
        let fit = |groups: [Vec<(Kind, f32)>; 3]| {
            place(
                self.width as f32,
                (settings.margin + 4.0) * scale,
                settings.spacing * scale,
                (th.bar_height - 10.0).max(16.0) * scale,
                (settings.margin + 5.0) * scale,
                settings.radius * scale,
                groups,
                settings.cluster.then_some((settings.group_gap * scale, settings.group_padding * scale)),
            )
        };
        let first = fit(groups.clone());
        if first.hidden.is_empty() {
            return first;
        }
        // something gave way: the overflow cell keeps it reachable
        groups[2].push((Kind::Tray(Tray::More), 28.0 * scale));
        fit(groups)
    }

    /// The applets the Dux overflow cell lists.
    pub(super) fn dux_hidden(&self) -> Vec<Tray> {
        self.dux_layout()
            .hidden
            .into_iter()
            .filter_map(|k| match k {
                Kind::Tray(t) => Some(t),
                Kind::Media => Some(Tray::Media),
                Kind::Notifications => Some(Tray::Notifications),
                Kind::Clipboard => Some(Tray::Clipboard),
                Kind::Cpu => Some(Tray::Cpu),
                Kind::Memory => Some(Tray::Memory),
                Kind::Previous | Kind::Toggle | Kind::Next => None,
            })
            .collect()
    }
    fn dux_text(&mut self, pm: &mut Pixmap, text: &str, item: Item, secondary: bool) {
        let Some(fonts) = self.fonts.as_mut() else { return };
        let scale = 1.0;
        let size = 12.0;
        let text = controlcenter::fit(text, item.w - 14.0 * scale, size, Weight::Regular, fonts);
        let th = crate::theme::th();
        let (top, bottom) = fonts.ink_vertical(&text, size, Weight::Regular).unwrap_or((0.0, 0.0));
        let y = item.y + (item.h - (bottom - top)) / 2.0 - top - fonts.ascent(size, Weight::Regular);
        fonts.draw(pm, &text, item.x + 7.0 * scale, y, size, Weight::Regular, if secondary { th.text_secondary.color(1.0) } else { th.bar_text.color(1.0) });
    }
    fn dux_symbol(&mut self, pm: &mut Pixmap, name: &str, item: Item, active: bool) {
        let th = crate::theme::th();
        let scale = 1.0;
        let size: f32 = 14.0 * scale;
        let ink = if active { th.accent.t() } else { th.bar_text.t() };
        controlcenter::blit_symbol(pm, &mut self.symbols, true, name, ink, size, item.x + item.w / 2.0, item.y + item.h / 2.0);
    }
    pub(super) fn dux_draw(&mut self, pm: &mut Pixmap) {
        let th = crate::theme::th();
        let lay = self.dux_layout();
        let snap = self.snapshot();
        let player = self.dux_player();
        let perf = self.dux_perf.lock().ok().and_then(|p| *p);
        let wx = self.shared.weather.lock().ok().and_then(|v| v.clone());
        let scale = 1.0;
        if th.bar_layout.background == "band" {
            let margin = th.bar_layout.margin * scale;
            let height = th.bar_height * scale;
            fill_rounded(pm, margin, margin, self.width as f32 - margin * 2.0, height, th.bar_layout.band_radius.min(height / 2.0), th.background.color(1.0));
        }
        for face in lay.faces() {
            fill_rounded(pm, face.x, face.y, face.w, face.h, face.radius, th.bar_c());
            if lay.cluster {
                // the island's own 1 px outline (the theme's `outline` strength)
                stroke_rounded(pm, face.x + 0.5, face.y + 0.5, face.w - 1.0, face.h - 1.0, (face.radius - 0.5).max(0.0), th.outline_c(), 1.0);
            }
        }
        for item in lay.items {
            let hover_opacity = self.dux_hover_opacity(item.kind);
            if hover_opacity > 0.0 {
                fill_rounded(pm, item.x, item.y + 4.0 * scale, item.w, item.h - 8.0 * scale, (th.bar_layout.radius * scale).min(item.h / 2.0), th.bar_text.color(hover_opacity));
            }
            match item.kind {
                Kind::Media => self.dux_text(pm, player.as_ref().map_or("No media", |p| p.meta.title.as_str()), item, true),
                Kind::Previous => self.dux_symbol(pm, "backward", item, false),
                Kind::Toggle => self.dux_symbol(pm, if player.as_ref().is_some_and(|p| p.status == mpris::Status::Playing) { "pause" } else { "play" }, item, true),
                Kind::Next => self.dux_symbol(pm, "forward", item, false),
                Kind::Notifications => self.dux_symbol(pm, "bell", item, false),
                Kind::Clipboard => self.dux_symbol(pm, "clipboard", item, false),
                Kind::Cpu | Kind::Memory => {
                    let value = perf.and_then(|p| if item.kind == Kind::Cpu { p.0 } else { p.1 });
                    let text = value.map_or_else(|| "—%".to_string(), |v| format!("{:.0}%", v * 100.0));
                    let icon = Item { x: item.x + 7.0, w: 14.0, ..item };
                    self.dux_symbol(pm, if item.kind == Kind::Cpu { "memory" } else { "developer_board" }, icon, false);
                    self.dux_text(pm, &text, Item { x: item.x + 18.0, w: item.w - 18.0, ..item }, true);
                }
                Kind::Tray(Tray::Window) => {
                    let title = self.dux_window_title().to_string();
                    self.dux_text(pm, &title, item, true);
                }
                Kind::Tray(Tray::Clock) => {
                    let title = self.clock_text.clone();
                    self.dux_text(pm, &title, item, false);
                }
                Kind::Tray(Tray::Weather) => {
                    let icon = Item { x: item.x + 7.0, w: 14.0, ..item };
                    self.dux_symbol(pm, wx.as_ref().map_or("weather-clear", |w| weather::symbol_for(w.code, w.night)), icon, false);
                    let label = Item { x: item.x + 18.0, w: item.w - 18.0, ..item };
                    if let Some(w) = &wx {
                        self.dux_text(pm, &format!("{}°C", w.temp_c), label, false);
                    } else {
                        self.dux_text(pm, "--°C", label, true);
                    }
                }
                Kind::Tray(Tray::Battery) => {
                    let title = snap.battery.as_ref().map_or_else(|| "—%".into(), |b| format!("{}%", b.percent));
                    self.dux_symbol(pm, "battery", Item { x: item.x + 7.0, w: 14.0, ..item }, false);
                    self.dux_text(pm, &title, Item { x: item.x + 18.0, w: item.w - 18.0, ..item }, false);
                }
                Kind::Tray(Tray::Workspaces) => {
                    for (index, x, width) in self.dux_workspace_cells(item) {
                        let ws = &self.workspaces[index];
                        let height = item.h * 0.5;
                        fill_rounded(
                            pm,
                            x,
                            item.y + (item.h - height) / 2.0,
                            width,
                            height,
                            height / 2.0,
                            if ws.active { th.accent.color(1.0) } else { th.bar_text.color(if ws.occupied { 0.28 } else { 0.16 }) },
                        );
                    }
                }
                Kind::Tray(Tray::Task(key)) => {
                    if let Some(t) = self.task(key).cloned() {
                        let icon = self.v_icons.get(&t.app_id, 32).clone();
                        crate::gfx::blit_scaled(pm, &icon, item.x + (item.w - 16.0) / 2.0, item.y + (item.h - 16.0) / 2.0, 16.0, 16.0, 1.0);
                        if t.active() {
                            fill_rounded(pm, item.x + item.w / 2.0 - 4.0, item.y + item.h - 4.0, 8.0, 2.0, 1.0, th.accent.color(1.0));
                        }
                    }
                }
                Kind::Tray(Tray::More) => {
                    for offset in [-5.0, 0.0, 5.0] {
                        fill_rounded(pm, item.x + item.w / 2.0 + offset - 1.5, item.y + item.h / 2.0 - 1.5, 3.0, 3.0, 1.5, th.bar_text.color(0.85));
                    }
                }
                Kind::Tray(Tray::Logo) if th.branding.launcher_label.is_some() || !matches!(th.branding.logo, crate::panel::branding::Logo::Style) => {
                    let label = th.branding.launcher_label.clone();
                    let icon_cx = if label.is_some() { item.x + 7.0 + 7.0 } else { item.x + item.w / 2.0 };
                    let drawn = !matches!(th.branding.logo, crate::panel::branding::Logo::None) && self.draw_brand(pm, 14.0 * scale, icon_cx, item.y + item.h / 2.0, th.bar_text.t());
                    if let Some(l) = label {
                        let x = if drawn { item.x + 18.0 } else { item.x };
                        self.dux_text(pm, &l, Item { x, w: item.w - (x - item.x), ..item }, false);
                    } else if !drawn {
                        self.dux_symbol(pm, "apps", item, false);
                    }
                }
                Kind::Tray(Tray::Lock) => {
                    let on = self.locks.caps() || self.locks.num();
                    self.dux_symbol(pm, "lock", item, on);
                }
                Kind::Tray(Tray::Awake) => self.dux_symbol(pm, "cup", item, snap.awake),
                Kind::Tray(Tray::Indicator(key)) => self.indicator_draw(pm, key, item.x + item.w / 2.0, item.y + item.h / 2.0, 14.0 * scale),
                Kind::Tray(Tray::Indicators) => self.indicator_overflow_draw(pm, item.x + item.w / 2.0, item.y + item.h / 2.0, 14.0 * scale),
                Kind::Tray(kind) => self.dux_symbol(
                    pm,
                    match kind {
                        Tray::Logo => "apps",
                        Tray::Power => "power",
                        Tray::Control => snap.volume.as_ref().map(controlcenter::speaker_icon).unwrap_or("speaker-muted"),
                        Tray::Network => {
                            if snap.net.as_ref().is_some_and(|n| n.wifi_enabled) {
                                "wifi-3"
                            } else {
                                "wifi-off"
                            }
                        }
                        Tray::Audio => snap.volume.as_ref().map(controlcenter::speaker_icon).unwrap_or("speaker-2"),
                        Tray::Bluetooth => "bluetooth",
                        Tray::Layout => "layout",
                        other => super::applets::material_symbol(other),
                    },
                    item,
                    self.open == Open::Control && kind == Tray::Control,
                ),
            }
        }
    }
    pub(super) fn dux_custom_press(&mut self, x: f32, y: f32, button: Button) -> bool {
        let layout = self.dux_layout();
        let kind = layout.hit(x, y).map(|i| i.kind);
        if button == Button::Left {
            if let Some(item) = layout.hit(x, y).filter(|i| i.kind == Kind::Tray(Tray::Workspaces)) {
                if let Some((index, _, _)) = self.dux_workspace_cells(*item).into_iter().find(|(_, left, width)| x >= *left && x < *left + *width) {
                    self.close();
                    self.active.switch_workspace(index);
                    return true;
                }
                // The pill's inset/gap must not fall through to the legacy equal-width dot hit test.
                return true;
            }
        }
        if button != Button::Left {
            return false;
        }
        match kind {
            Some(Kind::Tray(Tray::Logo)) => {
                let item = layout.hit(x, y).copied().expect("hit");
                self.close();
                let at = self.anchor_of((item.x, item.y + self.strip_top(), item.w, item.h));
                self.open_launcher(Some(at));
                true
            }
            Some(Kind::Tray(Tray::Clock)) => {
                let item = layout.hit(x, y).unwrap();
                let span = Span { x: item.x, w: item.w };
                self.close();
                self.dashboard(span);
                true
            }
            Some(Kind::Previous | Kind::Toggle | Kind::Next) => {
                if let (Some(worker), Some(player)) = (&self.dux_mpris, self.dux_player()) {
                    worker.send(match kind {
                        Some(Kind::Previous) => Cmd::Previous(player.bus),
                        Some(Kind::Next) => Cmd::Next(player.bus),
                        _ => Cmd::PlayPause(player.bus),
                    });
                }
                true
            }
            Some(Kind::Notifications) => {
                let item = layout.hit(x, y).copied().expect("hit");
                self.applet_press(Tray::Notifications, (item.x, item.y, item.w, item.h), Button::Left)
            }
            Some(Kind::Clipboard) => {
                let item = layout.hit(x, y).copied().expect("hit");
                self.applet_press(Tray::Clipboard, (item.x, item.y, item.w, item.h), Button::Left)
            }
            Some(Kind::Media) => {
                self.close();
                vbar::launch_ui(vec![crate::launch::sibling_or_path("decdash"), "media".into()]);
                true
            }
            Some(k @ (Kind::Cpu | Kind::Memory)) => {
                let item = layout.hit(x, y).copied().expect("hit");
                self.applet_press(if k == Kind::Cpu { Tray::Cpu } else { Tray::Memory }, (item.x, item.y + self.strip_top(), item.w, item.h), Button::Left)
            }
            _ => false,
        }
    }
    pub(super) fn dux_tick(&mut self) -> bool {
        let generation = crate::theme::generation();
        if self.dux_material_generation != generation && (self.dux() || self.cc_style() == controlcenter::settings::Style::Dux) {
            self.symbols.prepare_material();
            self.dux_material_generation = generation;
        }
        use crate::panel::AppletId as A;
        let p = self.panel();
        // the pill rail follows the focused window itself (see tick_inner)
        let redraw = if !self.pill_bar() && (self.dux() || p.shows(A::WindowTitle)) { self.v_refresh_window() } else { false };
        if cfg!(test) {
            return redraw;
        }
        if !p.shows(A::Media) {
            self.dux_mpris = None;
        }
        if p.shows(A::Media) && self.dux_mpris.is_none() && self.waker.is_some() {
            let shared = self.shared.clone();
            let wake = self.waker.clone();
            self.dux_mpris = Some(mpris::Mpris::start(Arc::new(move || {
                shared.changed.store(true, Ordering::SeqCst);
                if let Some(w) = &wake {
                    w.wake();
                }
            })));
        }
        if !p.shows(A::Cpu) && !p.shows(A::Memory) {
            return redraw;
        }
        if self.dux_refresh.elapsed() < Duration::from_secs(2) || self.dux_busy.swap(true, Ordering::SeqCst) {
            return redraw;
        }
        self.dux_refresh = Instant::now();
        let sampler = self.dux_sampler.clone();
        let result = self.dux_perf.clone();
        let busy = self.dux_busy.clone();
        let shared = self.shared.clone();
        let wake = self.waker.clone();
        std::thread::spawn(move || {
            struct BusyGuard(Arc<AtomicBool>);
            impl Drop for BusyGuard {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::SeqCst);
                }
            }
            let _busy = BusyGuard(busy);
            let values = sampler.lock().ok().map(|mut s| {
                let stats = s.sample(Instant::now());
                (stats.primed.then_some(stats.cpu), (stats.mem.total_kb > 0).then_some(stats.mem.used_frac()))
            });
            if let Ok(mut output) = result.lock() {
                *output = values;
            }
            shared.changed.store(true, Ordering::SeqCst);
            if let Some(w) = wake {
                w.wake();
            }
        });
        redraw
    }

    pub(super) fn dux_samples_resources(&self) -> bool {
        let p = self.panel();
        p.shows(crate::panel::AppletId::Cpu) || p.shows(crate::panel::AppletId::Memory)
    }
}

#[cfg(test)]
mod cluster_tests;

#[cfg(test)]
mod tests {
    use super::*;
    /// Process-global theme/font publication: run alone with private config and package fonts.
    #[test]
    #[ignore = "isolated actual-font Dux bar raster evidence"]
    fn dux_bar_native_render_evidence() {
        use serde_json::json;
        let output = std::path::PathBuf::from(std::env::var_os("DUX_BAR_EVIDENCE_DIR").expect("explicit private evidence path required"));
        assert!(std::env::var_os("DECEBAL_CONFIG").is_some(), "explicit private config required");
        std::fs::create_dir_all(&output).unwrap();
        let source = crate::theme::live::Source::discover();
        let selection = source.selection();
        let mut ledger = Vec::new();
        for (mode, dark) in [(crate::theme::model::Mode::Dark, true), (crate::theme::model::Mode::Light, false)] {
            let mut selection = selection.clone();
            selection.mode = mode;
            let base = source.build(&selection, dark).0;
            assert!(!base.fonts.files.is_empty(), "real package Google Sans/Material font files required");
            assert!(base.fonts.package_faces.iter().flat_map(|face| &face.names).any(|name| name == "Google Sans Flex"), "actual Google Sans face required");
            for scale in [1.0, 2.0] {
                for band in [38.0, 26.0] {
                    for width in [720, 1280, 1920] {
                        let mut theme = base.clone();
                        theme.bar_style = "dux".into();
                        theme.bar_height = band;
                        theme.font_scale = scale;
                        theme.bar_layout = crate::config::BarLayout::default();
                        crate::theme::live::publish_theme(theme);
                        let mut bar = DecBar::new(Box::new(active::StaticApp("Native renderer fixture".into())));
                        assert!(bar.symbols.get_material("apps", ink(), 14).is_some(), "actual Material Symbols outline missing");
                        let family = bar.fonts.as_ref().and_then(|fonts| fonts.family_for(crate::text::Role::Ui, 'A')).unwrap();
                        assert!(family.contains("Google Sans"), "unexpected native fallback: {family}");
                        bar.width = width;
                        bar.clock_text = "14:07  •  Fri 30".into();
                        bar.workspaces = (0..3).map(|n| Workspace { name: (n + 1).to_string(), active: n == 0, occupied: n < 2, urgent: false }).collect();
                        bar.v_win = Some(("fixture".into(), "Native renderer fixture".into()));
                        *bar.shared.snap.lock().unwrap() = Snapshot::default();
                        *bar.shared.weather.lock().unwrap() = None;
                        let layout = bar.dux_layout();
                        assert!(!layout.groups.is_empty());
                        let logo = layout.items.iter().find(|item| item.kind == Kind::Tray(Tray::Logo)).unwrap();
                        assert_eq!((logo.x, logo.y, logo.h), (8.0, 9.0, (band - 10.0).max(16.0)));
                        let material = bar.dux_regions();
                        assert_eq!((material[0].x, material[0].y, material[0].w, material[0].h), (4.0, 4.0, width as f32 - 8.0, band));
                        assert_eq!(bar.surface_spec().unwrap().reserve, (band + 8.0) as i32);
                        assert!(!bar.input_rects().iter().any(|r| r.x <= 0 && r.y <= 0));
                        for g in &layout.groups {
                            assert_eq!(g.h, (band - 10.0).max(16.0));
                            assert!(g.x >= 0.0 && g.x + g.w <= width as f32);
                        }
                        for (index, g) in layout.groups.iter().enumerate() {
                            for other in layout.groups.iter().skip(index + 1) {
                                assert!(g.x + g.w <= other.x || other.x + other.w <= g.x);
                            }
                        }
                        let mut pixels = Pixmap::new(width, (bar.bar_bottom() + 16.0).ceil() as u32).unwrap();
                        bar.draw(&mut pixels);
                        assert!(pixels.data().chunks_exact(4).any(|p| p[3] > 0));
                        assert_eq!(pixels.pixel(0, 0).unwrap().alpha(), 0);
                        let name = format!("bar-{}-{}-{}-{}.png", if dark { "dark" } else { "light" }, (scale * 100.0) as u32, band as u32, width);
                        pixels.save_png(output.join(&name)).unwrap();
                        ledger.push(json!({"png":name,"mode":if dark { "dark" } else { "light" },"font":family,"scale":scale,"band":band,"width":width,"input":bar.input_rects().iter().map(|r|json!([r.x,r.y,r.w,r.h])).collect::<Vec<_>>(),"groups":layout.groups.iter().map(|g|json!([g.x,g.y,g.w,g.h,g.radius])).collect::<Vec<_>>(),"items":layout.items.iter().map(|i|json!({"kind":format!("{:?}",i.kind),"rect":[i.x,i.y,i.w,i.h]})).collect::<Vec<_>>(),"cpu":null,"memory":null,"weather":null,"media":null,"background":"band"}));
                    }
                }
            }
        }
        std::fs::write(output.join("bar-layouts.json"), serde_json::to_string_pretty(&ledger).unwrap()).unwrap();
    }

    #[test]
    fn default_index_centering_keeps_clock_centred_when_music_is_missing() {
        let layout = place(1920.0, 8.0, 4.0, 28.0, 9.0, 14.0, [vec![], vec![(Kind::Tray(Tray::Clock), 100.0), (Kind::Tray(Tray::Weather), 60.0)], vec![]], None);
        let clock = layout.items.iter().find(|i| i.kind == Kind::Tray(Tray::Clock)).unwrap();
        assert_eq!(clock.x + clock.w / 2.0, 960.0);
    }

    #[test]
    fn three_groups_never_overlap_and_gaps_are_not_input() {
        for scale in [1.0, 2.0] {
            for width in [180.0, 320.0, 480.0, 800.0, 1280.0, 1920.0] {
                let groups = [
                    vec![(Kind::Tray(Tray::Logo), 28.0 * scale), (Kind::Tray(Tray::Workspaces), 90.0 * scale), (Kind::Tray(Tray::Window), 160.0 * scale)],
                    vec![(Kind::Media, 114.0 * scale), (Kind::Tray(Tray::Clock), 100.0 * scale)],
                    vec![(Kind::Notifications, 28.0 * scale), (Kind::Tray(Tray::Control), 28.0 * scale)],
                ];
                let layout = place(width, 8.0 * scale, 4.0 * scale, 28.0 * scale, 5.0 * scale, 14.0 * scale, groups, None);
                for (index, g) in layout.groups.iter().enumerate() {
                    assert!(g.x >= 0.0 && g.x + g.w <= width);
                    for other in layout.groups.iter().skip(index + 1) {
                        assert!(g.x + g.w <= other.x || other.x + other.w <= g.x);
                    }
                }
                assert!(layout.hit(0.0, 0.0).is_none());
            }
        }
    }
}
