//! Number formatting and element ordering that the fixtures pin down.
//!
//! * [`fixed`] prints a number with a fixed count of decimals, rounding half up on the shortest
//!   decimal form of the value (`6.85` prints `6.9`, `73` prints `73.0`).
//! * [`user_rating`] prints at most one decimal and drops a zero fraction (`8` and `7.5`).
//! * [`stable_map_order`] gives a stable order for map entries (ratings and provider
//!   IDs), compatible with existing NFO files: the bucket order of a 64-slot hash table keyed by the entry name. Media
//!   centres do not care about this order, but the fixtures compare bytes.

/// Prints `value` with `decimals` decimals. The value is read as the shortest decimal text that
/// identifies the `f32` (so `6.85` is a tie, not `6.8499999`) and a tie rounds up.
pub fn fixed(value: f32, decimals: usize) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let negative = value < 0.0;
    let text = value.abs().to_string();
    let (int_part, frac_part) = text.split_once('.').unwrap_or((&text, ""));
    let mut digits: Vec<u8> = int_part.bytes().map(|b| b - b'0').collect();
    let mut frac: Vec<u8> = frac_part.bytes().map(|b| b - b'0').collect();
    let round_up = frac.get(decimals).is_some_and(|d| *d >= 5);
    frac.resize(decimals, 0);
    digits.extend(frac);
    if round_up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
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
    let int_len = digits.len() - decimals;
    let (int_digits, frac_digits) = digits.split_at(int_len);
    let mut out = String::new();
    let all_zero = digits.iter().all(|d| *d == 0);
    if negative && !all_zero {
        out.push('-');
    }
    if int_digits.is_empty() {
        out.push('0');
    }
    out.extend(int_digits.iter().map(|d| char::from(b'0' + d)));
    if decimals > 0 {
        out.push('.');
        out.extend(frac_digits.iter().map(|d| char::from(b'0' + d)));
    }
    out
}

/// Prints a user rating with at most one decimal and no trailing `.0`: `8`, `7.5`, `0`.
pub fn user_rating(value: f32) -> String {
    let text = format!("{:.1}", f64::from(value));
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A 32-bit polynomial hash of a string (base 31, over UTF-16 code units).
fn string_hash(text: &str) -> u32 {
    text.encode_utf16().fold(0u32, |h, unit| {
        h.wrapping_mul(31).wrapping_add(u32::from(unit))
    })
}

/// The slot of `key` in the 64-slot table.
fn slot(key: &str) -> u32 {
    let h = string_hash(key);
    (h ^ (h >> 16)) & 63
}

/// Sorts `items` into a stable ordering compatible with existing NFO files: by the slot of their key, entries in
/// the same slot keeping their given order.
pub fn stable_map_order<T>(items: &mut [T], key: impl Fn(&T) -> String) {
    items.sort_by_key(|item| slot(&key(item)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_rounds_ties_up_on_the_short_form() {
        assert_eq!(fixed(6.85, 1), "6.9");
        assert_eq!(fixed(7.4, 1), "7.4");
        assert_eq!(fixed(7.7, 1), "7.7");
        assert_eq!(fixed(73.0, 1), "73.0");
        assert_eq!(fixed(0.0, 1), "0.0");
        assert_eq!(fixed(9.96, 1), "10.0");
        assert_eq!(fixed(0.04, 1), "0.0");
        assert_eq!(fixed(0.05, 1), "0.1");
        assert_eq!(fixed(1.7777, 2), "1.78");
        assert_eq!(fixed(2.4, 2), "2.40");
        assert_eq!(fixed(0.0, 2), "0.00");
        assert_eq!(fixed(1.0, 0), "1");
        assert_eq!(fixed(-1.25, 1), "-1.3");
    }

    #[test]
    fn user_rating_drops_the_zero_fraction() {
        assert_eq!(user_rating(8.0), "8");
        assert_eq!(user_rating(7.5), "7.5");
        assert_eq!(user_rating(0.0), "0");
        assert_eq!(user_rating(10.0), "10");
        assert_eq!(user_rating(7.25), "7.2");
    }

    #[test]
    fn stable_map_order_matches_the_fixtures() {
        let mut ratings = vec!["imdb", "metacritic", "themoviedb"];
        stable_map_order(&mut ratings, |s| (*s).to_owned());
        assert_eq!(ratings, ["metacritic", "themoviedb", "imdb"]);
        let mut ids = vec!["imdb", "tmdb", "trakt", "tvdb"];
        stable_map_order(&mut ids, |s| (*s).to_owned());
        assert_eq!(ids, ["tmdb", "imdb", "trakt", "tvdb"]);
    }
}
