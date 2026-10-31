//! The Settings window's accessibility adapter: the AT-SPI tree of what was last drawn, and the narrow semantic
//! dispatcher that runs assistive-technology requests through the same typed paths as the keyboard
//! ([`Model::apply`], `open_popup`, `begin_edit`/`commit_edit`, focus and scrolling). Nothing here synthesises
//! pointer or key events.
//!
//! Keys (stable node ids): `frame`, `sidebar`, `search`, `search:clear`, `pages`, `pane:<pane>`,
//! `hit:<pane>:<label>`, `content`, `title`, `ctl:<control id>`, `ctl:<id>/opt:<i>` (segments, tabs, list
//! choices), `ctl:<id>/menu`, `colorpop`, `colorpop:sw:<i>`, `colorpop:hex`, `colorpop:default`, `toast`, plus
//! content-derived keys for headings, notes, cards and galleries.

use super::{SbRow, SettingsApp, Zone, SWATCHES};
use crate::a11y::{ActionDef, ActionError, NodeId, Op, Role, Spec, State, Target, TextContent, TextInfo, TreeBuilder, ValueInfo};
use crate::settings::model::CtlVal;
use crate::settings::panes::PANES;
use crate::settings::system::registry::SECTIONS;
use crate::settings::ui::{self, Item, Kind, Placed, Rect};
use crate::text::Weight;
use crate::wallhaven::textfield::TextField;
use crate::window_app::Response;

/// Most choices exported per list (a font list can be long; the rest is reachable by type-ahead in the open list).
const MAX_CHOICES: usize = 500;

fn ir(r: Rect) -> Option<crate::a11y::Rect> {
    Some((r.0.round() as i32, r.1.round() as i32, r.2.round().max(0.0) as i32, r.3.round().max(0.0) as i32))
}

fn clip_key(s: &str) -> String {
    s.chars().take(48).collect()
}

fn section_of(pane: &str) -> &'static str {
    SECTIONS.iter().find(|s| s.panes.contains(&pane)).map_or("", |s| s.title)
}

struct Ctx<'a> {
    app: &'a SettingsApp,
    w: f32,
    h: f32,
    sbw: f32,
}

impl Ctx<'_> {
    fn showing_content(&self, r: Rect) -> bool {
        r.1 + r.3 > 0.0 && r.1 < self.h && r.0 + r.2 > self.sbw && r.0 < self.w && r.2 > 0.0
    }
    fn showing_sidebar(&self, r: Rect) -> bool {
        r.1 + r.3 > self.app.nav_top() && r.1 < self.h
    }
}

fn reset_able(id: &str) -> bool {
    id.starts_with("r:") || id.starts_with("j:")
}

/// The focus key: what the keyboard would act on right now.
fn focus_key(app: &SettingsApp) -> Option<String> {
    if let Some(pp) = &app.popup {
        return Some(match pp.hover {
            Some(i) => format!("ctl:{}/opt:{i}", pp.id),
            None => format!("ctl:{}/menu", pp.id),
        });
    }
    if app.colorpop.is_some() {
        return Some("colorpop:hex".into());
    }
    match app.zone {
        Zone::Search => Some("search".into()),
        Zone::Sidebar => {
            let id = PANES.get(app.m.sel.pane)?.id;
            let row = app.sidebar_hits.iter().zip(&app.sidebar_labels).find(|((_, i), l)| *i == app.m.sel.pane && l.is_none());
            Some(match row {
                Some(_) => format!("pane:{id}"),
                None => "pages".into(),
            })
        }
        Zone::Content => {
            let f = app.it.focus.as_ref()?;
            let p = app.frame.find(f)?;
            Some(match &p.ctl.kind {
                Kind::Seg(_, Some(i)) => format!("ctl:{f}/opt:{i}"),
                _ => format!("ctl:{f}"),
            })
        }
    }
}

/// Builds the tree of the last frame.
pub(super) fn tree(app: &SettingsApp, b: &mut TreeBuilder) {
    let (w, h) = (app.size.0 as f32, app.size.1 as f32);
    let c = Ctx { app, w, h, sbw: app.sb() };
    let Some(frame) = b.add(NodeId::ROOT, Spec::new("frame", Role::Frame, "System Settings").bounds(ir((0.0, 0.0, w, h))).state(State::Resizable, true).state(State::Showing, true)) else { return };
    sidebar(&c, b, frame);
    content(&c, b, frame);
    overlays(&c, b, frame);
    if let Some(k) = focus_key(app) {
        b.focus(&k);
    }
}

fn sidebar(c: &Ctx, b: &mut TreeBuilder, frame: NodeId) {
    let app = c.app;
    let Some(sb) = b.add(frame, Spec::new("sidebar", Role::Panel, "Sidebar").bounds(ir((0.0, 0.0, c.sbw, c.h))).state(State::Showing, true)) else { return };
    let sr = app.search_rect;
    let f = &app.search;
    b.add(
        sb,
        Spec::new("search", Role::Entry, "Search settings")
            .bounds(ir(sr))
            .state(State::Showing, true)
            .state(State::Focusable, true)
            .state(State::Editable, true)
            .state(State::SingleLine, true)
            .state(State::SelectableText, true)
            .text(TextInfo::plain(f.text(), f.cursor(), f.selection(), true))
            .attr("placeholder-text", "Search settings")
            .action(ActionDef::new("activate", "Open the first result").key("Return")),
    );
    if !f.text().is_empty() {
        let s = app.s;
        b.add(
            sb,
            Spec::new("search:clear", Role::PushButton, "Clear search")
                .bounds(ir((sr.0 + sr.2 - 28.0 * s, sr.1, 28.0 * s, sr.3)))
                .state(State::Showing, true)
                .action(ActionDef::new("click", "Clear the search text")),
        );
    }
    let top = app.nav_top();
    let Some(list) = b.add(
        sb,
        Spec::new("pages", Role::List, if f.text().trim().is_empty() { "Settings pages" } else { "Search results" })
            .bounds(ir((0.0, top, c.sbw, (c.h - top).max(0.0))))
            .state(State::Showing, true)
            .selection(),
    ) else {
        return;
    };
    for ((rect, i), label) in app.sidebar_hits.iter().zip(&app.sidebar_labels) {
        let Some(def) = PANES.get(*i) else { continue };
        let current = *i == app.m.sel.pane;
        let (key, name, desc) = match label {
            None => (format!("pane:{}", def.id), def.title.to_string(), section_of(def.id).to_string()),
            Some(l) => (format!("hit:{}:{}", def.id, clip_key(l)), l.clone(), def.title.to_string()),
        };
        b.add(
            list,
            Spec::new(key, Role::ListItem, name)
                .description(desc)
                .bounds(ir(*rect))
                .state(State::Showing, c.showing_sidebar(*rect))
                .state(State::Selectable, true)
                .state(State::Selected, current && label.is_none())
                .state(State::Focusable, true)
                .action(ActionDef::new("activate", "Open this page")),
        );
    }
    if app.sidebar_hits.is_empty() {
        b.add(sb, Spec::new("noresults", Role::Label, "No results").state(State::Showing, true));
    }
}

fn content(c: &Ctx, b: &mut TreeBuilder, frame: NodeId) {
    let app = c.app;
    let title = PANES[app.m.sel.pane.min(PANES.len() - 1)].title;
    let Some(pane) =
        b.add(frame, Spec::new("content", Role::ScrollPane, title).bounds(ir((c.sbw, 0.0, c.w - c.sbw, c.h))).state(State::Showing, true).attr("scroll-offset", format!("{}", app.scroll.round())))
    else {
        return;
    };
    b.add(pane, Spec::new("title", Role::Heading, title).bounds(ir(app.frame.head)).state(State::Showing, c.showing_content(app.frame.head)).attr("level", "1"));
    let context = app.page_context();
    if !context.is_empty() {
        b.add(pane, Spec::new("context", Role::Label, context).bounds(ir(app.frame.head)).state(State::Showing, c.showing_content(app.frame.head)));
    }
    let Some((_, items)) = &app.items_cache else { return };
    let mut header: Option<String> = None;
    let mut dialog: Option<NodeId> = None;
    for (i, item) in items.iter().enumerate() {
        let Some((area, range)) = app.frame.spans.get(i).cloned() else { break };
        let placed: Vec<&Placed> = app.frame.placed.get(range).map(|v| v.iter().collect()).unwrap_or_default();
        let showing = c.showing_content(area);
        // The explicit widget-layout choice (a note and its buttons) is a dialog: nothing changes until it is answered.
        let is_choice_note =
            app.m.widget_choice.is_some() && matches!(item, Item::Note { .. }) && matches!(items.get(i + 1), Some(Item::Card(rows)) if rows.iter().any(|r| r.ctls.iter().any(|x| x.id == "wc:keep")));
        if is_choice_note {
            dialog = b.add(
                pane,
                Spec::new("dialog:widget-choice", Role::Dialog, "Choose the widget layout")
                    .bounds(ir(area))
                    .state(State::Showing, showing)
                    .action(ActionDef::new("close", "Cancel the choice").key("Escape")),
            );
        }
        let parent = match (dialog, item) {
            (Some(d), Item::Note { .. }) if is_choice_note => d,
            (Some(d), Item::Card(rows)) if rows.iter().any(|r| r.ctls.iter().any(|x| x.id.starts_with("wc:"))) => d,
            _ => pane,
        };
        match item {
            Item::Space(_) => {}
            Item::Header(t) => {
                header = Some(t.clone());
                b.add(parent, Spec::new(format!("h:{}", clip_key(t)), Role::Heading, t.clone()).bounds(ir(area)).state(State::Showing, showing).attr("level", "2"));
            }
            Item::Note { text, warn } => {
                let role = if *warn { Role::Alert } else { Role::Label };
                b.add(parent, Spec::new(format!("note:{}", clip_key(text)), role, text.clone()).bounds(ir(area)).state(State::Showing, showing));
            }
            Item::Card(rows) => {
                let first = rows.iter().find_map(|r| r.ctls.first().map(|x| x.id.clone())).or_else(|| rows.first().map(|r| r.label.clone())).unwrap_or_default();
                let name = header.clone().unwrap_or_default();
                let Some(card) = b.add(parent, Spec::new(format!("card:{}", clip_key(&first)), Role::Panel, name).bounds(ir(area)).state(State::Showing, showing)) else { continue };
                let mut k = 0usize;
                for row in rows {
                    let n = row.ctls.len();
                    let ps: Vec<&Placed> = placed.get(k..k + n).map(|v| v.to_vec()).unwrap_or_default();
                    k += n;
                    card_row(c, b, card, row, &ps, area);
                }
            }
            Item::Nav { id, secondary, .. } => {
                if let Some(p) = placed.first() {
                    let name = if *secondary { "Sections" } else { "Tabs" };
                    choice_group(c, b, parent, p, Role::PageTabList, Role::PageTab, name, "");
                }
                let _ = id;
            }
            Item::Thumbs(list) => gallery(c, b, parent, &placed, header.as_deref().unwrap_or("Wallpapers"), area, |i| list.get(i).map(|t| (t.selected, String::new()))),
            Item::Layouts(list) => gallery(c, b, parent, &placed, header.as_deref().unwrap_or("Layouts"), area, |i| list.get(i).map(|t| (t.selected, t.symbol.clone()))),
            Item::Themes(list) => gallery(c, b, parent, &placed, header.as_deref().unwrap_or("Themes"), area, |i| {
                list.get(i).map(|t| (t.selected, if t.error { "This theme has an error".into() } else { String::new() }))
            }),
            Item::Swatches(list) => gallery(c, b, parent, &placed, header.as_deref().unwrap_or("Colours"), area, |i| {
                list.get(i).map(|t| (t.selected, if t.unset { "No override (theme default)".into() } else { format!("#{:02x}{:02x}{:02x}", t.color.0, t.color.1, t.color.2) }))
            }),
            Item::IconThemes(rows) => gallery(c, b, parent, &placed, header.as_deref().unwrap_or("Icon themes"), area, |i| rows.get(i).map(|r| (r.current, r.detail.clone()))),
            Item::ThemeCards(cards) => {
                for card in cards {
                    let mut desc = vec![card.meta.clone(), card.description.clone()];
                    if let Some(e) = &card.error {
                        desc.push(format!("Cannot be used: {e}"));
                    }
                    if card.current {
                        desc.insert(0, "Current theme".into());
                    }
                    let Some(panel) = b.add(
                        parent,
                        Spec::new(format!("tcard:{}", clip_key(&card.id)), Role::Panel, card.name.clone())
                            .description(desc.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(". "))
                            .bounds(ir(area))
                            .state(State::Showing, showing),
                    ) else {
                        continue;
                    };
                    for p in placed.iter().filter(|p| p.ctl.id.ends_with(&format!(":{}", card.id)) && p.ctl.id.starts_with("l:tcard:")) {
                        control(c, b, panel, p, &p.label.clone(), "");
                    }
                }
            }
            Item::Palette(chips) => {
                let Some(l) = b.add(parent, Spec::new(format!("palette:{}", i), Role::List, header.clone().unwrap_or_else(|| "Palette".into())).bounds(ir(area)).state(State::Showing, showing)) else {
                    continue;
                };
                for ch in chips {
                    let (r, g, bb) = ch.color;
                    b.add(
                        l,
                        Spec::new(format!("chip:{}", clip_key(&ch.name)), Role::ListItem, ch.name.clone())
                            .description(format!("#{r:02x}{g:02x}{bb:02x}"))
                            .bounds(ir(area))
                            .state(State::Showing, showing),
                    );
                }
            }
            Item::Preview { id, caption } => {
                b.add(parent, Spec::new(format!("preview:{}", clip_key(id)), Role::Image, caption.clone()).bounds(ir(area)).state(State::Showing, showing));
            }
            Item::WidgetPreview { caption, .. } => {
                b.add(
                    parent,
                    Spec::new(format!("wpreview:{}", clip_key(caption)), Role::Image, if caption.is_empty() { "Widget preview".to_string() } else { caption.clone() })
                        .bounds(ir(area))
                        .state(State::Showing, showing),
                );
            }
            Item::IconGrid(_) => {
                b.add(parent, Spec::new(format!("icongrid:{i}"), Role::Image, "Icon preview").bounds(ir(area)).state(State::Showing, showing));
            }
            Item::FontSamples(lines) => {
                let Some(l) = b.add(parent, Spec::new(format!("fonts:{i}"), Role::Panel, header.clone().unwrap_or_else(|| "Font preview".into())).bounds(ir(area)).state(State::Showing, showing))
                else {
                    continue;
                };
                for s in lines {
                    b.add(l, Spec::new(format!("font:{}", clip_key(&s.label)), Role::Label, s.label.clone()).description(s.text.clone()).bounds(ir(area)).state(State::Showing, showing));
                }
            }
            Item::Editor(v) => {
                let Some(p) = placed.first() else { continue };
                let mut text = String::new();
                let (mut caret, mut anchor) = (0usize, None);
                for (li, l) in v.lines.iter().enumerate() {
                    if li == v.cursor.line {
                        caret = text.chars().count() + v.cursor.col.min(l.chars().count());
                    }
                    if let Some(a) = v.anchor.filter(|a| a.line == li) {
                        anchor = Some(text.chars().count() + a.col.min(l.chars().count()));
                    }
                    if li > 0 {
                        text.push('\n');
                        if li == v.cursor.line {
                            caret += 1;
                        }
                        if v.anchor.is_some_and(|a| a.line == li) {
                            anchor = anchor.map(|x| x + 1);
                        }
                    }
                    text.push_str(l);
                }
                let sel = anchor.filter(|a| *a != caret).map(|a| (a.min(caret), a.max(caret)));
                let mut desc = vec![v.status.clone()];
                desc.extend(v.errors.iter().map(|(l, e)| format!("Line {}: {e}", l + 1)));
                let editable = !v.read_only;
                b.add(
                    parent,
                    Spec::new(format!("ctl:{}", p.ctl.id), Role::Text, "Configuration file")
                        .description(desc.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(". "))
                        .bounds(ir(p.rect))
                        .state(State::Showing, c.showing_content(p.rect))
                        .state(State::Focusable, true)
                        .state(State::MultiLine, true)
                        .state(State::Editable, editable)
                        .state(State::ReadOnly, !editable)
                        .state(State::SelectableText, true)
                        .state(State::InvalidEntry, !v.errors.is_empty())
                        .text(TextInfo { content: TextContent::Plain(text), caret, selection: sel, editable, multi_line: true })
                        .action(ActionDef::new("activate", if v.active { "Editing" } else { "Start editing" }).key("Return")),
                );
            }
        }
    }
}

fn card_row(c: &Ctx, b: &mut TreeBuilder, card: NodeId, row: &ui::Row, ps: &[&Placed], area: Rect) {
    let ctls: Vec<&&Placed> = ps.iter().collect();
    let row_rect = ps.first().map(|p| p.row_rect).unwrap_or(area);
    let showing = c.showing_content(row_rect);
    match ctls.len() {
        0 => {
            if !row.label.is_empty() {
                b.add(
                    card,
                    Spec::new(format!("row:{}", clip_key(&row.label)), Role::Label, row.label.clone())
                        .description(row.detail.clone())
                        .bounds(ir(row_rect))
                        .state(State::Showing, showing)
                        .state(State::Selected, row.selected),
                );
            }
        }
        1 => {
            let p = ctls[0];
            let own = match &p.ctl.kind {
                Kind::Button { label, .. } | Kind::Chip { label, .. } => label.clone(),
                _ => String::new(),
            };
            // A lone button reads its own caption; the row label (if different) becomes its description.
            let (name, desc) = if !own.is_empty() && !row.label.is_empty() && own != row.label {
                (own, [row.label.as_str(), row.detail.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(". "))
            } else if row.label.is_empty() {
                (own, row.detail.clone())
            } else {
                (row.label.clone(), row.detail.clone())
            };
            control(c, b, card, p, &name, &desc);
        }
        _ => {
            let Some(g) = b.add(
                card,
                Spec::new(format!("row:{}", clip_key(if row.label.is_empty() { &ps[0].ctl.id } else { &row.label })), Role::Grouping, row.label.clone())
                    .description(row.detail.clone())
                    .bounds(ir(row_rect))
                    .state(State::Showing, showing)
                    .state(State::Selected, row.selected),
            ) else {
                return;
            };
            // Buttons read their caption; other controls take the row label (numbered when several share it).
            let unnamed = ps.iter().filter(|p| !matches!(p.ctl.kind, Kind::Button { .. } | Kind::Chip { .. })).count();
            let mut n = 0;
            for p in ps.iter() {
                let own = match &p.ctl.kind {
                    Kind::Button { label, .. } | Kind::Chip { label, .. } => label.clone(),
                    _ if unnamed > 1 => {
                        n += 1;
                        format!("{} {n}", row.label)
                    }
                    _ => row.label.clone(),
                };
                control(c, b, g, p, &own, "");
            }
        }
    }
}

/// A segmented choice or a tab strip: a selection container of radio buttons / page tabs.
fn choice_group(c: &Ctx, b: &mut TreeBuilder, parent: NodeId, p: &Placed, role: Role, item: Role, name: &str, desc: &str) {
    let Kind::Seg(opts, sel) = &p.ctl.kind else { return };
    let id = &p.ctl.id;
    let mut spec = Spec::new(format!("ctl:{id}"), role, name)
        .description(desc)
        .bounds(ir(p.rect))
        .state(State::Showing, c.showing_content(p.rect))
        .state(State::Focusable, p.ctl.focusable())
        .enabled(p.ctl.enabled)
        .selection();
    if reset_able(id) {
        spec = spec.action(ActionDef::new("reset", "Reset to the default"));
    }
    let Some(g) = b.add(parent, spec) else { return };
    for (i, (label, _)) in opts.iter().enumerate().take(MAX_CHOICES) {
        let r = p.parts.get(i).copied().unwrap_or(p.rect);
        b.add(
            g,
            Spec::new(format!("ctl:{id}/opt:{i}"), item, label.clone())
                .bounds(ir(r))
                .state(State::Showing, c.showing_content(r))
                .enabled(p.ctl.enabled)
                .state(State::Selectable, true)
                .state(State::Selected, *sel == Some(i))
                .state(State::Checkable, item == Role::RadioButton)
                .state(State::Checked, item == Role::RadioButton && *sel == Some(i))
                .state(State::Focusable, p.ctl.focusable())
                .action(ActionDef::new("click", "Choose this option")),
        );
    }
}

fn gallery(c: &Ctx, b: &mut TreeBuilder, parent: NodeId, placed: &[&Placed], name: &str, area: Rect, info: impl Fn(usize) -> Option<(bool, String)>) {
    let key = placed.first().map(|p| p.ctl.id.clone()).unwrap_or_else(|| name.to_string());
    let Some(l) = b.add(parent, Spec::new(format!("gallery:{}", clip_key(&key)), Role::List, name).bounds(ir(area)).state(State::Showing, c.showing_content(area)).selection()) else { return };
    for (i, p) in placed.iter().enumerate() {
        let (selected, desc) = info(i).unwrap_or_default();
        b.add(
            l,
            Spec::new(format!("ctl:{}", p.ctl.id), Role::ListItem, p.label.clone())
                .description(desc)
                .bounds(ir(p.rect))
                .state(State::Showing, c.showing_content(p.rect))
                .enabled(p.ctl.enabled)
                .state(State::Selectable, true)
                .state(State::Selected, selected)
                .state(State::Focusable, p.ctl.focusable())
                .action(ActionDef::new("activate", "Choose this").key("Return")),
        );
    }
}

/// One control of a card row.
fn control(c: &Ctx, b: &mut TreeBuilder, parent: NodeId, p: &Placed, name: &str, desc: &str) {
    let app = c.app;
    let id = &p.ctl.id;
    let key = format!("ctl:{id}");
    let showing = c.showing_content(p.rect);
    let base = |role: Role, name: &str| {
        Spec::new(key.clone(), role, name).description(desc).bounds(ir(p.rect)).state(State::Showing, showing).enabled(p.ctl.enabled).state(State::Focusable, p.ctl.focusable())
    };
    let reset = |s: Spec| if reset_able(id) { s.action(ActionDef::new("reset", "Reset to the default")) } else { s };
    let spec = match &p.ctl.kind {
        Kind::Toggle(on) => reset(base(Role::Switch, name).state(State::Checkable, true).state(State::Checked, *on).action(ActionDef::new("toggle", "Turn on or off").key("space"))),
        Kind::Slider { v, min, max, step, unit } | Kind::Stepper { v, min, max, step, unit } => {
            let shown = app.it.drag.as_ref().filter(|(d, _)| d == id).map_or(*v, |(_, d)| *d);
            let role = if matches!(p.ctl.kind, Kind::Slider { .. }) { Role::Slider } else { Role::SpinButton };
            reset(
                base(role, name)
                    .state(State::Horizontal, role == Role::Slider)
                    .value(ValueInfo { current: shown, min: *min, max: *max, step: *step, text: format!("{}{}", ui::format_num(shown, *step), unit) })
                    .action(ActionDef::new("increment", "Increase").key("Right"))
                    .action(ActionDef::new("decrement", "Decrease").key("Left")),
            )
        }
        Kind::Seg(..) => {
            choice_group(c, b, parent, p, Role::Grouping, Role::RadioButton, name, desc);
            return;
        }
        Kind::Popup(opts, sel) => {
            let open = app.popup.as_ref().filter(|pp| pp.id == *id);
            let current = sel.and_then(|i| opts.get(i)).map_or("Choose…".to_string(), |o| o.0.clone());
            let d = if desc.is_empty() { current.clone() } else { format!("{current}. {desc}") };
            let spec = reset(
                Spec::new(key.clone(), Role::ComboBox, name)
                    .description(d)
                    .bounds(ir(p.rect))
                    .state(State::Showing, showing)
                    .enabled(p.ctl.enabled)
                    .state(State::Focusable, p.ctl.focusable())
                    .state(State::HasPopup, true)
                    .state(State::Expandable, true)
                    .state(State::Expanded, open.is_some())
                    .state(State::Collapsed, open.is_none())
                    .action(ActionDef::new("press", if open.is_some() { "Close the list" } else { "Open the list" }).key("Return")),
            );
            let Some(combo) = b.add(parent, spec) else { return };
            choice_menu(c, b, combo, p, name, opts, *sel);
            return;
        }
        Kind::Color(hex) => reset(
            base(Role::PushButton, name)
                .state(State::HasPopup, true)
                .attr("color", if hex.is_empty() { "default".to_string() } else { hex.clone() })
                .description(if hex.is_empty() { "Theme default".to_string() } else { hex.clone() })
                .action(ActionDef::new("press", "Choose a colour").key("Return")),
        ),
        Kind::Text { val, hint } => {
            let secret = id.contains(":secret");
            let editing = app.edit.as_ref().filter(|(e, _)| e == id).map(|(_, f)| f);
            let (shown, caret, sel) = match editing {
                Some(f) => (f.text().to_string(), f.cursor(), f.selection()),
                None => (val.clone(), val.chars().count(), None),
            };
            let content = if secret { TextContent::Masked { chars: shown.chars().count() } } else { TextContent::Plain(shown) };
            let mut s = reset(base(if secret { Role::PasswordText } else { Role::Entry }, name))
                .state(State::Editable, p.ctl.enabled)
                .state(State::SingleLine, true)
                .state(State::SelectableText, !secret)
                .text(TextInfo { content, caret, selection: sel, editable: p.ctl.enabled, multi_line: false })
                .action(ActionDef::new("activate", if editing.is_some() { "Save the text" } else { "Edit the text" }).key("Return"));
            if !hint.is_empty() {
                s = s.attr("placeholder-text", hint.clone());
            }
            if editing.is_some() {
                s = s.attr("draft", "true");
            }
            s
        }
        Kind::Key(k) => {
            let recording = app.it.recording.as_ref().is_some_and(|(r, _)| r == id);
            base(Role::PushButton, name)
                .description(if recording {
                    "Type the new shortcut".to_string()
                } else if desc.is_empty() {
                    k.clone()
                } else {
                    format!("{k}. {desc}")
                })
                .state(State::Pressed, recording)
                .state(State::Armed, recording)
                .attr("shortcut", k.clone())
                .action(ActionDef::new("click", "Record a new shortcut").key("Return"))
        }
        Kind::Button { danger, .. } => {
            let mut s = base(Role::PushButton, name).action(ActionDef::new("click", "Press").key("Return"));
            if *danger {
                s = s.attr("destructive", "true");
            }
            s
        }
        Kind::Chip { on, .. } => base(Role::ToggleButton, name).state(State::Checkable, true).state(State::Checked, *on).state(State::Pressed, *on).action(ActionDef::new("click", "Turn on or off")),
        Kind::Label(t) => Spec::new(key.clone(), Role::Label, if name.is_empty() { t.clone() } else { name.to_string() })
            .description([t.as_str(), desc].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(". "))
            .bounds(ir(p.rect))
            .state(State::Showing, showing),
    };
    b.add(parent, spec);
}

/// The choices of a list control; while its popup is open they are on screen and keyboard focus moves among them.
fn choice_menu(c: &Ctx, b: &mut TreeBuilder, combo: NodeId, p: &Placed, name: &str, opts: &[ui::Opt], sel: Option<usize>) {
    let app = c.app;
    let id = &p.ctl.id;
    let open = app.popup.as_ref().filter(|pp| pp.id == *id);
    let mut spec =
        Spec::new(format!("ctl:{id}/menu"), Role::Menu, name).bounds(ir(open.map_or(p.rect, |pp| pp.rect))).state(State::Showing, open.is_some()).state(State::Modal, open.is_some()).selection();
    if open.is_some() {
        spec = spec.action(ActionDef::new("close", "Close the list").key("Escape"));
    }
    let Some(menu) = b.add(combo, spec) else { return };
    let ih = 26.0 * app.s;
    for (i, (label, _)) in opts.iter().enumerate().take(MAX_CHOICES) {
        let row = open.filter(|pp| i >= pp.top && i < pp.top + pp.visible).map(|pp| (pp.rect.0 + 4.0 * app.s, pp.rect.1 + 4.0 * app.s + (i - pp.top) as f32 * ih, pp.rect.2 - 8.0 * app.s, ih));
        let selected = open.map_or(sel, |pp| pp.sel) == Some(i);
        b.add(
            menu,
            Spec::new(format!("ctl:{id}/opt:{i}"), Role::MenuItem, label.clone())
                .bounds(ir(row.unwrap_or(p.rect)))
                .state(State::Showing, row.is_some())
                .enabled(p.ctl.enabled)
                .state(State::Selectable, true)
                .state(State::Selected, selected)
                .state(State::Checked, selected)
                .state(State::Focusable, true)
                .action(ActionDef::new("click", "Choose this")),
        );
    }
    if opts.len() > MAX_CHOICES {
        b.add(menu, Spec::new(format!("ctl:{id}/more"), Role::Label, format!("{} more choices: open the list and type to find them", opts.len() - MAX_CHOICES)));
    }
}

fn overlays(c: &Ctx, b: &mut TreeBuilder, frame: NodeId) {
    let app = c.app;
    let s = app.s;
    if let Some(n) = app.m.notice.as_ref().filter(|n| !n.error) {
        let tw = app.fonts.as_ref().map_or(n.text.chars().count() as f32 * 7.0 * s, |f| f.measure(&n.text, 12.5 * s, Weight::Medium)) + 44.0 * s;
        let r = (c.sbw + (c.w - c.sbw - tw) / 2.0, c.h - 52.0 * s, tw, 32.0 * s);
        // Busy while accepted changes are still on their way to the disk: the toast says so, and never "Saved".
        b.add(frame, Spec::new("toast", Role::Notification, n.text.clone()).bounds(ir(r)).state(State::Showing, true).state(State::Busy, app.m.saving()));
    }
    if let Some(cp) = &app.colorpop {
        let r = cp.rect;
        let name = app.frame.find(&cp.id).map(|p| p.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| "Colour".into());
        let Some(d) = b.add(
            frame,
            Spec::new("colorpop", Role::Dialog, format!("Choose a colour: {name}"))
                .bounds(ir(r))
                .state(State::Showing, true)
                .state(State::Modal, true)
                .action(ActionDef::new("close", "Close without changing").key("Escape")),
        ) else {
            return;
        };
        let Some(list) = b.add(d, Spec::new("colorpop:swatches", Role::List, "Swatches").bounds(ir(r)).state(State::Showing, true)) else { return };
        for (i, hex) in SWATCHES.iter().enumerate() {
            let (col, row) = (i % 6, i / 6);
            let (cx, cy) = (r.0 + 22.0 * s + col as f32 * 30.0 * s, r.1 + 22.0 * s + row as f32 * 30.0 * s);
            b.add(
                list,
                Spec::new(format!("colorpop:sw:{i}"), Role::PushButton, *hex)
                    .bounds(ir((cx - 11.0 * s, cy - 11.0 * s, 22.0 * s, 22.0 * s)))
                    .state(State::Showing, true)
                    .action(ActionDef::new("click", "Use this colour")),
            );
        }
        let fy = r.1 + 22.0 * s + 3.0 * 30.0 * s - 6.0 * s;
        let fr = (r.0 + 10.0 * s, fy, r.2 - 20.0 * s - 70.0 * s, 26.0 * s);
        b.add(
            d,
            Spec::new("colorpop:hex", Role::Entry, "Hex colour")
                .bounds(ir(fr))
                .state(State::Showing, true)
                .state(State::Focusable, true)
                .state(State::Editable, true)
                .state(State::SingleLine, true)
                .state(State::SelectableText, true)
                .text(TextInfo::plain(cp.hex.text(), cp.hex.cursor(), cp.hex.selection(), true))
                .action(ActionDef::new("activate", "Use this colour").key("Return")),
        );
        let br = (fr.0 + fr.2 + 8.0 * s, fy, 52.0 * s, 26.0 * s);
        b.add(
            d,
            Spec::new("colorpop:default", Role::PushButton, "Default")
                .description("Use the theme's colour")
                .bounds(ir(br))
                .state(State::Showing, true)
                .action(ActionDef::new("click", "Reset to the default")),
        );
    }
}

// ---------------------------------------------------------------- dispatcher

fn refused(e: impl Into<String>) -> ActionError {
    ActionError::Refused(e.into())
}

/// Text operations on a single-line field, exactly like the keys would edit it.
fn text_op(f: &mut TextField, op: &Op, secret: bool) -> Result<(), ActionError> {
    let n = f.text().chars().count();
    match op {
        Op::SetCaret(o) => f.set_cursor(*o),
        Op::SetSelection(Some((a, z))) => f.set_selection(*a, *z),
        Op::SetSelection(None) => {
            let c = f.cursor();
            f.set_cursor(c);
        }
        Op::SetText(t) => f.set(t),
        Op::InsertText { offset, text } => {
            f.set_cursor((*offset).min(n));
            f.insert(text);
        }
        Op::DeleteText { start, end, cut } => {
            f.set_selection((*start).min(n), (*end).min(n));
            if *cut && !secret {
                if let Some(t) = f.selected_text() {
                    crate::clipboard::copy(&t);
                }
            }
            f.delete_selection();
        }
        Op::CopyText { start, end } => {
            if secret {
                return Err(ActionError::Unsupported("a password field cannot be copied".into()));
            }
            let t: String = f.text().chars().skip(*start).take(end.saturating_sub(*start)).collect();
            crate::clipboard::copy(&t);
        }
        Op::PasteText { offset } => {
            let t = crate::settings::configpane::system_clipboard().unwrap_or_default();
            f.set_cursor((*offset).min(n));
            f.insert(t.lines().next().unwrap_or(""));
        }
        _ => return Err(ActionError::Unsupported("not a text operation".into())),
    }
    Ok(())
}

fn is_text_op(op: &Op) -> bool {
    matches!(op, Op::SetCaret(_) | Op::SetSelection(_) | Op::SetText(_) | Op::InsertText { .. } | Op::DeleteText { .. } | Op::CopyText { .. } | Op::PasteText { .. })
}

impl SettingsApp {
    /// Scrolls the content (or the sidebar) so a node's window rectangle is in view.
    fn a11y_scroll_into_view(&mut self, r: Option<crate::a11y::Rect>, sidebar: bool) {
        let Some((_, y, _, h)) = r else { return };
        let (y, h) = (y as f32, h as f32);
        let margin = 16.0 * self.s;
        if sidebar {
            let top = self.nav_top();
            let view = self.size.1 as f32;
            if y < top {
                self.sb_scroll = (self.sb_scroll - (top - y) - margin).max(0.0);
            } else if y + h > view {
                self.sb_scroll += y + h - view + margin;
            }
            return;
        }
        let view = self.content_h();
        if y < margin {
            self.scroll = (self.scroll + y - margin).max(0.0);
        } else if y + h > view - margin {
            self.scroll += y + h - (view - margin);
        }
        self.reuse_items = true;
    }

    /// The same post-processing as typing in the search field: keep a visible pane selected.
    fn a11y_after_search(&mut self) {
        let vis = self.visible_panes();
        if let Some(&first) = vis.first() {
            if !vis.contains(&self.m.sel.pane) {
                self.select_pane(first);
            }
        }
    }

    fn a11y_focus_ctl(&mut self, id: &str) {
        if self.edit.as_ref().is_some_and(|(e, _)| e != id) {
            self.commit_edit();
        }
        self.zone = Zone::Content;
        self.it.focus = Some(id.to_string());
        self.it.ring = true;
        self.ensure_visible(id);
    }

    /// Runs `f` (a model change); a new error notice it raised becomes the action's error.
    fn a11y_apply(&mut self, id: &str, v: CtlVal) -> Result<(), ActionError> {
        let before = self.m.notice.as_ref().map(|n| n.at);
        self.m.apply(id, v);
        match &self.m.notice {
            Some(n) if n.error && Some(n.at) != before => Err(refused(n.text.clone())),
            _ => Ok(()),
        }
    }
}

/// The narrow semantic dispatcher.
pub(super) fn act(app: &mut SettingsApp, t: &Target) -> Result<Response, ActionError> {
    let key = t.key().to_string();
    let op = t.op.clone();
    app.reuse_items = false;
    // Modal overlays: like a click outside them, nothing else may act while one is open.
    if let Some(pp) = &app.popup {
        let mine = key == format!("ctl:{}", pp.id) || key.starts_with(&format!("ctl:{}/", pp.id));
        if !mine && op != Op::ScrollTo {
            return Err(refused("a list is open: choose one of its items or close it first"));
        }
    }
    if app.colorpop.is_some() && !key.starts_with("colorpop") && op != Op::ScrollTo {
        return Err(refused("the colour chooser is open: pick a colour or close it first"));
    }
    let sidebar = key == "search" || key == "search:clear" || key == "pages" || key.starts_with("pane:") || key.starts_with("hit:");
    if op == Op::ScrollTo {
        app.a11y_scroll_into_view(t.node.bounds, sidebar);
        return Ok(Response::REDRAW);
    }
    // A pending shortcut recording ends when anything else is used, as with a click elsewhere.
    if let Some(rec) = app.m.sel.recording.clone() {
        let target = if rec == "new" { "k:new:keys".to_string() } else { rec };
        if key != format!("ctl:{target}") {
            app.m.sel.recording = None;
        }
    }
    match key.as_str() {
        "search" => {
            app.commit_edit();
            app.zone = Zone::Search;
            match &op {
                Op::Focus => {}
                Op::Action("activate") => {
                    let first = app.sidebar_rows().into_iter().find_map(|r| match r {
                        SbRow::Pane(i) => Some((i, None)),
                        SbRow::Hit(i, l) => Some((i, Some(l))),
                        SbRow::Header(_) => None,
                    });
                    if let Some((i, label)) = first {
                        app.open_pane(i, label.as_deref());
                    }
                    app.zone = Zone::Sidebar;
                }
                op if is_text_op(op) => {
                    text_op(&mut app.search, op, false)?;
                    app.a11y_after_search();
                }
                _ => return Err(ActionError::Unsupported(format!("{op:?} on the search field"))),
            }
            return Ok(Response::REDRAW);
        }
        "search:clear" => {
            app.search.clear();
            app.zone = Zone::Search;
            return Ok(Response::REDRAW);
        }
        "pages" => {
            let Op::SelectChild(i) = op else { return Err(ActionError::Unsupported("the page list only selects".into())) };
            let child = t.node.children.get(i).copied();
            let rows: Vec<(usize, Option<String>)> = app.sidebar_hits.iter().zip(&app.sidebar_labels).map(|((_, p), l)| (*p, l.clone())).collect();
            // Children are the sidebar rows in order (plus nothing else).
            let Some((pane, label)) = rows.get(i).cloned() else { return Err(ActionError::Invalid(format!("no page {i}"))) };
            let _ = child;
            app.commit_edit();
            app.zone = Zone::Sidebar;
            app.open_pane(pane, label.as_deref());
            return Ok(Response::REDRAW);
        }
        "dialog:widget-choice" => {
            let Op::Action("close") = op else { return Err(ActionError::Unsupported(format!("{op:?}"))) };
            super::super::widgetchoice::apply(&mut app.m, "wc:cancel", CtlVal::Click);
            app.it.focus = None;
            app.zone = Zone::Sidebar;
            return Ok(Response::REDRAW);
        }
        "colorpop" => {
            let Op::Action("close") = op else { return Err(ActionError::Unsupported(format!("{op:?}"))) };
            app.colorpop = None;
            return Ok(Response::REDRAW);
        }
        "colorpop:hex" => {
            match &op {
                Op::Focus => {}
                Op::Action("activate") => {
                    let Some(cp) = app.colorpop.take() else { return Err(ActionError::Gone) };
                    app.a11y_apply(&cp.id, CtlVal::Str(cp.hex.text().to_string()))?;
                }
                op if is_text_op(op) => {
                    let Some(cp) = app.colorpop.as_mut() else { return Err(ActionError::Gone) };
                    text_op(&mut cp.hex, op, false)?;
                }
                _ => return Err(ActionError::Unsupported(format!("{op:?} on the hex field"))),
            }
            return Ok(Response::REDRAW);
        }
        "colorpop:default" => {
            let Some(cp) = app.colorpop.take() else { return Err(ActionError::Gone) };
            app.a11y_apply(&cp.id, CtlVal::Reset)?;
            return Ok(Response::REDRAW);
        }
        _ => {}
    }
    if let Some(i) = key.strip_prefix("colorpop:sw:").and_then(|i| i.parse::<usize>().ok()) {
        let (Some(cp), Some(hex)) = (app.colorpop.take(), SWATCHES.get(i)) else { return Err(ActionError::Gone) };
        app.a11y_apply(&cp.id, CtlVal::Str(hex.to_string()))?;
        return Ok(Response::REDRAW);
    }
    if let Some(rest) = key.strip_prefix("pane:").or_else(|| key.strip_prefix("hit:")) {
        let (pane_id, label) = match key.starts_with("hit:") {
            true => rest.split_once(':').map(|(p, l)| (p, Some(l))).unwrap_or((rest, None)),
            false => (rest, None),
        };
        let Some(i) = PANES.iter().position(|p| p.id == pane_id) else { return Err(ActionError::UnknownNode) };
        // The full label of a search hit (keys are clipped): the row's own name.
        let label = label.map(|_| t.node.name.clone());
        match op {
            Op::Focus | Op::Action("activate") => {
                app.commit_edit();
                app.zone = Zone::Sidebar;
                app.open_pane(i, label.as_deref());
            }
            _ => return Err(ActionError::Unsupported(format!("{op:?} on a page"))),
        }
        return Ok(Response::REDRAW);
    }
    let Some(rest) = key.strip_prefix("ctl:") else { return Err(ActionError::Unsupported(format!("{} cannot act", t.node.role.name()))) };
    // Choices: ctl:<id>/opt:<i> and the list ctl:<id>/menu.
    if let Some((id, part)) = rest.rsplit_once('/') {
        let id = id.to_string();
        let Some(p) = app.frame.find(&id).cloned() else { return Err(ActionError::UnknownNode) };
        if part == "menu" {
            return match op {
                Op::Action("close") => {
                    app.popup = None;
                    Ok(Response::REDRAW)
                }
                Op::SelectChild(i) => choose(app, &p, i),
                _ => Err(ActionError::Unsupported(format!("{op:?} on a list"))),
            };
        }
        let Some(i) = part.strip_prefix("opt:").and_then(|i| i.parse::<usize>().ok()) else { return Err(ActionError::UnknownNode) };
        return match op {
            Op::Focus => {
                if let Some(pp) = app.popup.as_mut().filter(|pp| pp.id == id) {
                    pp.hover = Some(i);
                    if i < pp.top {
                        pp.top = i;
                    } else if i >= pp.top + pp.visible {
                        pp.top = i + 1 - pp.visible;
                    }
                } else {
                    app.a11y_focus_ctl(&id);
                }
                Ok(Response::REDRAW)
            }
            Op::Action("click") => choose(app, &p, i),
            _ => Err(ActionError::Unsupported(format!("{op:?} on a choice"))),
        };
    }
    let id = rest.to_string();
    if id == crate::settings::configpane::EDITOR_ID {
        return match &op {
            Op::Focus => {
                app.a11y_focus_ctl(&id);
                Ok(Response::REDRAW)
            }
            Op::Action("activate") => {
                app.a11y_focus_ctl(&id);
                app.a11y_apply(&id, CtlVal::Click)?;
                Ok(Response::REDRAW)
            }
            op if is_text_op(op) => {
                app.a11y_focus_ctl(&id);
                crate::settings::configpane::a11y_edit(&mut app.m, op).map_err(refused)?;
                Ok(Response::REDRAW)
            }
            _ => Err(ActionError::Unsupported(format!("{op:?} on the editor"))),
        };
    }
    let Some(p) = app.frame.find(&id).cloned() else { return Err(ActionError::UnknownNode) };
    if op == Op::Focus {
        app.a11y_focus_ctl(&id);
        return Ok(Response::REDRAW);
    }
    if op == Op::Action("reset") {
        app.a11y_focus_ctl(&id);
        app.a11y_apply(&id, CtlVal::Reset)?;
        return Ok(Response::REDRAW);
    }
    match (&p.ctl.kind, &op) {
        (Kind::Toggle(v), Op::Action("toggle")) => {
            app.a11y_focus_ctl(&id);
            app.a11y_apply(&id, CtlVal::Bool(!v))?;
        }
        (Kind::Slider { v, min, max, step, .. } | Kind::Stepper { v, min, max, step, .. }, _) => {
            let nv = match &op {
                Op::SetValue(x) => ui::snap(*x, *min, *max, *step),
                Op::Action("increment") => ui::snap(v + step, *min, *max, *step),
                Op::Action("decrement") => ui::snap(v - step, *min, *max, *step),
                _ => return Err(ActionError::Unsupported(format!("{op:?} on a number"))),
            };
            app.a11y_focus_ctl(&id);
            if (nv - v).abs() > 1e-9 {
                app.a11y_apply(&id, CtlVal::Num(nv))?;
            }
        }
        (Kind::Seg(..), Op::SelectChild(i)) => return choose(app, &p, *i),
        (Kind::Popup(..), Op::Action("press")) => {
            app.a11y_focus_ctl(&id);
            if app.popup.as_ref().is_some_and(|pp| pp.id == id) {
                app.popup = None;
            } else {
                app.open_popup(&p);
            }
        }
        (Kind::Color(_), Op::Action("press")) => {
            app.a11y_focus_ctl(&id);
            app.open_color(&p);
        }
        (Kind::Text { .. }, Op::Action("activate")) => {
            app.a11y_focus_ctl(&id);
            if app.edit.as_ref().is_some_and(|(e, _)| *e == id) {
                let before = app.m.notice.as_ref().map(|n| n.at);
                app.commit_edit();
                if let Some(n) = app.m.notice.as_ref().filter(|n| n.error && Some(n.at) != before) {
                    return Err(refused(n.text.clone()));
                }
            } else {
                app.begin_edit(&p);
            }
        }
        (Kind::Text { .. }, op) if is_text_op(op) => {
            app.a11y_focus_ctl(&id);
            if !app.edit.as_ref().is_some_and(|(e, _)| *e == id) {
                app.begin_edit(&p);
            }
            let secret = id.contains(":secret");
            let Some((_, f)) = app.edit.as_mut() else { return Err(ActionError::Gone) };
            text_op(f, op, secret)?;
        }
        (Kind::Key(_) | Kind::Button { .. } | Kind::Chip { .. }, Op::Action("click") | Op::Action("activate")) => {
            app.a11y_focus_ctl(&id);
            app.a11y_apply(&id, CtlVal::Click)?;
        }
        _ => return Err(ActionError::Unsupported(format!("{op:?} on {}", t.node.role.name()))),
    }
    Ok(Response::REDRAW)
}

/// Chooses option `i` of a segmented control, tab strip or list (through the open list when it is open).
fn choose(app: &mut SettingsApp, p: &Placed, i: usize) -> Result<Response, ActionError> {
    let id = p.ctl.id.clone();
    let opts = match &p.ctl.kind {
        Kind::Seg(o, _) | Kind::Popup(o, _) => o.clone(),
        _ => return Err(ActionError::Unsupported("not a choice".into())),
    };
    let Some((_, v)) = opts.get(i) else { return Err(ActionError::Invalid(format!("no option {i}"))) };
    if app.popup.as_ref().is_some_and(|pp| pp.id == id) {
        let before = app.m.notice.as_ref().map(|n| n.at);
        app.popup_choose(i);
        if let Some(n) = app.m.notice.as_ref().filter(|n| n.error && Some(n.at) != before) {
            return Err(refused(n.text.clone()));
        }
    } else {
        app.a11y_focus_ctl(&id);
        app.a11y_apply(&id, CtlVal::Str(v.clone()))?;
    }
    Ok(Response::REDRAW)
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::{act, tree};
    use crate::a11y::tree::{stamp, Ids, Tree};
    use crate::a11y::{action::validate, ActionError, Op, Request, Role, State};

    pub(super) fn app(pane: Option<&str>) -> SettingsApp {
        let d = std::env::temp_dir().join(format!("decsettings-a11y-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let paths = Paths { rdwm_config: d.join("config.toml"), shell_dir: d.join("shell"), rdwmctl: "/nonexistent".into(), socket: d.join("none.sock") };
        SettingsApp::new(paths, pane)
    }

    pub(super) struct Rig {
        pub a: SettingsApp,
        pub pm: Pixmap,
        pub ids: Ids,
        pub tree: Tree,
    }

    impl Rig {
        pub fn new(pane: Option<&str>, w: u32, h: u32) -> Rig {
            let mut a = app(pane);
            a.resize(w, h);
            let mut r = Rig { a, pm: Pixmap::new(w, h).unwrap(), ids: Ids::default(), tree: Tree::default() };
            r.redraw();
            r
        }
        pub fn redraw(&mut self) {
            self.a.reuse_items = false;
            self.a.draw(&mut self.pm);
            let mut b = crate::a11y::TreeBuilder::new(&mut self.ids, "System Settings");
            self.a.a11y_tree(&mut b);
            let mut t = b.finish();
            t.set_window(crate::a11y::tree::WindowInfo { origin: Some((0, 0)), active: true });
            let prev = std::mem::take(&mut self.tree);
            stamp(if prev.is_empty() { None } else { Some(&prev) }, &mut t);
            self.tree = t;
        }
        pub fn node(&self, key: &str) -> &crate::a11y::tree::Node {
            self.tree.by_key(key).unwrap_or_else(|| panic!("no node {key}; have {:?}", self.tree.nodes().iter().map(|n| n.key.as_str()).collect::<Vec<_>>()))
        }
        /// An AT request against the current revision, validated and dispatched like the host does.
        pub fn act(&mut self, key: &str, op: Op) -> Result<(), ActionError> {
            let n = self.node(key);
            let req = Request { id: 1, revision: self.tree.revision, node: n.id, op, expires: None };
            let tree = self.tree.clone();
            let r = validate(&tree, &req).and_then(|t| self.a.a11y_action(&t)).map(|_| ());
            self.redraw();
            r
        }
    }

    fn ev(key: Key, ch: Option<char>) -> KeyEvent {
        KeyEvent { key, text: ch.map(String::from).unwrap_or_default(), ch, ctrl: false, shift: false, alt: false }
    }

    fn file(r: &Rig) -> String {
        std::fs::read_to_string(&r.a.m.paths.rdwm_config).unwrap_or_default()
    }

    /// Prints the tree (`cargo test dump_tree -- --ignored --nocapture`) for review.
    #[test]
    #[ignore = "review aid"]
    fn dump_tree_for_review() {
        for pane in ["general", "appearance", "keyboard", "accessibility", "config"] {
            let r = Rig::new(Some(pane), 1100, 800);
            eprintln!("=== {pane}: {} nodes", r.tree.len());
            for n in r.tree.nodes() {
                let mut d = 0;
                let mut c = n.parent;
                while let Some(p) = c {
                    d += 1;
                    c = r.tree.get(p).and_then(|x| x.parent);
                }
                eprintln!(
                    "{}{} [{}] {:?} desc={:?} acts={:?} b={:?}",
                    "  ".repeat(d),
                    n.key,
                    n.role.name(),
                    n.name,
                    n.description.chars().take(40).collect::<String>(),
                    n.actions.iter().map(|a| a.name).collect::<Vec<_>>(),
                    n.bounds
                );
            }
        }
    }

    #[test]
    fn tree_describes_sidebar_search_and_page() {
        let r = Rig::new(Some("general"), 1100, 800);
        assert_eq!(r.node("frame").role, Role::Frame);
        let search = r.node("search");
        assert_eq!(search.role, Role::Entry);
        assert!(search.text.as_ref().is_some_and(|t| t.editable));
        let pages = r.node("pages");
        assert!(pages.selection);
        assert_eq!(pages.children.len(), r.a.sidebar_hits.len());
        assert!(r.node("pane:general").states.contains(State::Selected));
        assert!(!r.node("pane:appearance").states.contains(State::Selected));
        assert_eq!(r.node("pane:appearance").description, "Appearance", "section as description");
        assert_eq!(r.node("content").name, "General");
        assert_eq!(r.node("title").role, Role::Heading);
        let sw = r.node("ctl:r:general:default_binds");
        assert_eq!(sw.role, Role::Switch);
        assert!(sw.actions.iter().any(|a| a.name == "toggle") && sw.actions.iter().any(|a| a.name == "reset"));
        for n in r.tree.nodes() {
            if let Some((x, y, w, h)) = n.bounds {
                assert!(w >= 0 && h >= 0, "{}", n.key);
                if n.states.contains(State::Showing) && n.key != "pages" {
                    assert!(x < 1100 && y < 800 && x + w > 0 && y + h > 0, "showing node {} is on screen: {:?}", n.key, n.bounds);
                }
            }
        }
        // Ids are stable across repaints.
        let mut r2 = r;
        let before = r2.node("ctl:r:general:default_binds").id;
        r2.redraw();
        assert_eq!(r2.node("ctl:r:general:default_binds").id, before);
    }

    #[test]
    fn controls_change_the_real_config_file() {
        let mut r = Rig::new(Some("general"), 1100, 800);
        let was = r.node("ctl:r:general:default_binds").states.contains(State::Checked);
        r.act("ctl:r:general:default_binds", Op::Action("toggle")).unwrap();
        assert!(file(&r).contains(&format!("default_binds = {}", !was)), "{}", file(&r));
        assert_eq!(r.node("ctl:r:general:default_binds").states.contains(State::Checked), !was);
        let v = r.node("ctl:r:general:chord_timeout_ms").value.clone().unwrap();
        let target = ui::snap(v.min + v.step * 3.0, v.min, v.max, v.step);
        r.act("ctl:r:general:chord_timeout_ms", Op::SetValue(target)).unwrap();
        assert!(file(&r).contains(&format!("chord_timeout_ms = {}", target as i64)), "{}", file(&r));
        assert_eq!(r.node("ctl:r:general:chord_timeout_ms").value.as_ref().unwrap().current, target);
        r.act("ctl:r:general:chord_timeout_ms", Op::Action("increment")).unwrap();
        assert_eq!(r.node("ctl:r:general:chord_timeout_ms").value.as_ref().unwrap().current, (target + v.step).min(v.max));
        // Text: a draft until activated (Enter), exactly like typing.
        r.act("ctl:r:general:tags", Op::SetText("web,code,chat".into())).unwrap();
        assert!(!file(&r).contains("web"), "a draft is not written");
        let tags = r.node("ctl:r:general:tags");
        assert!(tags.attributes.iter().any(|(k, _)| k == "draft"));
        assert_eq!(tags.text.as_ref().unwrap().visible(), "web,code,chat");
        r.act("ctl:r:general:tags", Op::Action("activate")).unwrap();
        assert!(file(&r).contains("web"), "{}", file(&r));
        // Reset (the right-click path) removes the key again.
        r.act("ctl:r:general:default_binds", Op::Action("reset")).unwrap();
        assert!(!file(&r).contains("default_binds"), "{}", file(&r));
    }

    #[test]
    fn an_action_answers_while_the_disk_is_blocked_and_says_saved_only_after_the_commit() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::time::{Duration, Instant};
        let mut r = Rig::new(Some("general"), 1100, 800);
        let (gate, calls) = (Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let (g, c) = (gate.clone(), calls.clone());
        r.a.m.use_worker_with_sync_hook(Arc::new(move |_| {
            // the second write fails once the disk is back
            let n = c.fetch_add(1, Ordering::SeqCst);
            let t0 = Instant::now();
            while g.load(Ordering::SeqCst) && t0.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if n == 1 {
                return Err(std::io::Error::from_raw_os_error(28));
            }
            Ok(())
        }));
        let was = r.node("ctl:r:general:default_binds").states.contains(State::Checked);
        let t0 = Instant::now();
        r.act("ctl:r:general:default_binds", Op::Action("toggle")).unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let v = r.node("ctl:r:general:chord_timeout_ms").value.clone().unwrap();
        let target = ui::snap(v.min + v.step * 2.0, v.min, v.max, v.step);
        r.act("ctl:r:general:chord_timeout_ms", Op::SetValue(target)).unwrap();
        assert!(t0.elapsed() < Duration::from_millis(500), "the AT waited {:?} for the disk", t0.elapsed());
        assert_eq!(r.node("ctl:r:general:default_binds").states.contains(State::Checked), !was, "shown at once");
        let toast = r.node("toast");
        assert_eq!(toast.name, crate::settings::model::SAVING);
        assert!(toast.states.contains(State::Busy), "pending, not saved");
        assert!(!file(&r).contains("default_binds"), "nothing written yet");
        gate.store(false, Ordering::SeqCst);
        r.a.m.settle();
        r.redraw();
        assert!(file(&r).contains(&format!("default_binds = {}", !was)), "{}", file(&r));
        assert!(!file(&r).contains("chord_timeout_ms"), "the failed write left the file alone");
        assert_ne!(r.node("ctl:r:general:chord_timeout_ms").value.as_ref().unwrap().current, target, "and the control shows the saved value again");
        let alert = r.tree.nodes().iter().find(|n| n.role == Role::Alert).map(|n| n.name.clone()).unwrap_or_default();
        assert!(alert.contains("Not saved") && alert.contains("No space left"), "the failure is on the page: {alert:?}");
        assert!(r.tree.by_key("toast").is_none_or(|t| !t.states.contains(State::Busy)));
    }

    #[test]
    fn stale_disabled_and_refused_requests_fail_clearly() {
        let mut r = Rig::new(Some("general"), 1100, 800);
        let (id, rev) = (r.node("ctl:r:general:default_binds").id, r.tree.revision);
        r.act("ctl:r:general:default_binds", Op::Action("toggle")).unwrap();
        let old = Request { id: 2, revision: rev, node: id, op: Op::Action("toggle"), expires: None };
        assert!(matches!(validate(&r.tree, &old), Err(ActionError::Stale { .. })), "the AT read the switch before it changed");
        let before = file(&r);
        // Model validation errors come back as refusals and show as an alert on the page.
        let names: Vec<String> = (0..40).map(|i| format!("t{i}")).collect();
        r.act("ctl:r:general:tags", Op::SetText(names.join(","))).unwrap();
        let e = r.act("ctl:r:general:tags", Op::Action("activate")).unwrap_err();
        assert!(matches!(e, ActionError::Refused(_)), "{e:?}");
        assert_eq!(file(&r), before, "nothing was written");
        assert!(r.tree.nodes().iter().any(|n| n.role == Role::Alert), "the error is on the page");
        let mut a = Rig::new(Some("appearance"), 1100, 900);
        assert!(!a.node("ctl:th:market").enabled());
        assert_eq!(a.act("ctl:th:market", Op::Action("click")).unwrap_err(), ActionError::Disabled);
        assert!(a.act("ctl:th:market", Op::Action("explode")).is_err());
    }

    #[test]
    fn keyboard_focus_and_at_focus_agree() {
        let mut r = Rig::new(Some("general"), 1100, 800);
        r.act("ctl:r:general:default_binds", Op::Focus).unwrap();
        assert_eq!((r.a.zone, r.a.it.focus.as_deref()), (Zone::Content, Some("r:general:default_binds")));
        assert_eq!(r.tree.focus, Some(r.node("ctl:r:general:default_binds").id));
        assert!(r.node("ctl:r:general:default_binds").states.contains(State::Focused));
        // The keyboard continues from where the AT put the focus.
        let was = r.node("ctl:r:general:default_binds").states.contains(State::Checked);
        r.a.key(&ev(Key::Char, Some(' ')));
        r.redraw();
        assert_eq!(r.node("ctl:r:general:default_binds").states.contains(State::Checked), !was, "Space toggled the AT-focused switch");
        r.a.key(&ev(Key::Tab, None));
        r.redraw();
        let k = format!("ctl:{}", r.a.it.focus.clone().unwrap());
        assert_eq!(r.tree.focus, Some(r.node(&k).id), "AT focus follows Tab");
        assert_eq!(r.tree.nodes().iter().filter(|n| n.states.contains(State::Focused)).count(), 1);
        // Sidebar navigation.
        r.act("pane:appearance", Op::Focus).unwrap();
        assert_eq!(PANES[r.a.m.sel.pane].id, "appearance");
        assert_eq!(r.tree.focus, Some(r.node("pane:appearance").id));
        r.a.key(&ev(Key::Down, None));
        r.redraw();
        let now = PANES[r.a.m.sel.pane].id;
        assert_ne!(now, "appearance");
        assert_eq!(r.tree.focus, Some(r.node(&format!("pane:{now}")).id));
        r.a.key(&ev(Key::Up, None));
        r.a.key(&ev(Key::Up, None));
        r.a.key(&ev(Key::Up, None));
        r.a.key(&ev(Key::Up, None));
        r.a.key(&ev(Key::Up, None));
        r.a.key(&ev(Key::Up, None));
        r.redraw();
        assert_eq!(r.a.zone, Zone::Search);
        assert_eq!(r.tree.focus, Some(r.node("search").id));
    }

    #[test]
    fn a_list_control_is_a_modal_menu() {
        let mut r = Rig::new(Some("keyboard"), 1100, 800);
        let combo = "ctl:k:new:action";
        assert!(r.node(combo).states.contains(State::Collapsed));
        r.act(combo, Op::Action("press")).unwrap();
        assert!(r.a.popup.is_some());
        assert!(r.node(combo).states.contains(State::Expanded));
        let menu = r.node("ctl:k:new:action/menu");
        assert!(menu.states.contains(State::Showing) && menu.states.contains(State::Modal));
        assert_eq!(r.tree.focus.and_then(|f| r.tree.get(f)).map(|n| n.role), Some(Role::Menu), "nothing chosen yet: the list itself");
        r.act("ctl:k:new:action/opt:3", Op::Focus).unwrap();
        assert_eq!(r.tree.focus, Some(r.node("ctl:k:new:action/opt:3").id));
        assert!(matches!(r.act("ctl:k:new:args", Op::Focus), Err(ActionError::Refused(_))), "modal");
        r.act("ctl:k:new:action/opt:1", Op::Action("click")).unwrap();
        assert!(r.a.popup.is_none());
        assert_eq!(r.a.m.sel.new_bind.action, "close");
        assert!(r.node("ctl:k:new:action/opt:1").states.contains(State::Selected));
        // Choosing without opening the list (Selection.SelectChild on the menu) is the same model action.
        r.act("ctl:k:new:action/menu", Op::SelectChild(2)).unwrap();
        assert_eq!(r.a.m.sel.new_bind.action, "kill");
        r.act(combo, Op::Action("press")).unwrap();
        r.act("ctl:k:new:action/menu", Op::Action("close")).unwrap();
        assert!(r.a.popup.is_none());
    }

    #[test]
    fn tabs_and_galleries_select_through_the_model() {
        let mut r = Rig::new(Some("appearance"), 1100, 900);
        let tabs = r.node("ctl:ap:tab");
        assert_eq!(tabs.role, Role::PageTabList);
        assert!(r.node("ctl:ap:tab/opt:0").states.contains(State::Selected));
        r.act("ctl:ap:tab", Op::SelectChild(1)).unwrap();
        assert!(r.node("ctl:ap:tab/opt:1").states.contains(State::Selected));
        assert_eq!(r.a.m.sel.tab.id(), "palettes");
        assert_eq!(r.tree.focus, Some(r.node("ctl:ap:tab/opt:1").id), "the strip keeps the focus, on the new tab");
        r.act("ctl:ap:tab/opt:2", Op::Action("click")).unwrap();
        assert_eq!(r.a.m.sel.tab.id(), "customise");
    }

    #[test]
    fn search_edits_text_with_caret_and_selection_and_opens_results() {
        let mut r = Rig::new(Some("general"), 1100, 800);
        r.act("search", Op::InsertText { offset: 0, text: "cursr".into() }).unwrap();
        r.act("search", Op::InsertText { offset: 4, text: "o".into() }).unwrap();
        assert_eq!(r.a.search.text(), "cursor");
        let t = r.node("search").text.clone().unwrap();
        assert_eq!((t.visible().as_str(), t.caret), ("cursor", 5));
        assert_eq!(r.node("pages").name, "Search results");
        r.act("search", Op::SetSelection(Some((0, 3)))).unwrap();
        assert_eq!(r.node("search").text.as_ref().unwrap().selection, Some((0, 3)));
        r.act("search", Op::SetCaret(6)).unwrap();
        assert_eq!(r.node("search").text.as_ref().unwrap().selection, None);
        r.act("search", Op::Action("activate")).unwrap();
        assert_eq!(r.a.zone, Zone::Sidebar);
        let first = r.tree.get(r.node("pages").children[0]).unwrap().clone();
        assert!(first.description.contains(PANES[r.a.m.sel.pane].title) || first.name == PANES[r.a.m.sel.pane].title, "opened the first result: {first:?}");
        r.act("search:clear", Op::Action("click")).unwrap();
        assert_eq!(r.a.search.text(), "");
        assert_eq!(r.node("pages").name, "Settings pages");
    }

    #[test]
    fn password_fields_are_masked_and_never_copied() {
        let mut r = Rig::new(Some("general"), 1100, 800);
        // A secret field on its own page, drawn by the real painter.
        let items = vec![Item::Card(vec![ui::Row::new("API key").ctl(ui::Ctl::new("t:key:secret", Kind::Text { val: "•".repeat(12), hint: "paste".into() }))])];
        let mut p = ui::Painter { fonts: r.a.fonts.as_mut(), s: 1.0 };
        let frame = ui::draw_items(&mut r.pm, &mut p, ui::Head::new("Test"), &items, 0.0, 236.0, 864.0, &r.a.it, &|_| None);
        r.a.frame = frame;
        r.a.items_cache = Some((r.a.m.sel.pane, items));
        let mut b = crate::a11y::TreeBuilder::new(&mut r.ids, "System Settings");
        tree(&r.a, &mut b);
        r.tree = b.finish();
        let n = r.node("ctl:t:key:secret").clone();
        assert_eq!(n.role, Role::PasswordText);
        assert!(n.text.as_ref().unwrap().masked());
        let req = |op| Request { id: 1, revision: r.tree.revision, node: n.id, op, expires: None };
        assert!(matches!(validate(&r.tree, &req(Op::CopyText { start: 0, end: 3 })), Err(ActionError::Unsupported(_))));
        assert!(matches!(validate(&r.tree, &req(Op::DeleteText { start: 0, end: 3, cut: true })), Err(ActionError::Unsupported(_))));
        let rq = req(Op::InsertText { offset: 0, text: "hunter2".into() });
        let tr = r.tree.clone();
        act(&mut r.a, &validate(&tr, &rq).unwrap()).unwrap();
        assert_eq!(r.a.edit.as_ref().map(|(_, f)| f.text()), Some("hunter2"), "the draft holds the typed secret");
        let mut b = crate::a11y::TreeBuilder::new(&mut r.ids, "System Settings");
        tree(&r.a, &mut b);
        let t = b.finish();
        let x = t.by_key("ctl:t:key:secret").unwrap().text.clone().unwrap();
        assert_eq!(x.visible(), "•••••••");
        assert!(!format!("{t:?}").contains("hunter2"), "the snapshot never carries the secret");
    }

    #[test]
    fn the_widget_layout_choice_is_a_dialog() {
        let mut r = Rig::new(Some("appearance"), 1100, 900);
        r.a.m.widget_choice = Some(crate::settings::widgetchoice::Pending {
            selection: crate::theme::model::Selection { name: "Dux".into(), ..Default::default() },
            reset_colors: false,
            empty_desktop: false,
            widgets: serde_json::json!({}),
            style: None,
        });
        r.redraw();
        let d = r.node("dialog:widget-choice").clone();
        assert_eq!(d.role, Role::Dialog);
        let keys: Vec<String> = r.tree.nodes().iter().filter(|n| n.key.starts_with("ctl:wc:")).map(|n| n.key.clone()).collect();
        assert_eq!(keys.len(), 3, "{keys:?}");
        assert!(r.tree.nodes().iter().any(|n| n.parent == Some(d.id) && n.role == Role::Label), "the question is inside the dialog");
        r.act("dialog:widget-choice", Op::Action("close")).unwrap();
        assert!(r.a.m.widget_choice.is_none());
        assert!(r.tree.by_key("dialog:widget-choice").is_none());
    }

    #[test]
    fn bounds_follow_the_ui_scale() {
        let r1 = Rig::new(Some("general"), 1100, 800);
        let mut a = app(Some("general"));
        a.s = 2.0;
        a.resize(2200, 1600);
        let mut r2 = Rig { a, pm: Pixmap::new(2200, 1600).unwrap(), ids: Ids::default(), tree: Tree::default() };
        r2.redraw();
        let (b1, b2) = (r1.node("ctl:r:general:default_binds").bounds.unwrap(), r2.node("ctl:r:general:default_binds").bounds.unwrap());
        assert!((b2.0 - 2 * b1.0).abs() <= 2 && (b2.1 - 2 * b1.1).abs() <= 2 && (b2.2 - 2 * b1.2).abs() <= 2, "{b1:?} vs {b2:?}");
        assert_eq!(r2.node("frame").bounds, Some((0, 0, 2200, 1600)));
    }

    #[test]
    fn scroll_to_reveals_offscreen_controls() {
        let mut r = Rig::new(Some("general"), 900, 420);
        let k = "ctl:r:general:tags";
        assert!(!r.node(k).states.contains(State::Showing), "{:?}", r.node(k).bounds);
        r.act(k, Op::ScrollTo).unwrap();
        r.redraw();
        assert!(r.node(k).states.contains(State::Showing), "{:?}", r.node(k).bounds);
        // Focus scrolls too.
        r.act("ctl:r:general:default_binds", Op::Focus).unwrap();
        r.redraw();
        assert!(r.node("ctl:r:general:default_binds").states.contains(State::Showing));
    }

    #[test]
    fn the_configuration_editor_is_editable_text() {
        let mut r = Rig::new(Some("config"), 1100, 900);
        let ed = r.tree.nodes().iter().find(|n| n.role == Role::Text).cloned().expect("editor");
        let t = ed.text.clone().unwrap();
        assert!(t.multi_line);
        let before = t.visible();
        r.act(&ed.key, Op::InsertText { offset: 0, text: "# from an AT\n".into() }).unwrap();
        let after = r.node(&ed.key).text.clone().unwrap();
        assert_eq!(after.visible(), format!("# from an AT\n{before}"));
        assert_eq!(after.caret, "# from an AT\n".chars().count());
        r.act(&ed.key, Op::SetSelection(Some((2, 6)))).unwrap();
        assert_eq!(r.node(&ed.key).text.as_ref().unwrap().selection, Some((2, 6)));
        r.act(&ed.key, Op::DeleteText { start: 0, end: 13, cut: false }).unwrap();
        assert_eq!(r.node(&ed.key).text.as_ref().unwrap().visible(), before);
    }
}
