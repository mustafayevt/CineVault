//! What the template editor asks of the template engine: diagnostics for a template as typed,
//! and the token list behind the `$` autocomplete.

use cv_template::tokens::{
    RenamerOptions, movie_aliases, movie_validator, renamer_engine, tv_aliases, tv_validator,
};
use cv_template::validate::{Diagnostic, Severity, ValidationReport};

use super::model::PatternField;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingSeverity {
    /// The template renders but probably not as intended.
    Warning,
    /// The template cannot be used.
    Error,
}

/// One finding about a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: FindingSeverity,
    pub message: String,
    /// Where the marked text starts and how long it is, in UTF-16 code units of the template as
    /// typed (what a web view indexes by).
    pub offset: u32,
    pub len: u32,
    /// Replacements for the marked text, best first.
    pub suggestions: Vec<String>,
}

/// A token the editor can offer: the alias a template writes and the model path it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInfo {
    pub name: String,
    pub target: String,
}

/// The tokens of the movie templates (`tv` false) or the TV templates.
pub fn token_catalogue(tv: bool) -> Vec<TokenInfo> {
    let aliases = if tv { tv_aliases() } else { movie_aliases() };
    aliases
        .entries()
        .iter()
        .map(|(name, target)| TokenInfo {
            name: (*name).to_owned(),
            target: (*target).to_owned(),
        })
        .collect()
}

/// Checks one template as typed. An empty template is an error: it would name everything the
/// same. For the episode file template the TV rules apply too (one episode token, the season
/// before the episode).
pub fn check_pattern(field: PatternField, pattern: &str) -> Vec<Finding> {
    if pattern.trim().is_empty() {
        return vec![Finding {
            severity: FindingSeverity::Error,
            message: "The template is empty, so every item would get the same name.".to_owned(),
            offset: 0,
            len: 0,
            suggestions: Vec::new(),
        }];
    }
    let engine = renamer_engine(&RenamerOptions::default());
    let report: ValidationReport = if field.is_tv() {
        let validator = tv_validator(&engine);
        if field == PatternField::EpisodeFile {
            validator.check_tv_patterns(None, pattern).file
        } else {
            validator.validate(pattern)
        }
    } else {
        movie_validator(&engine).validate(pattern)
    };
    report
        .diagnostics
        .iter()
        .map(|d| finding(pattern, d))
        .collect()
}

fn finding(pattern: &str, d: &Diagnostic) -> Finding {
    let start = floor_boundary(pattern, d.offset);
    let end = floor_boundary(pattern, d.offset.saturating_add(d.len));
    let offset = utf16_len(&pattern[..start]);
    let len = utf16_len(&pattern[start..end]);
    Finding {
        severity: match d.severity {
            Severity::Warning => FindingSeverity::Warning,
            Severity::Error => FindingSeverity::Error,
        },
        message: sentence(&d.message),
        offset,
        len,
        suggestions: d.suggestions.clone(),
    }
}

/// The validator's terse note as a sentence for the editor: "unknown token `titel`; did you mean
/// `title`?" reads "There is no token “titel”. Did you mean “title”?".
fn sentence(message: &str) -> String {
    let mut text = message.replace("; did you mean ", ". Did you mean ");
    for (terse, plain) in [
        ("unknown token ", "there is no token "),
        ("unknown renderer ", "there is no format "),
        ("the file pattern ", "the episode file template "),
        ("the season pattern ", "the season folder template "),
    ] {
        if let Some(rest) = text.strip_prefix(terse) {
            text = format!("{plain}{rest}");
        }
    }
    // Code quotes become the quotes of the interface.
    let mut quoted = String::with_capacity(text.len());
    let mut open = true;
    for c in text.chars() {
        if c == '`' {
            quoted.push(if open { '\u{201C}' } else { '\u{201D}' });
            open = !open;
        } else {
            quoted.push(c);
        }
    }
    let mut chars = quoted.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => return String::new(),
    };
    if !out.ends_with(['.', '?', '!']) {
        out.push('.');
    }
    out
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}
