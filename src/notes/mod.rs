//! Native Notes application and shared widget storage API.
pub mod store;
use crate::{
    app::Button,
    gfx::{fill_rect, fill_rounded},
    settings::editor::{Editor, Lang, Pos},
    text::{Fonts, Weight},
    window_app::{Key, KeyEvent, Response, Waker, WindowApp},
};
pub use store::{Error, Note, Snapshot, Store};
use tiny_skia::Pixmap;
pub struct Notes {
    store: Store,
    rows: Vec<Note>,
    base: Option<Note>,
    title: Editor,
    body: Editor,
    focus_body: bool,
    status: String,
    fonts: Option<Fonts>,
    w: u32,
    h: u32,
    list_top: usize,
    pin_latest: Option<Box<dyn FnMut(Note)>>,
}
impl Notes {
    pub fn new(store: Store, blank: bool) -> Self {
        let mut s = Self {
            store,
            rows: vec![],
            base: None,
            title: Editor::new("", Lang::Plain),
            body: Editor::new("", Lang::Plain),
            focus_body: false,
            status: String::new(),
            fonts: None,
            w: 900,
            h: 620,
            list_top: 0,
            pin_latest: None,
        };
        s.refresh();
        if !blank {
            if let Some(n) = s.rows.first().cloned() {
                s.load(n)
            }
        }
        s
    }
    /// Parent supplies the already-authorized widget integration. Ctrl+P emits
    /// owned latest-note data only after a successful save.
    pub fn on_pin_latest(mut self, callback: impl FnMut(Note) + 'static) -> Self {
        self.pin_latest = Some(Box::new(callback));
        self
    }
    fn refresh(&mut self) {
        match self.store.snapshot() {
            Ok(s) => {
                self.rows = s.notes;
                if s.corrupt_count > 0 {
                    self.status = format!("{} unreadable notes preserved", s.corrupt_count)
                }
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    fn load(&mut self, n: Note) {
        self.title.set_text(&n.title);
        self.body.set_text(&n.body);
        self.base = Some(n);
        self.focus_body = true;
    }
    fn dirty(&self) -> bool {
        self.title.is_modified() || self.body.is_modified()
    }
    /// Reliable explicit save. Failed writes leave both editors and their undo history intact.
    pub fn save(&mut self, copy: bool) -> bool {
        if !self.dirty() && !copy {
            return true;
        }
        let title = self.title.text();
        let body = self.body.text();
        let r = if copy {
            self.store.create(&title, &body)
        } else if let Some(b) = &self.base {
            self.store.update(b, &title, &body)
        } else {
            self.store.create(&title, &body)
        };
        match r {
            Ok(n) => {
                self.base = Some(n);
                self.title.mark_saved();
                self.body.mark_saved();
                self.status = "Saved privately".into();
                self.refresh();
                true
            }
            Err(e) => {
                self.status = e.to_string();
                false
            }
        }
    }
    fn new_draft(&mut self) {
        if self.save(false) {
            self.base = None;
            self.title.set_text("");
            self.body.set_text("");
            self.focus_body = false;
            self.status = "New note · Ctrl+S to save".into()
        }
    }
    fn focus_timer(&mut self) {
        if !self.save(false) {
            return;
        }
        // Only the title is shared; note bodies never become process arguments.
        let title = timer_title(&self.title.text());
        if crate::launch::spawn_argv(&[crate::launch::sibling_or_path("dectimer"), "--task".into(), title]) {
            self.status = "Opened Timer · choose a duration and start when ready".into();
        } else {
            self.status = "Could not open Timer; check that dectimer is installed".into();
        }
    }
    fn edit(&mut self, e: &KeyEvent) {
        let ed = if self.focus_body { &mut self.body } else { &mut self.title };
        if e.ctrl {
            match e.ch {
                Some('a') => ed.select_all(),
                Some('c') => {
                    if let Some(text) = ed.selected_text() {
                        ed.clip = text.clone();
                        crate::clipboard::copy(&text);
                    }
                }
                Some('x') => {
                    if let Some(text) = ed.selected_text() {
                        if crate::clipboard::copy(&text) {
                            ed.cut();
                            ed.clip = text;
                        }
                    }
                }
                Some('v') => {
                    let text = crate::settings::configpane::system_clipboard().unwrap_or_else(|| ed.clip.clone());
                    ed.paste(&text);
                }
                Some('z') => {
                    if e.shift {
                        ed.redo();
                    } else {
                        ed.undo();
                    }
                }
                Some('y') => {
                    ed.redo();
                }
                _ => {}
            }
            return;
        }
        match e.key {
            Key::Char => ed.insert_str(&e.text),
            Key::Enter if self.focus_body => ed.newline(),
            Key::Backspace => ed.backspace(),
            Key::Delete => ed.delete(),
            Key::Left => ed.left(e.shift),
            Key::Right => ed.right(e.shift),
            Key::Up => ed.up(e.shift),
            Key::Down => ed.down(e.shift),
            Key::Home => ed.home(e.shift),
            Key::End => ed.end(e.shift),
            Key::PageUp => ed.page_up(e.shift),
            Key::PageDown => ed.page_down(e.shift),
            _ => {}
        }
    }
}
impl WindowApp for Notes {
    fn title(&self) -> String {
        format!("{}Notes — Decebal", if self.dirty() { "● " } else { "" })
    }
    fn app_id(&self) -> &'static str {
        "decnotes"
    }
    fn initial_size(&self) -> (u32, u32) {
        (900, 620)
    }
    fn set_waker(&mut self, w: Waker) {
        crate::theme::live::start_watcher(move || w());
    }
    fn set_screen(&mut self, _: u32, _: u32) {}
    fn resize(&mut self, w: u32, h: u32) {
        self.w = w;
        self.h = h
    }
    fn set_rounded(&mut self, _: bool) {}
    fn draw(&mut self, pm: &mut Pixmap) {
        let dirty = self.dirty();
        let t = crate::theme::current();
        pm.fill(t.background.color(1.));
        let w = pm.width() as f32;
        let h = pm.height() as f32;
        let side = (w * 0.28).clamp(100., 240.);
        fill_rect(pm, 0., 0., side, h, t.sidebar.color(1.));
        fill_rounded(pm, 16., 58., side - 32., 34., 8., t.accent.color(1.));
        fill_rounded(pm, 16., h - 48., side - 32., 32., 8., t.surface.color(1.));
        fill_rounded(pm, w - 96., 16., 80., 32., 8., t.accent.color(1.));
        fill_rounded(pm, w - 230., 16., 124., 32., 8., t.surface.color(1.));
        if self.fonts.is_none() {
            self.fonts = Fonts::load()
        }
        let Some(f) = self.fonts.as_mut() else { return };
        let ink = t.text.color(1.);
        let sub = t.text_secondary.color(1.);
        f.draw(pm, "Notes", 20., 18., 22., Weight::SemiBold, ink);
        f.draw(pm, "+ New note", 26., 66., 14., Weight::Medium, t.accent_text.color(1.));
        f.draw(pm, "Widgets…", 26., h - 40., 13., Weight::Medium, ink);
        f.draw(pm, "Save", w - 76., 24., 14., Weight::Medium, t.accent_text.color(1.));
        f.draw(pm, "Focus timer", w - 215., 24., 14., Weight::Medium, ink);
        let visible = ((h - 146.) / 60.).max(0.) as usize;
        for (i, n) in self.rows.iter().skip(self.list_top).take(visible).enumerate() {
            let y = 110. + i as f32 * 60.;
            if self.base.as_ref().is_some_and(|b| b.id == n.id) {
                fill_rounded(pm, 8., y - 6., side - 16., 52., 8., t.surface.color(1.));
            }
            let title = if n.title.is_empty() { "Untitled" } else { &n.title };
            let max = ((side - 40.) / 9.) as usize;
            f.draw(pm, &title.chars().take(max).collect::<String>(), 20., y, 15., Weight::Medium, ink);
            let preview = n.body.lines().next().unwrap_or("").chars().take(max).collect::<String>();
            f.draw(pm, &preview, 20., y + 24., 12., Weight::Regular, sub);
        }
        if self.rows.is_empty() {
            f.draw(pm, "Your ideas, here.", 20., 120., 13., Weight::Regular, sub);
        }
        let x = side + 28.;
        let width = (w - x - 28.).max(1.);
        for (index, ed) in [&mut self.title, &mut self.body].into_iter().enumerate() {
            let y = if index == 0 { 78. } else { 146. };
            let px = if index == 0 { 24. } else { 16. };
            let lineh = px + 10.;
            let maxlines = if index == 0 { 1 } else { ((h - y - 72.) / lineh).max(1.) as usize };
            ed.page = maxlines;
            if ed.cursor.line < ed.top {
                ed.top = ed.cursor.line
            }
            if ed.cursor.line >= ed.top + maxlines {
                ed.top = ed.cursor.line + 1 - maxlines
            }
            let active = self.focus_body == (index == 1);
            let prefix: String = ed.line(ed.cursor.line).chars().take(ed.cursor.col).collect();
            let cx = f.measure(&prefix, px, Weight::Regular);
            if active {
                ed.left = (cx - width + 4.).max(0.);
            }
            for (i, line) in ed.lines().iter().skip(ed.top).take(maxlines).enumerate() {
                let line_no = ed.top + i;
                if let Some((a, b)) = ed.selection() {
                    if line_no >= a.line && line_no <= b.line {
                        let begin = if line_no == a.line { a.col } else { 0 };
                        let end = if line_no == b.line { b.col } else { line.chars().count() };
                        let before: String = line.chars().take(begin).collect();
                        let selected: String = line.chars().skip(begin).take(end.saturating_sub(begin)).collect();
                        let left = (f.measure(&before, px, Weight::Regular) - ed.left).clamp(0., width);
                        let right = (f.measure(&before, px, Weight::Regular) + f.measure(&selected, px, Weight::Regular) - ed.left).clamp(0., width);
                        if right > left {
                            fill_rect(pm, x + left, y + i as f32 * lineh, right - left, lineh, t.accent.color(0.25));
                        }
                    }
                }
                let mut consumed = 0.;
                let mut text = String::new();
                for c in line.chars() {
                    let cw = f.measure(&c.to_string(), px, Weight::Regular);
                    consumed += cw;
                    if consumed <= ed.left {
                        continue;
                    }
                    if consumed - ed.left > width {
                        break;
                    }
                    text.push(c);
                }
                f.draw(pm, &text, x, y + i as f32 * lineh, px, Weight::Regular, ink);
            }
            if ed.text().is_empty() {
                f.draw(pm, if index == 0 { "Untitled note" } else { "Start writing…" }, x, y, px, Weight::Regular, t.text_faint.color(1.));
            }
            if active {
                fill_rect(pm, x + (cx - ed.left).max(0.), y + (ed.cursor.line - ed.top) as f32 * lineh, 1.5, px, t.accent.color(1.));
            }
        }
        fill_rect(pm, side, h - 58., w - side, 58., t.background.color(1.));
        let status = if self.status.is_empty() { "Ctrl+N New · Ctrl+S Save · Tab Title/body · Ctrl+Shift+S Save copy" } else { &self.status };
        f.draw(pm, &status.chars().take(((w - side - 40.) / 7.).max(1.) as usize).collect::<String>(), x, h - 40., 12., Weight::Regular, sub);
        f.draw(pm, if dirty { "Unsaved changes · save before closing" } else { "Private notes · stored on this device" }, x, h - 20., 11., Weight::Regular, sub);
    }
    fn pointer_motion(&mut self, _: f32, _: f32) -> Response {
        Response::NONE
    }
    fn pointer_leave(&mut self) -> Response {
        Response::NONE
    }
    fn pointer_button(&mut self, x: f32, y: f32, button: Button, pressed: bool) -> Response {
        if button != Button::Left || !pressed {
            return Response::NONE;
        }
        let side = (self.w as f32 * 0.28).clamp(100., 240.);
        if x < side {
            if (self.h as f32 - 48. ..self.h as f32 - 16.).contains(&y) {
                if !crate::launch::spawn_argv(&[crate::launch::sibling_or_path("decsettings"), "widgets".into()]) {
                    self.status = "Could not open Widget settings".into();
                }
            } else if (58. ..92.).contains(&y) {
                self.new_draft()
            } else if y >= 104. {
                let i = self.list_top + ((y - 104.) / 60.) as usize;
                if let Some(n) = self.rows.get(i).cloned() {
                    if self.save(false) {
                        self.load(n)
                    }
                }
            }
        } else if y < 58. && x > self.w as f32 - 100. {
            self.save(false);
        } else if (16. ..48.).contains(&y) && (self.w as f32 - 230. ..self.w as f32 - 106.).contains(&x) {
            self.focus_timer();
        } else {
            self.focus_body = y >= 130.;
            let ed = if self.focus_body { &mut self.body } else { &mut self.title };
            let px = if self.focus_body { 16. } else { 24. };
            let start = if self.focus_body { 146. } else { 78. };
            let line = (ed.top + ((y - start).max(0.) / (px + 10.)) as usize).min(ed.line_count().saturating_sub(1));
            let col = if let Some(f) = &self.fonts {
                let wanted = (x - side - 28. + ed.left).max(0.);
                let mut width = 0.;
                ed.line(line)
                    .chars()
                    .take_while(|c| {
                        width += f.measure(&c.to_string(), px, Weight::Regular);
                        width < wanted
                    })
                    .count()
            } else {
                0
            };
            ed.set_cursor(Pos::new(line, col), false);
        }
        Response::REDRAW
    }
    fn scroll(&mut self, x: f32, _: f32, dy: f32) -> Response {
        if x < (self.w as f32 * 0.28).clamp(100., 240.) {
            self.list_top = if dy > 0. { (self.list_top + 1).min(self.rows.len().saturating_sub(1)) } else { self.list_top.saturating_sub(1) }
        } else {
            self.body.top = if dy > 0. { (self.body.top + 3).min(self.body.line_count().saturating_sub(1)) } else { self.body.top.saturating_sub(3) }
        }
        Response::REDRAW
    }
    fn key(&mut self, e: &KeyEvent) -> Response {
        if e.ctrl && e.ch == Some('s') {
            self.save(e.shift);
        } else if e.ctrl && e.shift && e.ch == Some('t') {
            self.focus_timer();
        } else if e.ctrl && e.ch == Some('p') {
            if self.save(false) {
                if let Some(callback) = &mut self.pin_latest {
                    if let Ok(Some(note)) = self.store.latest() {
                        callback(note);
                    }
                } else {
                    self.status = "Pin latest note is available through the Widgets creator".into();
                }
            }
        } else if e.ctrl && e.ch == Some('n') {
            self.new_draft()
        } else if e.key == Key::Tab {
            self.focus_body = !self.focus_body
        } else {
            self.edit(e);
            if self.dirty() {
                self.save(false);
            }
        }
        Response::REDRAW
    }
    fn close_requested(&mut self) -> Response {
        if self.save(false) {
            Response::act(crate::window_app::Action::Close)
        } else {
            Response::REDRAW
        }
    }
    fn tick(&mut self) -> Response {
        Response::NONE
    }
}
/// Timer state permits at most 120 UTF-8 bytes and no control characters.
fn timer_title(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > 120 {
            break;
        }
        out.push(c);
    }
    if out.is_empty() {
        "Focus time".into()
    } else {
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    #[test]
    fn focus_title_is_bounded_unicode_and_literal() {
        assert_eq!(timer_title("  A {task}; $(literal)  "), "A {task}; $(literal)");
        assert_eq!(timer_title("\n\t"), "Focus time");
        assert_eq!(timer_title(&"🦀".repeat(40)), "🦀".repeat(30));
        assert_eq!(timer_title("A\nB"), "AB");
    }
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("decnotes-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))))
        }
        fn store(&self) -> Store {
            Store::in_dir(&self.0)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn persistence_permissions_conflicts() {
        let t = Temp::new();
        let s = t.store();
        assert_eq!(s.snapshot().unwrap().count(), 0);
        assert!(!t.0.exists());
        let n = s.create("{\"café\"}", "日本語\n{literal}").unwrap();
        assert_eq!(fs::metadata(&t.0).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(t.0.join(format!("{}.json", n.id))).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(t.store().read(&n.id).unwrap(), n);
        let updated = s.update(&n, "new", &n.body).unwrap();
        assert!(matches!(s.update(&n, "stale", "bad"), Err(Error::Conflict)));
        assert_eq!(s.latest().unwrap(), Some(updated.clone()));
        assert!(s.read("../bad").is_err());
        fs::write(t.0.join("00000000000000000000000000000000.json"), "broken").unwrap();
        let snapshot = s.snapshot().unwrap();
        assert_eq!(snapshot.corrupt_count, 1);
        assert_eq!(snapshot.notes, vec![updated]);
        assert_eq!(fs::read_to_string(t.0.join("00000000000000000000000000000000.json")).unwrap(), "broken");
    }
    #[test]
    fn simultaneous_writers() {
        let t = Temp::new();
        let s = t.store();
        let n = s.create("base", "").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let s = s.clone();
                let n = n.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    s.update(&n, "change", "")
                })
            })
            .collect();
        barrier.wait();
        let ok = handles.into_iter().filter_map(|h| h.join().unwrap().ok()).count();
        assert_eq!(ok, 1);
    }
    fn key(ch: char, ctrl: bool, shift: bool) -> KeyEvent {
        KeyEvent { key: Key::Char, text: if ctrl { String::new() } else { ch.to_string() }, ch: Some(ch), ctrl, shift, alt: false }
    }
    #[test]
    fn draft_flow_and_failure() {
        let t = Temp::new();
        let mut app = Notes::new(t.store(), true);
        app.key(&key('é', false, false));
        app.key(&key('s', true, false));
        assert_eq!(app.store.latest().unwrap().unwrap().title, "é");
        app.key(&key('n', true, false));
        app.key(&key('字', false, false));
        assert!(app.base.is_some());
        let mut pm = Pixmap::new(900, 620).unwrap();
        app.draw(&mut pm);
        assert!(pm.data().iter().any(|b| *b != 0));
        if let Some(path) = std::env::var_os("DECNOTES_TEST_PNG") {
            pm.save_png(path).unwrap();
        }
        let original = app.store.create("external", "").unwrap();
        app.load(original.clone());
        app.store.update(&original, "elsewhere", "").unwrap();
        app.key(&key('x', false, false));
        assert!(!app.save(false));
        assert!(app.dirty());
        assert!(app.title.text().contains('x'));
        assert_eq!(app.store.read(&original.id).unwrap().title, "elsewhere");
        assert!(app.save(true));
    }
    #[test]
    fn native_preview_and_normal_close() {
        let t = Temp::new();
        t.store().create("A little space to think", "Pick one useful thing to finish today.\n\nBuild carefully. Leave the next step clear.").unwrap();
        let mut app = Notes::new(t.store(), false);
        let mut pm = Pixmap::new(900, 620).unwrap();
        app.draw(&mut pm);
        assert_eq!(app.close_requested().action, Some(crate::window_app::Action::Close));
        if let Some(dir) = std::env::var_os("ECOSYSTEM_EVIDENCE_DIR") {
            fs::create_dir_all(&dir).unwrap();
            pm.save_png(std::path::PathBuf::from(dir).join("notes.png")).unwrap();
        }
    }
    #[test]
    fn autosave_io_failure_preserves_draft() {
        let t = Temp::new();
        let mut app = Notes::new(t.store(), true);
        app.key(&key('a', false, false));
        let original = app.base.clone().unwrap();
        let blocked = t.0.join("blocked");
        fs::write(&blocked, "not a directory").unwrap();
        app.store = Store::in_dir(blocked);
        app.key(&key('字', false, false));
        assert_eq!(app.title.text(), "a字");
        assert!(app.dirty());
        assert!(app.status.contains("draft preserved"));
        assert_eq!(t.store().read(&original.id).unwrap().title, "a");
        assert!(!app.save(false));
        assert_eq!(app.close_requested().action, None);
        assert_eq!(app.title.text(), "a字");
    }
}
