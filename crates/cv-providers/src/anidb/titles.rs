//! AniDB's title dump and the local fuzzy search over it.
//!
//! AniDB has no search request. It publishes a dump of every title in every language
//! (`anime-titles.dat.gz`) and allows one download of it a day, so the provider keeps the dump
//! (and the parsed index) and searches it here. A line of the dump is
//!
//! ```text
//! <anime id>|<type>|<language>|<title>
//! ```
//!
//! with type 1 the main title, 2 a synonym, 3 a short title and 4 an official title. Lines that
//! start with `#` are comments.

use std::collections::HashMap;
use std::io::Read;

/// The largest dump read: far above the real one (about 15 MB unpacked).
const MAX_DUMP_BYTES: u64 = 128 * 1024 * 1024;

/// The names an anime is shown under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Names {
    pub aid: u64,
    /// The main title, in romaji (language `x-jat`).
    pub main: String,
    /// The official English title.
    pub english: Option<String>,
    /// The official Japanese title.
    pub japanese: Option<String>,
}

/// One scored result.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Match {
    pub names: Names,
    /// How well the best of the anime's titles matches, `0.0..=1.0`.
    pub score: f32,
}

/// Every title of every anime, ready for searching.
#[derive(Debug, Default)]
pub(super) struct TitleIndex {
    animes: Vec<Names>,
    /// `(index into animes, normalised title)`.
    entries: Vec<(usize, String)>,
}

/// Letters and digits in lower case, words separated by one space.
pub(super) fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
    out
}

/// The dump as text: gunzipped when it starts with the gzip magic bytes, else taken as text. A
/// server may send the file already unpacked.
pub(super) fn dump_text(bytes: &[u8]) -> Result<String, String> {
    let raw = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_DUMP_BYTES)
            .read_to_end(&mut out)
            .map_err(|e| format!("the title dump is not valid gzip ({e})"))?;
        out
    } else {
        bytes.to_vec()
    };
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

impl TitleIndex {
    /// Reads the dump. A dump without a single title line is not a dump.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut index = Self::default();
        let mut by_aid: HashMap<u64, usize> = HashMap::new();
        for line in text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(4, '|');
            let (Some(aid), Some(kind), Some(language), Some(title)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let (Ok(aid), Ok(kind)) = (aid.trim().parse::<u64>(), kind.trim().parse::<u8>()) else {
                continue;
            };
            let title = title.trim();
            if aid == 0 || title.is_empty() {
                continue;
            }
            let at = *by_aid.entry(aid).or_insert_with(|| {
                index.animes.push(Names {
                    aid,
                    main: String::new(),
                    english: None,
                    japanese: None,
                });
                index.animes.len() - 1
            });
            let names = &mut index.animes[at];
            let language = language.trim().to_ascii_lowercase();
            match (kind, language.as_str()) {
                (1, _) => names.main = title.to_owned(),
                (4, "en") if names.english.is_none() => names.english = Some(title.to_owned()),
                (4, "ja") if names.japanese.is_none() => names.japanese = Some(title.to_owned()),
                _ => {}
            }
            let normal = normalise(title);
            if !normal.is_empty() {
                index.entries.push((at, normal));
            }
        }
        if index.animes.is_empty() {
            return Err("the title dump has no titles".to_owned());
        }
        // An anime without a main title is shown under its first official or synonym title.
        for names in &mut index.animes {
            if names.main.is_empty() {
                names.main = names
                    .english
                    .clone()
                    .or_else(|| names.japanese.clone())
                    .unwrap_or_default();
            }
        }
        Ok(index)
    }

    /// The number of anime in the dump.
    pub fn len(&self) -> usize {
        self.animes.len()
    }

    /// The anime that best match `query`, best first, at most `limit`. An anime counts by its
    /// best title; anything below `MIN_SCORE` is left out.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Match> {
        let wanted = normalise(query);
        if wanted.is_empty() {
            return Vec::new();
        }
        let wanted_grams = bigrams(&wanted);
        let mut best: HashMap<usize, f32> = HashMap::new();
        for (at, title) in &self.entries {
            let score = score(&wanted, &wanted_grams, title);
            if score >= MIN_SCORE {
                let slot = best.entry(*at).or_insert(0.0);
                if score > *slot {
                    *slot = score;
                }
            }
        }
        let mut found: Vec<Match> = best
            .into_iter()
            .map(|(at, score)| Match {
                names: self.animes[at].clone(),
                score,
            })
            .collect();
        found.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.names.aid.cmp(&b.names.aid))
        });
        found.truncate(limit);
        found
    }
}

/// The lowest score a title needs to count as a match.
const MIN_SCORE: f32 = 0.55;

/// The sorted character pairs of a normalised title.
fn bigrams(text: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = text.chars().collect();
    let mut pairs: Vec<(char, char)> = chars.windows(2).map(|w| (w[0], w[1])).collect();
    pairs.sort_unstable();
    pairs
}

/// The Dice coefficient of two sorted pair lists.
fn dice(a: &[(char, char)], b: &[(char, char)]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    (2 * shared) as f32 / (a.len() + b.len()) as f32
}

/// How well a normalised title matches the normalised query: `1.0` for the same words, a high
/// score when the title holds the query as whole words, else the pair similarity.
pub(super) fn score(query: &str, query_grams: &[(char, char)], title: &str) -> f32 {
    if query == title {
        return 1.0;
    }
    let contains =
        title.len() > query.len() && format!(" {title} ").contains(&format!(" {query} "));
    if !contains && title.len() > query.len() * 3 + 8 {
        // Far longer and not containing it: cannot be close.
        return 0.0;
    }
    let similar = dice(query_grams, &bigrams(title));
    if contains {
        similar.max(0.7 + 0.25 * query.len() as f32 / title.len() as f32)
    } else {
        similar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = "# created: Mon Jan 01 00:00:00 2024\n\
# <aid>|<type>|<language>|<title>\n\
# type: 1=primary title, 2=synonyms, 3=shorttitles, 4=official title\n\
100|1|x-jat|Hoshi no Tabi\n\
100|4|en|Star Voyage\n\
100|4|ja|星の旅\n\
100|2|en|Voyage of Stars\n\
100|3|en|SV\n\
101|1|x-jat|Hoshi no Tabi: Gekijouban\n\
101|4|en|Star Voyage: The Movie\n\
102|1|x-jat|Neko no Heya\n\
bad line\n\
x|1|en|Not a number\n\
103|1|en|\n";

    #[test]
    fn the_dump_is_read_into_names_and_titles() {
        let index = TitleIndex::parse(DUMP).unwrap();
        assert_eq!(index.len(), 3);
        let star = index.search("star voyage", 10);
        assert_eq!(star[0].names.aid, 100);
        assert_eq!(star[0].names.main, "Hoshi no Tabi");
        assert_eq!(star[0].names.english.as_deref(), Some("Star Voyage"));
        assert_eq!(star[0].names.japanese.as_deref(), Some("星の旅"));
        assert_eq!(star[0].score, 1.0);
    }

    #[test]
    fn a_dump_without_titles_is_not_a_dump() {
        assert!(TitleIndex::parse("# only comments\n").is_err());
        assert!(TitleIndex::parse("<html>blocked</html>").is_err());
    }

    #[test]
    fn matches_are_ranked_by_their_best_title() {
        let index = TitleIndex::parse(DUMP).unwrap();
        // The exact title first, then the film whose title holds the query as words.
        let ids: Vec<_> = index
            .search("Star Voyage", 10)
            .iter()
            .map(|m| m.names.aid)
            .collect();
        assert_eq!(ids, [100, 101]);
        // Any language and any kind of title counts, and punctuation does not matter.
        assert_eq!(index.search("voyage OF stars!", 10)[0].names.aid, 100);
        assert_eq!(
            index.search("hoshi no tabi gekijouban", 10)[0].names.aid,
            101
        );
        assert_eq!(index.search("星の旅", 10)[0].names.aid, 100);
    }

    #[test]
    fn small_typos_still_match_and_unrelated_titles_do_not() {
        let index = TitleIndex::parse(DUMP).unwrap();
        assert_eq!(index.search("Star Voyge", 10)[0].names.aid, 100);
        assert!(index.search("completely different", 10).is_empty());
        assert!(index.search("  ", 10).is_empty());
        assert_eq!(index.search("star", 1).len(), 1);
    }

    #[test]
    fn normalising_keeps_letters_and_digits() {
        assert_eq!(
            normalise("  Re:Zero — Starting Life!! 2nd "),
            "re zero starting life 2nd"
        );
        assert_eq!(normalise("星の旅"), "星の旅");
        assert_eq!(normalise("!!!"), "");
    }

    #[test]
    fn gzip_is_unpacked_and_plain_text_passes() {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(DUMP.as_bytes()).unwrap();
        let packed = encoder.finish().unwrap();
        assert_eq!(dump_text(&packed).unwrap(), DUMP);
        assert_eq!(dump_text(DUMP.as_bytes()).unwrap(), DUMP);
        assert!(dump_text(&[0x1f, 0x8b, 0x00, 0x01]).is_err());
    }
}
