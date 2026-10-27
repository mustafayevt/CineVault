//! Renderers that only the renamer engine has, or that need more than the value they get:
//! the renamer `first`, `replace(file.csv)` and the movie-set index renderers.
//!
//! Use [`register_renamer`] to put all three on an engine. `replace(file.csv)` is also useful
//! to export engines: [`register_replace_tables`].

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::eval::{Engine, NamedRenderer, RenderContext};
use crate::renderers::{Replacement, replacement};
use crate::value::Value;

/// Settings of the renamer renderers.
#[derive(Debug, Clone)]
pub struct RenamerRenderers {
    /// What `first` prints for a title that starts with a digit, or for a number or date
    /// (`renamerFirstCharacterNumberReplacement`, `#` by default).
    pub number_replacement: String,
    /// The app's data folder, where `replace(file.csv)` finds its tables. `None` turns tables off.
    pub data_dir: Option<PathBuf>,
}

impl Default for RenamerRenderers {
    fn default() -> Self {
        RenamerRenderers {
            number_replacement: "#".to_owned(),
            data_dir: None,
        }
    }
}

/// Registers the renamer variant of `first`, `replace` with CSV tables (when a data folder is
/// set) and `indexOfMovieSet` / `indexOfMovieSetWithDummy`. Call it after
/// [`Engine::standard`]: it replaces the generic `first` and `replace`.
pub fn register_renamer(engine: &mut Engine, settings: &RenamerRenderers) {
    engine.register(RenamerFirst {
        number_replacement: settings.number_replacement.clone(),
    });
    if let Some(dir) = &settings.data_dir {
        register_replace_tables(engine, dir);
    }
    register_movie_set_index(engine);
}

/// Replaces `replace` with a version that also reads `replace(file.csv)` from `dir`.
pub fn register_replace_tables(engine: &mut Engine, dir: impl Into<PathBuf>) {
    engine.register(ReplaceTables::new(dir));
}

/// Registers `indexOfMovieSet` and `indexOfMovieSetWithDummy`.
pub fn register_movie_set_index(engine: &mut Engine) {
    engine.register(MovieSetIndex {
        name: "indexOfMovieSet",
        list: "movies",
    });
    engine.register(MovieSetIndex {
        name: "indexOfMovieSetWithDummy",
        list: "moviesWithDummy",
    });
}

// ---- first (renamer) -----------------------------------------------------------------------

/// The first letter or digit of the text, folded to ASCII and upper-cased. A digit, a number
/// and a date print the number replacement instead. Nothing printable gives an empty text.
struct RenamerFirst {
    number_replacement: String,
}

impl NamedRenderer for RenamerFirst {
    fn name(&self) -> &str {
        "first"
    }

    fn render(&self, input: &Value, _: Option<&str>, _: &RenderContext) -> Result<Value, String> {
        match input {
            Value::Null => return Ok(Value::text("")),
            Value::Int(_) | Value::Float(_) | Value::DateTime(_) => {
                return Ok(Value::text(&self.number_replacement));
            }
            _ => {}
        }
        for c in input.to_text(false).chars() {
            let folded = fold_to_ascii(c);
            let Some(letter) = folded.chars().find(|c| c.is_alphanumeric()) else {
                continue;
            };
            if letter.is_numeric() {
                return Ok(Value::text(&self.number_replacement));
            }
            return Ok(Value::text(letter.to_uppercase().collect::<String>()));
        }
        Ok(Value::text(""))
    }
}

/// Letters with marks (and a few ligatures) written in plain ASCII. Characters that have no
/// ASCII form stay as they are.
pub fn fold_to_ascii(c: char) -> String {
    if c.is_ascii() {
        return c.to_string();
    }
    const GROUPS: &[(&str, &str)] = &[
        ("ÀÁÂÃÄÅĀĂĄǍ", "A"),
        ("àáâãäåāăąǎ", "a"),
        ("ÆǼ", "AE"),
        ("æǽ", "ae"),
        ("ÇĆĈĊČ", "C"),
        ("çćĉċč", "c"),
        ("ĎĐÐ", "D"),
        ("ďđð", "d"),
        ("ÈÉÊËĒĔĖĘĚ", "E"),
        ("èéêëēĕėęě", "e"),
        ("ĜĞĠĢ", "G"),
        ("ĝğġģ", "g"),
        ("ĤĦ", "H"),
        ("ĥħ", "h"),
        ("ÌÍÎÏĨĪĬĮİǏ", "I"),
        ("ìíîïĩīĭįıǐ", "i"),
        ("Ĳ", "IJ"),
        ("ĳ", "ij"),
        ("Ĵ", "J"),
        ("ĵ", "j"),
        ("Ķ", "K"),
        ("ķ", "k"),
        ("ĹĻĽĿŁ", "L"),
        ("ĺļľŀł", "l"),
        ("ÑŃŅŇ", "N"),
        ("ñńņň", "n"),
        ("ÒÓÔÕÖØŌŎŐǑǾ", "O"),
        ("òóôõöøōŏőǒǿ", "o"),
        ("Œ", "OE"),
        ("œ", "oe"),
        ("ŔŖŘ", "R"),
        ("ŕŗř", "r"),
        ("ŚŜŞŠȘ", "S"),
        ("śŝşšș", "s"),
        ("ß", "ss"),
        ("ŢŤŦȚ", "T"),
        ("ţťŧț", "t"),
        ("Þ", "TH"),
        ("þ", "th"),
        ("ÙÚÛÜŨŪŬŮŰŲǓ", "U"),
        ("ùúûüũūŭůűųǔ", "u"),
        ("Ŵ", "W"),
        ("ŵ", "w"),
        ("ÝŶŸ", "Y"),
        ("ýÿŷ", "y"),
        ("ŹŻŽ", "Z"),
        ("źżž", "z"),
    ];
    GROUPS
        .iter()
        .find(|(chars, _)| chars.contains(c))
        .map_or_else(|| c.to_string(), |(_, ascii)| (*ascii).to_owned())
}

// ---- replace(file.csv) ---------------------------------------------------------------------

/// The replacement pairs of one table, in file order.
type Pairs = Arc<Vec<(String, String)>>;

/// The largest table file read.
const MAX_TABLE_BYTES: u64 = 1 << 20;

/// `replace` with CSV tables.
///
/// `replace(from,to)` and `replace(,,to)` work as in the generic renderer. `replace(name.csv)`
/// reads `name.csv` from the data folder and applies its pairs to the text one after the other,
/// in file order, every occurrence each. The file is UTF-8 with one pair per line:
/// `from,to`. Fields may be quoted with `"` (a doubled `""` is a quote) to hold commas; a line
/// with only `from` removes it; empty lines and lines that start with `#` are skipped. Tables
/// are cached and read again when the file changes. Only a plain file name is accepted: no
/// folders, so a template cannot read anything else.
pub struct ReplaceTables {
    dir: PathBuf,
    cache: Mutex<HashMap<String, (SystemTime, Pairs)>>,
}

impl ReplaceTables {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ReplaceTables {
            dir: dir.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn table(&self, name: &str) -> Result<Pairs, String> {
        if name.contains(['/', '\\', ':']) || name.starts_with('.') || name.contains("..") {
            return Err(format!("`{name}` must be a file name in the data folder"));
        }
        let path = self.dir.join(name);
        let missing = |e: std::io::Error| format!("table `{name}` cannot be read: {e}");
        let meta = fs::metadata(&path).map_err(missing)?;
        if !meta.is_file() || meta.len() > MAX_TABLE_BYTES {
            return Err(format!("table `{name}` is not a file of at most 1 MiB"));
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "table cache is unavailable".to_owned())?;
        if let Some((stamp, pairs)) = cache.get(name)
            && *stamp == modified
        {
            return Ok(Arc::clone(pairs));
        }
        let bytes = fs::read(&path).map_err(missing)?;
        let text = String::from_utf8_lossy(&bytes);
        let pairs: Pairs = Arc::new(parse_pairs(&text));
        cache.insert(name.to_owned(), (modified, Arc::clone(&pairs)));
        Ok(pairs)
    }
}

impl NamedRenderer for ReplaceTables {
    fn name(&self) -> &str {
        "replace"
    }

    fn render(
        &self,
        input: &Value,
        params: Option<&str>,
        _: &RenderContext,
    ) -> Result<Value, String> {
        let text = input.to_text(false);
        match replacement(params)? {
            Replacement::Literal(from, to) => Ok(Value::text(text.replace(from, to))),
            Replacement::Table(name) => {
                let mut out = text;
                for (from, to) in self.table(name)?.iter() {
                    out = out.replace(from.as_str(), to);
                }
                Ok(Value::text(out))
            }
        }
    }
}

/// The pairs of a table file; see [`ReplaceTables`] for the format.
fn parse_pairs(text: &str) -> Vec<(String, String)> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = split_fields(line).into_iter();
            let from = fields.next().filter(|from| !from.is_empty())?;
            Some((from, fields.next().unwrap_or_default()))
        })
        .collect()
}

fn split_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            other => field.push(other),
        }
    }
    fields.push(field);
    fields
}

// ---- movie set index -----------------------------------------------------------------------

/// `${movie;indexOfMovieSet}` and `${movie;indexOfMovieSetWithDummy}`: the position of a movie
/// in its movie set, counted from 0, or nothing when the movie is in no set.
///
/// The input is the movie. It needs an `id` and a `movieSet` whose `movies` (or, for the
/// `WithDummy` form, `moviesWithDummy`, which also lists the movies the set is missing) is the
/// set in play order, each entry with an `id`. The position is printed as text, so the first
/// movie prints `0` even where numeric zero prints empty.
struct MovieSetIndex {
    name: &'static str,
    list: &'static str,
}

impl NamedRenderer for MovieSetIndex {
    fn name(&self) -> &str {
        self.name
    }

    fn render(&self, input: &Value, _: Option<&str>, _: &RenderContext) -> Result<Value, String> {
        let position = (|| {
            let id = input.get("id")?.to_text(false);
            let movies = input.get("movieSet")?.get(self.list)?;
            movies
                .items()
                .iter()
                .position(|m| m.get("id").is_some_and(|other| other.to_text(false) == id))
        })();
        Ok(Value::text(
            position.map_or_else(String::new, |p| p.to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn engine(dir: Option<&Path>) -> Engine {
        let mut engine = Engine::standard();
        register_renamer(
            &mut engine,
            &RenamerRenderers {
                number_replacement: "#".to_owned(),
                data_dir: dir.map(Path::to_path_buf),
            },
        );
        engine
    }

    fn run(template: &str, token: Value) -> String {
        let model = Value::map([("t", token)]);
        engine(None).render_str(template, &model).unwrap()
    }

    #[test]
    fn first_folds_marks_and_replaces_digits() {
        assert_eq!(run("${t;first}", "the matrix".into()), "T");
        assert_eq!(run("${t;first}", "Amélie... Ünï".into()), "A");
        assert_eq!(run("${t;first}", "Ñandú".into()), "N");
        assert_eq!(run("${t;first}", "Øresund".into()), "O");
        assert_eq!(run("${t;first}", "ßeta".into()), "S");
        assert_eq!(run("${t;first}", "2001: A Space Odyssey".into()), "#");
        assert_eq!(run("${t;first}", "\"Quoted\" title".into()), "Q");
        assert_eq!(run("${t;first}", "?!".into()), "");
        assert_eq!(run("[${t;first}]", "".into()), "[]");
        assert_eq!(run("[${t;first}]", Value::Null), "[]");
        assert_eq!(run("${t;first}", 1999_i64.into()), "#");
        assert_eq!(run("${t;lower;first}", "ABC".into()), "A");
    }

    #[test]
    fn first_uses_the_configured_number_replacement() {
        let mut engine = Engine::standard();
        register_renamer(
            &mut engine,
            &RenamerRenderers {
                number_replacement: "0-9".to_owned(),
                data_dir: None,
            },
        );
        let model = Value::map([("t", Value::text("10 Things"))]);
        assert_eq!(engine.render_str("${t;first}", &model).unwrap(), "0-9");
    }

    #[test]
    fn replace_reads_a_table_from_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("fix.csv"),
            "\u{feff}# comment\nColour,Color\n\"a, b\",\"x \"\"y\"\"\"\nThe \nUnused\n\n",
        )
        .unwrap();
        let engine = engine(Some(dir.path()));
        let model = Value::map([("t", Value::text("The Colour of a, b"))]);
        assert_eq!(
            engine.render_str("${t;replace(fix.csv)}", &model).unwrap(),
            "Color of x \"y\""
        );
        // The literal forms still work.
        assert_eq!(
            engine.render_str("${t;replace(The,A)}", &model).unwrap(),
            "A Colour of a, b"
        );
    }

    #[test]
    fn replace_table_is_reread_when_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.csv");
        fs::write(&path, "a,b\n").unwrap();
        let engine = engine(Some(dir.path()));
        let model = Value::map([("t", Value::text("aaa"))]);
        assert_eq!(
            engine.render_str("${t;replace(t.csv)}", &model).unwrap(),
            "bbb"
        );
        fs::write(&path, "a,cc\n").unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(
            engine.render_str("${t;replace(t.csv)}", &model).unwrap(),
            "cccccc"
        );
    }

    #[test]
    fn replace_table_refuses_paths_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("t.csv"), "a,b\n").unwrap();
        let engine = engine(Some(dir.path()));
        let model = Value::map([("t", Value::text("a"))]);
        for params in [
            "../t.csv",
            "sub/t.csv",
            "C:t.csv",
            ".hidden.csv",
            "missing.csv",
        ] {
            let template = format!("${{t;replace({params})}}");
            assert!(engine.render_str(&template, &model).is_err(), "{params}");
        }
        // Without a data folder a table is an error, never a read.
        assert!(
            Engine::standard()
                .render_str("${t;replace(t.csv)}", &model)
                .is_err()
        );
    }

    fn movie(id: &str, set: Option<(&[&str], &[&str])>) -> Value {
        let list =
            |ids: &[&str]| Value::list(ids.iter().map(|i| Value::map([("id", Value::text(i))])));
        let mut fields = vec![("id", Value::text(id))];
        if let Some((movies, with_dummy)) = set {
            fields.push((
                "movieSet",
                Value::map([
                    ("movies", list(movies)),
                    ("moviesWithDummy", list(with_dummy)),
                ]),
            ));
        }
        Value::map([("movie", Value::map(fields))])
    }

    #[test]
    fn movie_set_index_counts_from_zero() {
        let engine = engine(None);
        let set: (&[&str], &[&str]) = (&["a", "c"], &["a", "b", "c"]);
        let plain = |id| {
            engine
                .render_str("${movie;indexOfMovieSet}", &movie(id, Some(set)))
                .unwrap()
        };
        let dummy = |id| {
            engine
                .render_str("${movie;indexOfMovieSetWithDummy}", &movie(id, Some(set)))
                .unwrap()
        };
        assert_eq!((plain("a"), plain("c")), ("0".to_owned(), "1".to_owned()));
        assert_eq!((dummy("a"), dummy("c")), ("0".to_owned(), "2".to_owned()));
        // A movie that is in no set, or not in the list, prints nothing.
        assert_eq!(
            engine
                .render_str("[${movie;indexOfMovieSet}]", &movie("a", None))
                .unwrap(),
            "[]"
        );
        assert_eq!(
            engine
                .render_str("[${movie;indexOfMovieSet}]", &movie("z", Some(set)))
                .unwrap(),
            "[]"
        );
    }

    #[test]
    fn movie_set_index_prints_zero_even_when_zero_is_empty() {
        let engine = engine(None).zero_is_empty(true);
        let model = movie("a", Some((&["a"], &["a"])));
        assert_eq!(
            engine
                .render_str("${movie;indexOfMovieSet}", &model)
                .unwrap(),
            "0"
        );
    }
}
