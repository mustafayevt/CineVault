//! The built-in named renderers: `${token;name(params)}`.
//!
//! Output matches the compatibility fixtures. Dates use the common `yyyy-MM-dd` pattern
//! letters, `number` uses printf-style patterns, and the text renderers have fixed rules for
//! case and first letters.

use crate::datetime::DEFAULT_DATE_PATTERN;
use crate::decimal::{self, Rounding, fixed};
use crate::eval::{Engine, FnRenderer, RenderContext};
use crate::value::Value;

type Output = Result<Value, String>;

/// Registers `first`, `upper`, `lower`, `title`, `array`, `uniqueArray`, `date`, `number`,
/// `bitrate`, `filesize`, `framerate`, `split` and `replace`.
pub fn register_standard(engine: &mut Engine) {
    engine.register(FnRenderer::new("first", first));
    engine.register(FnRenderer::new("upper", upper));
    engine.register(FnRenderer::new("lower", lower));
    engine.register(FnRenderer::new("title", title));
    engine.register(FnRenderer::new("array", array));
    engine.register(FnRenderer::new("uniqueArray", unique_array));
    engine.register(FnRenderer::new("date", date));
    engine.register(FnRenderer::new("number", number));
    engine.register(FnRenderer::new("bitrate", bitrate));
    engine.register(FnRenderer::new("filesize", filesize));
    engine.register(FnRenderer::new("framerate", framerate));
    engine.register(FnRenderer::new("split", split));
    engine.register(FnRenderer::new("replace", replace));
}

fn text(input: &Value) -> String {
    input.to_text(false)
}

/// The first letter in upper case, or `#` when the text starts with anything else.
fn first(input: &Value, _: Option<&str>, _: &RenderContext) -> Output {
    let text = text(input);
    let Some(c) = text.chars().next() else {
        return Ok(Value::text(""));
    };
    if c.is_alphabetic() {
        Ok(Value::text(c.to_uppercase().collect::<String>()))
    } else {
        Ok(Value::text("#"))
    }
}

fn upper(input: &Value, _: Option<&str>, _: &RenderContext) -> Output {
    Ok(Value::text(text(input).to_uppercase()))
}

fn lower(input: &Value, _: Option<&str>, _: &RenderContext) -> Output {
    Ok(Value::text(text(input).to_lowercase()))
}

/// Upper-cases the first letter of every word. Letters and digits (and apostrophes inside a
/// word) belong to a word; every other character starts a new one.
fn title(input: &Value, _: Option<&str>, _: &RenderContext) -> Output {
    let mut out = String::new();
    let mut in_word = false;
    for c in text(input).chars() {
        if !in_word && c.is_alphabetic() {
            let mut upper = c.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(u), None) => out.push(u),
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
        in_word = c.is_alphanumeric() || (in_word && matches!(c, '\'' | '\u{2019}'));
    }
    Ok(Value::text(out))
}

fn joined(input: &Value, params: Option<&str>, unique: bool) -> Output {
    let delimiter = params.filter(|p| !p.is_empty()).unwrap_or(", ");
    let mut seen: Vec<String> = Vec::new();
    for item in input.items() {
        let item = item.to_text(false);
        if unique && seen.contains(&item) {
            continue;
        }
        seen.push(item);
    }
    Ok(Value::text(seen.join(delimiter)))
}

fn array(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    joined(input, params, false)
}

fn unique_array(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    joined(input, params, true)
}

fn date(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    let Value::DateTime(date) = input else {
        return Ok(Value::text(""));
    };
    let pattern = params
        .filter(|p| !p.is_empty())
        .unwrap_or(DEFAULT_DATE_PATTERN);
    date.format(pattern)
        .map(Value::text)
        .ok_or_else(|| format!("`{pattern}` is not a date pattern"))
}

fn number(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    let Some(pattern) = params.filter(|p| !p.is_empty()) else {
        return Ok(Value::text(text(input)));
    };
    match input {
        Value::Null => Ok(Value::text("")),
        Value::Int(_) | Value::Float(_) => decimal::format(pattern, input).map(Value::text),
        // Text that is not a number cannot be formatted as one; show it unchanged.
        other => Ok(Value::text(text(other))),
    }
}

fn amount(input: &Value) -> Option<f64> {
    input.as_f64().filter(|v| v.is_finite())
}

fn two_decimals(value: f64, unit: &str, decimals: usize) -> Value {
    let (int_part, frac) = fixed(value.abs(), decimals, Rounding::HalfUp);
    let sign = if value < 0.0 { "-" } else { "" };
    if frac.is_empty() {
        Value::text(format!("{sign}{int_part} {unit}"))
    } else {
        Value::text(format!("{sign}{int_part}.{frac} {unit}"))
    }
}

/// The input is in kilobits per second; the default unit is `Mbps`.
fn bitrate(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    let Some(value) = amount(input) else {
        return Ok(Value::text(""));
    };
    match params.map(str::trim).filter(|p| !p.is_empty()) {
        None | Some("Mbps") => Ok(two_decimals(value / 1000.0, "Mbps", 1)),
        Some(unit @ ("k" | "kbps")) => Ok(two_decimals(value, unit, 1)),
        Some(other) => Err(format!("unknown unit `{other}`, use k, kbps or Mbps")),
    }
}

/// The input is in bytes; the default unit is `GB` (10^9 bytes).
fn filesize(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    let Some(value) = amount(input) else {
        return Ok(Value::text(""));
    };
    let unit = params
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("GB");
    let size = match unit {
        // Kilobytes are whole numbers (integer division).
        "KB" => (value / 1e3).floor(),
        "KiB" => (value / 1024.0).floor(),
        "MB" => value / 1e6,
        "MiB" => value / 1_048_576.0,
        "GB" => value / 1e9,
        "GiB" => value / 1_073_741_824.0,
        other => {
            return Err(format!(
                "unknown unit `{other}`, use KB, KiB, MB, MiB, GB or GiB"
            ));
        }
    };
    Ok(two_decimals(size, unit, 2))
}

/// Up to three decimals, none for whole numbers; `round` rounds to a whole number.
fn framerate(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    let Some(value) = amount(input) else {
        return Ok(Value::text(""));
    };
    let sign = if value < 0.0 { "-" } else { "" };
    if params.map(str::trim) == Some("round") {
        let (int_part, _) = fixed(value.abs(), 0, Rounding::HalfUp);
        return Ok(Value::text(format!("{sign}{int_part}")));
    }
    let (int_part, frac) = fixed(value.abs(), 3, Rounding::HalfEven);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        Ok(Value::text(format!("{sign}{int_part}")))
    } else {
        Ok(Value::text(format!("{sign}{int_part}.{frac}")))
    }
}

/// Picks parts of a comma-separated text: `split(i)` is the part at index `i` (from 0),
/// `split(from,to)` joins the parts from `from` to `to`, both included. An index past the end
/// gives the whole text.
fn split(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    let parts: Vec<String> = match input {
        Value::List(items) => items.iter().map(|i| i.to_text(false)).collect(),
        other => text(other)
            .split(',')
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
            .collect(),
    };
    let params = params.ok_or("needs an index or a range")?;
    let number = |s: &str| {
        s.trim()
            .parse::<usize>()
            .map_err(|_| format!("`{s}` is not an index"))
    };
    let whole = || Value::text(parts.join(", "));
    match params.split_once(',') {
        None => Ok(parts.get(number(params)?).map_or_else(whole, Value::text)),
        Some((from, to)) => {
            let (from, to) = (number(from)?, number(to)?);
            if from >= parts.len() || to < from {
                return Ok(whole());
            }
            let slice: Vec<&str> = parts[from..=to.min(parts.len() - 1)]
                .iter()
                .map(String::as_str)
                .collect();
            Ok(Value::text(slice.join(", ")))
        }
    }
}

/// What a `replace(...)` call asks for.
pub(crate) enum Replacement<'a> {
    /// `replace(from,to)` and `replace(,,to)` (commas become `to`).
    Literal(&'a str, &'a str),
    /// `replace(file.csv)`: a table in the data folder.
    Table(&'a str),
}

pub(crate) fn replacement(params: Option<&str>) -> Result<Replacement<'_>, String> {
    let params = params.ok_or("needs `from,to` or a `.csv` file")?;
    if let Some(to) = params.strip_prefix(",,") {
        return Ok(Replacement::Literal(",", to));
    }
    let trimmed = params.trim();
    if trimmed.len() > 4 && trimmed[trimmed.len() - 4..].eq_ignore_ascii_case(".csv") {
        return Ok(Replacement::Table(trimmed));
    }
    match params.split_once(',') {
        Some(("", _)) | None => Err("needs `from,to` with a non-empty `from`".to_owned()),
        Some((from, to)) => Ok(Replacement::Literal(from, to)),
    }
}

/// `replace(from,to)` replaces every occurrence of a literal; `replace(,,to)` replaces commas.
/// A `.csv` table needs the data folder: register [`crate::renamer_renderers::ReplaceTables`].
fn replace(input: &Value, params: Option<&str>, _: &RenderContext) -> Output {
    match replacement(params)? {
        Replacement::Literal(from, to) => Ok(Value::text(text(input).replace(from, to))),
        Replacement::Table(name) => Err(format!(
            "`{name}` needs the data folder, which this engine does not have"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::DateTime;

    fn run(template: &str, token: Value) -> String {
        let model = Value::map([("t", token)]);
        Engine::standard().render_str(template, &model).unwrap()
    }

    #[test]
    fn case_renderers() {
        assert_eq!(run("${t;upper}", "Dune: Part".into()), "DUNE: PART");
        assert_eq!(run("${t;lower}", "Dune: Part".into()), "dune: part");
        assert_eq!(
            run("${t;lower;title}", "WHAT?! \"QUOTED\" <TAG> | pipe".into()),
            "What?! \"Quoted\" <Tag> | Pipe"
        );
        assert_eq!(
            run("${t;title}", "don't stop (voice) ünï".into()),
            "Don't Stop (Voice) Ünï"
        );
        assert_eq!(run("${t;upper}", Value::Null), "");
    }

    #[test]
    fn first_letter() {
        assert_eq!(run("${t;first}", "the matrix".into()), "T");
        assert_eq!(run("${t;first}", "2001: A Space Odyssey".into()), "#");
        assert_eq!(run("${t;first}", "Ünï".into()), "Ü");
        assert_eq!(run("${t;first}", "?!".into()), "#");
        assert_eq!(run("x${t;first}", "".into()), "x");
        assert_eq!(run("${t;lower;first}", "ABC".into()), "A");
        assert_eq!(run("${t;upper;first}", "abc".into()), "A");
    }

    #[test]
    fn arrays() {
        let genres = || Value::list([Value::text("A"), Value::text("B"), Value::text("A")]);
        assert_eq!(run("${t;array}", genres()), "A, B, A");
        assert_eq!(run("${t;array( | )}", genres()), "A | B | A");
        assert_eq!(run("${t;uniqueArray(;)}", genres()), "A;B");
        assert_eq!(run("${t;array}", Value::list([])), "");
        assert_eq!(run("${t;array}", "single".into()), "single");
    }

    #[test]
    fn dates() {
        let d = Value::DateTime(DateTime::parse("1992-11-25", 0).unwrap());
        assert_eq!(run("${t;date(dd.MM.yyyy)}", d.clone()), "25.11.1992");
        assert_eq!(run("${t;date}", d), "25.11.1992 00:00:00 +0000");
        assert_eq!(run("${t;date(yyyy)}", Value::Null), "");
        let model = Value::map([(
            "t",
            Value::DateTime(DateTime::parse("2024-01-01", 0).unwrap()),
        )]);
        assert!(
            Engine::standard()
                .render_str("${t;date(yyyy Q)}", &model)
                .is_err()
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(run("${t;number(%02d)}", 3_i64.into()), "03");
        assert_eq!(run("${t;number(%.1f)}", 0.0_f64.into()), "0.0");
        assert_eq!(run("${t;number(%.1f)}", Value::Null), "");
        assert_eq!(run("${t;number(%d)}", "abc".into()), "abc");
        assert_eq!(run("${t;number}", 7_i64.into()), "7");
    }

    #[test]
    fn sizes_and_rates() {
        assert_eq!(run("${t;bitrate}", 18_000_000_i64.into()), "18000.0 Mbps");
        assert_eq!(
            run("${t;bitrate(kbps)}", 18_000_000_i64.into()),
            "18000000.0 kbps"
        );
        assert_eq!(run("${t;bitrate}", 0_i64.into()), "0.0 Mbps");
        assert_eq!(run("${t;filesize}", 21_474_836_480_i64.into()), "21.47 GB");
        assert_eq!(
            run("${t;filesize(MiB)}", 21_474_836_480_i64.into()),
            "20480.00 MiB"
        );
        assert_eq!(run("${t;filesize(GiB)}", 0_i64.into()), "0.00 GiB");
        assert_eq!(
            run("${t;filesize(KB)}", 21_474_836_480_i64.into()),
            "21474836.00 KB"
        );
        assert_eq!(
            run("${t;filesize(MB)}", 21_474_836_480_i64.into()),
            "21474.84 MB"
        );
        assert_eq!(
            run("${t;bitrate(k)}", 18_000_000_i64.into()),
            "18000000.0 k"
        );
        assert_eq!(run("${t;framerate}", 23.976_f64.into()), "23.976");
        assert_eq!(run("${t;framerate}", 24.0_f64.into()), "24");
        assert_eq!(run("${t;framerate}", 0.0_f64.into()), "0");
        assert_eq!(run("${t;framerate(round)}", 23.976_f64.into()), "24");
        assert_eq!(run("${t;framerate}", 29.97_f64.into()), "29.97");
        let model = Value::map([("t", Value::Int(1))]);
        assert!(
            Engine::standard()
                .render_str("${t;filesize(XB)}", &model)
                .is_err()
        );
    }

    #[test]
    fn split_and_replace() {
        let genres = "Science Fiction, Adventure, Drama";
        assert_eq!(run("${t;split(1)}", genres.into()), "Adventure");
        assert_eq!(run("${t;split(9)}", genres.into()), genres);
        assert_eq!(
            run("${t;split(0,1)}", genres.into()),
            "Science Fiction, Adventure"
        );
        assert_eq!(run("${t;split(1,9)}", genres.into()), "Adventure, Drama");
        assert_eq!(run("${t;split(7,9)}", genres.into()), genres);
        assert_eq!(run("${t;split(1)}", "".into()), "");
        assert_eq!(run("${t;replace(The,A)}", "The Matrix".into()), "A Matrix");
        assert_eq!(run("${t;replace(,,x)}", "a,b".into()), "axb");
        let model = Value::map([("t", Value::text("a"))]);
        assert!(
            Engine::standard()
                .render_str("${t;replace(x.csv)}", &model)
                .is_err()
        );
    }
}
