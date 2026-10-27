//! Template validation for the editors: syntax errors with positions, unknown tokens with
//! suggestions, unknown renderers, and the rules for TV naming patterns.
//!
//! A [`Validator`] knows which names a template may use: the alias map (`${title}`), the model
//! roots and their fields (`${movie.title}`, `${movies}`), the renderers of an [`Engine`] and the
//! loop variables a `foreach` brings into scope. [`Validator::validate`] parses the template as
//! the user wrote it (aliases are not rewritten first), so every position points into the
//! user's text:
//!
//! ```
//! use cv_template::Engine;
//! use cv_template::tokens::movie_validator;
//!
//! let validator = movie_validator(&Engine::standard());
//! let report = validator.validate("${titel} (${year})");
//! let unknown = &report.diagnostics[0];
//! assert_eq!((unknown.offset, unknown.len), (2, 5));
//! assert_eq!(unknown.suggestions[0], "title");
//! ```
//!
//! Validation never renders anything and never reads files, so it is cheap enough to run on
//! every keystroke. A template with a syntax error reports that one error, the first, and no
//! token checks, because there is no tree to check.
//!
//! # TV patterns
//!
//! [`Validator::check_tv_patterns`] applies the rules for episode file names, on top of the
//! normal checks. A pattern pair is *recommended* when
//!
//! - the file pattern has exactly one episode token (`episodeNr`, `episodeNr2` and the aired and
//!   DVD variants);
//! - it has at most one title token (`title`, `originalTitle`, `englishTitle`);
//! - it has at most one season token (`seasonNr`, `seasonNr2` and the aired and DVD variants),
//!   and so does the season-folder pattern;
//! - there is a season token in the file pattern or in the season-folder pattern;
//! - the season comes before the episode (a season in the folder always does);
//! - the title is not between the season and the episode.
//!
//! A file pattern with `originalFilename` or `originalBasename` is never checked: it keeps the
//! name the file already has. A broken rule is a warning, not an error: the template still
//! renders.

use std::collections::HashSet;

use crate::eval::Engine;
use crate::parse::{Call, Node, ParseOptions, Path, RegexpNode, Span, SyntaxError, Template};
use crate::tokens::AliasMap;
use crate::value::Value;

/// How serious a [`Diagnostic`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The template does not mean what the user probably wants, but it renders.
    Warning,
    /// The template cannot be used as written.
    Error,
}

/// What a [`Diagnostic`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The template does not parse.
    Syntax,
    /// A token that is not an alias, a model root or a loop variable.
    UnknownToken,
    /// A model path whose field the model does not have (`movie.titel`).
    UnknownField,
    /// A renderer the engine does not have (`${title;uper}`).
    UnknownRenderer,
    /// A `@regexp` replacement refers to a group the pattern does not have.
    RegexpGroup,
    /// A TV file pattern without exactly one episode token.
    TvEpisodeToken,
    /// A TV pattern with more than one season token, or no season token at all.
    TvSeasonToken,
    /// A TV file pattern with more than one title token.
    TvTitleToken,
    /// The season does not come before the episode, or the title is between them.
    TvTokenOrder,
}

/// One finding, with the text it applies to.
///
/// `offset` and `len` are bytes of the template as written; `line` and `column` count from 1
/// (columns in characters) and describe the start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub offset: usize,
    pub len: usize,
    pub line: usize,
    pub column: usize,
    /// Replacements for the marked text, best first. Empty when there is nothing to suggest.
    pub suggestions: Vec<String>,
}

/// The findings for one template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// In template order, except that a TV rule about the whole pattern comes last.
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// Whether there is nothing to report.
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Whether the template cannot be used as written.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }
}

/// The result of [`Validator::check_tv_patterns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvPatternReport {
    /// The findings for the season-folder pattern, when one was given.
    pub season: Option<ValidationReport>,
    /// The findings for the episode file pattern.
    pub file: ValidationReport,
}

impl TvPatternReport {
    /// Whether the pair passes every rule and has no errors.
    pub fn is_recommended(&self) -> bool {
        self.file.is_clean() && self.season.as_ref().is_none_or(ValidationReport::is_clean)
    }
}

/// A model root such as `movie` and the fields a template may read from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoot {
    pub name: String,
    /// The field names, or `None` when any field is accepted.
    pub fields: Option<Vec<String>>,
}

impl ModelRoot {
    /// A root that accepts any field.
    pub fn open(name: impl Into<String>) -> ModelRoot {
        ModelRoot {
            name: name.into(),
            fields: None,
        }
    }

    /// A root with a fixed set of fields.
    pub fn with_fields<S: Into<String>>(
        name: impl Into<String>,
        fields: impl IntoIterator<Item = S>,
    ) -> ModelRoot {
        ModelRoot {
            name: name.into(),
            fields: Some(fields.into_iter().map(Into::into).collect()),
        }
    }

    /// A root whose fields are the keys of `value`; a value that is not a map accepts any field.
    pub fn from_value(name: impl Into<String>, value: Option<&Value>) -> ModelRoot {
        match value {
            Some(Value::Map(map)) => ModelRoot::with_fields(name, map.keys().cloned()),
            _ => ModelRoot::open(name),
        }
    }
}

/// Checks templates against the names a model offers.
#[derive(Debug, Clone)]
pub struct Validator {
    renderers: Vec<String>,
    aliases: Option<AliasMap>,
    roots: Vec<ModelRoot>,
    options: ParseOptions,
}

impl Validator {
    /// A validator for export-style templates: the model `roots` and the renderers of `engine`.
    pub fn new(engine: &Engine, roots: impl IntoIterator<Item = ModelRoot>) -> Validator {
        Validator {
            renderers: engine
                .renderer_names()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            aliases: None,
            roots: roots.into_iter().collect(),
            options: ParseOptions::default(),
        }
    }

    /// Accepts the short names of `aliases` as tokens.
    pub fn with_aliases(mut self, aliases: AliasMap) -> Validator {
        self.aliases = Some(aliases);
        self
    }

    /// Parses templates with `options`; the renamer reads typed backslashes literally.
    pub fn with_parse_options(mut self, options: ParseOptions) -> Validator {
        self.options = options;
        self
    }

    /// Checks one template: syntax, tokens, renderers and `@regexp` groups.
    pub fn validate(&self, source: &str) -> ValidationReport {
        self.check(source).0
    }

    fn check(&self, source: &str) -> (ValidationReport, Option<Template>) {
        let template = match Template::parse_with(source, self.options) {
            Ok(template) => template,
            Err(error) => {
                return (
                    ValidationReport {
                        diagnostics: vec![syntax_diagnostic(source, &error)],
                    },
                    None,
                );
            }
        };
        let mut diagnostics = Vec::new();
        let mut scope = Vec::new();
        walk(
            &template.nodes,
            &mut scope,
            &mut |visit, scope| match visit {
                Visit::Token { path, chain } => {
                    self.check_path(source, path, scope, &mut diagnostics);
                    self.check_chain(source, chain, &mut diagnostics);
                }
                Visit::Regexp(node) => check_regexp(source, node, &mut diagnostics),
            },
        );
        (ValidationReport { diagnostics }, Some(template))
    }

    fn check_path(&self, source: &str, path: &Path, scope: &[&str], out: &mut Vec<Diagnostic>) {
        let Some(first) = path.segments.first() else {
            return;
        };
        if in_scope(&first.name, scope)
            || self
                .aliases
                .as_ref()
                .is_some_and(|aliases| aliases.contains(&first.name))
        {
            return;
        }
        if let Some(root) = self.roots.iter().find(|root| root.name == first.name) {
            if let (Some(fields), Some(second)) = (&root.fields, path.segments.get(1))
                && !fields.contains(&second.name)
            {
                let suggestions = suggest(&second.name, fields.iter().map(String::as_str));
                out.push(diagnostic(
                    source,
                    Severity::Error,
                    DiagnosticKind::UnknownField,
                    format!("`{}` has no field `{}`", root.name, second.name),
                    second.span,
                    suggestions,
                ));
            }
            return;
        }
        let mut candidates: Vec<String> = scope.iter().map(|name| (*name).to_owned()).collect();
        if let Some(aliases) = &self.aliases {
            candidates.extend(aliases.names().map(str::to_owned));
        }
        candidates.extend(self.roots.iter().map(|root| root.name.clone()));
        let suggestions = suggest(&first.name, candidates);
        out.push(diagnostic(
            source,
            Severity::Error,
            DiagnosticKind::UnknownToken,
            format!("unknown token `{}`", first.name),
            first.span,
            suggestions,
        ));
    }

    fn check_chain(&self, source: &str, chain: &[Call], out: &mut Vec<Diagnostic>) {
        for call in chain {
            if self.renderers.contains(&call.name) {
                continue;
            }
            let suggestions = suggest(&call.name, self.renderers.iter().map(String::as_str));
            out.push(diagnostic(
                source,
                Severity::Error,
                DiagnosticKind::UnknownRenderer,
                format!("unknown renderer `{}`", call.name),
                call.span,
                suggestions,
            ));
        }
    }

    /// Checks an episode file pattern and, when there is one, the season-folder pattern, and
    /// applies the TV rules described in the [module documentation](self).
    ///
    /// The validator needs the TV alias map (or full `episode.*` paths) for the rules to find
    /// the tokens.
    pub fn check_tv_patterns(
        &self,
        season_pattern: Option<&str>,
        file_pattern: &str,
    ) -> TvPatternReport {
        let (mut file, file_template) = self.check(file_pattern);
        let season_checked = season_pattern.map(|pattern| (pattern, self.check(pattern)));

        let file_tokens = file_template
            .as_ref()
            .map(|template| self.tv_tokens(template));
        let season_tokens = season_checked
            .as_ref()
            .map(|(_, (_, template))| template.as_ref().map(|t| self.tv_tokens(t)));
        let (mut season_report, season_pattern_text) = match season_checked {
            Some((text, (report, _))) => (Some(report), Some(text)),
            None => (None, None),
        };

        let Some(tokens) = file_tokens else {
            return TvPatternReport {
                season: season_report,
                file,
            };
        };
        // The file keeps the name it has: nothing to check.
        if tokens.keeps_original_name {
            return TvPatternReport {
                season: season_report,
                file,
            };
        }

        let whole = Span {
            offset: 0,
            len: file_pattern.len(),
        };
        fn warn(
            report: &mut ValidationReport,
            source: &str,
            kind: DiagnosticKind,
            message: String,
            span: Span,
        ) {
            report.diagnostics.push(diagnostic(
                source,
                Severity::Warning,
                kind,
                message,
                span,
                Vec::new(),
            ));
        }

        match tokens.episode.as_slice() {
            [_] => {}
            [] => warn(
                &mut file,
                file_pattern,
                DiagnosticKind::TvEpisodeToken,
                "the file pattern needs an episode token such as `${episodeNr2}`".to_owned(),
                whole,
            ),
            [_, extra, ..] => warn(
                &mut file,
                file_pattern,
                DiagnosticKind::TvEpisodeToken,
                "the file pattern has more than one episode token".to_owned(),
                *extra,
            ),
        }
        if let [_, extra, ..] = tokens.title.as_slice() {
            warn(
                &mut file,
                file_pattern,
                DiagnosticKind::TvTitleToken,
                "the file pattern has more than one title token".to_owned(),
                *extra,
            );
        }
        if let [_, extra, ..] = tokens.season.as_slice() {
            warn(
                &mut file,
                file_pattern,
                DiagnosticKind::TvSeasonToken,
                "the file pattern has more than one season token".to_owned(),
                *extra,
            );
        }
        // A season-folder pattern that does not parse says nothing about its tokens.
        let season_folder = match &season_tokens {
            Some(Some(found)) => Some(found.season.len()),
            Some(None) => None,
            None => Some(0),
        };
        if let (Some(report), Some(text), Some(Some(found))) =
            (season_report.as_mut(), season_pattern_text, &season_tokens)
            && let [_, extra, ..] = found.season.as_slice()
        {
            warn(
                report,
                text,
                DiagnosticKind::TvSeasonToken,
                "the season pattern has more than one season token".to_owned(),
                *extra,
            );
        }
        if tokens.season.is_empty() && season_folder == Some(0) {
            warn(
                &mut file,
                file_pattern,
                DiagnosticKind::TvSeasonToken,
                "neither the file pattern nor the season pattern has a season token such as `${seasonNr2}`"
                    .to_owned(),
                whole,
            );
        }
        if let (Some(season), Some(episode)) = (tokens.season.first(), tokens.episode.first())
            && season.offset > episode.offset
        {
            warn(
                &mut file,
                file_pattern,
                DiagnosticKind::TvTokenOrder,
                "the season must come before the episode".to_owned(),
                *season,
            );
        }
        if let (Some(season), Some(episode)) = (tokens.season.first(), tokens.episode.first()) {
            let (low, high) = (
                season.offset.min(episode.offset),
                season.offset.max(episode.offset),
            );
            for title in &tokens.title {
                if title.offset > low && title.offset < high {
                    warn(
                        &mut file,
                        file_pattern,
                        DiagnosticKind::TvTokenOrder,
                        "the title must not sit between the season and the episode".to_owned(),
                        *title,
                    );
                }
            }
        }

        TvPatternReport {
            season: season_report,
            file,
        }
    }

    /// Sorts the tokens of a TV pattern into the groups the rules count.
    fn tv_tokens(&self, template: &Template) -> TvTokens {
        let mut tokens = TvTokens::default();
        let mut scope = Vec::new();
        walk(&template.nodes, &mut scope, &mut |visit, scope| {
            let Visit::Token { path, .. } = visit else {
                return;
            };
            let Some(canonical) = self.canonical(path, scope) else {
                return;
            };
            match canonical.as_str() {
                "episode.episode" | "episode.airedEpisode" | "episode.dvdEpisode" => {
                    tokens.episode.push(path.span);
                }
                "episode.season" | "episode.airedSeason" | "episode.dvdSeason" => {
                    tokens.season.push(path.span);
                }
                "episode.title" | "episode.originalTitle" | "episode.englishTitle" => {
                    tokens.title.push(path.span);
                }
                "episode.originalFilename" | "episode.originalBasename" => {
                    tokens.keeps_original_name = true;
                }
                _ => {}
            }
        });
        tokens
    }

    /// The model path a token stands for: the alias target without its renderer, or the first
    /// two segments of a full path.
    fn canonical(&self, path: &Path, scope: &[&str]) -> Option<String> {
        let first = path.segments.first()?;
        if in_scope(&first.name, scope) {
            return None;
        }
        if let Some(target) = self
            .aliases
            .as_ref()
            .and_then(|aliases| aliases.target(&first.name))
        {
            return target.split(';').next().map(str::to_owned);
        }
        let second = path.segments.get(1)?;
        Some(format!("{}.{}", first.name, second.name))
    }
}

#[derive(Debug, Default)]
struct TvTokens {
    episode: Vec<Span>,
    season: Vec<Span>,
    title: Vec<Span>,
    keeps_original_name: bool,
}

/// Something the walker found in a template.
enum Visit<'a> {
    /// A token read from the model, with the renderers applied to it.
    Token {
        path: &'a Path,
        chain: &'a [Call],
    },
    Regexp(&'a RegexpNode),
}

/// Visits every token of `nodes` in template order with the `foreach` variables in scope.
fn walk<'a>(
    nodes: &'a [Node],
    scope: &mut Vec<&'a str>,
    visit: &mut dyn FnMut(Visit<'a>, &[&'a str]),
) {
    for node in nodes {
        match node {
            Node::Text(_) => {}
            Node::Expr(node) => visit(
                Visit::Token {
                    path: &node.expr.path,
                    chain: &node.expr.chain,
                },
                scope,
            ),
            Node::If(node) => {
                visit(
                    Visit::Token {
                        path: &node.path,
                        chain: &[],
                    },
                    scope,
                );
                walk(&node.then, scope, visit);
                walk(&node.otherwise, scope, visit);
            }
            Node::Foreach(node) => {
                visit(
                    Visit::Token {
                        path: &node.list.path,
                        chain: &node.list.chain,
                    },
                    scope,
                );
                scope.push(&node.var);
                walk(&node.body, scope, visit);
                scope.pop();
            }
            Node::Regexp(node) => {
                visit(
                    Visit::Token {
                        path: &node.token.path,
                        chain: &node.token.chain,
                    },
                    scope,
                );
                visit(Visit::Regexp(node), scope);
            }
        }
    }
}

/// Whether `name` is a `foreach` variable in scope or one of its helpers (`index_x`,
/// `first_x`, `last_x`).
fn in_scope(name: &str, scope: &[&str]) -> bool {
    scope.iter().any(|var| {
        name == *var
            || ["index_", "first_", "last_"]
                .iter()
                .any(|prefix| name.strip_prefix(prefix) == Some(*var))
    })
}

/// A `@regexp` replacement that names a group the pattern does not have prints nothing for it.
fn check_regexp(source: &str, node: &RegexpNode, out: &mut Vec<Diagnostic>) {
    let groups = node.regex.captures_len() - 1;
    let mut chars = node.destination.chars().peekable();
    let mut worst = 0usize;
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek().is_some_and(char::is_ascii_digit) {
            let mut number = 0usize;
            while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                number = number.saturating_mul(10).saturating_add(d as usize);
                chars.next();
            }
            worst = worst.max(number);
        }
    }
    if worst > groups {
        out.push(diagnostic(
            source,
            Severity::Warning,
            DiagnosticKind::RegexpGroup,
            format!(
                "the replacement uses `${worst}`, but the pattern has {groups} group{}",
                if groups == 1 { "" } else { "s" }
            ),
            node.span,
            Vec::new(),
        ));
    }
}

fn syntax_diagnostic(source: &str, error: &SyntaxError) -> Diagnostic {
    let mut found = diagnostic(
        source,
        Severity::Error,
        DiagnosticKind::Syntax,
        error.message.clone(),
        Span {
            offset: error.offset,
            len: error.len,
        },
        Vec::new(),
    );
    // The parser's own line and column agree with the ones computed from the offset.
    found.line = error.line;
    found.column = error.column;
    found
}

fn diagnostic(
    source: &str,
    severity: Severity,
    kind: DiagnosticKind,
    message: String,
    span: Span,
    suggestions: Vec<String>,
) -> Diagnostic {
    let offset = span.offset.min(source.len());
    let before = &source[..floor_boundary(source, offset)];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let message = match suggestions.first() {
        Some(best) => format!("{message}; did you mean `{best}`?"),
        None => message,
    };
    Diagnostic {
        severity,
        kind,
        message,
        offset: span.offset,
        len: span.len.max(1),
        line,
        column: before[line_start..].chars().count() + 1,
        suggestions,
    }
}

fn floor_boundary(text: &str, mut index: usize) -> usize {
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// The most similar of `candidates` to `word`, best first, at most three. Similarity is the
/// edit distance (a swap of two neighbouring letters counts as one edit) ignoring case; a name
/// that starts with what was typed counts as one edit away.
pub fn suggest<S: AsRef<str>>(word: &str, candidates: impl IntoIterator<Item = S>) -> Vec<String> {
    let typed: Vec<char> = word.to_lowercase().chars().collect();
    let allowed = match typed.len() {
        0..=2 => 0,
        3..=4 => 1,
        5..=8 => 2,
        _ => 3,
    };
    let candidates: Vec<S> = candidates.into_iter().collect();
    let mut seen = HashSet::new();
    let mut found: Vec<(usize, &str)> = Vec::new();
    for candidate in candidates.iter().map(AsRef::as_ref) {
        if candidate == word || !seen.insert(candidate) {
            continue;
        }
        let other: Vec<char> = candidate.to_lowercase().chars().collect();
        let mut distance = edit_distance(&typed, &other);
        if typed.len() >= 3 && other.len() > typed.len() && other.starts_with(&typed) {
            distance = distance.min(1);
        }
        if distance <= allowed {
            found.push((distance, candidate));
        }
    }
    found.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then(a.1.cmp(b.1))
    });
    found
        .into_iter()
        .take(3)
        .map(|(_, name)| name.to_owned())
        .collect()
}

/// Optimal string alignment distance: insertions, deletions, substitutions and swaps of
/// neighbouring characters.
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut rows = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[n][m]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::{movie_validator, tv_validator};

    fn engine() -> Engine {
        Engine::standard()
    }

    fn movie(source: &str) -> ValidationReport {
        movie_validator(&engine()).validate(source)
    }

    #[test]
    fn a_typo_is_marked_and_corrected() {
        let report = movie("${titel} (${year})");
        assert_eq!(report.diagnostics.len(), 1);
        let d = &report.diagnostics[0];
        assert_eq!(d.kind, DiagnosticKind::UnknownToken);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!((d.offset, d.len), (2, 5));
        assert_eq!((d.line, d.column), (1, 3));
        assert_eq!(d.suggestions.first().map(String::as_str), Some("title"));
        assert!(d.message.contains("did you mean `title`"));
    }

    #[test]
    fn known_tokens_pass() {
        let sources = [
            "${title} (${year})",
            "${title}${ - ,edition,} (${year})",
            "${movie.title} ${movie.actors[0].name} ${movieSet.title}",
            "${if edition}${edition}${else}none${end}",
            "${foreach actors a , }${a.name}${index_a}${end}",
            "${title;upper;first}",
            "${@regexp \"(\\w+): (\\w+)\" title \"$2 $1\"}",
            "${videoBitRate} ${movieSetIndex2} ${hdrFormat} ${3Dformat2}",
            "plain text, no tokens",
            "",
        ];
        for source in sources {
            let report = movie(source);
            assert!(report.is_clean(), "{source}: {:?}", report.diagnostics);
        }
    }

    #[test]
    fn positions_are_in_the_template_as_written() {
        let source = "line one\n${title}\n  ${ - ,editon,} ${if yeer}x${end}";
        let report = movie(source);
        assert_eq!(report.diagnostics.len(), 2);
        let (a, b) = (&report.diagnostics[0], &report.diagnostics[1]);
        assert_eq!(&source[a.offset..a.offset + a.len], "editon");
        assert_eq!((a.line, a.column), (3, 9));
        assert_eq!(a.suggestions[0], "edition");
        assert_eq!(&source[b.offset..b.offset + b.len], "yeer");
        assert_eq!(b.line, 3);
        assert_eq!(b.suggestions[0], "year");
    }

    #[test]
    fn positions_count_characters_for_the_column() {
        let report = movie("Ünïcode ${titel}");
        let d = &report.diagnostics[0];
        assert_eq!(d.offset, "Ünïcode ".len() + 2);
        assert_eq!(d.column, 8 + 3);
    }

    #[test]
    fn unknown_fields_of_a_model_root() {
        let report = movie("${movie.titel} ${movie.actors[0].anything} ${movieSet.titl}");
        let kinds: Vec<_> = report.diagnostics.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            [DiagnosticKind::UnknownField, DiagnosticKind::UnknownField]
        );
        assert_eq!(report.diagnostics[0].suggestions[0], "title");
        assert_eq!(report.diagnostics[1].suggestions[0], "title");
        let source = "${movie.titel}";
        let d = &movie(source).diagnostics[0];
        assert_eq!(&source[d.offset..d.offset + d.len], "titel");
    }

    #[test]
    fn loop_variables_are_in_scope_only_inside_their_loop() {
        let report = movie("${foreach actors a}${a.name}${end}${a.name}");
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].offset, 36);
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::UnknownToken);
        // The list of a foreach is read before the variable exists.
        let report = movie("${foreach a a}x${end}");
        assert_eq!(report.diagnostics.len(), 1);
    }

    #[test]
    fn variables_suggest_themselves() {
        let report = movie("${foreach actors actor}${actr.name}${end}");
        assert_eq!(report.diagnostics[0].suggestions[0], "actor");
    }

    #[test]
    fn unknown_renderers() {
        let source = "${title;uper}";
        let report = movie(source);
        let d = &report.diagnostics[0];
        assert_eq!(d.kind, DiagnosticKind::UnknownRenderer);
        assert_eq!(&source[d.offset..d.offset + d.len], "uper");
        assert_eq!(d.suggestions[0], "upper");
        // Renderer calls with parameters, and chains.
        assert!(movie("${title;replace(a,b);lower}").is_clean());
        assert_eq!(movie("${title;lower;nope(1)}").diagnostics.len(), 1);
    }

    #[test]
    fn renderers_come_from_the_engine() {
        let bare = Engine::new();
        let validator = movie_validator(&bare);
        assert_eq!(validator.validate("${title;upper}").diagnostics.len(), 1);
    }

    #[test]
    fn syntax_errors_carry_positions() {
        let cases = [
            ("ab ${title", "never closed", 1, 4),
            ("x${end}", "without an open block", 1, 2),
            ("${if year}x", "no `${end}`", 1, 1),
            ("${@nope x}", "unknown processor", 1, 1),
            ("${title;}", "not a renderer name", 1, 1),
        ];
        for (source, fragment, line, column) in cases {
            let report = movie(source);
            assert_eq!(report.diagnostics.len(), 1, "{source}");
            let d = &report.diagnostics[0];
            assert_eq!(d.kind, DiagnosticKind::Syntax, "{source}");
            assert!(d.message.contains(fragment), "{source}: {}", d.message);
            assert_eq!((d.line, d.column), (line, column), "{source}");
            assert!(d.len >= 1);
            assert!(report.has_errors());
        }
    }

    #[test]
    fn a_syntax_error_reports_only_itself() {
        let report = movie("${titel} ${year");
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::Syntax);
    }

    #[test]
    fn regexp_groups_are_checked() {
        let report = movie(r#"${@regexp "(\w+) (\w+)" title "$2 $1 $3"}"#);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::RegexpGroup);
        assert_eq!(report.diagnostics[0].severity, Severity::Warning);
        assert!(!report.has_errors());
        assert!(movie(r#"${@regexp "(\w+)" title "$0 $1"}"#).is_clean());
        // The token inside is checked too.
        let report = movie(r#"${@regexp "(\w+)" titel "$1"}"#);
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::UnknownToken);
    }

    #[test]
    fn tv_alias_map_and_roots() {
        let validator = tv_validator(&engine());
        assert!(
            validator
                .validate("${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}")
                .is_clean()
        );
        assert!(
            validator
                .validate("${tvShow.title} ${episode.firstAired} ${season.title}")
                .is_clean()
        );
        // A movie alias is not a TV alias.
        let report = validator.validate("${movieSetIndex}");
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::UnknownToken);
        let report = validator.validate("${showTitel}");
        assert_eq!(report.diagnostics[0].suggestions[0], "showTitle");
    }

    #[test]
    fn export_templates_use_their_own_roots() {
        let validator = Validator::new(
            &engine(),
            [
                ModelRoot::open("movies"),
                ModelRoot::with_fields("library", ["name"]),
            ],
        );
        assert!(
            validator
                .validate("${foreach movies m}${m.title}${end}${library.name}")
                .is_clean()
        );
        let report = validator.validate("${foreach movie m}x${end}${library.nam}");
        assert_eq!(report.diagnostics.len(), 2);
        assert_eq!(report.diagnostics[0].suggestions[0], "movies");
        assert_eq!(report.diagnostics[1].suggestions[0], "name");
    }

    #[test]
    fn suggestions_rank_and_limit() {
        let names = ["title", "titleSortable", "originalTitle", "tags", "year"];
        assert_eq!(suggest("titel", names), ["title"]);
        assert_eq!(suggest("Title", names)[0], "title");
        assert_eq!(suggest("yeer", names), ["year"]);
        assert!(suggest("zzzzzz", names).is_empty());
        assert!(suggest("ab", names).is_empty());
        assert!(suggest("title", ["title"]).is_empty());
        assert_eq!(suggest("tit", names)[0], "title");
    }

    #[test]
    fn edit_distance_counts_swaps_once() {
        let d = |a: &str, b: &str| {
            edit_distance(
                &a.chars().collect::<Vec<_>>(),
                &b.chars().collect::<Vec<_>>(),
            )
        };
        assert_eq!(d("titel", "title"), 1);
        assert_eq!(d("kitten", "sitting"), 3);
        assert_eq!(d("", "abc"), 3);
        assert_eq!(d("same", "same"), 0);
    }

    fn tv(season: Option<&str>, file: &str) -> TvPatternReport {
        tv_validator(&engine()).check_tv_patterns(season, file)
    }

    fn kinds(report: &ValidationReport) -> Vec<DiagnosticKind> {
        report.diagnostics.iter().map(|d| d.kind).collect()
    }

    #[test]
    fn the_default_tv_patterns_are_recommended() {
        let report = tv(
            Some("Season ${seasonNr}"),
            "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}",
        );
        assert!(report.is_recommended(), "{report:?}");
        assert!(
            tv(
                None,
                "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}"
            )
            .is_recommended()
        );
    }

    #[test]
    fn a_season_folder_can_carry_the_season() {
        assert!(
            tv(
                Some("Season ${seasonNr2}"),
                "${showTitle} - E${episodeNr2} - ${title}"
            )
            .is_recommended()
        );
        let report = tv(None, "${showTitle} - E${episodeNr2} - ${title}");
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvSeasonToken]);
        assert_eq!(report.file.diagnostics[0].severity, Severity::Warning);
        assert!(!report.is_recommended());
    }

    #[test]
    fn exactly_one_episode_token() {
        let report = tv(None, "S${seasonNr2} ${title}");
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvEpisodeToken]);
        let source = "S${seasonNr2}E${episodeNr2}-E${episodeNrDvd2}";
        let report = tv(None, source);
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvEpisodeToken]);
        let d = &report.file.diagnostics[0];
        assert_eq!(&source[d.offset..d.offset + d.len], "episodeNrDvd2");
        assert!(report.file.diagnostics[0].message.contains("more than one"));
    }

    #[test]
    fn full_model_paths_count_like_aliases() {
        let report = tv(
            None,
            "S${episode.season}E${episode.episode}E${episode.airedEpisode}",
        );
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvEpisodeToken]);
        assert!(tv(None, "S${episode.season}E${episode.episode}").is_recommended());
    }

    #[test]
    fn at_most_one_season_and_title_token() {
        let report = tv(None, "S${seasonNr2}${seasonNrDvd}E${episodeNr2}");
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvSeasonToken]);
        let report = tv(
            Some("${seasonNr}${seasonNr2}"),
            "S${seasonNr2}E${episodeNr2}",
        );
        assert!(report.file.is_clean());
        assert_eq!(
            kinds(report.season.as_ref().unwrap()),
            [DiagnosticKind::TvSeasonToken]
        );
        let report = tv(
            None,
            "S${seasonNr2}E${episodeNr2} ${title} ${originalTitle}",
        );
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvTitleToken]);
    }

    #[test]
    fn season_before_episode_and_title_outside() {
        let report = tv(None, "E${episodeNr2}S${seasonNr2}");
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvTokenOrder]);
        let source = "S${seasonNr2} ${title} E${episodeNr2}";
        let report = tv(None, source);
        assert_eq!(kinds(&report.file), [DiagnosticKind::TvTokenOrder]);
        let d = &report.file.diagnostics[0];
        assert_eq!(&source[d.offset..d.offset + d.len], "title");
        // A title after the episode, or before the season, is fine.
        assert!(tv(None, "${title} S${seasonNr2}E${episodeNr2}").is_recommended());
        // A season in the folder is always before the episode.
        assert!(tv(Some("S${seasonNr}"), "${title} E${episodeNr2}").is_recommended());
    }

    #[test]
    fn original_file_names_bypass_the_rules() {
        assert!(tv(None, "${originalFilename}").is_recommended());
        assert!(tv(None, "${originalBasename} ${episodeNr}${episodeNr}").is_recommended());
        // Errors are still reported.
        assert!(!tv(None, "${originalFilename} ${titel}").is_recommended());
    }

    #[test]
    fn tv_rules_wait_for_a_template_that_parses() {
        let report = tv(Some("Season ${seasonNr"), "S${seasonNr2}E${episodeNr2}");
        assert!(report.file.is_clean());
        assert_eq!(
            kinds(report.season.as_ref().unwrap()),
            [DiagnosticKind::Syntax]
        );
        assert!(!report.is_recommended());
        let report = tv(None, "S${seasonNr2}E${episodeNr2");
        assert_eq!(kinds(&report.file), [DiagnosticKind::Syntax]);
        // A season pattern that does not parse is not blamed for a missing season token.
        let report = tv(Some("${"), "E${episodeNr2}");
        assert!(report.file.is_clean());
    }

    #[test]
    fn loop_variables_do_not_count_as_tv_tokens() {
        let report = tv(
            None,
            "${foreach showGenres title}${title}${end}S${seasonNr2}E${episodeNr2}",
        );
        assert!(report.is_recommended(), "{report:?}");
    }
}
