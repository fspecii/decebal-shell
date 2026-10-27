//! Spotlight: one search box for apps, system commands, package install/remove and a calculator.
//!
//! The result list is built by [`search`] (pure apart from the `pacman` queries); [`Spotlight`] is
//! the window UI around it.

use crate::app::Button;
use crate::apps::{self, AppEntry};
use crate::calc;
use crate::gfx::{blit_scaled, color, fill_rect, fill_rounded, stroke_rounded};
use crate::icons::IconCache;
use crate::launch;
use crate::menubar::symbols::SymbolCache;
use crate::text::{Fonts, Weight};
use crate::wallhaven::textfield::TextField;
use crate::window_app::{Action, Key, KeyEvent, Response, Waker, WindowApp};
use std::process::{Command, Stdio};
use tiny_skia::{Color, Pixmap};

const CARD_W: u32 = 680;
const HEADER_H: f32 = 64.0;
const ROW_H: f32 = 48.0;
const MAX_ROWS: usize = 8;
const PAD: f32 = 12.0;
const ICON: u32 = 32;
const MAX_APPS: usize = 6;
const MAX_PACKAGES: usize = 30;

fn text_color() -> Color {
    color(245, 245, 247, 1.0)
}
fn subtext_color() -> Color {
    color(255, 255, 255, 0.55)
}

/// What Enter does for a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// A `.desktop` `Exec=` line.
    Exec(String),
    Argv(Vec<String>),
    /// A shell command shown in a terminal window (package installs need sudo and a password prompt).
    Terminal(String),
    Copy(String),
    Url(String),
    /// Replace the search text (a suggestion).
    SetQuery(String),
    /// A built-in toggle handled by the bar's helpers.
    DarkMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Icon {
    App(String),
    /// A symbol name on a coloured badge.
    Badge(&'static str, (u8, u8, u8)),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub subtitle: String,
    pub kind: &'static str,
    pub icon: Icon,
    pub act: Act,
    /// Needs a second Enter (log out, restart, shut down).
    pub danger: bool,
}

const BLUE: (u8, u8, u8) = (10, 132, 255);
const GREEN: (u8, u8, u8) = (48, 176, 90);
const RED: (u8, u8, u8) = (235, 77, 75);
const GRAY: (u8, u8, u8) = (110, 110, 118);
const ORANGE: (u8, u8, u8) = (240, 150, 40);

fn cmd(title: &str, subtitle: &str, symbol: &'static str, badge: (u8, u8, u8), act: Act, danger: bool) -> Item {
    Item { title: title.into(), subtitle: subtitle.into(), kind: "Command", icon: Icon::Badge(symbol, badge), act, danger }
}

fn argv(parts: &[&str]) -> Act {
    Act::Argv(parts.iter().map(|s| s.to_string()).collect())
}

/// Built-in commands. The second field is extra search words.
fn commands() -> Vec<(Item, &'static str)> {
    vec![
        (cmd("Install Packages", "Browse and install Arch packages", "download", GREEN, Act::Terminal("omarchy-pkg-install".into()), false), "app store add program software pacman"),
        (cmd("Remove Packages", "Uninstall programs from this computer", "trash-can", RED, Act::Terminal("omarchy-pkg-remove".into()), false), "uninstall delete program software pacman"),
        (cmd("Update System", "Install the latest updates", "refresh", BLUE, Act::Terminal("omarchy-update".into()), false), "upgrade software update pacman"),
        (cmd("Lock Screen", "", "lock", GRAY, argv(&["loginctl", "lock-session"]), false), ""),
        (cmd("Sleep", "", "moon", GRAY, argv(&["systemctl", "suspend"]), false), "suspend"),
        (cmd("Restart", "Press Enter again to confirm", "refresh", ORANGE, argv(&["systemctl", "reboot"]), true), "reboot"),
        (cmd("Shut Down", "Press Enter again to confirm", "power", RED, argv(&["systemctl", "poweroff"]), true), "poweroff turn off"),
        (cmd("Log Out", "Press Enter again to confirm", "logout", ORANGE, argv(&["loginctl", "terminate-user", &std::env::var("USER").unwrap_or_default()]), true), "sign out exit"),
        (cmd("Toggle Dark Mode", "Switch between light and dark", "dark-mode", GRAY, Act::DarkMode, false), "appearance theme light"),
        (cmd("Screenshot", "Capture the screen", "screenshot", GRAY, argv(&["omarchy-capture-screenshot"]), false), "capture screen"),
        (cmd("Wallpapers", "Browse Wallhaven", "picture", BLUE, Act::Exec("machaven".into()), false), "wallhaven background wallpaper"),
        (cmd("Launchpad", "All applications", "grid", BLUE, Act::Exec("maclaunch".into()), false), "apps applications"),
    ]
}

fn matches_words(hay: &str, q: &str) -> bool {
    let hay = hay.to_lowercase();
    q.split_whitespace().all(|w| hay.split(|c: char| !c.is_alphanumeric()).any(|h| h.starts_with(w)))
}

/// `("install", "foo bar")` for `install foo bar`; also `i foo`.
fn package_verb(q: &str) -> Option<(&'static str, &str)> {
    let (verb, rest) = q.split_once(char::is_whitespace)?;
    let rest = rest.trim();
    match verb.to_lowercase().as_str() {
        "install" | "i" | "get" => Some(("install", rest)),
        "remove" | "uninstall" | "rm" | "delete" => Some(("remove", rest)),
        _ => None,
    }
}

/// One `pacman -Ss` / `-Qs` hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub repo: String,
    pub version: String,
    pub installed: bool,
    pub desc: String,
}

pub fn parse_pacman(out: &str) -> Vec<Package> {
    let mut pkgs = Vec::new();
    let mut lines = out.lines();
    while let Some(head) = lines.next() {
        if head.starts_with(char::is_whitespace) {
            continue;
        }
        let mut words = head.split_whitespace();
        let Some((repo, name)) = words.next().and_then(|w| w.split_once('/')) else { continue };
        let version = words.next().unwrap_or("").to_string();
        let installed = head.contains("[installed") || repo == "local";
        let desc = lines.next().map(|l| l.trim().to_string()).unwrap_or_default();
        pkgs.push(Package { name: name.into(), repo: repo.into(), version, installed, desc });
    }
    pkgs
}

/// Package names pacman reports are `[a-z0-9@._+-]`; anything else is never put in a shell command.
fn safe_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('-') && n.chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
}

fn pacman(flag: &str, terms: &str) -> Vec<Package> {
    let mut c = Command::new("pacman");
    c.arg(flag).arg("--");
    c.args(terms.split_whitespace());
    let out = c.stdin(Stdio::null()).stderr(Stdio::null()).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut pkgs = parse_pacman(&out);
    let q = terms.to_lowercase();
    // Exact name first, then names that start with the text, then the rest in pacman's order.
    pkgs.sort_by_key(|p| {
        if p.name == q {
            0
        } else if p.name.starts_with(&q) {
            1
        } else {
            2
        }
    });
    pkgs.truncate(MAX_PACKAGES);
    pkgs
}

fn package_items(verb: &str, terms: &str) -> Vec<Item> {
    let flag = if verb == "install" { "-Ss" } else { "-Qs" };
    pacman_items(verb, pacman(flag, terms))
}

pub fn pacman_items(verb: &str, pkgs: Vec<Package>) -> Vec<Item> {
    pkgs.into_iter()
        .filter(|p| safe_name(&p.name))
        .map(|p| {
            let (kind, badge, symbol, act) = if verb == "install" {
                ("Install", GREEN, "download", Act::Terminal(format!("sudo pacman -S --needed {}", p.name)))
            } else {
                ("Remove", RED, "trash-can", Act::Terminal(format!("sudo pacman -Rns {}", p.name)))
            };
            let title = if verb == "install" && p.installed { format!("{} (installed)", p.name) } else { p.name.clone() };
            let subtitle = if p.desc.is_empty() { format!("{} {}", p.repo, p.version) } else { p.desc.clone() };
            Item { title, subtitle, kind, icon: Icon::Badge(symbol, badge), act, danger: false }
        })
        .collect()
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Everything shown for `query`, best first.
pub fn search(query: &str, apps: &[AppEntry]) -> Vec<Item> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    if let Some((verb, rest)) = package_verb(q) {
        if rest.chars().count() >= 2 {
            return package_items(verb, rest);
        }
    }
    let mut out = Vec::new();
    if let Some(v) = calc::eval(q) {
        let r = calc::format(v);
        out.push(Item { title: format!("= {r}"), subtitle: format!("{q}  ·  Enter copies the result"), kind: "Calculator", icon: Icon::Badge("equal", ORANGE), act: Act::Copy(r), danger: false });
    }
    let lq = q.to_lowercase();
    let mut cmds: Vec<Item> = commands().into_iter().filter(|(i, extra)| matches_words(&format!("{} {extra}", i.title), &lq)).map(|(i, _)| i).collect();
    // An exact or prefix title match outranks apps, like a real Spotlight "Top Hit".
    let top: Vec<Item> = cmds.iter().filter(|i| i.title.to_lowercase().starts_with(&lq)).cloned().collect();
    cmds.retain(|i| !i.title.to_lowercase().starts_with(&lq));
    out.extend(top);
    let app_items = apps::filter(apps, q).into_iter().take(MAX_APPS).filter_map(|i| apps.get(i)).map(|a| Item {
        title: a.name.clone(),
        subtitle: String::new(),
        kind: "Application",
        icon: Icon::App(a.icon.clone()),
        act: Act::Exec(a.exec.clone()),
        danger: false,
    });
    out.extend(app_items);
    out.extend(cmds);
    if out.is_empty() {
        out.push(Item {
            title: format!("Search packages for “{q}”"),
            subtitle: "Find and install it".into(),
            kind: "Packages",
            icon: Icon::Badge("download", GREEN),
            act: Act::SetQuery(format!("install {q}")),
            danger: false,
        });
    }
    if out.first().is_none_or(|i| i.kind != "Calculator") {
        out.push(Item {
            title: format!("Search the web for “{q}”"),
            subtitle: String::new(),
            kind: "Web",
            icon: Icon::Badge("globe", BLUE),
            act: Act::Url(format!("https://duckduckgo.com/?q={}", url_encode(q))),
            danger: false,
        });
    }
    out
}

/// Opens `shell_cmd` in a terminal window, keeping it open on failure so errors can be read.
fn run_in_terminal(shell_cmd: &str) {
    let script = format!("{shell_cmd}; s=$?; if [ $s -ne 0 ]; then echo; read -rn1 -p \"Failed ($s). Press any key to close.\"; fi");
    let attempts: [Vec<String>; 2] = [
        vec!["xdg-terminal-exec".into(), "--app-id=org.omarchy.terminal".into(), "--title=Omarchy".into(), "-e".into(), "bash".into(), "-c".into(), script.clone()],
        vec!["foot".into(), "bash".into(), "-c".into(), script],
    ];
    for a in attempts {
        if Command::new("setsid").args(&a).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok() {
            return;
        }
    }
}

fn copy_text(text: &str) {
    for tool in [["wl-copy"].as_slice(), ["xclip", "-selection", "clipboard"].as_slice()] {
        let Ok(mut child) = Command::new(tool[0]).args(&tool[1..]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { continue };
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return;
    }
}

fn run(act: &Act) {
    match act {
        Act::Exec(e) => {
            launch::spawn_exec(e);
        }
        Act::Argv(a) => {
            let _ = Command::new("setsid").args(a).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|mut c| {
                std::thread::spawn(move || {
                    let _ = c.wait();
                });
            });
        }
        Act::Terminal(c) => run_in_terminal(c),
        Act::Copy(t) => copy_text(t),
        Act::Url(u) => {
            launch::spawn_exec(&format!("xdg-open {u}"));
        }
        Act::DarkMode => crate::controlcenter::sys::toggle_appearance(),
        Act::SetQuery(_) => {}
    }
}

pub struct Spotlight {
    apps: Vec<AppEntry>,
    field: TextField,
    items: Vec<Item>,
    sel: usize,
    scroll: usize,
    confirm: Option<usize>,
    size: (u32, u32),
    origin: (f32, f32),
    overlay: bool,
    rounded: bool,
    fonts: Option<Fonts>,
    icons: IconCache,
    symbols: SymbolCache,
}

impl Spotlight {
    pub fn new() -> Self {
        Self::with_apps(apps::list())
    }

    pub fn with_apps(apps: Vec<AppEntry>) -> Self {
        Self {
            apps,
            field: TextField::default(),
            items: Vec::new(),
            sel: 0,
            scroll: 0,
            confirm: None,
            size: (CARD_W, HEADER_H as u32),
            origin: (0.0, 0.0),
            overlay: false,
            rounded: true,
            fonts: Fonts::load(),
            icons: IconCache::default(),
            symbols: SymbolCache::default(),
        }
    }

    fn visible_rows(&self) -> usize {
        if self.overlay {
            MAX_ROWS
        } else {
            (((self.size.1 as f32 - HEADER_H) / ROW_H).floor() as usize).max(1)
        }
    }

    fn shown(&self) -> usize {
        self.items.len().min(self.visible_rows())
    }

    /// Height of the card for the current results (the whole window when it is not an overlay).
    fn card_h(&self) -> f32 {
        if self.overlay {
            HEADER_H + if self.items.is_empty() { 0.0 } else { self.shown() as f32 * ROW_H + PAD }
        } else {
            self.size.1 as f32
        }
    }

    fn card_w(&self) -> f32 {
        self.size.0 as f32
    }

    fn requery(&mut self) {
        self.items = search(self.field.text(), &self.apps);
        self.sel = 0;
        self.scroll = 0;
        self.confirm = None;
    }

    fn move_sel(&mut self, delta: isize) -> Response {
        if self.items.is_empty() {
            return Response::NONE;
        }
        let n = self.items.len() as isize;
        self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
        self.confirm = None;
        let rows = self.visible_rows();
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + rows {
            self.scroll = self.sel + 1 - rows;
        }
        Response::REDRAW
    }

    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < 0.0 || x >= self.card_w() || y < HEADER_H {
            return None;
        }
        let i = ((y - HEADER_H) / ROW_H) as usize + self.scroll;
        (i < self.items.len() && i < self.scroll + self.visible_rows()).then_some(i)
    }

    fn activate(&mut self) -> Response {
        let Some(item) = self.items.get(self.sel).cloned() else { return Response::NONE };
        if item.danger && self.confirm != Some(self.sel) {
            self.confirm = Some(self.sel);
            return Response::REDRAW;
        }
        if let Act::SetQuery(q) = &item.act {
            self.field.set(q);
            self.requery();
            return Response::REDRAW;
        }
        run(&item.act);
        Response::act(Action::Close)
    }

    fn measure(&self, s: &str, px: f32, w: Weight) -> f32 {
        self.fonts.as_ref().map_or(s.chars().count() as f32 * px * 0.55, |f| f.measure(s, px, w))
    }

    fn fit(&self, s: &str, px: f32, max_w: f32) -> String {
        if self.measure(s, px, Weight::Regular) <= max_w {
            return s.to_string();
        }
        let mut t = s.to_string();
        while !t.is_empty() && self.measure(&format!("{t}…"), px, Weight::Regular) > max_w {
            t.pop();
        }
        format!("{}…", t.trim_end())
    }

    #[allow(clippy::too_many_arguments)] // a drawing helper: target, text, position, font size, weight, colour
    fn text_at(&mut self, pm: &mut Pixmap, s: &str, x: f32, cy: f32, px: f32, weight: Weight, c: Color) {
        if let Some(f) = &mut self.fonts {
            f.draw(pm, s, x, cy - px * 0.62, px, weight, c);
        }
    }

    fn draw_card(&mut self, pm: &mut Pixmap) {
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        pm.fill(Color::TRANSPARENT);
        let radius = if self.rounded || self.overlay { 16.0 } else { 0.0 };
        fill_rounded(pm, 0.0, 0.0, w, h, radius, color(33, 33, 38, 0.97));

        // Search line.
        if let Some(sym) = self.symbols.get("magnifyingglass", (160, 160, 168), 44) {
            blit_scaled(pm, sym, PAD + 10.0, HEADER_H / 2.0 - 11.0, 22.0, 22.0, 1.0);
        }
        let tx = PAD + 46.0;
        let cy = HEADER_H / 2.0;
        if self.field.text().is_empty() {
            self.text_at(pm, "Spotlight Search", tx, cy, 20.0, Weight::Regular, subtext_color());
        } else {
            let t = self.field.text().to_string();
            self.text_at(pm, &t, tx, cy, 20.0, Weight::Regular, text_color());
            let cx = (tx + self.measure(self.field.before_cursor(), 20.0, Weight::Regular)).round();
            fill_rect(pm, cx, cy - 12.0, 1.5, 24.0, text_color());
        }
        if !self.items.is_empty() {
            fill_rect(pm, 0.0, HEADER_H - 1.0, w, 1.0, color(255, 255, 255, 0.10));
        }

        let end = (self.scroll + self.visible_rows()).min(self.items.len());
        for pos in self.scroll..end {
            let y = HEADER_H + PAD / 2.0 + (pos - self.scroll) as f32 * ROW_H - PAD / 2.0;
            let item = self.items[pos].clone();
            let selected = pos == self.sel;
            if selected {
                fill_rounded(pm, 8.0, y + 2.0, w - 16.0, ROW_H - 4.0, 9.0, color(10, 132, 255, 1.0));
            }
            let ix = 20.0;
            let iy = y + (ROW_H - ICON as f32) / 2.0;
            match &item.icon {
                Icon::App(id) => {
                    let icon = self.icons.get(id, ICON);
                    blit_scaled(pm, icon, ix, iy, ICON as f32, ICON as f32, 1.0);
                }
                Icon::Badge(sym, (r, g, b)) => {
                    fill_rounded(pm, ix, iy, ICON as f32, ICON as f32, 8.0, color(*r, *g, *b, 1.0));
                    if let Some(s) = self.symbols.get(sym, (255, 255, 255), 40) {
                        blit_scaled(pm, s, ix + 6.0, iy + 6.0, 20.0, 20.0, 1.0);
                    }
                }
            }
            let kind_w = self.measure(item.kind, 12.0, Weight::Regular);
            let text_x = ix + ICON as f32 + 14.0;
            let max_w = w - text_x - kind_w - 40.0;
            let danger_armed = self.confirm == Some(pos);
            let (main, sub) = if selected { (color(255, 255, 255, 1.0), color(255, 255, 255, 0.78)) } else { (text_color(), subtext_color()) };
            let sub_text = if item.danger && !danger_armed { String::new() } else { item.subtitle.clone() };
            let sub_color = if danger_armed && !selected { color(255, 120, 110, 1.0) } else { sub };
            if sub_text.is_empty() {
                let title = self.fit(&item.title, 15.0, max_w);
                self.text_at(pm, &title, text_x, y + ROW_H / 2.0, 15.0, Weight::Regular, main);
            } else {
                let title = self.fit(&item.title, 15.0, max_w);
                let subt = self.fit(&sub_text, 11.5, max_w);
                self.text_at(pm, &title, text_x, y + ROW_H / 2.0 - 8.0, 15.0, Weight::Regular, main);
                self.text_at(pm, &subt, text_x, y + ROW_H / 2.0 + 10.0, 11.5, Weight::Regular, sub_color);
            }
            self.text_at(pm, item.kind, w - 20.0 - kind_w, y + ROW_H / 2.0, 12.0, Weight::Regular, if selected { color(255, 255, 255, 0.8) } else { subtext_color() });
        }
        if !self.overlay || !self.items.is_empty() || !self.field.text().is_empty() {
            stroke_rounded(pm, 0.5, 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0), color(255, 255, 255, 0.12), 1.0);
        }
        if let Some(f) = &self.fonts {
            f.trim();
        }
    }
}

impl Default for Spotlight {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowApp for Spotlight {
    fn title(&self) -> String {
        "Spotlight".into()
    }

    fn app_id(&self) -> &'static str {
        "macspot"
    }

    fn overlay(&self) -> bool {
        true
    }

    fn set_overlay(&mut self, overlay: bool) {
        self.overlay = overlay;
    }

    fn initial_size(&self) -> (u32, u32) {
        (CARD_W, HEADER_H as u32 + MAX_ROWS as u32 * ROW_H as u32 / 2)
    }

    fn set_waker(&mut self, _: Waker) {}

    fn set_screen(&mut self, _: u32, _: u32) {}

    fn resize(&mut self, w: u32, h: u32) {
        if self.overlay {
            let cw = CARD_W.min(w.saturating_sub(40)).max(300);
            self.size = (cw, h);
            self.origin = ((w as f32 - cw as f32) / 2.0, (h as f32 * 0.2).round());
        } else {
            self.size = (w, h);
            self.origin = (0.0, 0.0);
        }
    }

    fn set_rounded(&mut self, rounded: bool) {
        self.rounded = rounded;
    }

    fn draw(&mut self, pm: &mut Pixmap) {
        if !self.overlay {
            self.draw_card(pm);
            return;
        }
        pm.fill(color(0, 0, 0, 0.18));
        if let Some(mut card) = Pixmap::new(self.size.0, self.card_h().ceil() as u32) {
            self.draw_card(&mut card);
            pm.draw_pixmap(self.origin.0 as i32, self.origin.1 as i32, card.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
    }

    fn pointer_motion(&mut self, x: f32, y: f32) -> Response {
        match self.row_at(x - self.origin.0, y - self.origin.1) {
            Some(i) if i != self.sel => {
                self.sel = i;
                self.confirm = None;
                Response::REDRAW
            }
            _ => Response::NONE,
        }
    }

    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }

    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if !pressed || button != Button::Left {
            return Response::NONE;
        }
        let (px, py) = (x - self.origin.0, y - self.origin.1);
        if self.overlay && (px < 0.0 || py < 0.0 || px >= self.card_w() || py >= self.card_h()) {
            return Response::act(Action::Close);
        }
        match self.row_at(px, py) {
            Some(i) => {
                self.sel = i;
                self.activate()
            }
            None => Response::NONE,
        }
    }

    fn scroll(&mut self, _: f32, _: f32, dy: f32) -> Response {
        if dy > 0.0 {
            self.move_sel(1)
        } else if dy < 0.0 {
            self.move_sel(-1)
        } else {
            Response::NONE
        }
    }

    fn key(&mut self, ev: &KeyEvent) -> Response {
        match ev.key {
            Key::Escape => Response::act(Action::Close),
            Key::Enter => self.activate(),
            Key::Down | Key::Tab => self.move_sel(1),
            Key::Up => self.move_sel(-1),
            Key::PageDown => self.move_sel(MAX_ROWS as isize),
            Key::PageUp => self.move_sel(-(MAX_ROWS as isize)),
            Key::Left => {
                self.field.left();
                Response::REDRAW
            }
            Key::Right => {
                self.field.right();
                Response::REDRAW
            }
            Key::Home => {
                self.field.home();
                Response::REDRAW
            }
            Key::End => {
                self.field.end();
                Response::REDRAW
            }
            Key::Backspace => {
                self.field.backspace();
                self.requery();
                Response::REDRAW
            }
            Key::Delete => {
                self.field.delete();
                self.requery();
                Response::REDRAW
            }
            Key::Char if ev.ctrl && ev.ch == Some('u') => {
                self.field.clear();
                self.requery();
                Response::REDRAW
            }
            Key::Char if !ev.ctrl && !ev.alt && !ev.text.is_empty() => {
                self.field.insert(&ev.text);
                self.requery();
                Response::REDRAW
            }
            Key::Char => Response::NONE,
        }
    }

    fn tick(&mut self) -> Response {
        Response::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppEntry {
        AppEntry { name: name.into(), exec: format!("{}-bin", name.to_lowercase()), icon: String::new() }
    }

    fn key(k: Key, text: &str) -> KeyEvent {
        KeyEvent { key: k, text: text.into(), ch: text.chars().next(), ctrl: false, shift: false, alt: false }
    }

    fn spot() -> Spotlight {
        let mut s = Spotlight::with_apps(vec![app("Firefox"), app("Files"), app("Terminal")]);
        s.fonts = None;
        s
    }

    fn type_text(s: &mut Spotlight, t: &str) {
        for c in t.chars() {
            s.key(&key(Key::Char, &c.to_string()));
        }
    }

    #[test]
    fn empty_query_shows_nothing() {
        assert!(search("  ", &[app("Firefox")]).is_empty());
    }

    #[test]
    fn apps_and_commands_match() {
        let r = search("fire", &[app("Firefox"), app("Terminal")]);
        assert_eq!(r[0].title, "Firefox");
        let r = search("lock", &[]);
        assert_eq!(r[0].title, "Lock Screen");
        assert!(search("uninstall", &[]).iter().any(|i| i.title == "Remove Packages"), "extra search words");
    }

    #[test]
    fn command_title_prefix_beats_apps() {
        let r = search("shut", &[app("Shutter")]);
        assert_eq!(r[0].title, "Shut Down");
        assert_eq!(r[1].title, "Shutter");
    }

    #[test]
    fn calculator_comes_first_and_copies() {
        let r = search("2+3*4", &[]);
        assert_eq!(r[0].kind, "Calculator");
        assert_eq!(r[0].act, Act::Copy("14".into()));
    }

    #[test]
    fn unknown_text_offers_packages_and_web() {
        let r = search("zzqq", &[app("Firefox")]);
        assert_eq!(r[0].act, Act::SetQuery("install zzqq".into()));
        assert!(matches!(r.last().unwrap().act, Act::Url(ref u) if u.ends_with("zzqq")));
    }

    #[test]
    fn parses_pacman_output() {
        let out = "extra/firefox 131.0-1 [installed]\n    Fast, Private & Safe Web Browser\nextra/firefox-i18n-de 131.0-1\n    German language pack\nlocal/vim 9.1-1\n    Vi Improved\n";
        let p = parse_pacman(out);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].name.as_str(), p[0].installed, p[0].desc.as_str()), ("firefox", true, "Fast, Private & Safe Web Browser"));
        assert!(!p[1].installed);
        assert!(p[2].installed, "local repo means installed");
    }

    #[test]
    fn install_and_remove_items_build_pacman_commands() {
        let pkgs = vec![
            Package { name: "htop".into(), repo: "extra".into(), version: "3.3".into(), installed: false, desc: "Monitor".into() },
            Package { name: "bad;rm".into(), repo: "extra".into(), version: "1".into(), installed: false, desc: String::new() },
        ];
        let i = pacman_items("install", pkgs.clone());
        assert_eq!(i.len(), 1, "unsafe names are dropped");
        assert_eq!(i[0].act, Act::Terminal("sudo pacman -S --needed htop".into()));
        assert_eq!(pacman_items("remove", pkgs)[0].act, Act::Terminal("sudo pacman -Rns htop".into()));
    }

    #[test]
    fn package_verbs() {
        assert_eq!(package_verb("install htop"), Some(("install", "htop")));
        assert_eq!(package_verb("Uninstall   htop "), Some(("remove", "htop")));
        assert_eq!(package_verb("installer"), None);
    }

    #[test]
    fn dangerous_commands_need_two_enters() {
        let mut s = spot();
        type_text(&mut s, "shut");
        assert_eq!(s.items[0].title, "Shut Down");
        let r = s.key(&key(Key::Enter, ""));
        assert_eq!(r.action, None, "first Enter only arms it");
        assert_eq!(s.confirm, Some(0));
        s.key(&key(Key::Down, ""));
        assert_eq!(s.confirm, None, "moving disarms");
    }

    #[test]
    fn suggestion_rewrites_the_query() {
        let mut s = spot();
        type_text(&mut s, "zzqq");
        let r = s.key(&key(Key::Enter, ""));
        assert_eq!(r.action, None);
        assert_eq!(s.field.text(), "install zzqq");
    }

    #[test]
    fn selection_wraps_and_scrolls() {
        let mut s = spot();
        type_text(&mut s, "a");
        let n = s.items.len();
        s.key(&key(Key::Up, ""));
        assert_eq!(s.sel, n - 1);
        s.key(&key(Key::Down, ""));
        assert_eq!(s.sel, 0);
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn overlay_card_grows_with_results_and_background_closes() {
        let mut s = spot();
        s.set_overlay(true);
        s.resize(3440, 1440);
        let empty = s.card_h();
        type_text(&mut s, "fi");
        assert!(s.card_h() > empty);
        assert_eq!(s.pointer_button(5.0, 5.0, Button::Left, true).action, Some(Action::Close));
        let mut pm = Pixmap::new(3440, 1440).unwrap();
        s.draw(&mut pm);
        assert!(pm.pixel(5, 5).unwrap().alpha() > 0);
    }

    #[test]
    fn escape_closes() {
        assert_eq!(spot().key(&key(Key::Escape, "")).action, Some(Action::Close));
    }
}
