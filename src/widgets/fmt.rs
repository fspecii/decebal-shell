//! Format strings: `"{cpu.usage:pct} of {cpu.cores} cores"`, `"{clock:%H:%M}"`, `"{battery.level:pct|no battery}"`.
//!
//! `{path}` inserts a value, `{path:spec}` formats it, `{path:spec|fallback}` (or `{path|fallback}`) shows `fallback` when the value is
//! missing. `{{` and `}}` are literal braces. There is nothing else: no arithmetic, no calls, no nesting.
//!
//! Specs: `pct` (0.42 -> 42%), `pct1` (42.3%), `int`, `.N` (N decimals), `bytes`, `rate` (bytes per second), `dur` (seconds -> 1d 2h / 3h 05m / 4m 07s),
//! `mmss` (seconds -> 3:07), `upper`, `lower`, and for `clock` a strftime subset `%H %I %M %S %p %a %A %b %B %d %e %m %y %Y %j %%`.

use super::data::{Data, Time, Val};

#[derive(Debug, Clone, PartialEq)]
pub enum Spec {
    None,
    Time(String),
    Pct(usize),
    Dec(usize),
    Int,
    Bytes,
    Rate,
    Dur,
    Mmss,
    Upper,
    Lower,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Lit(String),
    Ref { path: String, spec: Spec, fallback: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fmt(pub Vec<Part>);

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn parse_spec(path: &str, s: &str) -> Result<Spec, String> {
    if s.starts_with('%') {
        if path != "clock" {
            return Err(format!("time formats (`{s}`) only work on `clock`"));
        }
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '%' && !matches!(chars.next(), Some('H' | 'I' | 'M' | 'S' | 'p' | 'a' | 'A' | 'b' | 'B' | 'd' | 'e' | 'm' | 'y' | 'Y' | 'j' | '%')) {
                return Err(format!("unsupported time code in `{s}` (use %H %I %M %S %p %a %A %b %B %d %e %m %y %Y %j %%)"));
            }
        }
        return Ok(Spec::Time(s.into()));
    }
    Ok(match s {
        "" => Spec::None,
        "pct" => Spec::Pct(0),
        "pct1" => Spec::Pct(1),
        "int" => Spec::Int,
        "bytes" => Spec::Bytes,
        "rate" => Spec::Rate,
        "dur" => Spec::Dur,
        "mmss" => Spec::Mmss,
        "upper" => Spec::Upper,
        "lower" => Spec::Lower,
        d if d.starts_with('.') => Spec::Dec(d[1..].parse::<usize>().ok().filter(|n| *n <= 4).ok_or_else(|| format!("`{d}`: use .0 to .4"))?),
        other => return Err(format!("unknown format `{other}` (pct pct1 int .N bytes rate dur mmss upper lower, or %-codes for clock)")),
    })
}

impl Fmt {
    pub fn lit(s: &str) -> Fmt {
        Fmt(vec![Part::Lit(s.into())])
    }

    pub fn parse(s: &str) -> Result<Fmt, String> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '{' if it.peek() == Some(&'{') => {
                    it.next();
                    lit.push('{');
                }
                '}' if it.peek() == Some(&'}') => {
                    it.next();
                    lit.push('}');
                }
                '}' => return Err("unmatched `}` (write `}}` for a literal brace)".into()),
                '{' => {
                    let mut body = String::new();
                    loop {
                        match it.next() {
                            Some('}') => break,
                            Some(ch) => body.push(ch),
                            None => return Err("missing `}`".into()),
                        }
                    }
                    if !lit.is_empty() {
                        parts.push(Part::Lit(std::mem::take(&mut lit)));
                    }
                    let (head, fallback) = match body.split_once('|') {
                        Some((h, f)) => (h, Some(f.to_string())),
                        None => (body.as_str(), None),
                    };
                    let (path, spec) = match head.split_once(':') {
                        Some((p, sp)) => (p.trim(), sp),
                        None => (head.trim(), ""),
                    };
                    if path.is_empty() {
                        return Err("empty `{}`".into());
                    }
                    if !super::data::known(path) && !path.starts_with("cmd.") {
                        return Err(format!("unknown data `{path}`{}", super::data::suggest(path)));
                    }
                    parts.push(Part::Ref { path: path.into(), spec: parse_spec(path, spec)?, fallback });
                }
                c => lit.push(c),
            }
        }
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Ok(Fmt(parts))
    }

    /// Every data path the string reads.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|p| if let Part::Ref { path, .. } = p { Some(path.as_str()) } else { None })
    }

    pub fn eval(&self, d: &Data) -> String {
        let mut out = String::new();
        for p in &self.0 {
            match p {
                Part::Lit(s) => out.push_str(s),
                Part::Ref { path, spec, fallback } => {
                    let text = if path == "clock" { Some(format_value(&Val::Num(0.0), spec, d)) } else { d.get(path).filter(|_| true).map(|v| format_value(v, spec, d)) };
                    match text {
                        Some(t) => out.push_str(&t),
                        None => out.push_str(fallback.as_deref().unwrap_or("")),
                    }
                }
            }
        }
        out
    }
}

pub fn strftime(spec: &str, t: &Time) -> String {
    let mut out = String::new();
    let mut it = spec.chars();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let h12 = if t.hour % 12 == 0 { 12 } else { t.hour % 12 };
        match it.next() {
            Some('H') => out.push_str(&format!("{:02}", t.hour)),
            Some('I') => out.push_str(&format!("{h12:02}")),
            Some('M') => out.push_str(&format!("{:02}", t.min)),
            Some('S') => out.push_str(&format!("{:02}", t.sec)),
            Some('p') => out.push_str(if t.hour >= 12 { "PM" } else { "AM" }),
            Some('a') => out.push_str(&DAYS[(t.wday % 7) as usize][..3]),
            Some('A') => out.push_str(DAYS[(t.wday % 7) as usize]),
            Some('b') => out.push_str(&MONTHS[(t.mon % 12) as usize][..3]),
            Some('B') => out.push_str(MONTHS[(t.mon % 12) as usize]),
            Some('d') => out.push_str(&format!("{:02}", t.mday)),
            Some('e') => out.push_str(&t.mday.to_string()),
            Some('m') => out.push_str(&format!("{:02}", t.mon + 1)),
            Some('y') => out.push_str(&format!("{:02}", t.year.rem_euclid(100))),
            Some('Y') => out.push_str(&t.year.to_string()),
            Some('j') => out.push_str(&format!("{:03}", t.yday + 1)),
            Some('%') => out.push('%'),
            _ => {}
        }
    }
    out
}

fn dur(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (d, h, m, sec) = (s / 86400, s % 86400 / 3600, s % 3600 / 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m {sec:02}s")
    }
}

pub fn format_value(v: &Val, spec: &Spec, d: &Data) -> String {
    if let Spec::Time(s) = spec {
        return strftime(s, &d.time);
    }
    let n = v.num();
    match (spec, v) {
        (Spec::Pct(p), _) => n.map(|n| format!("{:.*}%", *p, n * 100.0)).unwrap_or_default(),
        (Spec::Dec(p), _) => n.map(|n| format!("{n:.*}", *p)).unwrap_or_default(),
        (Spec::Int, _) => n.map(|n| format!("{}", n.round() as i64)).unwrap_or_default(),
        (Spec::Bytes, _) => n.map(|n| crate::dash::sysinfo::human_bytes(n.max(0.0) as u64)).unwrap_or_default(),
        (Spec::Rate, _) => n.map(crate::dash::sysinfo::human_rate).unwrap_or_default(),
        (Spec::Dur, _) => n.map(dur).unwrap_or_default(),
        (Spec::Mmss, _) => n.map(|n| format!("{}:{:02}", n.max(0.0) as u64 / 60, n.max(0.0) as u64 % 60)).unwrap_or_default(),
        (Spec::Upper, v) => plain(v).to_uppercase(),
        (Spec::Lower, v) => plain(v).to_lowercase(),
        (_, v) => plain(v),
    }
}

fn plain(v: &Val) -> String {
    match v {
        Val::Text(s) => s.clone(),
        Val::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        Val::Num(n) => format!("{n:.1}"),
        Val::Bool(b) => b.to_string(),
        Val::Series(s) => s.last().map(|x| format!("{x:.1}")).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::data::sample;

    fn ev(s: &str) -> String {
        Fmt::parse(s).unwrap().eval(&sample())
    }

    #[test]
    fn values_and_formats() {
        assert_eq!(ev("{clock:%H:%M}"), "14:07");
        assert_eq!(ev("{clock:%a %e %b %Y}"), "Fri 23 Oct 2026");
        assert_eq!(ev("{clock:%I:%M %p}"), "02:07 PM");
        assert_eq!(ev("CPU {cpu.usage:pct}"), "CPU 37%");
        assert_eq!(ev("{cpu.usage:pct1}"), "37.0%");
        assert_eq!(ev("{cpu.temp:int}°"), "54°");
        assert_eq!(ev("{cpu.load:.1}"), "1.4");
        assert_eq!(ev("{mem.used:bytes} / {mem.total:bytes}"), "18.4 GiB / 29.8 GiB");
        assert_eq!(ev("{media.position:mmss}/{media.length:mmss}"), "1:25/3:44");
        assert_eq!(ev("{uptime:dur}"), "1d 2h");
        assert_eq!(ev("{media.title:upper}"), "MIDNIGHT CITY");
        assert_eq!(ev("{window.title}"), "Decebal - theme engine");
        assert_eq!(ev("{{literal}} {user.name}"), "{literal} ada");
    }

    #[test]
    fn fallbacks_for_missing_values() {
        let d = Data::empty();
        assert_eq!(Fmt::parse("{battery.level:pct|no battery}").unwrap().eval(&d), "no battery");
        assert_eq!(Fmt::parse("a{media.title}b").unwrap().eval(&d), "ab");
        assert_eq!(Fmt::parse("{opt.who|world}").unwrap().eval(&d), "world");
    }

    #[test]
    fn mistakes_are_named() {
        for (s, needle) in [
            ("{cpu.usag}", "did you mean `cpu.usage`"),
            ("{cpu.usage:%H}", "only work on `clock`"),
            ("{clock:%Q}", "unsupported time code"),
            ("{cpu.usage:fancy}", "unknown format"),
            ("{cpu.usage", "missing `}`"),
            ("oops}", "unmatched"),
            ("{}", "empty"),
            ("{cpu.load:.9}", ".0 to .4"),
        ] {
            let e = Fmt::parse(s).unwrap_err();
            assert!(e.contains(needle), "{s}: {e}");
        }
    }

    #[test]
    fn paths_are_listed() {
        let f = Fmt::parse("{cpu.usage:pct} {mem.usage} x {opt.a}").unwrap();
        assert_eq!(f.paths().collect::<Vec<_>>(), ["cpu.usage", "mem.usage", "opt.a"]);
    }
}
