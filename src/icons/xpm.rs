//! A small XPM3 reader (what `/usr/share/pixmaps` still holds for old applications): one colour per pixel,
//! `c` (colour) visuals only, `None` is transparent. Returns a premultiplied pixmap.

use tiny_skia::Pixmap;

fn named(name: &str) -> Option<(u8, u8, u8)> {
    let n = name.trim().to_ascii_lowercase().replace(' ', "");
    Some(match n.as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "orange" => (255, 165, 0),
        "brown" => (165, 42, 42),
        "purple" => (160, 32, 240),
        "pink" => (255, 192, 203),
        "gray" | "grey" => (190, 190, 190),
        "darkgray" | "darkgrey" => (169, 169, 169),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "darkgreen" => (0, 100, 0),
        "darkblue" => (0, 0, 139),
        "darkred" => (139, 0, 0),
        _ => {
            // grayNN / greyNN
            let pct = n.strip_prefix("gray").or_else(|| n.strip_prefix("grey"))?.parse::<u32>().ok().filter(|p| *p <= 100)?;
            let v = (pct * 255 / 100) as u8;
            (v, v, v)
        }
    })
}

/// `None` = transparent. `Err` = not a colour we understand.
fn colour(spec: &str) -> Result<Option<(u8, u8, u8)>, ()> {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if let Some(h) = spec.strip_prefix('#') {
        if !h.chars().all(|c| c.is_ascii_hexdigit()) || h.is_empty() || h.len() % 3 != 0 {
            return Err(());
        }
        let n = h.len() / 3;
        let ch = |i: usize| -> Option<u8> {
            let v = u32::from_str_radix(&h[i * n..(i + 1) * n], 16).ok()?;
            // scale 1..4 hex digits to 8 bit
            Some(match n {
                1 => (v * 17) as u8,
                2 => v as u8,
                3 => (v >> 4) as u8,
                _ => (v >> 8) as u8,
            })
        };
        return match (ch(0), ch(1), ch(2)) {
            (Some(r), Some(g), Some(b)) => Ok(Some((r, g, b))),
            _ => Err(()),
        };
    }
    named(spec).map(Some).ok_or(())
}

/// The quoted strings of the C source, escapes `\\` and `\"` resolved.
fn strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut inside, mut esc, mut comment) = (false, false, false);
    let mut prev = '\0';
    for c in text.chars() {
        if comment {
            if prev == '*' && c == '/' {
                comment = false;
            }
            prev = c;
            continue;
        }
        if inside {
            if esc {
                cur.push(c);
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                inside = false;
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            inside = true;
        } else if prev == '/' && c == '*' {
            comment = true;
        }
        prev = c;
    }
    out
}

pub fn parse(text: &str) -> Option<Pixmap> {
    let s = strings(text);
    let mut head = s.first()?.split_whitespace().map(|v| v.parse::<usize>().ok());
    let (w, h, ncol, cpp) = (head.next()??, head.next()??, head.next()??, head.next()??);
    if w == 0 || h == 0 || w > 2048 || h > 2048 || cpp == 0 || cpp > 4 || ncol == 0 || ncol > 65536 || s.len() < 1 + ncol + h {
        return None;
    }
    let mut table: std::collections::HashMap<&str, Option<(u8, u8, u8)>> = std::collections::HashMap::new();
    for line in &s[1..1 + ncol] {
        if line.len() < cpp || !line.is_char_boundary(cpp) {
            return None;
        }
        let (key, rest) = line.split_at(cpp);
        let toks: Vec<&str> = rest.split_whitespace().collect();
        // key tokens: c (colour), m (mono), g4/g (grey), s (symbolic name); take the colour value
        let is_key = |t: &str| matches!(t, "c" | "m" | "g" | "g4" | "s");
        let pos = toks.iter().position(|t| *t == "c").or_else(|| toks.iter().position(|t| is_key(t)))?;
        let val: Vec<&str> = toks[pos + 1..].iter().take_while(|t| !is_key(t)).copied().collect();
        table.insert(key, colour(&val.join(" ")).unwrap_or(Some((128, 128, 128))));
    }
    let mut pm = Pixmap::new(w as u32, h as u32)?;
    for (y, row) in s[1 + ncol..1 + ncol + h].iter().enumerate() {
        for x in 0..w {
            let key = row.get(x * cpp..(x + 1) * cpp)?;
            if let Some(Some((r, g, b))) = table.get(key) {
                let i = (y * w + x) * 4;
                pm.data_mut()[i..i + 4].copy_from_slice(&[*r, *g, *b, 255]);
            }
        }
    }
    Some(pm)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "/* XPM */
static char * test_xpm[] = {
\"3 2 3 1\",
\"  c None\",
\". c #ff0000\",
\"# c blue\",
\".#.\",
\" ##\"};
";

    #[test]
    fn reads_colours_transparency_and_size() {
        let pm = parse(SAMPLE).expect("parses");
        assert_eq!((pm.width(), pm.height()), (3, 2));
        let px = |x, y| {
            let p = pm.pixel(x, y).unwrap();
            (p.red(), p.green(), p.blue(), p.alpha())
        };
        assert_eq!(px(0, 0), (255, 0, 0, 255));
        assert_eq!(px(1, 0), (0, 0, 255, 255));
        assert_eq!(px(0, 1).3, 0, "None is transparent");
    }

    #[test]
    fn two_character_keys_and_wide_hex_colours() {
        let t = "/* XPM */\nstatic char *x[] = {\n\"2 1 2 2\",\n\"aa c #ffff00000000\",\n\"b  c #0f0\",\n\"aab \"};";
        // the second key is "b " (with a space): the row reads "aa" then "b "
        let pm = parse(t).expect("parses");
        let p = pm.pixel(0, 0).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (255, 0, 0));
        let p = pm.pixel(1, 0).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (0, 255, 0));
    }

    #[test]
    fn garbage_is_none_not_a_panic() {
        assert!(parse("").is_none());
        assert!(parse("\"1 1 1 1\",").is_none());
        assert!(parse("\"0 0 0 0\"").is_none());
        assert!(parse("\"2 2 1 1\", \"a c red\", \"aa\"").is_none(), "missing pixel rows");
        assert!(parse("\"99999 1 1 1\",\"a c red\",\"a\"").is_none());
    }
}
