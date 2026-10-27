//! Unit conversion for Spotlight: `5 km to mi`, `100 f in c`, `2.5 gb to mb`.

#[derive(Clone, Copy, PartialEq)]
enum Dim {
    Length,
    Mass,
    Volume,
    Data,
    Time,
    Speed,
    Temp,
}

/// (names, factor to the dimension's base unit, dimension, display name). Base: m, g, l, byte, s, m/s.
const UNITS: &[(&[&str], f64, Dim, &str)] = &[
    (&["mm", "millimeter", "millimeters"], 0.001, Dim::Length, "mm"),
    (&["cm", "centimeter", "centimeters"], 0.01, Dim::Length, "cm"),
    (&["m", "meter", "meters", "metre", "metres"], 1.0, Dim::Length, "m"),
    (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], 1000.0, Dim::Length, "km"),
    (&["in", "inch", "inches"], 0.0254, Dim::Length, "in"),
    (&["ft", "foot", "feet"], 0.3048, Dim::Length, "ft"),
    (&["yd", "yard", "yards"], 0.9144, Dim::Length, "yd"),
    (&["mi", "mile", "miles"], 1609.344, Dim::Length, "mi"),
    (&["mg", "milligram", "milligrams"], 0.001, Dim::Mass, "mg"),
    (&["g", "gram", "grams"], 1.0, Dim::Mass, "g"),
    (&["kg", "kilogram", "kilograms", "kilo", "kilos"], 1000.0, Dim::Mass, "kg"),
    (&["t", "tonne", "tonnes"], 1_000_000.0, Dim::Mass, "t"),
    (&["oz", "ounce", "ounces"], 28.349523125, Dim::Mass, "oz"),
    (&["lb", "lbs", "pound", "pounds"], 453.59237, Dim::Mass, "lb"),
    (&["ml", "milliliter", "milliliters"], 0.001, Dim::Volume, "ml"),
    (&["l", "liter", "liters", "litre", "litres"], 1.0, Dim::Volume, "l"),
    (&["tsp", "teaspoon", "teaspoons"], 0.00492892159375, Dim::Volume, "tsp"),
    (&["tbsp", "tablespoon", "tablespoons"], 0.01478676478125, Dim::Volume, "tbsp"),
    (&["floz"], 0.0295735295625, Dim::Volume, "fl oz"),
    (&["cup", "cups"], 0.2365882365, Dim::Volume, "cup"),
    (&["pt", "pint", "pints"], 0.473176473, Dim::Volume, "pt"),
    (&["gal", "gallon", "gallons"], 3.785411784, Dim::Volume, "gal"),
    (&["b", "byte", "bytes"], 1.0, Dim::Data, "B"),
    (&["kb"], 1e3, Dim::Data, "kB"),
    (&["mb"], 1e6, Dim::Data, "MB"),
    (&["gb"], 1e9, Dim::Data, "GB"),
    (&["tb"], 1e12, Dim::Data, "TB"),
    (&["kib"], 1024.0, Dim::Data, "KiB"),
    (&["mib"], 1048576.0, Dim::Data, "MiB"),
    (&["gib"], 1073741824.0, Dim::Data, "GiB"),
    (&["tib"], 1099511627776.0, Dim::Data, "TiB"),
    (&["ms", "millisecond", "milliseconds"], 0.001, Dim::Time, "ms"),
    (&["s", "sec", "secs", "second", "seconds"], 1.0, Dim::Time, "s"),
    (&["min", "mins", "minute", "minutes"], 60.0, Dim::Time, "min"),
    (&["h", "hr", "hrs", "hour", "hours"], 3600.0, Dim::Time, "h"),
    (&["d", "day", "days"], 86400.0, Dim::Time, "d"),
    (&["wk", "week", "weeks"], 604800.0, Dim::Time, "wk"),
    (&["m/s", "mps"], 1.0, Dim::Speed, "m/s"),
    (&["km/h", "kmh", "kph"], 1.0 / 3.6, Dim::Speed, "km/h"),
    (&["mph"], 0.44704, Dim::Speed, "mph"),
    (&["kn", "knot", "knots"], 0.514444, Dim::Speed, "kn"),
    (&["c", "celsius", "°c"], 1.0, Dim::Temp, "°C"),
    (&["f", "fahrenheit", "°f"], 1.0, Dim::Temp, "°F"),
    (&["k", "kelvin"], 1.0, Dim::Temp, "K"),
];

fn find(name: &str) -> Option<&'static (&'static [&'static str], f64, Dim, &'static str)> {
    let n = name.trim().to_lowercase();
    let n = n.trim_end_matches('.');
    // Case matters only between e.g. MB/mb, which users type either way; treat all as the SI/decimal form.
    UNITS.iter().find(|u| u.0.contains(&n))
}

fn to_celsius(v: f64, unit: &str) -> f64 {
    match unit {
        "°F" => (v - 32.0) * 5.0 / 9.0,
        "K" => v - 273.15,
        _ => v,
    }
}

fn from_celsius(c: f64, unit: &str) -> f64 {
    match unit {
        "°F" => c * 9.0 / 5.0 + 32.0,
        "K" => c + 273.15,
        _ => c,
    }
}

fn split_number(s: &str) -> Option<(f64, &str)> {
    let s = s.trim();
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '+')).unwrap_or(s.len());
    let (num, rest) = s.split_at(end);
    let v: f64 = num.replace(',', ".").parse().ok()?;
    (!rest.trim().is_empty()).then_some((v, rest.trim()))
}

/// `Some((result, description))` for `5 km to mi`: result is the bare number text, description reads `5 km = 3.10686 mi`.
pub fn convert(input: &str) -> Option<(String, String)> {
    let lower = input.trim().to_lowercase();
    let (lhs, rhs) = [" to ", " in ", " as ", "->", " into "].iter().find_map(|sep| lower.split_once(sep))?;
    let (value, from_name) = split_number(lhs)?;
    let from = find(from_name)?;
    let to = find(rhs)?;
    if from.2 != to.2 {
        return None;
    }
    let out = if from.2 == Dim::Temp { from_celsius(to_celsius(value, from.3), to.3) } else { value * from.1 / to.1 };
    if !out.is_finite() {
        return None;
    }
    let text = fmt_num(out);
    Some((text.clone(), format!("{} {} = {} {}", fmt_num(value), from.3, text, to.3)))
}

fn fmt_num(v: f64) -> String {
    let a = v.abs();
    if a != 0.0 && !(1e-4..1e12).contains(&a) {
        return format!("{v:e}");
    }
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> Option<String> {
        convert(s).map(|c| c.0)
    }

    #[test]
    fn lengths_and_mass() {
        assert_eq!(r("1 km to m").as_deref(), Some("1000"));
        assert_eq!(r("5 miles in km").as_deref(), Some("8.04672"));
        assert_eq!(r("1 kg to lb").as_deref(), Some("2.204623"));
    }

    #[test]
    fn temperature() {
        assert_eq!(r("100 c to f").as_deref(), Some("212"));
        assert_eq!(r("32 f in c").as_deref(), Some("0"));
        assert_eq!(r("0 c to k").as_deref(), Some("273.15"));
    }

    #[test]
    fn data_and_time() {
        assert_eq!(r("2 gib to mib").as_deref(), Some("2048"));
        assert_eq!(r("90 min to h").as_deref(), Some("1.5"));
    }

    #[test]
    fn description_reads_naturally() {
        assert_eq!(convert("1 km to m").unwrap().1, "1 km = 1000 m");
    }

    #[test]
    fn rejects_mismatches_and_noise() {
        assert_eq!(r("5 km to kg"), None);
        assert_eq!(r("firefox to chrome"), None);
        assert_eq!(r("5 to m"), None);
        assert_eq!(r("km to m"), None);
    }
}
