//! The bar's status items beyond the Control Center: layout switcher, sound, Wi-Fi, Bluetooth,
//! battery and the calendar. Opening one shows a popup under the item (see [`popup`]); scrolling
//! over the sound and layout items adjusts them directly. Every external command runs on a worker.

use super::popup::{Act, CalHit, Calendar, Icon, Popup, Row};
use super::*;

const POPUP_REFRESH: Duration = Duration::from_secs(4);

impl DecBar {
    pub(super) fn tray_span(&self, kind: Tray) -> Option<Span> {
        self.tray().into_iter().find(|(k, _)| *k == kind).map(|(_, s)| s)
    }

    /// Stay Awake off (the coffee cup was clicked): the same switch as the Control Center tile; the icon goes with it.
    pub(super) fn awake_off(&mut self) {
        awake::set(false);
        self.update_snapshot(|s| s.awake = false);
    }

    /// Right click on the coffee cup of the horizontal bar: the options popup. True when it was over the cup.
    pub(super) fn awake_options(&mut self, x: f32) -> bool {
        if !self.tray_span(Tray::Awake).is_some_and(|s| x >= s.x && x < s.x + s.w) {
            return false;
        }
        self.open_popup(Tray::Awake);
        true
    }

    /// Opens the process manager popup (`decmon --popup`, a toggle) with `placement` arguments (`--x` / `--top`).
    pub(super) fn process_monitor(&self, placement: Vec<String>) {
        let mut argv = vec![crate::launch::sibling_or_path("decmon"), "--popup".to_string()];
        argv.extend(placement);
        if sys::is_dry_run() {
            let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            sys::spawn(&refs);
        } else {
            crate::launch::spawn_argv(&argv);
        }
    }

    /// The weather popup: conditions, location, today's range and the short forecast, then Refresh. Without a reading
    /// (offline, first start) it says so and still offers Refresh.
    pub(super) fn weather_rows(&self) -> Option<Vec<Row>> {
        let wx = self.shared.weather.lock().ok().and_then(|w| w.clone());
        let busy = self.shared.weather_busy.load(Ordering::SeqCst);
        let failed = self.shared.weather_failed.load(Ordering::SeqCst);
        let now = weather::now();
        let refresh = |detail: &str| Row::item("Refresh", Act::WeatherRefresh).icon(Icon::Symbol("refresh")).detail(detail);
        let Some(w) = wx else {
            let note = if busy {
                "Fetching the weather…"
            } else if failed {
                "No weather data (offline?)"
            } else {
                "No weather data yet"
            };
            return Some(vec![Row::header("Weather"), Row::text(note), Row::sep(), refresh(if busy { "…" } else { "" })]);
        };
        let words = if w.desc.is_empty() { weather::condition_words(w.code, w.night).to_string() } else { w.desc.clone() };
        let mut rows = vec![Row::header(if w.location.is_empty() { "Weather" } else { &w.location }).detail(&words)];
        let sym = weather::symbol_for(w.code, w.night);
        let now_line = match w.feels_c {
            Some(f) if f != w.temp_c => format!("{}°C, feels like {}°", w.temp_c, f),
            _ => format!("{}°C", w.temp_c),
        };
        rows.push(Row::item(&now_line, Act::None).icon(Icon::Symbol(sym)));
        if let Some(h) = w.humidity {
            rows.push(Row::text(&format!("Humidity {h}%")));
        }
        if let Some(k) = w.wind_kmh {
            rows.push(Row::text(format!("Wind {k} km/h {}", w.wind_dir).trim_end()));
        }
        if let Some(today) = w.days.first() {
            rows.push(Row::text(&format!("Today: high {}°, low {}°", today.high_c, today.low_c)));
        }
        if w.days.len() > 1 {
            rows.push(Row::sep());
            rows.push(Row::header("Forecast"));
            for d in w.days.iter().skip(1) {
                let day = weather::weekday_short(&d.date).unwrap_or(&d.date);
                rows.push(Row::item(day, Act::None).icon(Icon::Symbol(weather::symbol_for(d.code, false))).detail(&format!("{}° / {}°", d.high_c, d.low_c)));
            }
        }
        rows.push(Row::sep());
        if busy {
            rows.push(refresh("Refreshing…"));
        } else if failed {
            rows.push(refresh(&format!("Offline, from {}", weather::age_words(w.fetched, now))));
        } else {
            rows.push(refresh(&format!("Updated {}", weather::age_words(w.fetched, now))));
        }
        Some(rows)
    }

    /// Rows, width and compactness of the popup for `kind`.
    pub(super) fn build_popup(&self, kind: Tray) -> Option<(Vec<Row>, f32, bool)> {
        let snap = self.snapshot();
        match kind {
            Tray::Indicator(key) => Some(self.indicator_rows(key)),
            Tray::Indicators => Some(self.overflow_rows()),
            Tray::Layout => {
                let cur = self.layout_state()?;
                let mut rows = vec![Row::header("Window Layout").detail(&rdwm::title_of(&cur.name))];
                for (name, title, sym) in rdwm::LAYOUTS {
                    rows.push(Row::item(title, Act::SetLayout(name.to_string())).icon(Icon::Layout(name.to_string())).checked(cur.canonical() == *name).detail(sym));
                }
                if !rdwm::LAYOUTS.iter().any(|l| l.0 == cur.canonical()) {
                    rows.push(Row::item(&cur.name, Act::SetLayout(cur.name.clone())).checked(true).detail(&cur.symbol));
                }
                rows.push(Row::sep());
                rows.push(Row::item("More master windows", Act::IncMaster(1)).detail("+"));
                rows.push(Row::item("Fewer master windows", Act::IncMaster(-1)).detail("-"));
                rows.push(Row::item("Wider master", Act::Mfact(0.05)));
                rows.push(Row::item("Narrower master", Act::Mfact(-0.05)));
                rows.push(Row::sep());
                rows.push(Row::item("Toggle gaps", Act::ToggleGaps));
                rows.push(Row::item("Float or tile focused window", Act::ToggleFloat));
                Some((rows, 272.0, true))
            }
            Tray::Audio => {
                let v = snap.volume.as_ref()?;
                let shown = if v.muted { 0.0 } else { v.level.clamp(0.0, 1.0) };
                let mut rows = vec![
                    Row::header("Sound").detail(&snap.sink),
                    Row::slider(shown, controlcenter::speaker_icon(v), Act::VolumeSlider),
                    Row::item("Mute", Act::ToggleMute).checked(v.muted).detail(&format!("{}%", (v.level * 100.0).round() as u32)),
                ];
                if let Some(b) = snap.brightness {
                    rows.push(Row::sep());
                    rows.push(Row::header("Display"));
                    rows.push(Row::slider(b as f32 / 100.0, "sun-small", Act::BrightnessSlider));
                }
                Some((rows, 280.0, false))
            }
            Tray::Network => {
                let net = snap.net.as_ref()?;
                let mut rows = vec![Row::header("Wi-Fi").detail(&net.wifi_name), Row::item("Wi-Fi", Act::WifiToggle).checked(net.wifi_enabled).detail(if net.wifi_enabled { "On" } else { "Off" })];
                if net.wifi_enabled {
                    rows.push(Row::sep());
                    let list = self.shared.wifi.lock().map(|l| l.clone()).unwrap_or_default();
                    if list.is_empty() {
                        rows.push(Row::text("No networks found"));
                    }
                    for n in list.iter().take(10) {
                        let detail = if n.secure { format!("secured  {}%", n.signal) } else { format!("{}%", n.signal) };
                        rows.push(Row::item(&n.ssid, Act::WifiConnect(n.ssid.clone())).checked(n.active).detail(&detail));
                    }
                }
                if !net.wired_name.is_empty() {
                    rows.push(Row::sep());
                    rows.push(Row::text(&format!("Ethernet: {}", net.wired_name)));
                }
                if sys::have("nm-connection-editor") {
                    rows.push(Row::sep());
                    rows.push(Row::item("Network Settings…", Act::NetworkEditor));
                }
                Some((rows, 300.0, false))
            }
            Tray::Bluetooth => {
                let bt = snap.bluetooth.as_ref()?;
                let mut rows = vec![Row::header("Bluetooth"), Row::item("Bluetooth", Act::BtToggle).checked(bt.powered).detail(if bt.powered { "On" } else { "Off" })];
                if bt.powered {
                    rows.push(Row::sep());
                    let list = self.shared.bt.lock().map(|l| l.clone()).unwrap_or_default();
                    if list.is_empty() {
                        rows.push(Row::text("No paired devices"));
                    }
                    for d in list.iter().take(12) {
                        rows.push(Row::item(&d.name, Act::BtDevice { mac: d.mac.clone(), connect: !d.connected }).checked(d.connected).detail(if d.connected { "Connected" } else { "" }));
                    }
                }
                Some((rows, 280.0, false))
            }
            Tray::Weather => Some((self.weather_rows()?, 300.0, false)),
            Tray::Window => {
                let (id, title) = self.v_win.clone()?;
                let mut rows = vec![Row::header(&active::display_name(&id))];
                if !title.is_empty() {
                    rows.push(Row::text(&title));
                }
                rows.push(Row::sep());
                rows.push(Row::item("Close window", Act::CloseWindow));
                rows.push(Row::item("Minimise", Act::MinimizeWindow));
                if self.layout_state().is_some() {
                    rows.push(Row::item("Float or tile", Act::ToggleFloat));
                }
                Some((rows, 300.0, false))
            }
            Tray::Awake => {
                if !snap.awake {
                    return None;
                }
                let (on, end) = awake::status_lines(awake::since(), awake::until(), awake::now_secs());
                let until = awake::until();
                let mut rows = vec![Row::header("Stay Awake").detail(&on), Row::text(&end), Row::sep(), Row::item("Turn off Stay Awake", Act::AwakeOff).detail("click")];
                rows.push(Row::sep());
                rows.push(Row::header("Keep awake for"));
                for (secs, label) in awake::DURATIONS {
                    rows.push(Row::item(label, Act::AwakeFor(secs)).checked(secs.is_none() && until.is_none()));
                }
                Some((rows, 264.0, true))
            }
            Tray::Battery => {
                let b = snap.battery.as_ref()?;
                Some((vec![Row::header("Battery").detail(&format!("{}%", b.percent)), Row::text(if b.status.is_empty() { "Unknown" } else { &b.status })], 220.0, false))
            }
            _ => None,
        }
    }

    pub(super) fn open_popup(&mut self, kind: Tray) {
        if self.pill_bar() {
            return self.v_open_popup(kind);
        }
        self.close();
        let Some(span) = self.tray_span(kind).or_else(|| matches!(kind, Tray::Indicator(_)).then(|| self.tray_span(Tray::Indicators)).flatten()) else { return };
        let y = self.bar_bottom() + MENU_GAP;
        let width = self.width as f32;
        if kind == Tray::Clock {
            let Some(today) = clock::today() else { return };
            let x = (span.x + span.w - Calendar::W).clamp(4.0, (width - Calendar::W - 4.0).max(4.0));
            self.calendar = Some(Calendar::new(x, y, today));
            self.open = Open::Popup(kind);
            return;
        }
        let Some((rows, w, compact)) = self.build_popup(kind) else { return };
        let x = if kind == Tray::Layout { span.x - 4.0 } else { span.x + span.w / 2.0 - w / 2.0 };
        let x = x.clamp(4.0, (width - w - 4.0).max(4.0));
        self.popup = Some(Popup::layout(rows, x, y, w, compact));
        self.open = Open::Popup(kind);
        self.last_full_refresh = Instant::now();
        match kind {
            Tray::Audio => self.spawn_snapshot(true),
            Tray::Network | Tray::Bluetooth => self.spawn_lists(kind),
            Tray::Battery => self.spawn_snapshot(false),
            Tray::Weather if self.weather_stale() => self.spawn_weather(),
            _ => {}
        }
    }

    /// Rebuilds the open popup from fresh data, keeping its position.
    pub(super) fn rebuild_popup(&mut self) {
        let Open::Popup(kind) = self.open else { return };
        let Some(old) = self.popup.as_ref().map(|p| (p.x, p.y)) else { return };
        match self.build_popup(kind) {
            Some((rows, w, compact)) => {
                self.popup = Some(Popup::layout(rows, old.0, old.1, w, compact));
                if self.pill_bar() {
                    self.v_clamp_popup();
                }
                let n = self.popup.as_ref().map(|p| p.rows.len()).unwrap_or(0);
                if self.popup_hover.is_some_and(|h| h >= n) {
                    self.popup_hover = None;
                }
            }
            None => {
                self.close();
            }
        }
    }

    /// Refreshes the Wi-Fi or Bluetooth lists off the UI thread.
    pub(super) fn spawn_lists(&self, kind: Tray) {
        if self.shared.lists_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            match kind {
                Tray::Network => {
                    if let Ok(l) = shared.wifi.lock() {
                        if l.is_empty() {
                            sys::rescan_wifi();
                        }
                    }
                    let list = sys::probe_wifi_list();
                    let net = sys::probe_net();
                    if let Ok(mut l) = shared.wifi.lock() {
                        *l = list;
                    }
                    if let (Some(n), Ok(mut s)) = (net, shared.snap.lock()) {
                        s.net = Some(n);
                    }
                }
                _ => {
                    let list = sys::probe_bt_devices();
                    let bt = sys::probe_bluetooth();
                    if let Ok(mut l) = shared.bt.lock() {
                        *l = list;
                    }
                    if let (Some(b), Ok(mut s)) = (bt, shared.snap.lock()) {
                        s.bluetooth = Some(b);
                    }
                }
            }
            shared.changed.store(true, Ordering::SeqCst);
            shared.lists_busy.store(false, Ordering::SeqCst);
            if let Some(w) = waker {
                w.wake();
            }
        });
    }

    fn mark_volume_set(&self) {
        if let Ok(mut t) = self.shared.vol_set_at.lock() {
            *t = Some(Instant::now());
        }
    }

    /// Sets the volume (0..=1), updating the shown state at once.
    pub(super) fn set_volume_now(&mut self, level: f32) {
        let level = level.clamp(0.0, 1.0);
        self.update_snapshot(|s| s.volume = Some(sys::Volume { level, muted: false }));
        self.mark_volume_set();
        sys::set_volume(level);
    }

    pub(super) fn toggle_mute_now(&mut self) {
        self.update_snapshot(|s| {
            if let Some(v) = s.volume.as_mut() {
                v.muted = !v.muted;
            }
        });
        self.mark_volume_set();
        sys::toggle_mute();
    }

    /// Wheel over a status item. Returns true when something changed.
    pub(super) fn tray_scroll(&mut self, x: f32, dy: f32) -> bool {
        let Some((kind, _)) = self.tray().into_iter().find(|(_, s)| x >= s.x && x < s.x + s.w) else { return false };
        match kind {
            Tray::Indicator(key) => self.indicator_scroll(key, dy),
            Tray::Audio => {
                let Some(v) = self.snapshot().volume else { return false };
                let cur = if v.muted { 0.0 } else { v.level };
                self.set_volume_now((cur - dy.signum() * 0.05).clamp(0.0, 1.0));
                if matches!(self.open, Open::Popup(Tray::Audio)) {
                    self.rebuild_popup();
                }
                true
            }
            Tray::Layout => {
                rdwm::send_action(if dy > 0.0 { "cyclelayout" } else { "cyclelayout_back" }, &[]);
                false
            }
            _ => false,
        }
    }

    /// Pointer press inside the open popup/calendar.
    pub(super) fn popup_press(&mut self, x: f32, y: f32) -> bool {
        if let Some(cal) = self.calendar.as_mut() {
            match cal.hit(x, y) {
                Some(CalHit::Prev) => cal.shift(-1),
                Some(CalHit::Next) => cal.shift(1),
                Some(CalHit::Today) => {
                    if let Some(t) = clock::today() {
                        cal.year = t.0;
                        cal.mon0 = t.1;
                    }
                }
                None => return false,
            }
            return true;
        }
        let Some(row) = self.popup.as_ref().and_then(|p| p.hit(x, y)) else { return false };
        let (act, slider) = {
            let p = self.popup.as_ref().expect("popup present");
            (p.rows[row].act.clone(), matches!(p.rows[row].kind, popup::Kind::Slider(_)))
        };
        if slider {
            self.popup_drag = Some(row);
            self.popup_slide(row, x, true);
        } else {
            if matches!(act, Act::IndicatorOpen(_)) {
                self.indicator_anchor = ((x as i32).saturating_add(self.indicator_origin.0), (y as i32).saturating_add(self.indicator_origin.1));
            }
            self.run_act(act);
        }
        true
    }

    pub(super) fn popup_slide(&mut self, row: usize, px: f32, commit: bool) {
        let Some(p) = self.popup.as_ref() else { return };
        let v = p.slider_value(px);
        match p.rows[row].act {
            Act::VolumeSlider => {
                self.update_snapshot(|s| s.volume = Some(sys::Volume { level: v, muted: false }));
                self.mark_volume_set();
                if commit || self.cc_last_send.elapsed() > Duration::from_millis(60) {
                    sys::set_volume(v);
                    self.cc_last_send = Instant::now();
                }
            }
            Act::BrightnessSlider => {
                let pct = (v * 100.0).round() as u32;
                self.update_snapshot(|s| s.brightness = Some(pct));
                if commit || self.cc_last_send.elapsed() > Duration::from_millis(120) {
                    sys::set_brightness(pct);
                    self.cc_last_send = Instant::now();
                }
            }
            _ => {}
        }
        self.rebuild_popup();
    }

    pub(super) fn run_act(&mut self, act: Act) {
        match act {
            a @ (Act::IndicatorOpen(_) | Act::IndicatorActivate(_) | Act::IndicatorEvent { .. } | Act::IndicatorSubmenu(_) | Act::IndicatorBack | Act::IndicatorPage(_)) => {
                self.indicator_action(&a);
                if self.embedded {
                    self.status_place(self.embed.edge, self.embed.up, self.embed.screen_w);
                }
            }
            Act::None | Act::VolumeSlider | Act::BrightnessSlider => {}
            Act::AwakeOff => {
                self.close();
                self.awake_off();
            }
            Act::WeatherRefresh => {
                self.spawn_weather();
                self.rebuild_popup();
            }
            Act::AwakeFor(secs) => {
                awake::start(secs);
                self.update_snapshot(|s| s.awake = true);
            }
            Act::SetLayout(name) => {
                rdwm::send_action("setlayout", &[&name]);
                let sym = rdwm::LAYOUTS.iter().find(|l| l.0 == name).map(|l| l.2.to_string()).unwrap_or_default();
                if let Ok(mut l) = self.shared.layout.lock() {
                    let tags = l.as_ref().map(|s| s.tags.clone()).unwrap_or_default();
                    *l = Some(rdwm::LayoutState { name, symbol: sym, tags });
                }
                self.close();
            }
            Act::IncMaster(d) => rdwm::send_action("incnmaster", &[&d.to_string()]),
            Act::Mfact(d) => rdwm::send_action("setmfact", &[&format!("{d:.2}")]),
            Act::ToggleGaps => rdwm::send_action("togglegaps", &[]),
            Act::ToggleFloat => rdwm::send_action("togglefloating", &[]),
            Act::CloseWindow => {
                self.close();
                self.active.close_active();
            }
            Act::MinimizeWindow => {
                self.close();
                self.active.minimize_active();
            }
            Act::ToggleMute => self.toggle_mute_now(),
            Act::WifiToggle => {
                let on = self.snapshot().net.map(|n| n.wifi_enabled).unwrap_or(false);
                sys::set_wifi(!on);
                self.update_snapshot(|s| {
                    if let Some(n) = s.net.as_mut() {
                        n.wifi_enabled = !on;
                    }
                });
                if !on {
                    self.spawn_lists(Tray::Network);
                }
            }
            Act::WifiConnect(ssid) => {
                sys::connect_wifi(&ssid);
                self.close();
            }
            Act::NetworkEditor => {
                self.close();
                sys::spawn(&["sh", "-c", "sleep 0.15; nm-connection-editor"]);
            }
            Act::BtToggle => {
                let on = self.snapshot().bluetooth.map(|b| b.powered).unwrap_or(false);
                sys::set_bluetooth(!on);
                self.update_snapshot(|s| {
                    if let Some(b) = s.bluetooth.as_mut() {
                        b.powered = !on;
                    }
                });
                if !on {
                    self.spawn_lists(Tray::Bluetooth);
                }
            }
            Act::BtDevice { mac, connect } => {
                sys::set_bt_device(&mac, connect);
                if let Ok(mut l) = self.shared.bt.lock() {
                    for d in l.iter_mut().filter(|d| d.mac == mac) {
                        d.connected = connect;
                    }
                }
            }
        }
        self.rebuild_popup();
    }

    /// Periodic refresh while a popup with live data is open.
    pub(super) fn refresh_open_popup(&mut self) {
        let Open::Popup(kind) = self.open else { return };
        if self.popup_drag.is_some() || self.last_full_refresh.elapsed() < POPUP_REFRESH {
            return;
        }
        self.last_full_refresh = Instant::now();
        match kind {
            Tray::Network | Tray::Bluetooth => self.spawn_lists(kind),
            Tray::Audio | Tray::Battery => self.spawn_snapshot(false),
            _ => {}
        }
    }

    // ------------------------------------------------------------- drawing

    pub(super) fn draw_battery(&mut self, pm: &mut Pixmap, s: Span, b: &sys::Battery) {
        let (x, y, w, h) = (s.x + 3.0, 7.5, 20.0, 11.0);
        stroke_rounded(pm, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 3.0, crate::theme::th().bar_text.color(0.7), 1.0);
        crate::gfx::fill_rect(pm, x + w + 0.5, y + 3.5, 1.5, 4.0, crate::theme::th().bar_text.color(0.7));
        let th = crate::theme::th();
        let fill = if b.charging {
            th.ok.t()
        } else if b.percent <= 15 {
            th.danger.t()
        } else {
            ink()
        };
        let fw = ((w - 4.0) * b.percent as f32 / 100.0).max(1.0);
        fill_rounded(pm, x + 2.0, y + 2.0, fw, h - 4.0, 1.5, color(fill.0, fill.1, fill.2, 0.95));
        let t = format!("{}%", b.percent);
        self.text(pm, &t, x + w + 6.0, 11.0, Weight::Regular, (ink().0, ink().1, ink().2, 1.0));
    }

    pub(super) fn draw_layout_item(&mut self, pm: &mut Pixmap, s: Span) {
        let Some(l) = self.layout_state() else { return };
        popup::draw_layout_icon(pm, l.canonical(), s.x + s.w / 2.0 - 11.0, 6.0, 22.0, 14.0, (ink().0, ink().1, ink().2, 0.95));
    }
}
