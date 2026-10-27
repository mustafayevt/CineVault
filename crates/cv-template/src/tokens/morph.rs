//! Rewriting a template that uses aliases into one that uses model paths.

use super::aliases::AliasMap;
use crate::parse::split_segments;

impl AliasMap {
    /// Rewrites every alias in the template to its model path, so `${title}` becomes
    /// `${movie.title}` and `${videoBitRate}` becomes `${movie.mediaInfoVideoBitrate;bitrate}`.
    ///
    /// Aliases are replaced where a token is read: in `${token}` and `${prefix,token,suffix}`,
    /// in the condition of `${if}`, in the list of `${foreach}` and in the token of `${@regexp}`.
    /// Anything after the alias (`[0]`, `.name`, `;upper`) stays. Text outside `${...}`, unknown
    /// tokens, full model paths and the names of `foreach` variables are left as they are.
    pub fn morph(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len() + 16);
        // One entry per open block; a `foreach` carries its variable name.
        let mut blocks: Vec<Option<String>> = Vec::new();
        let mut rest = template;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(len) = body_len(after) else {
                // Never closed: the parser reports it, the text stays as written.
                out.push_str(&rest[start..]);
                return out;
            };
            let body = &after[..len];
            out.push_str("${");
            out.push_str(&self.morph_body(body, &mut blocks));
            out.push('}');
            rest = &after[len + 1..];
        }
        out.push_str(rest);
        out
    }

    fn morph_body(&self, body: &str, blocks: &mut Vec<Option<String>>) -> String {
        let scope = |blocks: &[Option<String>]| -> Vec<String> {
            blocks.iter().flatten().cloned().collect()
        };
        let trimmed = body.trim();
        if trimmed == "end" {
            blocks.pop();
            return body.to_owned();
        }
        if trimmed == "else" {
            return body.to_owned();
        }
        if let Some(rest) = keyword(body, "if") {
            let lead = &body[..body.len() - rest.len()];
            let vars = scope(blocks);
            blocks.push(None);
            return format!("{lead}{}", self.morph_condition(rest, &vars));
        }
        if let Some(rest) = keyword(body, "foreach") {
            let lead = &body[..body.len() - rest.len()];
            let vars = scope(blocks);
            let (list, tail) = split_word(rest);
            let variable = split_word(tail.trim_start()).0.to_owned();
            blocks.push(Some(variable));
            return format!("{lead}{}{tail}", self.morph_token(list, &vars));
        }
        let vars = scope(blocks);
        if let Some(args) = body.trim_start().strip_prefix("@regexp") {
            return self.morph_regexp(body, args, &vars);
        }
        let segments = split_segments(body);
        match segments.as_slice() {
            [token] => self.morph_token(token, &vars),
            [prefix, token, suffix] => {
                format!("{prefix},{},{suffix}", self.morph_token(token, &vars))
            }
            _ => body.to_owned(),
        }
    }

    /// `[!]token [op literal]`.
    fn morph_condition(&self, condition: &str, vars: &[String]) -> String {
        let leading = condition.len() - condition.trim_start().len();
        let (space, text) = condition.split_at(leading);
        let (bang, text) = match text.strip_prefix('!') {
            Some(after) if !after.starts_with('=') => ("!", after),
            _ => ("", text),
        };
        let spaces = text.len() - text.trim_start().len();
        let (inner_space, text) = text.split_at(spaces);
        let end = text
            .find(|c: char| c.is_whitespace() || matches!(c, '=' | '!' | '<' | '>'))
            .unwrap_or(text.len());
        let (token, tail) = text.split_at(end);
        format!(
            "{space}{bang}{inner_space}{}{tail}",
            self.morph_token(token, vars)
        )
    }

    /// `@regexp "pattern" token "replacement"`: only the token is read from the model.
    fn morph_regexp(&self, body: &str, args: &str, vars: &[String]) -> String {
        let args = args.trim_start();
        let Some(after_pattern) = skip_quoted(args) else {
            return body.to_owned();
        };
        let tail = after_pattern.trim_start();
        let (token, rest) = split_word(tail);
        let head_len = body.len() - tail.len();
        format!(
            "{}{}{rest}",
            &body[..head_len],
            self.morph_token(token, vars)
        )
    }

    /// Replaces the alias at the start of a token, keeping what follows it.
    fn morph_token(&self, token: &str, vars: &[String]) -> String {
        let lead_len = token.len() - token.trim_start().len();
        let (lead, text) = token.split_at(lead_len);
        let head_len = text
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(text.len());
        let (head, tail) = text.split_at(head_len);
        if vars.iter().any(|v| v == head) {
            return token.to_owned();
        }
        match self.target(head) {
            Some(target) => format!("{lead}{target}{tail}"),
            None => token.to_owned(),
        }
    }
}

/// The length of the body of a `${` whose text starts at `after`: up to the closing `}`, which
/// a `@regexp` pattern may hide inside quotes.
fn body_len(after: &str) -> Option<usize> {
    let quoted = after.trim_start().starts_with('@');
    let mut in_quotes = false;
    let mut chars = after.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if quoted && in_quotes => {
                chars.next();
            }
            '"' if quoted => in_quotes = !in_quotes,
            '}' if !in_quotes => return Some(i),
            _ => {}
        }
    }
    None
}

/// `rest` after `keyword` when `body` starts with it and whitespace follows.
fn keyword<'a>(body: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = body.trim_start().strip_prefix(keyword)?;
    rest.starts_with(char::is_whitespace).then_some(rest)
}

/// The first whitespace-separated word (after leading whitespace) and what follows it. The
/// leading whitespace stays with the word, so joining the two parts restores the input.
fn split_word(text: &str) -> (&str, &str) {
    let start = text.len() - text.trim_start().len();
    let end = text[start..]
        .find(char::is_whitespace)
        .map_or(text.len(), |i| start + i);
    text.split_at(end)
}

/// The text after a leading `"..."` (with backslash escapes), or `None` if it is not quoted.
fn skip_quoted(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('"')?;
    let mut chars = inner.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '"' => return Some(&inner[i + 1..]),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::tokens::{movie_aliases, tv_aliases};

    fn movie(template: &str) -> String {
        movie_aliases().morph(template)
    }

    #[test]
    fn plain_and_segment_tokens() {
        assert_eq!(
            movie("${title} (${year})"),
            "${movie.title} (${movie.year})"
        );
        assert_eq!(
            movie("${title}${ - ,edition,} (${year})"),
            "${movie.title}${ - ,movie.edition,} (${movie.year})"
        );
        assert_eq!(movie("${(,year,)}"), "${(,movie.year,)}");
        assert_eq!(movie("${ [,edition,]}"), "${ [,movie.edition,]}");
        assert_eq!(
            movie("${prefix:,note,:suffix}"),
            "${prefix:,movie.note,:suffix}"
        );
    }

    #[test]
    fn aliases_with_renderers_and_indexes() {
        assert_eq!(
            movie("${videoBitRate}"),
            "${movie.mediaInfoVideoBitrate;bitrate}"
        );
        assert_eq!(movie("${title;upper;first}"), "${movie.title;upper;first}");
        assert_eq!(movie("${title[0,2]}"), "${movie.title[0,2]}");
        assert_eq!(movie("${actors[0].name}"), "${movie.actors[0].name}");
        assert_eq!(movie("${3Dformat2}"), "${movie.video3DFormat2}");
        assert_eq!(movie("${movieSetIndex}"), "${movie;indexOfMovieSet}");
        // A prefix and suffix around a renderer call that has commas.
        assert_eq!(
            movie("${[,title;replace(a,b),]}"),
            "${[,movie.title;replace(a,b),]}"
        );
    }

    #[test]
    fn blocks() {
        assert_eq!(
            movie("${if edition}[${edition}]${else}[none]${end}"),
            "${if movie.edition}[${movie.edition}]${else}[none]${end}"
        );
        assert_eq!(
            movie("${if !edition}x${end}"),
            "${if !movie.edition}x${end}"
        );
        assert_eq!(
            movie("${if year > 2000}new${end}"),
            "${if movie.year > 2000}new${end}"
        );
        assert_eq!(
            movie("${if edition = 'Extended Edition'}ext${end}"),
            "${if movie.edition = 'Extended Edition'}ext${end}"
        );
        assert_eq!(
            movie("${foreach genres g , }${g}${end}"),
            "${foreach movie.genres g , }${g}${end}"
        );
        // A loop variable named like an alias is the variable inside the loop only.
        assert_eq!(
            movie("${foreach actors title}${title.name}${end}${title}"),
            "${foreach movie.actors title}${title.name}${end}${movie.title}"
        );
    }

    #[test]
    fn regexp_reads_the_token_only() {
        assert_eq!(
            movie(r#"${@regexp "(\w+): (\w+)" originalTitle "$2 $1"}"#),
            r#"${@regexp "(\w+): (\w+)" movie.originalTitle "$2 $1"}"#
        );
        // A brace inside the pattern does not end the token.
        assert_eq!(
            movie(r#"${@regexp "\d{4}" title "$0"}"#),
            r#"${@regexp "\d{4}" movie.title "$0"}"#
        );
    }

    #[test]
    fn leaves_everything_else_alone() {
        assert_eq!(movie("text only"), "text only");
        assert_eq!(movie(""), "");
        assert_eq!(movie("${unknownToken}"), "${unknownToken}");
        assert_eq!(movie("${movie.title}"), "${movie.title}");
        assert_eq!(movie("${title"), "${title");
        assert_eq!(movie("$${title}"), "$${movie.title}");
        assert_eq!(movie(r"\${title}"), r"\${movie.title}");
        // A movie alias is not a TV alias and the other way round.
        assert_eq!(movie("${showTitle}"), "${showTitle}");
        assert_eq!(
            tv_aliases().morph("${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}"),
            "${tvShow.title} - S${episode.season;number(%02d)}E${episode.episode;number(%02d)} - ${episode.title}"
        );
    }
}
