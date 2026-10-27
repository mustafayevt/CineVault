//! The template parser: text with `${...}` expressions to a tree the evaluator walks.
//!
//! Grammar, in the order the parser decides what a `${...}` is:
//!
//! - `${end}` and `${else}` close and split blocks;
//! - `${if [!]path}` and `${if path = literal}` (also `==` and `!=`) open a conditional block;
//! - `${foreach list item [separator]}` opens a loop block;
//! - `${@regexp "pattern" token "replacement $1"}` is the regular-expression processor;
//! - anything else is an expression: `${[prefix,]token[;renderer[(params)]]...[,suffix]}`.
//!
//! In text, `\\` is a backslash and `\${` is a literal `${`; any other backslash is kept.

use regex::{Regex, RegexBuilder};

/// Deepest nesting of `if` and `foreach` blocks the parser accepts.
pub const MAX_NESTING: usize = 64;

/// A template that does not parse. Offsets are byte offsets into the template; `line` and
/// `column` count from 1 (columns in characters).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message} (line {line}, column {column})")]
pub struct SyntaxError {
    pub message: String,
    pub offset: usize,
    /// Length in bytes of the offending text, at least 1.
    pub len: usize,
    pub line: usize,
    pub column: usize,
}

/// A byte range of the template source. `len` counts bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub offset: usize,
    pub len: usize,
}

impl Span {
    fn shifted(self, base: usize) -> Span {
        Span {
            offset: self.offset + base,
            len: self.len,
        }
    }
}

/// One part of a dotted path: a name followed by index suffixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub indexes: Vec<Index>,
    /// Where the name is written.
    pub span: Span,
}

/// `[n]`, `[last]` or `[start,count]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Index {
    At(usize),
    Last,
    Slice(usize, usize),
}

/// A dotted path such as `movie.actors[0].name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<Segment>,
    /// Where the whole path is written.
    pub span: Span,
}

impl Path {
    /// The path as written, for messages and for the output appender.
    pub fn display(&self) -> String {
        let mut out = String::new();
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                out.push('.');
            }
            out.push_str(&segment.name);
            for index in &segment.indexes {
                match index {
                    Index::At(n) => out.push_str(&format!("[{n}]")),
                    Index::Last => out.push_str("[last]"),
                    Index::Slice(a, b) => out.push_str(&format!("[{a},{b}]")),
                }
            }
        }
        out
    }
}

/// A named renderer call such as `array( | )`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub params: Option<String>,
    /// Where the renderer name is written.
    pub span: Span,
}

/// A path followed by a chain of renderers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub path: Path,
    pub chain: Vec<Call>,
    /// Where the path and renderer chain are written.
    pub span: Span,
}

#[derive(Debug, Clone)]
pub(crate) struct ExprNode {
    pub prefix: String,
    pub suffix: String,
    pub expr: Expr,
    /// The token text as written (path and renderers), handed to the output appender.
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Compare {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

#[derive(Debug, Clone)]
pub(crate) struct IfNode {
    pub negate: bool,
    pub path: Path,
    pub compare: Option<(Compare, String)>,
    pub then: Vec<Node>,
    pub otherwise: Vec<Node>,
}

#[derive(Debug, Clone)]
pub(crate) struct ForeachNode {
    pub list: Expr,
    pub var: String,
    pub separator: String,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone)]
pub(crate) struct RegexpNode {
    /// The whole `${@regexp ...}` directive.
    pub span: Span,
    pub regex: Regex,
    pub token: Expr,
    pub destination: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Text(String),
    Expr(ExprNode),
    If(IfNode),
    Foreach(ForeachNode),
    Regexp(RegexpNode),
}

/// A parsed template. Parse once and render many times.
#[derive(Debug, Clone)]
pub struct Template {
    pub(crate) nodes: Vec<Node>,
}

impl Template {
    /// Parses a template, or reports the first syntax error with its position.
    pub fn parse(source: &str) -> Result<Template, SyntaxError> {
        Template::parse_with(source, ParseOptions::default())
    }

    /// Parses a template with explicit options.
    pub fn parse_with(source: &str, options: ParseOptions) -> Result<Template, SyntaxError> {
        let mut parser = Parser {
            src: source,
            pos: 0,
            options,
        };
        let (nodes, end) = parser.block(0)?;
        match end {
            BlockEnd::Eof => Ok(Template { nodes }),
            BlockEnd::End(at) => Err(parser.error(at.0, at.1, "`${end}` without an open block")),
            BlockEnd::Else(at) => Err(parser.error(at.0, at.1, "`${else}` without an open `if`")),
        }
    }
}

/// How a template is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Whether a doubled backslash and a backslash before `${` are escapes. Export templates
    /// use them. The renamer reads the backslashes users type for paths literally, so it turns
    /// this off.
    pub backslash_escapes: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            backslash_escapes: true,
        }
    }
}

enum BlockEnd {
    Eof,
    /// Offset and length of the closing directive.
    End((usize, usize)),
    Else((usize, usize)),
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    options: ParseOptions,
}

impl Parser<'_> {
    /// The byte offset of `part`, which must be a slice of the template source.
    fn offset_of(&self, part: &str) -> usize {
        (part.as_ptr() as usize).saturating_sub(self.src.as_ptr() as usize)
    }

    fn error(&self, offset: usize, len: usize, message: &str) -> SyntaxError {
        let before = &self.src[..offset.min(self.src.len())];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        SyntaxError {
            message: message.to_owned(),
            offset,
            len: len.max(1),
            line,
            column: before[line_start..].chars().count() + 1,
        }
    }

    /// Parses nodes until the end of the input or a `${end}` / `${else}`.
    fn block(&mut self, depth: usize) -> Result<(Vec<Node>, BlockEnd), SyntaxError> {
        let mut nodes = Vec::new();
        let mut text = String::new();
        let bytes = self.src.as_bytes();
        loop {
            if self.pos >= bytes.len() {
                flush(&mut text, &mut nodes);
                return Ok((nodes, BlockEnd::Eof));
            }
            let c = bytes[self.pos];
            if c == b'\\' && self.options.backslash_escapes {
                match bytes.get(self.pos + 1) {
                    Some(b'\\') => {
                        text.push('\\');
                        self.pos += 2;
                    }
                    Some(b'$') if bytes.get(self.pos + 2) == Some(&b'{') => {
                        text.push('$');
                        self.pos += 2;
                    }
                    _ => {
                        text.push('\\');
                        self.pos += 1;
                    }
                }
                continue;
            }
            if c == b'$' && bytes.get(self.pos + 1) == Some(&b'{') {
                let open = self.pos;
                let body_start = open + 2;
                let close = self.find_close(open, body_start)?;
                let body = &self.src[body_start..close];
                self.pos = close + 1;
                let whole = (open, close + 1 - open);
                let trimmed = body.trim();
                if trimmed == "end" {
                    flush(&mut text, &mut nodes);
                    return Ok((nodes, BlockEnd::End(whole)));
                }
                if trimmed == "else" {
                    flush(&mut text, &mut nodes);
                    return Ok((nodes, BlockEnd::Else(whole)));
                }
                flush(&mut text, &mut nodes);
                if depth >= MAX_NESTING {
                    return Err(self.error(open, whole.1, "blocks are nested too deeply"));
                }
                let node = if let Some(rest) = keyword(body, "if") {
                    self.if_node(rest, whole, depth)?
                } else if let Some(rest) = keyword(body, "foreach") {
                    self.foreach_node(rest, whole, depth)?
                } else if trimmed.starts_with('@') {
                    self.annotation_node(trimmed, whole)?
                } else {
                    self.expr_node(body, body_start)?
                };
                nodes.push(node);
                continue;
            }
            // Copy one whole character.
            let ch = self.src[self.pos..].chars().next().unwrap_or('\u{fffd}');
            text.push(ch);
            self.pos += ch.len_utf8();
        }
    }

    /// The offset of the `}` that closes the `${` at `open`.
    fn find_close(&self, open: usize, body_start: usize) -> Result<usize, SyntaxError> {
        let rest = &self.src[body_start..];
        let quoted = rest.trim_start().starts_with('@');
        let mut in_quotes = false;
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' if quoted && in_quotes => {
                    chars.next();
                }
                '"' if quoted => in_quotes = !in_quotes,
                '}' if !in_quotes => return Ok(body_start + i),
                _ => {}
            }
        }
        Err(self.error(open, 2, "`${` is never closed"))
    }

    fn if_node(
        &mut self,
        rest: &str,
        whole: (usize, usize),
        depth: usize,
    ) -> Result<Node, SyntaxError> {
        let condition = rest.trim();
        let (negate, condition) = match condition.strip_prefix('!') {
            Some(c) if !c.starts_with('=') => (true, c.trim_start()),
            _ => (false, condition),
        };
        let (left, compare) = split_comparison(condition);
        let left = left.trim();
        let path = parse_path_at(left, self.offset_of(left))
            .map_err(|m| self.error(whole.0, whole.1, &m))?;
        let (then, end) = self.block(depth + 1)?;
        let otherwise = match end {
            BlockEnd::Eof => return Err(self.error(whole.0, whole.1, "`${if}` has no `${end}`")),
            BlockEnd::End(_) => Vec::new(),
            BlockEnd::Else(_) => {
                let (nodes, end) = self.block(depth + 1)?;
                match end {
                    BlockEnd::End(_) => nodes,
                    BlockEnd::Eof => {
                        return Err(self.error(whole.0, whole.1, "`${if}` has no `${end}`"));
                    }
                    BlockEnd::Else(at) => {
                        return Err(self.error(at.0, at.1, "`${if}` has two `${else}`"));
                    }
                }
            }
        };
        Ok(Node::If(IfNode {
            negate,
            path,
            compare,
            then,
            otherwise,
        }))
    }

    fn foreach_node(
        &mut self,
        rest: &str,
        whole: (usize, usize),
        depth: usize,
    ) -> Result<Node, SyntaxError> {
        let rest = rest.trim_start();
        let list_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (list_text, rest) = rest.split_at(list_end);
        let rest = rest.trim_start();
        if list_text.is_empty() || rest.is_empty() {
            return Err(self.error(
                whole.0,
                whole.1,
                "`foreach` needs a list and a variable name",
            ));
        }
        let var_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (var, rest) = rest.split_at(var_end);
        // Exactly one space or tab separates the variable from the separator text.
        let separator = rest.chars().next().map_or("", |c| &rest[c.len_utf8()..]);
        let list = parse_expr_at(list_text, self.offset_of(list_text))
            .map_err(|m| self.error(whole.0, whole.1, &m))?;
        let (body, end) = self.block(depth + 1)?;
        match end {
            BlockEnd::End(_) => {}
            BlockEnd::Eof => {
                return Err(self.error(whole.0, whole.1, "`${foreach}` has no `${end}`"));
            }
            BlockEnd::Else(at) => {
                return Err(self.error(at.0, at.1, "`${else}` inside `${foreach}`"));
            }
        }
        Ok(Node::Foreach(ForeachNode {
            list,
            var: var.to_owned(),
            separator: separator.to_owned(),
            body,
        }))
    }

    fn annotation_node(&mut self, body: &str, whole: (usize, usize)) -> Result<Node, SyntaxError> {
        let Some(args) = body.strip_prefix("@regexp") else {
            let name: String = body[1..]
                .chars()
                .take_while(|c| c.is_alphanumeric())
                .collect();
            return Err(self.error(whole.0, whole.1, &format!("unknown processor `@{name}`")));
        };
        let message = "`@regexp` needs a quoted pattern, a token and a quoted replacement";
        let mut rest = args.trim_start();
        let pattern =
            take_quoted(&mut rest).ok_or_else(|| self.error(whole.0, whole.1, message))?;
        rest = rest.trim_start();
        let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (token_text, mut rest) = rest.split_at(token_end);
        rest = rest.trim_start();
        let destination =
            take_quoted(&mut rest).ok_or_else(|| self.error(whole.0, whole.1, message))?;
        if !rest.trim().is_empty() || token_text.is_empty() {
            return Err(self.error(whole.0, whole.1, message));
        }
        let regex = compile_pattern(&pattern)
            .map_err(|m| self.error(whole.0, whole.1, &format!("invalid pattern: {m}")))?;
        let token = parse_expr_at(token_text, self.offset_of(token_text))
            .map_err(|m| self.error(whole.0, whole.1, &m))?;
        Ok(Node::Regexp(RegexpNode {
            span: Span {
                offset: whole.0,
                len: whole.1,
            },
            regex,
            token,
            destination,
            source: token_text.to_owned(),
        }))
    }

    fn expr_node(&mut self, body: &str, body_start: usize) -> Result<Node, SyntaxError> {
        let whole = (body_start - 2, body.len() + 3);
        let segments = split_segments(body);
        let (prefix, core, suffix) = match segments.as_slice() {
            [core] => ("", *core, ""),
            [prefix, core, suffix] => (*prefix, *core, *suffix),
            _ => {
                return Err(self.error(
                    whole.0,
                    whole.1,
                    "expected `${token}` or `${prefix,token,suffix}`",
                ));
            }
        };
        let core = core.trim();
        let expr = parse_expr_at(core, self.offset_of(core))
            .map_err(|m| self.error(whole.0, whole.1, &m))?;
        Ok(Node::Expr(ExprNode {
            prefix: prefix.to_owned(),
            suffix: suffix.to_owned(),
            expr,
            source: core.to_owned(),
        }))
    }
}

fn flush(text: &mut String, nodes: &mut Vec<Node>) {
    if !text.is_empty() {
        nodes.push(Node::Text(std::mem::take(text)));
    }
}

/// `body` without the leading keyword when it starts with `keyword` followed by whitespace.
fn keyword<'a>(body: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = body.trim_start().strip_prefix(keyword)?;
    rest.starts_with(char::is_whitespace).then_some(rest)
}

/// Splits `left op literal` outside of quotes, where `op` is `=`, `==`, `!=`, `>`, `>=`, `<` or
/// `<=`. Returns the whole text when there is no operator.
fn split_comparison(condition: &str) -> (&str, Option<(Compare, String)>) {
    let mut quote: Option<char> = None;
    let bytes = condition.as_bytes();
    for (i, c) in condition.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if matches!(c, '!' | '=' | '>' | '<') => {
                let with_equals = bytes.get(i + 1) == Some(&b'=');
                let (op, len) = match (c, with_equals) {
                    ('!', true) => (Compare::NotEqual, 2),
                    ('!', false) => continue,
                    ('=', true) => (Compare::Equal, 2),
                    ('=', false) => (Compare::Equal, 1),
                    ('>', true) => (Compare::GreaterOrEqual, 2),
                    ('>', false) => (Compare::Greater, 1),
                    ('<', true) => (Compare::LessOrEqual, 2),
                    _ => (Compare::Less, 1),
                };
                return (&condition[..i], Some((op, literal(&condition[i + len..]))));
            }
            None => {}
        }
    }
    (condition, None)
}

/// A comparison operand: quotes around it are removed.
fn literal(text: &str) -> String {
    let text = text.trim();
    for quote in ['\'', '"'] {
        if let Some(inner) = text.strip_prefix(quote).and_then(|t| t.strip_suffix(quote)) {
            return inner.to_owned();
        }
    }
    text.to_owned()
}

/// Reads one `"..."` argument. `\"` is a quote; every other backslash is kept, so regular
/// expression escapes such as `\w` pass through.
fn take_quoted(rest: &mut &str) -> Option<String> {
    let inner = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = inner.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, other)) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return None,
            },
            '"' => {
                *rest = &inner[i + 1..];
                return Some(out);
            }
            other => out.push(other),
        }
    }
    None
}

/// Compiles a `@regexp` pattern. Matching is case-insensitive, and `\w`, `\d` and `\s` cover
/// ASCII only, so existing renamer patterns keep matching the same way.
pub fn compile_pattern(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(&ascii_classes(pattern))
        .case_insensitive(true)
        .build()
        .map_err(|e| e.to_string().lines().last().unwrap_or("invalid").to_owned())
}

fn ascii_classes(pattern: &str) -> String {
    const SPACE: &str = r" \t\n\x0B\x0C\r";
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut class_depth = 0usize;
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let Some(next) = chars.next() else {
                    out.push('\\');
                    break;
                };
                let in_class = class_depth > 0;
                match (next, in_class) {
                    ('w', true) => out.push_str("0-9A-Za-z_"),
                    ('w', false) => out.push_str("[0-9A-Za-z_]"),
                    ('W', false) => out.push_str("[^0-9A-Za-z_]"),
                    ('d', true) => out.push_str("0-9"),
                    ('d', false) => out.push_str("[0-9]"),
                    ('D', false) => out.push_str("[^0-9]"),
                    ('s', true) => out.push_str(SPACE),
                    ('s', false) => {
                        out.push('[');
                        out.push_str(SPACE);
                        out.push(']');
                    }
                    ('S', false) => {
                        out.push_str("[^");
                        out.push_str(SPACE);
                        out.push(']');
                    }
                    ('b', false) => out.push_str("(?-u:\\b)"),
                    _ => {
                        out.push('\\');
                        out.push(next);
                    }
                }
            }
            '[' => {
                class_depth += 1;
                out.push(c);
            }
            ']' => {
                class_depth = class_depth.saturating_sub(1);
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Splits an expression body at the commas that separate prefix, token and suffix. A comma
/// inside the parentheses of a renderer call (after a `;`) or inside an index such as
/// `[0,2]` belongs to the token.
pub(crate) fn split_segments(body: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut in_index = false;
    let mut seen_semicolon = false;
    let mut prev: Option<char> = None;
    for (i, c) in body.char_indices() {
        let after_name = prev.is_some_and(|p| p.is_alphanumeric() || p == '_' || p == ']');
        match c {
            '(' if depth > 0 => depth += 1,
            '(' if seen_semicolon && after_name => depth = 1,
            ')' if depth > 0 => depth -= 1,
            // An index such as `title[0,2]` keeps its comma; a bare `[` starts a prefix.
            '[' if depth == 0 && after_name && !seen_semicolon => in_index = true,
            ']' if in_index => in_index = false,
            ';' if depth == 0 => seen_semicolon = true,
            ',' if depth == 0 && !in_index => {
                segments.push(&body[start..i]);
                start = i + 1;
                seen_semicolon = false;
            }
            _ => {}
        }
        prev = Some(c);
    }
    segments.push(&body[start..]);
    segments
}

/// Parses `path[;renderer[(params)]]...`.
pub fn parse_expr(text: &str) -> Result<Expr, String> {
    parse_expr_at(text, 0)
}

/// [`parse_expr`] for text that starts at byte `base` of the template, so that the spans in the
/// result are positions in the template.
pub(crate) fn parse_expr_at(text: &str, base: usize) -> Result<Expr, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                parts.push((start, &text[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push((start, &text[start..]));
    let (first_start, first) = parts[0];
    let path = parse_path_at(first.trim(), base + first_start + leading(first))?;
    let mut chain = Vec::new();
    for (part_start, part) in &parts[1..] {
        chain.push(parse_call(part.trim(), base + part_start + leading(part))?);
    }
    Ok(Expr {
        path,
        chain,
        span: Span {
            offset: base,
            len: text.len(),
        },
    })
}

/// The number of bytes of leading whitespace.
fn leading(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

fn parse_call(text: &str, base: usize) -> Result<Call, String> {
    let (name, params) = match text.find('(') {
        None => (text, None),
        Some(open) => {
            let Some(inner) = text[open + 1..].strip_suffix(')') else {
                return Err(format!("renderer `{}` is missing its `)`", &text[..open]));
            };
            (text[..open].trim(), Some(inner.to_owned()))
        }
    };
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!("`{name}` is not a renderer name"));
    }
    Ok(Call {
        name: name.to_owned(),
        params,
        span: Span {
            offset: base,
            len: name.len(),
        },
    })
}

/// Parses `a.b[0].c[last]`.
pub fn parse_path(text: &str) -> Result<Path, String> {
    parse_path_at(text, 0)
}

/// [`parse_path`] for text that starts at byte `base` of the template.
pub(crate) fn parse_path_at(text: &str, base: usize) -> Result<Path, String> {
    if text.is_empty() {
        return Err("empty token".to_owned());
    }
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_brackets = false;
    let mut pieces = Vec::new();
    for (i, c) in text.char_indices() {
        match c {
            '[' => in_brackets = true,
            ']' => in_brackets = false,
            '.' if !in_brackets => {
                pieces.push((start, &text[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push((start, &text[start..]));
    for (piece_start, piece) in pieces {
        let (name, mut rest) = match piece.find('[') {
            Some(open) => piece.split_at(open),
            None => (piece, ""),
        };
        if name.is_empty() {
            return Err(format!("`{text}` has an empty name"));
        }
        if name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '{' | '}' | '(' | ')' | ']' | ',' | ';'))
        {
            return Err(format!("`{name}` is not a valid token name"));
        }
        let mut indexes = Vec::new();
        while !rest.is_empty() {
            let Some(inner_and_rest) = rest.strip_prefix('[') else {
                return Err(format!("unexpected `{rest}` after `{name}`"));
            };
            let Some(close) = inner_and_rest.find(']') else {
                return Err(format!("`{name}[` is missing its `]`"));
            };
            indexes.push(parse_index(&inner_and_rest[..close])?);
            rest = &inner_and_rest[close + 1..];
        }
        segments.push(Segment {
            name: name.to_owned(),
            indexes,
            span: Span {
                offset: piece_start,
                len: name.len(),
            }
            .shifted(base),
        });
    }
    Ok(Path {
        segments,
        span: Span {
            offset: base,
            len: text.len(),
        },
    })
}

fn parse_index(text: &str) -> Result<Index, String> {
    let text = text.trim();
    if text == "last" {
        return Ok(Index::Last);
    }
    let number = |s: &str| {
        s.trim()
            .parse::<usize>()
            .map_err(|_| format!("`[{text}]` is not an index, `last` or `start,count`"))
    };
    match text.split_once(',') {
        Some((start, count)) => Ok(Index::Slice(number(start)?, number(count)?)),
        None => Ok(Index::At(number(text)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(source: &str) -> SyntaxError {
        Template::parse(source).unwrap_err()
    }

    #[test]
    fn paths_and_indexes() {
        let p = parse_path("movie.actors[0].name").unwrap();
        assert_eq!(p.segments.len(), 3);
        assert_eq!(p.segments[1].indexes, vec![Index::At(0)]);
        assert_eq!(p.display(), "movie.actors[0].name");
        let p = parse_path("title[0,2]").unwrap();
        assert_eq!(p.segments[0].indexes, vec![Index::Slice(0, 2)]);
        let p = parse_path("a[last]").unwrap();
        assert_eq!(p.segments[0].indexes, vec![Index::Last]);
        assert!(parse_path("a[x]").is_err());
        assert!(parse_path("a[1").is_err());
        assert!(parse_path("a..b").is_err());
        assert!(parse_path("a b").is_err());
    }

    #[test]
    fn expressions_split_prefix_token_suffix() {
        let t = Template::parse("${ - ,edition,}").unwrap();
        let Node::Expr(e) = &t.nodes[0] else {
            panic!("expected an expression")
        };
        assert_eq!((e.prefix.as_str(), e.suffix.as_str()), (" - ", ""));
        assert_eq!(e.expr.path.display(), "edition");
    }

    #[test]
    fn renderer_commas_stay_with_the_call() {
        let t = Template::parse("${title;replace(The,A)}").unwrap();
        let Node::Expr(e) = &t.nodes[0] else {
            panic!("expected an expression")
        };
        assert_eq!(e.expr.chain[0].params.as_deref(), Some("The,A"));
        assert!(e.prefix.is_empty());
        // Prefix and suffix around a renderer call that has commas.
        let t = Template::parse("${[,title;replace(a,b),]}").unwrap();
        let Node::Expr(e) = &t.nodes[0] else {
            panic!("expected an expression")
        };
        assert_eq!((e.prefix.as_str(), e.suffix.as_str()), ("[", "]"));
        assert_eq!(e.expr.chain[0].params.as_deref(), Some("a,b"));
    }

    #[test]
    fn renderer_params_may_contain_semicolons() {
        let t = Template::parse("${tags;uniqueArray(;)}").unwrap();
        let Node::Expr(e) = &t.nodes[0] else {
            panic!("expected an expression")
        };
        assert_eq!(e.expr.chain[0].params.as_deref(), Some(";"));
    }

    #[test]
    fn unbalanced_prefix_paren_is_not_a_call() {
        let t = Template::parse("${(,year,)}").unwrap();
        let Node::Expr(e) = &t.nodes[0] else {
            panic!("expected an expression")
        };
        assert_eq!((e.prefix.as_str(), e.suffix.as_str()), ("(", ")"));
    }

    #[test]
    fn escapes_in_text() {
        let t = Template::parse(r"a\\b \${x} c\d").unwrap();
        let Node::Text(text) = &t.nodes[0] else {
            panic!("expected text")
        };
        assert_eq!(text, r"a\b ${x} c\d");
    }

    #[test]
    fn errors_carry_positions() {
        let e = err("line one\nab ${title");
        assert_eq!((e.line, e.column, e.offset), (2, 4, 12));
        assert!(e.message.contains("never closed"));
        let e = err("${foreach a b}x");
        assert!(e.message.contains("no `${end}`"));
        let e = err("x${end}");
        assert!(e.message.contains("without an open block"));
        let e = err("${if a}x${else}y${else}z${end}");
        assert!(e.message.contains("two"));
        assert!(err("${a,b}").message.contains("prefix"));
        assert!(err("${ }").message.contains("empty"));
        assert!(err("${@nope x}").message.contains("unknown processor"));
        assert!(
            err("${@regexp \"(\" x \"y\"}")
                .message
                .contains("invalid pattern")
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let source = "${if a}".repeat(MAX_NESTING + 2);
        assert!(err(&source).message.contains("nested too deeply"));
    }

    #[test]
    fn foreach_parts() {
        let t = Template::parse("${foreach movie.actors actor , }${actor.name}${end}").unwrap();
        let Node::Foreach(f) = &t.nodes[0] else {
            panic!("expected foreach")
        };
        assert_eq!(f.var, "actor");
        assert_eq!(f.separator, ", ");
        assert_eq!(f.list.path.display(), "movie.actors");
    }

    #[test]
    fn if_conditions() {
        let t = Template::parse("${if !a.b}x${else}y${end}").unwrap();
        let Node::If(i) = &t.nodes[0] else {
            panic!("expected if")
        };
        assert!(i.negate && i.compare.is_none());
        let t = Template::parse("${if kind == 'a b'}x${end}").unwrap();
        let Node::If(i) = &t.nodes[0] else {
            panic!("expected if")
        };
        assert_eq!(i.compare, Some((Compare::Equal, "a b".to_owned())));
        let t = Template::parse("${if kind != x}x${end}").unwrap();
        let Node::If(i) = &t.nodes[0] else {
            panic!("expected if")
        };
        assert_eq!(i.compare, Some((Compare::NotEqual, "x".to_owned())));
    }

    #[test]
    fn regexp_arguments() {
        let t = Template::parse(r#"${@regexp "(\w+) (\w{2})" movie.title "$2 $1"}"#).unwrap();
        let Node::Regexp(r) = &t.nodes[0] else {
            panic!("expected regexp")
        };
        assert_eq!(r.destination, "$2 $1");
        assert_eq!(r.token.path.display(), "movie.title");
    }

    #[test]
    fn ascii_classes_translate_inside_and_outside_brackets() {
        let re = compile_pattern(r"[\w-]+ \d+").unwrap();
        assert!(re.is_match("ab-c 12"));
        // Unicode letters are not \w.
        let re = compile_pattern(r"^\w+$").unwrap();
        assert!(!re.is_match("é"));
        assert!(re.is_match("ABC"));
    }
}
