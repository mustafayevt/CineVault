//! Language and flags of external audio and subtitle files, read from their file names.
//!
//! A subtitle or audio file next to a video (`Heat (1995).tr.forced.srt`) has no container to ask,
//! so everything comes from the name. The rules:
//!
//! 1. The extension and the video's base name are removed (case-insensitive). A name that does not
//!    start with the video's base name is read after its first dot, so a title word is never
//!    taken for a language.
//! 2. The rest is split into chunks at dots, spaces, brackets and commas, and the chunks into
//!    words at `-` and `_`.
//! 3. **Flags:** `forced` sets [`StreamFlag::Forced`]; `sdh` and `hearing impaired` set
//!    [`StreamFlag::HearingImpaired`]; `default` sets [`StreamFlag::Default`]; `commentary` sets
//!    [`StreamFlag::Comment`] and `dub` / `dubbed` set [`StreamFlag::Dub`]. `cc` and `hi` are
//!    ambiguous: next to a language they are the hearing-impaired flag; alone, `cc` is that flag
//!    and `hi` is Hindi.
//! 4. **Language:** a locale chunk (`pt-BR`, `es_MX`, `zh-Hans`) is tried first, then each word
//!    in turn as an ISO 639-1 code, an ISO 639-2 code or an English or native name. The result is
//!    an ISO 639-1 code (`tr`, `pt-BR`), or the ISO 639-2 code for the few languages without one.
//! 5. **Title:** the words after the language, minus the flag words and noise such as `subs`,
//!    joined by a space (`Heat.en.Director's Commentary.srt` has the title `Director's Commentary`).
//!    Without a language the title is empty.
//!
//! A `.sub` file is a VobSub image track; its language lives in the `.idx` file beside it, on a
//! line `id: en, index: 0` ([`idx_language`], [`external_subtitle`]).

mod languages;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::Path;

use cv_domain::{MediaFile, StreamFlag, SubtitleStream};

/// The canonical code of a language code or name (`eng`, `ger`, `English` give `en`; a language
/// without a two-letter code keeps its three-letter one). `None` when it is not a known language.
pub(crate) fn canonical_language(token: &str) -> Option<&'static str> {
    languages::canonical_code(token)
}

/// What a file name says about an external audio or subtitle stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalStreamInfo {
    /// ISO 639 code, with an upper-case region when the name gave one (`pt-BR`); empty when the
    /// name has no language.
    pub language: String,
    pub title: String,
    pub flags: BTreeSet<StreamFlag>,
}

/// Reads the language, title and flags of an external audio or subtitle file from its name.
///
/// `video_basename` is the video's file name without extension (`Heat (1995)`); `file_name` is the
/// audio or subtitle file's name with its extension.
#[must_use]
pub fn detect_external_stream(video_basename: &str, file_name: &str) -> ExternalStreamInfo {
    let stem = strip_extension(file_name);
    let rest = match strip_prefix_ignore_case(stem, video_basename) {
        Some(rest) if !video_basename.is_empty() => rest,
        // Not named after the video: skip the title part, which is everything before the first dot.
        _ => stem.split_once('.').map_or("", |(_, tail)| tail),
    };
    parse_remainder(rest)
}

/// Like [`detect_external_stream`] for a file on disk. For a `.sub` file with a `.idx` file of
/// the same name beside it, the language of the first track in the index wins.
#[must_use]
pub fn external_subtitle(video_basename: &str, subtitle: &Path) -> ExternalStreamInfo {
    let file_name = subtitle
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut info = detect_external_stream(video_basename, &file_name);
    let is_sub = subtitle
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("sub"));
    if is_sub
        && let Ok(index) = std::fs::read(subtitle.with_extension("idx"))
        && let Some(language) = idx_language(&String::from_utf8_lossy(&index))
    {
        info.language = language;
    }
    info
}

/// The language of the first track in a VobSub `.idx` file: the code on its first
/// `id: <code>,` line, as a canonical code. `None` when there is none or it is not a language
/// (`id: , index: 0` means unknown).
#[must_use]
pub fn idx_language(idx: &str) -> Option<String> {
    let id = idx.lines().find_map(|line| {
        let value = line.trim_start().strip_prefix("id:")?;
        Some(value.split(',').next().unwrap_or_default().trim())
    })?;
    normalize_language(id)
}

/// The stream of an external subtitle file as the library shows it: the stored stream (what
/// MediaInfo read) with what the file name says filling the gaps. Nothing reads the language of
/// a text subtitle (`.srt`, `.ass`) from inside it, and without MediaInfo nothing is stored at
/// all, so the language, the forced and SDH marks and the title often live only in the name
/// (`Heat (1995).deu.forced.srt`). `video_basename` is the video's file name without extension.
#[must_use]
pub fn subtitle_file_stream(file: &MediaFile, video_basename: &str) -> SubtitleStream {
    let named = detect_external_stream(video_basename, &file.filename);
    let mut stream = file.subtitles.first().cloned().unwrap_or_default();
    if stream.language.is_empty() {
        stream.language = named.language;
    }
    if stream.title.is_empty() {
        stream.title = named.title;
    }
    stream.flags.extend(named.flags);
    stream
}

/// The canonical form of a language code or name: `eng`, `English` and `en` give `en`;
/// `pt_br` gives `pt-BR`. `None` for anything that is not a known language.
#[must_use]
pub fn normalize_language(text: &str) -> Option<String> {
    let text = text.trim();
    locale(text)
        .or_else(|| languages::regional_alias(text).map(str::to_owned))
        .or_else(|| languages::canonical_code(text).map(str::to_owned))
}

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// A flag word that is dropped from the title.
    Flag(StreamFlag),
    /// A flag word that stays in the title (`commentary`).
    Kept(StreamFlag),
    /// `hearing` or `impaired`, until they are paired up.
    Hearing,
    Impaired,
    /// `cc`: closed captions, the hearing-impaired flag.
    Cc,
    /// `hi`: Hindi, or hearing-impaired next to another language.
    Hi,
    /// `subs`, `subtitles` and the like.
    Noise,
    Language(String),
    Text,
}

struct Word<'a> {
    text: &'a str,
    kind: Kind,
}

fn parse_remainder(rest: &str) -> ExternalStreamInfo {
    let mut words: Vec<Word<'_>> = Vec::new();
    let mut locale_at = None;
    for chunk in rest.split(is_chunk_separator).filter(|c| !c.is_empty()) {
        if let Some(code) = locale(chunk) {
            locale_at.get_or_insert(words.len());
            words.push(Word {
                text: chunk,
                kind: Kind::Language(code),
            });
            continue;
        }
        for piece in chunk.split(['-', '_']).filter(|p| !p.is_empty()) {
            words.push(Word {
                text: piece,
                kind: classify(piece),
            });
        }
    }
    pair_hearing_impaired(&mut words);

    // The language: the first locale chunk, else the first plain language word, else the
    // ambiguous `hi` as Hindi.
    let language_at = locale_at
        .or_else(|| {
            words
                .iter()
                .position(|w| matches!(w.kind, Kind::Language(_)))
        })
        .or_else(|| words.iter().position(|w| w.kind == Kind::Hi));
    let language = language_at.map(|at| match &words[at].kind {
        Kind::Language(code) => code.clone(),
        _ => "hi".to_owned(),
    });

    let mut flags = BTreeSet::new();
    let mut title: Vec<&str> = Vec::new();
    for (at, word) in words.iter().enumerate() {
        let after_language = language_at.is_some_and(|l| at > l);
        match &word.kind {
            Kind::Flag(flag) => {
                flags.insert(*flag);
            }
            Kind::Kept(flag) => {
                flags.insert(*flag);
                if after_language {
                    title.push(word.text);
                }
            }
            Kind::Cc => {
                flags.insert(StreamFlag::HearingImpaired);
            }
            Kind::Hi => {
                // The `hi` taken as the language is not also a flag.
                if language_at != Some(at) {
                    flags.insert(StreamFlag::HearingImpaired);
                }
            }
            Kind::Language(_) | Kind::Text | Kind::Hearing | Kind::Impaired => {
                if after_language {
                    title.push(word.text);
                }
            }
            Kind::Noise => {}
        }
    }

    ExternalStreamInfo {
        language: language.unwrap_or_default(),
        title: if language_at.is_some() {
            title.join(" ")
        } else {
            String::new()
        },
        flags,
    }
}

/// `hearing impaired` as two words is one flag; a lone `hearing` or `impaired` is plain text.
fn pair_hearing_impaired(words: &mut [Word<'_>]) {
    for at in 1..words.len() {
        if words[at - 1].kind == Kind::Hearing && words[at].kind == Kind::Impaired {
            words[at - 1].kind = Kind::Flag(StreamFlag::HearingImpaired);
            words[at].kind = Kind::Noise;
        }
    }
    for word in words {
        if matches!(word.kind, Kind::Hearing | Kind::Impaired) {
            word.kind = Kind::Text;
        }
    }
}

fn classify(piece: &str) -> Kind {
    match piece.to_lowercase().as_str() {
        "forced" | "force" => Kind::Flag(StreamFlag::Forced),
        "sdh" | "hearingimpaired" => Kind::Flag(StreamFlag::HearingImpaired),
        "default" => Kind::Flag(StreamFlag::Default),
        "dub" | "dubbed" => Kind::Flag(StreamFlag::Dub),
        "commentary" | "comment" => Kind::Kept(StreamFlag::Comment),
        "hearing" => Kind::Hearing,
        "impaired" => Kind::Impaired,
        "cc" => Kind::Cc,
        "hi" => Kind::Hi,
        "sub" | "subs" | "subtitle" | "subtitles" | "caption" | "captions" => Kind::Noise,
        _ => languages::regional_alias(piece)
            .or_else(|| languages::canonical_code(piece))
            .map_or(Kind::Text, |code| Kind::Language(code.to_owned())),
    }
}

/// A locale chunk: a known language, then an optional script and an optional region, joined by
/// `-` or `_` (`pt-BR`, `zh_Hans`, `zh-Hant-TW`). Gives `pt-BR`, `zh-Hans` ... A lone language
/// is not a locale.
fn locale(chunk: &str) -> Option<String> {
    let mut parts = chunk.split(['-', '_']);
    let mut code = languages::canonical_code(parts.next()?)?.to_owned();
    let mut rest = parts.peekable();
    rest.peek()?;
    if let Some(script) = rest.next_if(|p| languages::is_script(p)) {
        code.push('-');
        code.push_str(&script[..1].to_uppercase());
        code.push_str(&script[1..].to_lowercase());
    }
    if let Some(region) = rest.next_if(|p| languages::is_region(p)) {
        code.push('-');
        code.push_str(&region.to_uppercase());
    }
    // Anything left over means this is not a locale (`en-forced`), and a bare language
    // (nothing consumed) is not one either.
    (rest.next().is_none() && code.contains('-')).then_some(code)
}

fn is_chunk_separator(c: char) -> bool {
    matches!(c, '.' | ' ' | '[' | ']' | '(' | ')' | '{' | '}' | ',' | '+')
}

/// Removes a trailing extension of one to eight letters or digits.
fn strip_extension(file_name: &str) -> &str {
    match file_name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && (1..=8).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => file_name,
    }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let mut chars = text.char_indices();
    for expected in prefix.chars() {
        let (_, actual) = chars.next()?;
        if !actual.to_lowercase().eq(expected.to_lowercase()) {
            return None;
        }
    }
    Some(chars.next().map_or("", |(at, _)| &text[at..]))
}
