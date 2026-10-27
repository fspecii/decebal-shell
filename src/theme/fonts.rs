//! Fonts of a theme: four roles, per-role fallback chains, presets.
//!
//! * [`Role`]: `ui` (default text), `title` (window and popup titles), `mono` (terminal-like text, numbers that
//!   must line up) and `display` (large numbers: the clock, workspace digits).
//! * A chain is a list of family names. The first installed one draws the text; a character it lacks is drawn by
//!   the next installed family that has it (CJK, emoji, Nerd Font icons), then by a built-in tail of such fonts.
//! * [`Preset`]s are named sets of chains (`inter`, `segoe`, `rubik`...). A theme picks one (`[font] preset`), the
//!   user can pick another and override single roles; [`FontPlan::build`] merges the layers.
//! * [`FontPlan`] is the wanted fonts (the part of a resolved theme that decides how text looks);
//!   [`FontPlan::resolve`] checks it against the installed fonts ([`super::fontdb`]) once per theme change.

use super::fontdb::{norm, FontDb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// Default text.
    Ui,
    /// Window and popup titles.
    Title,
    /// Terminal-like text: numbers that must line up, clipboard entries.
    Mono,
    /// Large numbers: the clock, workspace digits.
    Display,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Ui, Role::Title, Role::Mono, Role::Display];

    pub fn idx(self) -> usize {
        self as usize
    }
    pub fn key(self) -> &'static str {
        match self {
            Role::Ui => "ui",
            Role::Title => "title",
            Role::Mono => "mono",
            Role::Display => "display",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Role::Ui => "Interface",
            Role::Title => "Titles",
            Role::Mono => "Monospace",
            Role::Display => "Clock and numbers",
        }
    }
    pub fn parse(s: &str) -> Option<Role> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ui" | "text" | "interface" | "sans" => Some(Role::Ui),
            "title" | "titles" => Some(Role::Title),
            "mono" | "monospace" => Some(Role::Mono),
            "display" | "clock" | "numbers" => Some(Role::Display),
            _ => None,
        }
    }
}

/// A named set of font chains.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    /// Chains by [`Role::idx`]: ui, title, mono, display.
    pub chains: [&'static [&'static str]; 4],
}

impl Preset {
    pub fn chain(&self, r: Role) -> &'static [&'static str] {
        self.chains[r.idx()]
    }

    /// The font that gives the preset its look (the first name of the interface chain).
    pub fn brand(&self) -> &'static str {
        self.chains[0][0]
    }

    /// How many leading names of the interface chain are the preset's own fonts (the rest are stand-ins).
    fn own_fonts(&self) -> usize {
        match self.id {
            "inter" | "cantarell" | "rubik" => 2,
            "segoe" => 3,
            "monospace-ui" => self.chains[0].len(),
            _ => 1,
        }
    }

    /// Is one of the preset's own fonts installed? (`system` always is; `monospace-ui` needs any monospace.)
    pub fn installed(&self, db: &FontDb) -> bool {
        self.chains[0].iter().take(self.own_fonts()).any(|f| db.has_family(f))
    }

    /// The first installed family of the interface chain, if any.
    pub fn first_installed(&self, db: &FontDb) -> Option<String> {
        self.chains[0].iter().find_map(|f| db.resolve(f))
    }
}

const SANS: &[&str] = &["sans-serif"];
const MONO: &[&str] = &["monospace"];
const TERM: &[&str] = &["JetBrains Mono", "Fira Code", "IBM Plex Mono", "Cascadia Mono", "Hack", "DejaVu Sans Mono", "Liberation Mono", "monospace"];
const SEGOE: &[&str] = &["Segoe UI Variable", "Segoe UI", "Selawik", "Noto Sans", "Open Sans"];
const SEGOE_TITLE: &[&str] = &["Segoe UI Variable Display", "Segoe UI Variable", "Segoe UI", "Selawik", "Noto Sans"];
const RUBIK: &[&str] = &["Rubik", "Google Sans Flex", "Inter"];

pub const PRESETS: &[Preset] = &[
    Preset { id: "system", title: "System", blurb: "The fontconfig default sans and monospace fonts", chains: [SANS, SANS, MONO, SANS] },
    Preset {
        id: "inter",
        title: "Inter",
        blurb: "macOS-like: Inter (SF Pro when installed) with a monospace for numbers",
        chains: [
            &["Inter", "Inter Variable", "SF Pro Text", "SF Pro"],
            &["Inter", "Inter Variable", "SF Pro Display"],
            &["SF Mono", "Menlo", "JetBrains Mono", "monospace"],
            &["Inter Display", "Inter", "Inter Variable", "SF Pro Display"],
        ],
    },
    Preset {
        id: "segoe",
        title: "Segoe",
        blurb: "Windows 11-like: Segoe UI Variable, then Selawik or Noto Sans",
        chains: [SEGOE, SEGOE_TITLE, &["Cascadia Mono", "Cascadia Code", "Consolas", "JetBrains Mono", "monospace"], SEGOE_TITLE],
    },
    Preset {
        id: "cantarell",
        title: "Cantarell",
        blurb: "GNOME: Cantarell, or Adwaita Sans",
        chains: [&["Cantarell", "Adwaita Sans", "Inter"], &["Cantarell", "Adwaita Sans", "Inter"], &["Adwaita Mono", "Source Code Pro", "monospace"], &["Cantarell", "Adwaita Sans", "Inter"]],
    },
    Preset {
        id: "rubik",
        title: "Rubik",
        blurb: "Caelestia: Rubik with Material Symbols icons and a Nerd Font monospace",
        chains: [&["Rubik", "Google Sans Flex", "Inter", "Material Symbols Rounded", "Material Symbols Outlined"], RUBIK, &["CaskaydiaCove NF", "JetBrains Mono", "monospace"], RUBIK],
    },
    Preset { id: "jetbrains", title: "JetBrains Mono", blurb: "JetBrains Mono everywhere", chains: [&["JetBrains Mono"], &["JetBrains Mono"], &["JetBrains Mono", "monospace"], &["JetBrains Mono"]] },
    Preset {
        id: "ibm-plex",
        title: "IBM Plex",
        blurb: "IBM Plex Sans with IBM Plex Mono",
        chains: [&["IBM Plex Sans", "IBM Plex Sans JP"], &["IBM Plex Sans"], &["IBM Plex Mono"], &["IBM Plex Sans Condensed", "IBM Plex Sans"]],
    },
    Preset { id: "fira", title: "Fira", blurb: "Fira Sans with Fira Code", chains: [&["Fira Sans"], &["Fira Sans"], &["Fira Code", "Fira Mono"], &["Fira Sans"]] },
    Preset { id: "noto", title: "Noto", blurb: "Noto Sans with Noto Sans Mono", chains: [&["Noto Sans"], &["Noto Sans"], &["Noto Sans Mono"], &["Noto Sans Display", "Noto Sans"]] },
    Preset { id: "monospace-ui", title: "Monospace UI", blurb: "Terminal look: a monospace font for everything", chains: [TERM, TERM, TERM, TERM] },
];

/// The ids of [`PRESETS`], for the theme-file validator (`Kind::Choice` needs a constant slice; a test keeps both in sync).
pub const PRESET_IDS: &[&str] = &["system", "inter", "segoe", "cantarell", "rubik", "jetbrains", "ibm-plex", "fira", "noto", "monospace-ui"];

/// What a theme without any font setting uses: the look macshell always had (Inter, else the system sans).
pub const DEFAULT_PRESET: &str = "inter";

/// Looks a preset up by id or title, ignoring case and punctuation ("IBM Plex" == "ibm-plex").
pub fn preset(name: &str) -> Option<&'static Preset> {
    let n = norm(name);
    PRESETS.iter().find(|p| p.id == name).or_else(|| PRESETS.iter().find(|p| norm(p.id) == n || norm(p.title) == n))
}

/// Tried after a role's own chain: the old default (Inter, then the system sans) so a theme never ends up unreadable.
const BASE_TAIL: [&[&str]; 4] = [&["Inter", "sans-serif"], &["Inter", "sans-serif"], &["monospace", "sans-serif"], &["Inter", "sans-serif"]];
/// Tried for characters no family of the chain has: emoji, CJK, icon fonts, symbols.
pub const SYMBOL_TAIL: &[&str] = &["emoji", "Noto Sans CJK", "nerd-font", "Noto Sans Symbols 2", "Noto Sans Symbols"];

// ---------------------------------------------------------------- the user's / theme's choices

/// Parses `Inter, Noto Sans CJK ,Noto Color Emoji` into a list of names (empty items dropped).
pub fn parse_chain(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().trim_matches('"').trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn chain_json(v: &serde_json::Value) -> Option<Vec<String>> {
    let list = match v {
        serde_json::Value::String(s) => parse_chain(s),
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).flat_map(parse_chain).collect(),
        _ => return None,
    };
    (!list.is_empty()).then_some(list)
}

/// The user's font choices (the `font` object of the `theme` key of macshell.json): a preset and per-role chains.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontChoice {
    pub preset: Option<String>,
    /// Chains the user set by role (ui, title, mono, display); they go in front of the preset's chain.
    pub roles: [Option<Vec<String>>; 4],
}

impl FontChoice {
    pub fn is_empty(&self) -> bool {
        self.preset.is_none() && self.roles.iter().all(Option::is_none)
    }

    pub fn role(&self, r: Role) -> Option<&Vec<String>> {
        self.roles[r.idx()].as_ref()
    }

    /// Reads the keys of the `font` object (`preset`, `ui`, `title`, `mono`, `display`); bad values are ignored.
    pub fn from_json(o: &serde_json::Map<String, serde_json::Value>) -> FontChoice {
        let mut c = FontChoice { preset: o.get("preset").and_then(|x| x.as_str()).and_then(preset).map(|p| p.id.to_string()), ..FontChoice::default() };
        for r in Role::ALL {
            c.roles[r.idx()] = o.get(r.key()).and_then(chain_json);
        }
        c
    }

    pub fn write_json(&self, o: &mut serde_json::Map<String, serde_json::Value>) {
        if let Some(p) = &self.preset {
            o.insert("preset".into(), p.clone().into());
        }
        for r in Role::ALL {
            if let Some(c) = self.role(r) {
                // one name stays a plain string, a chain is a list
                o.insert(r.key().into(), if c.len() == 1 { c[0].clone().into() } else { serde_json::Value::from(c.clone()) });
            }
        }
    }
}

/// The font keys of a theme file after merging (`[font]` table or the `font_*` metric keys).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ThemeFonts {
    pub preset: Option<String>,
    /// Legacy `family`: the font for text, titles and display numbers.
    pub family: Option<String>,
    pub roles: [Option<Vec<String>>; 4],
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|x| seen.insert(norm(x))).collect()
}

/// The wanted fonts of a resolved theme: the preset in use and one chain of family names per role, before looking at what is installed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FontPlan {
    pub preset: String,
    pub chains: [Vec<String>; 4],
}

impl FontPlan {
    pub fn chain(&self, r: Role) -> &[String] {
        &self.chains[r.idx()]
    }

    /// Layers, lowest first: preset (the user's, else the theme's, else [`DEFAULT_PRESET`]) < theme file roles <
    /// the legacy `family` < the user's `font` string < the user's per-role chains. A user preset replaces the theme's
    /// own font keys. Explicit names stay in front of the preset's chain as fallbacks-after.
    pub fn build(theme: &ThemeFonts, user: &FontChoice, user_family: Option<&str>) -> FontPlan {
        let theme_preset = theme.preset.as_deref().and_then(preset);
        let p = user.preset.as_deref().and_then(preset).or(theme_preset).unwrap_or_else(|| preset(DEFAULT_PRESET).expect("default preset"));
        let theme_layer = user.preset.is_none();
        let mut chains: [Vec<String>; 4] = Default::default();
        for r in Role::ALL {
            let mut c: Vec<String> = user.role(r).cloned().unwrap_or_default();
            // the single-font settings (old `font`) mean "the font of the text": ui, titles and display numbers
            let legacy = |f: &Option<String>| f.iter().filter(|_| r != Role::Mono).cloned().collect::<Vec<_>>();
            c.extend(user_family.map(str::to_string).filter(|_| r != Role::Mono));
            if theme_layer {
                c.extend(theme.roles[r.idx()].clone().unwrap_or_default());
                c.extend(legacy(&theme.family));
            }
            c.extend(p.chain(r).iter().map(|s| s.to_string()));
            chains[r.idx()] = dedup(c);
        }
        FontPlan { preset: p.id.to_string(), chains }
    }

    /// The font that gives the interface its look, as a theme author wrote it (empty = automatic).
    pub fn primary_ui(&self) -> &str {
        self.chains[0].first().map(String::as_str).unwrap_or("")
    }

    /// Checks the plan against the installed fonts.
    pub fn resolve(&self, db: &FontDb) -> Resolved {
        let mut roles: [Vec<String>; 4] = Default::default();
        let mut missing: [Vec<String>; 4] = Default::default();
        for r in Role::ALL {
            let mut out: Vec<String> = Vec::new();
            let push = |out: &mut Vec<String>, f: String| {
                if !out.iter().any(|x| norm(x) == norm(&f)) {
                    out.push(f);
                }
            };
            for w in &self.chains[r.idx()] {
                match db.resolve(w) {
                    Some(f) => push(&mut out, f),
                    None => missing[r.idx()].push(w.clone()),
                }
            }
            for w in BASE_TAIL[r.idx()].iter().chain(SYMBOL_TAIL) {
                if let Some(f) = db.resolve(w) {
                    push(&mut out, f);
                }
            }
            roles[r.idx()] = out;
        }
        Resolved { preset: self.preset.clone(), roles, missing }
    }
}

/// A [`FontPlan`] checked against the installed fonts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resolved {
    pub preset: String,
    /// Installed families by role, in drawing order (the chain, then the built-in tails).
    pub roles: [Vec<String>; 4],
    /// Names the plan asked for that are not installed.
    pub missing: [Vec<String>; 4],
}

impl Resolved {
    pub fn chain(&self, r: Role) -> &[String] {
        &self.roles[r.idx()]
    }

    /// The family that draws a role's ordinary text.
    pub fn primary(&self, r: Role) -> Option<&str> {
        self.roles[r.idx()].first().map(String::as_str)
    }

    /// The families to hand to rdwm for its titlebars: the title role's own chain, generic aliases left out.
    pub fn title_for_rdwm(&self, plan: &FontPlan, db: &FontDb) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in plan.chain(Role::Title) {
            if matches!(norm(w).as_str(), "sansserif" | "monospace" | "sans" | "mono" | "system") {
                continue;
            }
            if let Some(f) = db.resolve(w) {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------- the `macthemectl font` command

/// What the user typed after `macthemectl font`.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    List,
    Families,
    Status,
    Preset(String),
    Set(Role, Option<Vec<String>>),
    Scale(Option<f32>),
    Reset,
}

pub const HELP: &str = "macthemectl font PRESET             inter segoe cantarell rubik jetbrains ibm-plex fira noto monospace-ui system
macthemectl font list               presets with the fonts each one found on this machine
macthemectl font families           installed font families (what the pickers offer)
macthemectl font status             the fonts in use, role by role
macthemectl font set ROLE FAMILY[,FALLBACK...]   ROLE: ui title mono display (none = back to the preset)
macthemectl font scale 0.8-1.4|none
macthemectl font reset              back to the theme's own fonts";

/// Parses the words after `font`. Pure, so it can be tested without touching any file.
pub fn parse_cmd(args: &[String]) -> Result<Cmd, String> {
    let Some(first) = args.first() else { return Ok(Cmd::Status) };
    match first.as_str() {
        "list" | "presets" => Ok(Cmd::List),
        "families" | "installed" => Ok(Cmd::Families),
        "status" | "get" => Ok(Cmd::Status),
        "reset" => Ok(Cmd::Reset),
        "scale" => match args.get(1).map(String::as_str) {
            None => Err("font scale needs a number (0.8-1.4) or none".into()),
            Some("none") | Some("") => Ok(Cmd::Scale(None)),
            Some(v) => v.parse::<f32>().ok().filter(|x| (0.8..=1.4).contains(x)).map(|x| Cmd::Scale(Some(x))).ok_or_else(|| "font scale must be a number between 0.8 and 1.4".to_string()),
        },
        "set" => {
            let role = args.get(1).ok_or("font set needs a role: ui, title, mono or display")?;
            let role = Role::parse(role).ok_or_else(|| format!("unknown font role `{role}` (ui, title, mono, display)"))?;
            let rest = &args[2..];
            if rest.is_empty() {
                return Err(format!("font set {} needs a family (or none)", role.key()));
            }
            if rest.len() == 1 && matches!(rest[0].as_str(), "none" | "default" | "") {
                return Ok(Cmd::Set(role, None));
            }
            let chain = dedup(rest.iter().flat_map(|a| parse_chain(a)).collect());
            if chain.is_empty() {
                return Err("font set needs at least one family name".into());
            }
            Ok(Cmd::Set(role, Some(chain)))
        }
        name => preset(name).map(|p| Cmd::Preset(p.id.to_string())).ok_or_else(|| format!("unknown font preset or command `{name}` (presets: {})", PRESET_IDS.join(", "))),
    }
}

/// Applies a changing command to the stored selection; the read-only ones do nothing.
/// Choosing a preset drops the per-role overrides and the old single `font` so the preset applies as listed.
pub fn apply_cmd(sel: &mut super::model::Selection, cmd: &Cmd) {
    match cmd {
        Cmd::Preset(id) => {
            sel.fonts = FontChoice { preset: Some(id.clone()), ..FontChoice::default() };
            sel.font = None;
        }
        Cmd::Set(r, chain) => sel.fonts.roles[r.idx()] = chain.clone(),
        Cmd::Scale(s) => sel.font_scale = *s,
        Cmd::Reset => {
            sel.fonts = FontChoice::default();
            sel.font = None;
        }
        Cmd::List | Cmd::Families | Cmd::Status => {}
    }
}

/// `font list`: one line per preset with what it resolves to here.
pub fn render_list(db: &FontDb, current: &str) -> String {
    let mut s = String::new();
    for p in PRESETS {
        let plan = FontPlan::build(&ThemeFonts::default(), &FontChoice { preset: Some(p.id.into()), ..FontChoice::default() }, None);
        let res = plan.resolve(db);
        let note = if p.installed(db) { String::new() } else { format!("  (not installed: {}; using {})", p.brand(), res.primary(Role::Ui).unwrap_or("nothing")) };
        s.push_str(&format!("{} {:<13} {}{note}\n", if p.id == current { "*" } else { " " }, p.id, p.blurb));
    }
    s
}

/// `font status`: preset and the families drawing each role.
pub fn render_status(theme: &super::model::Theme, db: &FontDb) -> String {
    let res = theme.fonts.resolve(db);
    let mut s = format!("preset {}  scale {}\n", theme.fonts.preset, theme.font_scale);
    for r in Role::ALL {
        let wanted = theme.fonts.chain(r).join(", ");
        let have: Vec<&str> = res.chain(r).iter().map(String::as_str).take(5).collect();
        s.push_str(&format!(
            "{:<8} {}\n         using {}{}\n",
            r.key(),
            wanted,
            have.join(" > "),
            if res.missing[r.idx()].is_empty() { String::new() } else { format!("   (not installed: {})", res.missing[r.idx()].join(", ")) }
        ));
    }
    s
}

/// The line the settings preview and the e2e screenshots draw: Latin, digits, CJK, emoji and icon glyphs.
pub const SAMPLE: &str = "The quick brown fox 0123 日本語 😀  \u{f015} \u{f013} \u{e795} Nerd icons";

#[cfg(test)]
mod tests {
    use super::super::fontdb::fake;
    use super::super::model::Selection;
    use super::*;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn preset_ids_match_the_table() {
        assert_eq!(PRESETS.iter().map(|p| p.id).collect::<Vec<_>>(), PRESET_IDS);
        assert!(preset(DEFAULT_PRESET).is_some());
        for p in PRESETS {
            assert!(p.chains.iter().all(|c| !c.is_empty()), "{}", p.id);
        }
        assert_eq!(preset("IBM Plex").unwrap().id, "ibm-plex");
        assert_eq!(preset("Monospace UI").unwrap().id, "monospace-ui");
        assert!(preset("comic").is_none());
    }

    #[test]
    fn a_preset_picks_the_first_installed_alternative() {
        let db = fake::db();
        let res = FontPlan::build(&ThemeFonts::default(), &FontChoice { preset: Some("segoe".into()), ..FontChoice::default() }, None).resolve(&db);
        // neither Segoe UI Variable, Segoe UI nor Selawik is installed: Noto Sans is the first alternative that is
        assert_eq!(res.primary(Role::Ui), Some("Noto Sans"));
        assert_eq!(res.primary(Role::Title), Some("Noto Sans"));
        assert!(res.missing[0].contains(&"Segoe UI Variable".to_string()) && !res.missing[0].contains(&"Noto Sans".to_string()));
        assert_eq!(res.primary(Role::Mono), Some("JetBrainsMono Nerd Font"), "JetBrains Mono is the 4th mono alternative, found through its Nerd Font name");
        assert_eq!(preset("segoe").unwrap().first_installed(&db).as_deref(), Some("Noto Sans"));
        assert!(preset("inter").unwrap().installed(&db) && preset("system").unwrap().installed(&db));
        assert!(!preset("rubik").unwrap().installed(&FontDb::from_listing("", "", "", "")));
    }

    #[test]
    fn nothing_installed_still_degrades_to_the_generic_fonts() {
        let db = FontDb::from_listing("/f/L.ttf\tLiberation Sans\t80\t0\t100\t0\tFalse\t\n", "Liberation Sans", "", "");
        let res = FontPlan::build(&ThemeFonts::default(), &FontChoice { preset: Some("rubik".into()), ..FontChoice::default() }, None).resolve(&db);
        assert_eq!(res.chain(Role::Ui), ["Liberation Sans"]);
        assert_eq!(res.chain(Role::Title), ["Liberation Sans"]);
    }

    #[test]
    fn the_tail_adds_cjk_emoji_and_icon_fonts_after_the_chain() {
        let db = fake::db();
        let res = FontPlan::build(&ThemeFonts::default(), &FontChoice::default(), None).resolve(&db);
        assert_eq!(res.chain(Role::Ui), ["Inter", "Liberation Sans", "Noto Color Emoji", "Noto Sans CJK JP", "JetBrainsMono Nerd Font"]);
        assert_eq!(res.preset, "inter");
    }

    #[test]
    fn layers_merge_in_the_documented_order() {
        let theme = ThemeFonts { preset: Some("segoe".into()), family: Some("Theme Family".into()), roles: [None, None, Some(strs(&["Theme Mono"])), None] };
        // theme alone: its mono beats the preset's, the family leads ui / title / display
        let p = FontPlan::build(&theme, &FontChoice::default(), None);
        assert_eq!(p.preset, "segoe");
        assert_eq!(&p.chains[0][..2], ["Theme Family", "Segoe UI Variable"]);
        assert_eq!(&p.chains[2][..2], ["Theme Mono", "Cascadia Mono"]);
        assert_eq!(p.primary_ui(), "Theme Family");
        // the user's preset replaces the theme's own font keys
        let p = FontPlan::build(&theme, &FontChoice { preset: Some("fira".into()), ..FontChoice::default() }, None);
        assert_eq!((p.preset.as_str(), p.chains[0][0].as_str(), p.chains[2][0].as_str()), ("fira", "Fira Sans", "Fira Code"));
        // a user role chain goes first, keeps the preset behind it as fallbacks
        let mut choice = FontChoice { preset: Some("fira".into()), ..FontChoice::default() };
        choice.roles[Role::Ui.idx()] = Some(strs(&["Inter", "Noto Sans CJK"]));
        let p = FontPlan::build(&theme, &choice, None);
        assert_eq!(p.chains[0], ["Inter", "Noto Sans CJK", "Fira Sans"]);
        assert_eq!(p.chains[1][0], "Fira Sans", "other roles are untouched");
        // the old single `font` string leads ui, title and display but not mono
        let p = FontPlan::build(&ThemeFonts::default(), &FontChoice::default(), Some("Foo"));
        assert_eq!((p.chains[0][0].as_str(), p.chains[1][0].as_str(), p.chains[3][0].as_str()), ("Foo", "Foo", "Foo"));
        assert_ne!(p.chains[2][0], "Foo");
        assert_eq!(p.preset, "inter");
    }

    #[test]
    fn unknown_presets_fall_back_to_the_default() {
        let p = FontPlan::build(&ThemeFonts { preset: Some("comic".into()), ..ThemeFonts::default() }, &FontChoice::default(), None);
        assert_eq!(p.preset, DEFAULT_PRESET);
    }

    #[test]
    fn choice_json_round_trips_and_ignores_junk() {
        let mut c = FontChoice { preset: Some("rubik".into()), ..FontChoice::default() };
        c.roles[0] = Some(strs(&["Inter", "Noto Sans CJK"]));
        c.roles[2] = Some(strs(&["Fira Code"]));
        let mut o = serde_json::Map::new();
        c.write_json(&mut o);
        assert_eq!(o["ui"], serde_json::json!(["Inter", "Noto Sans CJK"]));
        assert_eq!(o["mono"], serde_json::json!("Fira Code"));
        assert_eq!(FontChoice::from_json(&o), c);
        let junk = serde_json::json!({"preset": "comic", "ui": 5, "title": "", "mono": "A, B", "display": []});
        let c = FontChoice::from_json(junk.as_object().unwrap());
        assert_eq!(c.preset, None);
        assert_eq!(c.roles, [None, None, Some(strs(&["A", "B"])), None]);
    }

    #[test]
    fn font_command_parsing() {
        let a = |s: &str| -> Vec<String> { s.split_whitespace().map(String::from).collect() };
        assert_eq!(parse_cmd(&[]), Ok(Cmd::Status));
        assert_eq!(parse_cmd(&a("list")), Ok(Cmd::List));
        assert_eq!(parse_cmd(&a("families")), Ok(Cmd::Families));
        assert_eq!(parse_cmd(&a("inter")), Ok(Cmd::Preset("inter".into())));
        assert_eq!(parse_cmd(&a("IBM-Plex")), Ok(Cmd::Preset("ibm-plex".into())));
        assert_eq!(parse_cmd(&a("reset")), Ok(Cmd::Reset));
        assert_eq!(parse_cmd(&a("scale 1.1")), Ok(Cmd::Scale(Some(1.1))));
        assert_eq!(parse_cmd(&a("scale none")), Ok(Cmd::Scale(None)));
        assert!(parse_cmd(&a("scale 3")).is_err() && parse_cmd(&a("scale")).is_err());
        assert_eq!(parse_cmd(&a("set ui Inter,Noto")), Ok(Cmd::Set(Role::Ui, Some(strs(&["Inter", "Noto"])))));
        let args = vec!["set".to_string(), "mono".to_string(), "Fira Code, Hack".to_string(), "Noto Color Emoji".to_string()];
        assert_eq!(parse_cmd(&args), Ok(Cmd::Set(Role::Mono, Some(strs(&["Fira Code", "Hack", "Noto Color Emoji"])))));
        assert_eq!(parse_cmd(&a("set title none")), Ok(Cmd::Set(Role::Title, None)));
        assert!(parse_cmd(&a("set sparkle Inter")).unwrap_err().contains("unknown font role"));
        assert!(parse_cmd(&a("set ui")).is_err());
        assert!(parse_cmd(&a("comic")).unwrap_err().contains("presets:"));
    }

    #[test]
    fn commands_edit_the_selection() {
        let mut sel = Selection { font: Some("Old".into()), ..Selection::default() };
        sel.fonts.roles[1] = Some(strs(&["X"]));
        apply_cmd(&mut sel, &Cmd::Preset("segoe".into()));
        assert_eq!((sel.fonts.preset.as_deref(), sel.font.clone(), sel.fonts.roles[1].clone()), (Some("segoe"), None, None), "a preset replaces older choices");
        apply_cmd(&mut sel, &Cmd::Set(Role::Mono, Some(strs(&["Fira Code"]))));
        assert_eq!(sel.fonts.roles[2], Some(strs(&["Fira Code"])));
        assert_eq!(sel.fonts.preset.as_deref(), Some("segoe"));
        apply_cmd(&mut sel, &Cmd::Set(Role::Mono, None));
        assert_eq!(sel.fonts.roles[2], None);
        apply_cmd(&mut sel, &Cmd::Scale(Some(1.2)));
        assert_eq!(sel.font_scale, Some(1.2));
        apply_cmd(&mut sel, &Cmd::Reset);
        assert!(sel.fonts.is_empty() && sel.font.is_none());
        assert_eq!(sel.font_scale, Some(1.2), "reset keeps the text size");
    }

    #[test]
    fn rdwm_gets_the_installed_title_fonts_without_generic_names() {
        let db = fake::db();
        let plan = FontPlan::build(&ThemeFonts::default(), &FontChoice { preset: Some("segoe".into()), ..FontChoice::default() }, None);
        let res = plan.resolve(&db);
        assert_eq!(res.title_for_rdwm(&plan, &db), ["Noto Sans"]);
        let plan = FontPlan::build(&ThemeFonts::default(), &FontChoice { preset: Some("system".into()), ..FontChoice::default() }, None);
        assert!(plan.resolve(&db).title_for_rdwm(&plan, &db).is_empty(), "only a generic name: rdwm picks its own");
    }

    #[test]
    fn list_and_status_mention_what_is_missing() {
        let db = fake::db();
        let l = render_list(&db, "inter");
        assert!(l.contains("* inter") && l.contains("(not installed: Segoe UI Variable; using Noto Sans)"), "{l}");
        let d = crate::theme::builtin::def("windows11").unwrap();
        let t = d.resolve("windows11", true, &Selection::default());
        let s = render_status(&t, &db);
        assert!(s.contains("preset segoe") && s.contains("using Noto Sans") && s.contains("not installed: Segoe UI Variable"), "{s}");
    }
}
