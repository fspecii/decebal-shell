//! A tiny calculator for Spotlight: `+ - * / % ^`, parentheses, unary minus, decimals.

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(op @ (b'*' | b'/' | b'%' | b'x')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = match op {
                b'/' => v / r,
                b'%' => v % r,
                _ => v * r,
            };
        }
        Some(v)
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            b'-' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Option<f64> {
        if self.peek()? == b'(' {
            self.i += 1;
            let v = self.expr()?;
            (self.peek()? == b')').then(|| self.i += 1)?;
            return Some(v);
        }
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }
}

/// Evaluates `input`; `None` when it is not a complete arithmetic expression or the result is not finite.
/// A bare number is not a calculation, so it also yields `None`.
pub fn eval(input: &str) -> Option<f64> {
    if !input.bytes().any(|c| matches!(c, b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'x' | b'(')) {
        return None;
    }
    let mut p = Parser { s: input.as_bytes(), i: 0 };
    let v = p.expr()?;
    (p.peek().is_none() && v.is_finite()).then_some(v)
}

/// `1234.5`, `0.3333333333`, `42`: up to 10 decimals, no trailing zeros.
pub fn format(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("2+3*4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-3 + 5"), Some(2.0));
        assert_eq!(eval("7 % 4"), Some(3.0));
        assert_eq!(eval("2x8"), Some(16.0));
    }

    #[test]
    fn not_a_calculation() {
        assert_eq!(eval("42"), None);
        assert_eq!(eval("firefox"), None);
        assert_eq!(eval("2+"), None);
        assert_eq!(eval("(1+2"), None);
        assert_eq!(eval("1/0"), None);
    }

    #[test]
    fn formats_cleanly() {
        assert_eq!(format(14.0), "14");
        assert_eq!(format(1.0 / 3.0), "0.3333333333");
        assert_eq!(format(2.5), "2.5");
    }
}
