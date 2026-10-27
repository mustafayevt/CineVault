//! Shortest-digits decimal formatting with half-up / half-even rounding, so numbers render the
//! same on every platform.
//!
//! A number is formatted from its shortest decimal digits, and those digits are rounded, not
//! the binary value: `%.1f` of 0.25 is `0.3`, and `0.###` keeps `23.976`. Rust's own
//! formatting rounds the exact binary value, which gives a different result on ties.

use crate::value::Value;

/// How the digit string is rounded when digits are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Fixed decimals (`%.2f`): a dropped digit of 5 or more rounds up.
    HalfUp,
    /// Pattern formats (`0.##`): an exact tie rounds to the even neighbour.
    HalfEven,
}

/// `x` (finite, not negative) as integer and fraction digits with exactly `decimals` decimals.
pub fn fixed(x: f64, decimals: usize, rounding: Rounding) -> (String, String) {
    let (mut digits, mut point) = shortest_digits(x);
    let keep = i64::from(point) + decimals as i64;
    if keep < 0 {
        digits = vec![0];
        point = 1;
    } else if (keep as usize) < digits.len() {
        let keep = keep as usize;
        let dropped = digits[keep];
        let rest_nonzero = digits[keep + 1..].iter().any(|d| *d != 0);
        let last_kept_odd = keep > 0 && digits[keep - 1] % 2 == 1;
        let up = match rounding {
            Rounding::HalfUp => dropped >= 5,
            Rounding::HalfEven => dropped > 5 || (dropped == 5 && (rest_nonzero || last_kept_odd)),
        };
        digits.truncate(keep);
        if up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
        if digits.is_empty() {
            digits.push(0);
            point = 1;
        }
    }
    let ch = |d: u8| char::from(b'0' + d);
    let mut int_part = String::new();
    let mut frac_part = String::new();
    if point <= 0 {
        int_part.push('0');
        for _ in 0..(-point) {
            frac_part.push('0');
        }
        frac_part.extend(digits.iter().map(|d| ch(*d)));
    } else {
        for i in 0..point as usize {
            int_part.push(digits.get(i).map_or('0', |d| ch(*d)));
        }
        if digits.len() > point as usize {
            frac_part.extend(digits[point as usize..].iter().map(|d| ch(*d)));
        }
    }
    frac_part.truncate(decimals);
    while frac_part.len() < decimals {
        frac_part.push('0');
    }
    (int_part, frac_part)
}

/// The shortest digits that identify `x`, and the position of the decimal point relative to
/// the first digit (`point` digits come before it; zero or negative for values below 0.1).
fn shortest_digits(x: f64) -> (Vec<u8>, i32) {
    if x == 0.0 {
        return (vec![0], 1);
    }
    let text = format!("{x:e}");
    let (mantissa, exp) = text.split_once('e').unwrap_or((&text, "0"));
    let digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    let exp: i32 = exp.parse().unwrap_or(0);
    (digits, exp + 1)
}

/// Groups the integer digits in threes with commas.
fn group(int_part: &str) -> String {
    let mut out = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Default)]
struct Flags {
    left: bool,
    plus: bool,
    space: bool,
    zero: bool,
    comma: bool,
    paren: bool,
}

/// Formats one argument with a printf-style pattern. Supports `%d %f %s %S %x %X %n %%`
/// with the flags `- + space 0 , (`, a width and (for `%f` and `%s`) a precision. The same
/// argument serves every conversion in the pattern.
pub fn format(pattern: &str, arg: &Value) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut flags = Flags::default();
        while let Some(&f) = chars.peek() {
            match f {
                '-' => flags.left = true,
                '+' => flags.plus = true,
                ' ' => flags.space = true,
                '0' => flags.zero = true,
                ',' => flags.comma = true,
                '(' => flags.paren = true,
                _ => break,
            }
            chars.next();
        }
        let mut width = String::new();
        while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
            width.push(d);
            chars.next();
        }
        let mut precision: Option<usize> = None;
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut p = String::new();
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                p.push(d);
                chars.next();
            }
            precision = Some(p.parse().map_err(|_| "bad precision".to_owned())?);
        }
        let width: usize = if width.is_empty() {
            0
        } else {
            width.parse().map_err(|_| "bad width".to_owned())?
        };
        let conv = chars.next().ok_or("format ends after `%`")?;
        let body = match conv {
            '%' => "%".to_owned(),
            'n' => "\n".to_owned(),
            'd' => {
                let n = arg.as_i64().ok_or("`%d` needs a number")?;
                let digits = n.unsigned_abs().to_string();
                let digits = if flags.comma { group(&digits) } else { digits };
                signed(n < 0, &digits, &flags, width)
            }
            'f' => {
                let x = arg.as_f64().ok_or("`%f` needs a number")?;
                if !x.is_finite() {
                    return Err("`%f` needs a finite number".to_owned());
                }
                let (int_part, frac) = fixed(x.abs(), precision.unwrap_or(6), Rounding::HalfUp);
                let int_part = if flags.comma {
                    group(&int_part)
                } else {
                    int_part
                };
                let digits = if frac.is_empty() {
                    int_part
                } else {
                    format!("{int_part}.{frac}")
                };
                signed(x.is_sign_negative(), &digits, &flags, width)
            }
            's' | 'S' => {
                let mut text = arg.to_text(false);
                if let Some(p) = precision {
                    text = text.chars().take(p).collect();
                }
                if conv == 'S' {
                    text = text.to_uppercase();
                }
                text
            }
            'x' | 'X' => {
                let n = arg.as_i64().ok_or("`%x` needs a number")?;
                let text = if n < 0 && n >= i64::from(i32::MIN) {
                    format!("{:x}", n as i32)
                } else {
                    format!("{n:x}")
                };
                if conv == 'X' {
                    text.to_uppercase()
                } else {
                    text
                }
            }
            other => return Err(format!("unsupported conversion `%{other}`")),
        };
        out.push_str(&pad(&body, width, &flags));
    }
    Ok(out)
}

/// Applies sign flags to a number's digits.
fn signed(negative: bool, digits: &str, flags: &Flags, width: usize) -> String {
    let (open, close) = if negative && flags.paren {
        ("(", ")")
    } else {
        ("", "")
    };
    let sign = if negative && !flags.paren {
        "-"
    } else if !negative && flags.plus {
        "+"
    } else if !negative && flags.space {
        " "
    } else {
        ""
    };
    let prefix = format!("{open}{sign}");
    if flags.zero && !flags.left {
        let used = prefix.chars().count() + digits.chars().count() + close.len();
        let zeros = "0".repeat(width.saturating_sub(used));
        return format!("{prefix}{zeros}{digits}{close}");
    }
    format!("{prefix}{digits}{close}")
}

fn pad(body: &str, width: usize, flags: &Flags) -> String {
    let len = body.chars().count();
    if len >= width {
        return body.to_owned();
    }
    let fill = " ".repeat(width - len);
    // Zero padding of numbers was already applied in `signed`.
    if flags.left {
        format!("{body}{fill}")
    } else {
        format!("{fill}{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(pattern: &str, v: impl Into<Value>) -> String {
        format(pattern, &v.into()).unwrap()
    }

    #[test]
    fn fixed_rounds_half_up_on_the_shortest_digits() {
        assert_eq!(f("%.1f", 0.25), "0.3");
        assert_eq!(f("%.1f", 0.35), "0.4");
        assert_eq!(f("%.2f", 21.474_836_48), "21.47");
        assert_eq!(f("%.2f", 20_480.0), "20480.00");
        assert_eq!(f("%.1f", 0.0), "0.0");
        assert_eq!(f("%.0f", 0.5), "1");
        assert_eq!(f("%.1f", 9.96), "10.0");
        assert_eq!(f("%.1f", 0.04), "0.0");
        assert_eq!(f("%.3f", 0.0005), "0.001");
        assert_eq!(f("%f", 1.5), "1.500000");
        assert_eq!(f("%.1f", -2.25), "-2.3");
        assert_eq!(f("%.1f", -0.04), "-0.0");
    }

    #[test]
    fn integers_and_flags() {
        assert_eq!(f("%02d", 3_i64), "03");
        assert_eq!(f("%06d", 2024_i64), "002024");
        assert_eq!(f("%d", -5_i64), "-5");
        assert_eq!(f("%+d", 5_i64), "+5");
        assert_eq!(f("%5d|", 42_i64), "   42|");
        assert_eq!(f("%-5d|", 42_i64), "42   |");
        assert_eq!(f("%,d", 1_234_567_i64), "1,234,567");
        assert_eq!(f("%05d", -42_i64), "-0042");
        assert_eq!(f("%(d", -7_i64), "(7)");
        assert_eq!(f("S%02dE%02d", 3_i64), "S03E03");
        assert_eq!(f("%x", 255_i64), "ff");
        assert_eq!(f("%X", 255_i64), "FF");
    }

    #[test]
    fn strings_and_literals() {
        assert_eq!(f("%s!", "hi"), "hi!");
        assert_eq!(f("%.2s", "hello"), "he");
        assert_eq!(f("%S", "hi"), "HI");
        assert_eq!(f("100%%", 1_i64), "100%");
        assert_eq!(f("%8.2f|", 3.14169), "    3.14|");
        assert_eq!(f("%08.2f", -3.14169), "-0003.14");
    }

    #[test]
    fn errors() {
        assert!(format("%d", &Value::text("abc")).is_err());
        assert!(format("%", &Value::Int(1)).is_err());
        assert!(format("%q", &Value::Int(1)).is_err());
    }

    #[test]
    fn half_even_for_decimal_format() {
        assert_eq!(
            fixed(23.976, 3, Rounding::HalfEven),
            ("23".into(), "976".into())
        );
        assert_eq!(
            fixed(0.125, 2, Rounding::HalfEven),
            ("0".into(), "12".into())
        );
        assert_eq!(
            fixed(0.135, 2, Rounding::HalfEven),
            ("0".into(), "14".into())
        );
        assert_eq!(
            fixed(59.9401, 3, Rounding::HalfEven),
            ("59".into(), "940".into())
        );
        assert_eq!(
            fixed(23.976, 0, Rounding::HalfUp),
            ("24".into(), String::new())
        );
        assert_eq!(
            fixed(0.0004, 3, Rounding::HalfEven),
            ("0".into(), "000".into())
        );
    }
}
