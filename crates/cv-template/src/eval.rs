//! The evaluator: renders a parsed [`Template`] against a model, streaming into a writer.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::time::{Duration, Instant};

use crate::parse::{
    Call, Compare, Expr, ExprNode, ForeachNode, IfNode, Index, Node, Path, RegexpNode, SyntaxError,
    Template,
};
use crate::value::Value;

/// Why a template could not be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// A renderer failed on its input or parameters.
    #[error("renderer `{name}`: {message}")]
    Renderer { name: String, message: String },
    /// The output sink refused the text.
    #[error("the output could not be written")]
    Write,
    /// The output grew past [`Limits::max_output_bytes`]. Text written before the limit was hit
    /// has already reached the sink.
    #[error("the output is larger than the limit of {limit} bytes")]
    OutputTooLarge { limit: usize },
    /// Rendering took longer than [`Limits::max_duration`].
    #[error("rendering took longer than the time limit")]
    TimedOut,
    /// The template did more work than [`Limits::max_steps`] allows (a loop inside a loop over
    /// big lists, for example).
    #[error("the template needs more than {limit} evaluation steps")]
    TooManySteps { limit: u64 },
}

impl From<fmt::Error> for RenderError {
    fn from(_: fmt::Error) -> Self {
        RenderError::Write
    }
}

/// Parsing or rendering failed; returned by the one-call helpers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Syntax(#[from] SyntaxError),
    #[error(transparent)]
    Render(#[from] RenderError),
}

/// How much a single render may cost. Hitting a limit stops the render with an error; nothing
/// is cut off silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes the render may write to its sink.
    pub max_output_bytes: usize,
    /// The longest a render may run, or `None` for no time limit.
    pub max_duration: Option<Duration>,
    /// The most evaluation steps (nodes visited and loop iterations) a render may take.
    pub max_steps: u64,
}

impl Limits {
    /// The default for exports: roomy enough for a template over a library of hundreds of
    /// thousands of items (256 MiB of output, 60 seconds, 100 million steps).
    pub const EXPORT: Limits = Limits {
        max_output_bytes: 256 * 1024 * 1024,
        max_duration: Some(Duration::from_secs(60)),
        max_steps: 100_000_000,
    };

    /// For file and folder names: 256 KiB of output, 2 seconds, one million steps.
    pub const NAMING: Limits = Limits {
        max_output_bytes: 256 * 1024,
        max_duration: Some(Duration::from_secs(2)),
        max_steps: 1_000_000,
    };

    /// No limits at all, for tests and trusted callers.
    pub const UNBOUNDED: Limits = Limits {
        max_output_bytes: usize::MAX,
        max_duration: None,
        max_steps: u64::MAX,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::EXPORT
    }
}

/// What a renderer may look at besides its input.
#[derive(Debug, Clone, Copy)]
pub struct RenderContext {
    /// Whether numeric zero prints as nothing in this engine.
    pub zero_is_empty: bool,
}

/// A renderer a template calls by name: `${token;name(params)}`.
pub trait NamedRenderer: Send + Sync {
    fn name(&self) -> &str;

    /// Turns the value (the token or the previous renderer's output) into a new value.
    /// `params` is the text between the parentheses, if there were any.
    fn render(
        &self,
        input: &Value,
        params: Option<&str>,
        ctx: &RenderContext,
    ) -> Result<Value, String>;
}

/// Post-processes the text of every token before it is written. Literal text of the template,
/// prefixes and suffixes are not passed through it. Used for the HTML, XML and JSON encoders
/// and for the filesystem-safe filter of the renamer.
pub trait OutputAppender: Send + Sync {
    /// `value` is the rendered token; `token` is the token as written in the template.
    fn append<'a>(&self, value: &'a str, token: &str) -> Cow<'a, str>;

    /// The text a `${@regexp ...}` pattern is matched against. By default the token as it
    /// would be written ([`Self::append`]), so a file-name filter has already run.
    fn pattern_input<'a>(&self, value: &'a str, token: &str) -> Cow<'a, str> {
        self.append(value, token)
    }

    /// The text a `${@regexp ...}` writes, built from the token's groups and the destination.
    /// By default it is written as it is.
    fn regexp_output<'a>(&self, text: &'a str, _token: &str) -> Cow<'a, str> {
        Cow::Borrowed(text)
    }
}

/// A renderer made from a function.
pub struct FnRenderer {
    name: &'static str,
    f: fn(&Value, Option<&str>, &RenderContext) -> Result<Value, String>,
}

impl FnRenderer {
    pub const fn new(
        name: &'static str,
        f: fn(&Value, Option<&str>, &RenderContext) -> Result<Value, String>,
    ) -> Self {
        FnRenderer { name, f }
    }
}

impl NamedRenderer for FnRenderer {
    fn name(&self) -> &str {
        self.name
    }

    fn render(
        &self,
        input: &Value,
        params: Option<&str>,
        ctx: &RenderContext,
    ) -> Result<Value, String> {
        (self.f)(input, params, ctx)
    }
}

/// Evaluates templates. Build one per use and share it.
pub struct Engine {
    renderers: HashMap<String, Box<dyn NamedRenderer>>,
    appender: Option<Box<dyn OutputAppender>>,
    zero_is_empty: bool,
    limits: Limits,
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

impl Engine {
    /// An engine with no renderers, no output appender, and zero printed as `0`.
    pub fn new() -> Engine {
        Engine {
            renderers: HashMap::new(),
            appender: None,
            zero_is_empty: false,
            limits: Limits::default(),
        }
    }

    /// An engine with the built-in named renderers (see [`crate::renderers`]).
    pub fn standard() -> Engine {
        let mut engine = Engine::new();
        crate::renderers::register_standard(&mut engine);
        engine
    }

    /// Makes numeric zero print as nothing, as the renamer needs.
    pub fn zero_is_empty(mut self, on: bool) -> Engine {
        self.zero_is_empty = on;
        self
    }

    /// Sets the limits every render of this engine must stay within ([`Limits::EXPORT`] by
    /// default).
    pub fn with_limits(mut self, limits: Limits) -> Engine {
        self.limits = limits;
        self
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Sets the appender every rendered token passes through.
    pub fn with_appender(mut self, appender: impl OutputAppender + 'static) -> Engine {
        self.appender = Some(Box::new(appender));
        self
    }

    /// Like [`Engine::with_appender`] for an appender that is already boxed, such as the result
    /// of [`crate::encoder_for_extension`].
    pub fn with_boxed_appender(mut self, appender: Box<dyn OutputAppender>) -> Engine {
        self.appender = Some(appender);
        self
    }

    /// Adds a renderer, replacing one with the same name.
    pub fn register(&mut self, renderer: impl NamedRenderer + 'static) {
        self.renderers
            .insert(renderer.name().to_owned(), Box::new(renderer));
    }

    pub fn has_renderer(&self, name: &str) -> bool {
        self.renderers.contains_key(name)
    }

    /// The names of the registered renderers, sorted.
    pub fn renderer_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.renderers.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Renders to a string.
    pub fn render(&self, template: &Template, model: &Value) -> Result<String, RenderError> {
        let mut out = String::new();
        self.render_to(template, model, &mut out)?;
        Ok(out)
    }

    /// Parses and renders in one call.
    pub fn render_str(&self, source: &str, model: &Value) -> Result<String, Error> {
        Ok(self.render(&Template::parse(source)?, model)?)
    }

    /// Renders into `out` as the template is walked, so a large loop never builds the whole
    /// result in memory. The render stops with an error when it passes the engine's
    /// [`Limits`]; text written until then stays in `out`.
    pub fn render_to(
        &self,
        template: &Template,
        model: &Value,
        out: &mut dyn fmt::Write,
    ) -> Result<(), RenderError> {
        let mut sink = LimitedSink {
            inner: out,
            written: 0,
            max: self.limits.max_output_bytes,
            tripped: None,
        };
        let mut run = Run {
            engine: self,
            root: model,
            frames: Vec::new(),
            steps: 0,
            deadline: self
                .limits
                .max_duration
                .map(|limit| Instant::now().checked_add(limit)),
        };
        let result = run.nodes(&template.nodes, &mut sink);
        // A limit shows up as a write error deep inside the walk; report the real reason.
        match sink.tripped {
            Some(error) => Err(error),
            None => result,
        }
    }

    /// Like [`Engine::render_to`] for an [`io::Write`] such as a buffered file. The sink
    /// should be buffered; text goes to it in small pieces.
    pub fn render_to_io(
        &self,
        template: &Template,
        model: &Value,
        out: &mut dyn io::Write,
    ) -> Result<(), RenderError> {
        let mut adapter = IoSink { inner: out };
        self.render_to(template, model, &mut adapter)
    }
}

/// An [`io::Write`] seen as a [`fmt::Write`].
struct IoSink<'a> {
    inner: &'a mut dyn io::Write,
}

impl fmt::Write for IoSink<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.write_all(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

/// Counts what goes through to the real sink and refuses text past the output limit.
struct LimitedSink<'a> {
    inner: &'a mut dyn fmt::Write,
    written: usize,
    max: usize,
    tripped: Option<RenderError>,
}

impl fmt::Write for LimitedSink<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self.written.checked_add(s.len()) {
            Some(total) if total <= self.max => self.written = total,
            _ => {
                self.tripped
                    .get_or_insert(RenderError::OutputTooLarge { limit: self.max });
                return Err(fmt::Error);
            }
        }
        self.inner.write_str(s)
    }
}

/// One `foreach` level in scope.
struct Frame<'t> {
    var: &'t str,
    item: Value,
    index: usize,
    len: usize,
}

struct Run<'e, 'm, 't> {
    engine: &'e Engine,
    root: &'m Value,
    frames: Vec<Frame<'t>>,
    steps: u64,
    /// `None`: no time limit. `Some(None)`: a limit too far away to represent, so never reached.
    deadline: Option<Option<Instant>>,
}

impl<'t> Run<'_, '_, 't> {
    /// Counts one unit of work and enforces the step and time limits.
    fn tick(&mut self) -> Result<(), RenderError> {
        self.steps += 1;
        let limit = self.engine.limits.max_steps;
        if self.steps > limit {
            return Err(RenderError::TooManySteps { limit });
        }
        if self.steps.is_multiple_of(64)
            && let Some(Some(deadline)) = self.deadline
            && Instant::now() >= deadline
        {
            return Err(RenderError::TimedOut);
        }
        Ok(())
    }

    fn nodes(&mut self, nodes: &'t [Node], out: &mut dyn fmt::Write) -> Result<(), RenderError> {
        for node in nodes {
            self.tick()?;
            match node {
                Node::Text(text) => out.write_str(text)?,
                Node::Expr(node) => self.expr(node, out)?,
                Node::If(node) => self.conditional(node, out)?,
                Node::Foreach(node) => self.foreach(node, out)?,
                Node::Regexp(node) => self.regexp(node, out)?,
            }
        }
        Ok(())
    }

    fn expr(&mut self, node: &ExprNode, out: &mut dyn fmt::Write) -> Result<(), RenderError> {
        let text = self.text_of(&node.expr)?;
        if text.is_empty() {
            return Ok(());
        }
        out.write_str(&node.prefix)?;
        self.token(&text, &node.source, out)?;
        out.write_str(&node.suffix)?;
        Ok(())
    }

    /// Writes a rendered token through the output appender.
    fn token(&self, text: &str, source: &str, out: &mut dyn fmt::Write) -> Result<(), RenderError> {
        match &self.engine.appender {
            Some(appender) => out.write_str(&appender.append(text, source))?,
            None => out.write_str(text)?,
        }
        Ok(())
    }

    fn conditional(
        &mut self,
        node: &'t IfNode,
        out: &mut dyn fmt::Write,
    ) -> Result<(), RenderError> {
        let value = self.resolve(&node.path);
        let mut holds = match &node.compare {
            None => value.is_truthy(),
            Some((op, literal)) => compare(&value, *op, literal),
        };
        if node.negate {
            holds = !holds;
        }
        self.nodes(if holds { &node.then } else { &node.otherwise }, out)
    }

    fn foreach(
        &mut self,
        node: &'t ForeachNode,
        out: &mut dyn fmt::Write,
    ) -> Result<(), RenderError> {
        let list = self.value_of(&node.list)?;
        let items = match &list {
            Value::Null => return Ok(()),
            Value::List(items) => items.clone(),
            other => std::iter::once(other.clone()).collect(),
        };
        let len = items.len();
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                out.write_str(&node.separator)?;
            }
            self.tick()?;
            self.frames.push(Frame {
                var: &node.var,
                item: item.clone(),
                index,
                len,
            });
            let result = self.nodes(&node.body, out);
            self.frames.pop();
            result?;
        }
        Ok(())
    }

    fn regexp(&mut self, node: &RegexpNode, out: &mut dyn fmt::Write) -> Result<(), RenderError> {
        // The pattern sees the token as it would be written, after the output appender (so a
        // file-name filter has already turned `: ` into ` - `).
        let text = self.text_of(&node.token)?;
        let text = match &self.engine.appender {
            Some(appender) => appender.pattern_input(&text, &node.source).into_owned(),
            None => text,
        };
        let Some(found) = node.regex.captures(&text) else {
            return Ok(());
        };
        let mut result = String::new();
        let mut chars = node.destination.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '$' && chars.peek().is_some_and(char::is_ascii_digit) {
                let mut number = 0usize;
                while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                    // A group number past the pattern's groups reads as empty.
                    number = number.saturating_mul(10).saturating_add(d as usize);
                    chars.next();
                }
                if let Some(group) = found.get(number) {
                    result.push_str(group.as_str());
                }
            } else {
                result.push(c);
            }
        }
        match &self.engine.appender {
            Some(appender) => out.write_str(&appender.regexp_output(&result, &node.source))?,
            None => out.write_str(&result)?,
        }
        Ok(())
    }

    /// The printed text of an expression: its value after the renderer chain.
    fn text_of(&mut self, expr: &Expr) -> Result<String, RenderError> {
        let value = self.value_of(expr)?;
        Ok(value.to_text(self.engine.zero_is_empty))
    }

    fn value_of(&mut self, expr: &Expr) -> Result<Value, RenderError> {
        let mut value = self.resolve(&expr.path);
        for call in &expr.chain {
            value = self.call(call, &value)?;
        }
        Ok(value)
    }

    fn call(&self, call: &Call, input: &Value) -> Result<Value, RenderError> {
        let Some(renderer) = self.engine.renderers.get(&call.name) else {
            // A renderer the engine does not have passes the value through unchanged;
            // template validation reports unknown renderers.
            return Ok(input.clone());
        };
        let ctx = RenderContext {
            zero_is_empty: self.engine.zero_is_empty,
        };
        renderer
            .render(input, call.params.as_deref(), &ctx)
            .map_err(|message| RenderError::Renderer {
                name: call.name.clone(),
                message,
            })
    }

    /// Follows a path from the loop variables, then the model. Anything missing is null.
    fn resolve(&self, path: &Path) -> Value {
        let mut segments = path.segments.iter();
        let Some(first) = segments.next() else {
            return Value::Null;
        };
        let mut current = self.lookup(&first.name);
        for index in &first.indexes {
            current = apply_index(&current, *index);
        }
        for segment in segments {
            if matches!(current, Value::Null) {
                return Value::Null;
            }
            current = current.get(&segment.name).unwrap_or(Value::Null);
            for index in &segment.indexes {
                current = apply_index(&current, *index);
            }
        }
        current
    }

    fn lookup(&self, name: &str) -> Value {
        for frame in self.frames.iter().rev() {
            if name == frame.var {
                return frame.item.clone();
            }
            if let Some(var) = name.strip_prefix("index_") {
                if var == frame.var {
                    return Value::Int(frame.index as i64);
                }
            } else if let Some(var) = name.strip_prefix("first_") {
                if var == frame.var {
                    return Value::Bool(frame.index == 0);
                }
            } else if let Some(var) = name.strip_prefix("last_")
                && var == frame.var
            {
                return Value::Bool(frame.index + 1 == frame.len);
            }
        }
        self.root.get(name).unwrap_or(Value::Null)
    }
}

/// `value op literal` for `${if value op literal}`. Equality compares the printed text;
/// ordering compares numbers and is false when either side is not a number.
fn compare(value: &Value, op: Compare, literal: &str) -> bool {
    match op {
        Compare::Equal => value.to_text(false) == literal,
        Compare::NotEqual => value.to_text(false) != literal,
        ordering => {
            let (Some(left), Ok(right)) = (value.as_f64(), literal.trim().parse::<f64>()) else {
                return false;
            };
            match ordering {
                Compare::Greater => left > right,
                Compare::GreaterOrEqual => left >= right,
                Compare::Less => left < right,
                _ => left <= right,
            }
        }
    }
}

/// `[n]`, `[last]` and `[start,count]` on lists and on text (counted in characters).
fn apply_index(value: &Value, index: Index) -> Value {
    match value {
        Value::List(items) => match index {
            Index::At(n) => items.get(n).cloned().unwrap_or(Value::Null),
            Index::Last => items.last().cloned().unwrap_or(Value::Null),
            Index::Slice(start, count) => {
                Value::list(items.iter().skip(start).take(count).cloned())
            }
        },
        Value::Text(text) => match index {
            Index::At(n) => text
                .chars()
                .nth(n)
                .map_or(Value::Null, |c| Value::text(c.to_string())),
            Index::Last => text
                .chars()
                .last()
                .map_or(Value::Null, |c| Value::text(c.to_string())),
            Index::Slice(start, count) => {
                Value::text(text.chars().skip(start).take(count).collect::<String>())
            }
        },
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Value {
        let actor = |name: &str, role: &str| {
            Value::map([("name", Value::text(name)), ("role", Value::text(role))])
        };
        let movie = |title: &str, year: i64, edition: &str| {
            Value::map([
                ("title", Value::text(title)),
                ("year", Value::Int(year)),
                ("edition", Value::text(edition)),
                (
                    "actors",
                    Value::list([actor("Al", "Hero"), actor("Bo", "Friend")]),
                ),
            ])
        };
        Value::map([
            (
                "movies",
                Value::list([movie("Heat", 1995, ""), movie("Arrival", 2016, "Extended")]),
            ),
            ("movie", movie("Heat", 1995, "")),
            ("zero", Value::Int(0)),
            ("none", Value::Null),
        ])
    }

    fn render(source: &str) -> String {
        Engine::new().render_str(source, &model()).unwrap()
    }

    #[test]
    fn plain_tokens_and_paths() {
        assert_eq!(render("${movie.title} (${movie.year})"), "Heat (1995)");
        assert_eq!(render("${nothing}|${movie.nothing}|${none.x}"), "||");
    }

    #[test]
    fn prefix_and_suffix_appear_only_with_a_value() {
        assert_eq!(
            render("${movie.title}${ - ,movie.edition,} (${movie.year})"),
            "Heat (1995)"
        );
        assert_eq!(render("${[,movie.title,]}"), "[Heat]");
        assert_eq!(render("${ - ,missing,}x"), "x");
    }

    #[test]
    fn zero_values_depend_on_the_engine() {
        assert_eq!(render("T (${zero})"), "T (0)");
        let engine = Engine::new().zero_is_empty(true);
        assert_eq!(engine.render_str("T (${zero})", &model()).unwrap(), "T ()");
        assert_eq!(engine.render_str("T${ (,zero,)}", &model()).unwrap(), "T");
    }

    #[test]
    fn if_else_and_comparison() {
        assert_eq!(render("${if movie.title}yes${else}no${end}"), "yes");
        assert_eq!(render("${if movie.edition}yes${else}no${end}"), "no");
        assert_eq!(render("${if !movie.edition}none${end}"), "none");
        assert_eq!(render("${if movie.title == 'Heat'}hot${end}"), "hot");
        assert_eq!(render("${if movie.title = Heat}hot${end}"), "hot");
        assert_eq!(
            render("${if movie.title != Heat}x${else}same${end}"),
            "same"
        );
        assert_eq!(render("${if movie.actors}a${end}${if missing}b${end}"), "a");
        // Numbers are true, zero included; ordering compares numbers.
        assert_eq!(render("${if zero}t${else}f${end}"), "t");
        assert_eq!(render("${if movie.year > 1990}new${else}old${end}"), "new");
        assert_eq!(
            render("${if movie.year >= 1995}y${end}${if movie.year < 1995}n${end}"),
            "y"
        );
        assert_eq!(render("${if movie.year <= 1995}y${end}"), "y");
        assert_eq!(render("${if movie.title > 5}y${else}n${end}"), "n");
    }

    #[test]
    fn foreach_with_separator_and_loop_variables() {
        assert_eq!(
            render("${foreach movies movie}${movie.title};${end}"),
            "Heat;Arrival;"
        );
        assert_eq!(
            render("${foreach movie.actors a , }${a.name} as ${a.role}${end}"),
            "Al as Hero, Bo as Friend"
        );
        assert_eq!(
            render("${foreach movies m}${index_m}${if first_m}F${end}${if last_m}L${end} ${end}"),
            "0F 1L "
        );
        assert_eq!(render("${foreach missing x}never${end}"), "");
        // A scalar loops once.
        assert_eq!(render("${foreach movie.title t}[${t}]${end}"), "[Heat]");
    }

    #[test]
    fn nested_loops_shadow_outer_variables() {
        assert_eq!(
            render("${foreach movies m}${foreach m.actors m}${m.name}${end}|${end}"),
            "AlBo|AlBo|"
        );
    }

    #[test]
    fn indexing_and_slicing() {
        assert_eq!(render("${movie.actors[0].name}"), "Al");
        assert_eq!(render("${movie.actors[last].name}"), "Bo");
        assert_eq!(render("${movie.actors[5].name}"), "");
        assert_eq!(render("${movie.title[0]}"), "H");
        assert_eq!(render("${movie.title[1,2]}"), "ea");
        assert_eq!(render("${movie.title[2,10]}"), "at");
        assert_eq!(render("${movie.title[last]}"), "t");
        assert_eq!(
            render("${foreach movie.actors[0,1] a}${a.name}${end}"),
            "Al"
        );
    }

    #[test]
    fn regexp_processor() {
        assert_eq!(
            render(r#"${@regexp "(\w+) (\w+)" movie.title "$2 $1"}"#),
            ""
        );
        let m = Value::map([("t", Value::text("Dune: Part Two"))]);
        let engine = Engine::new();
        assert_eq!(
            engine
                .render_str(r#"${@regexp "(\w+) (\w+)" t "$2 $1"}"#, &m)
                .unwrap(),
            "Two Part"
        );
        // Case-insensitive, group 0, missing group.
        assert_eq!(
            engine
                .render_str(r#"${@regexp "PART" t "[$0|$3]"}"#, &m)
                .unwrap(),
            "[Part|]"
        );
        // Quantifier braces do not end the expression.
        assert_eq!(
            engine
                .render_str(r#"${@regexp "\w{4}" t "$0"}"#, &m)
                .unwrap(),
            "Dune"
        );
    }

    #[test]
    fn escapes_keep_text_literal() {
        assert_eq!(
            render(r"\${movie.title} ${movie.title}"),
            "${movie.title} Heat"
        );
    }

    #[test]
    fn unknown_renderer_passes_the_value_through() {
        assert_eq!(render("${movie.title;nope}"), "Heat");
        assert_eq!(render("${movie.title;a;b}"), "Heat");
    }

    #[test]
    fn an_appender_can_see_the_raw_value_and_wrap_the_regexp_result() {
        struct Marks;
        impl OutputAppender for Marks {
            fn append<'a>(&self, value: &'a str, _token: &str) -> Cow<'a, str> {
                Cow::Owned(format!("[{value}]"))
            }
            fn pattern_input<'a>(&self, value: &'a str, _token: &str) -> Cow<'a, str> {
                Cow::Borrowed(value)
            }
            fn regexp_output<'a>(&self, text: &'a str, _token: &str) -> Cow<'a, str> {
                Cow::Owned(format!("<{text}>"))
            }
        }
        let engine = Engine::new().with_appender(Marks);
        assert_eq!(
            engine
                .render_str(r#"x${@regexp "^(H)" movie.title "$1!"}y"#, &model())
                .unwrap(),
            "x<H!>y"
        );
        // The default keeps the old behaviour: the pattern sees the appended text and the
        // result is written as it is.
        struct Brackets;
        impl OutputAppender for Brackets {
            fn append<'a>(&self, value: &'a str, _token: &str) -> Cow<'a, str> {
                Cow::Owned(format!("[{value}]"))
            }
        }
        let engine = Engine::new().with_appender(Brackets);
        assert_eq!(
            engine
                .render_str(r#"${@regexp "(.)(H)" movie.title "$1$2"}"#, &model())
                .unwrap(),
            "[H"
        );
    }

    #[test]
    fn appender_wraps_tokens_but_not_literals() {
        struct Brackets;
        impl OutputAppender for Brackets {
            fn append<'a>(&self, value: &'a str, token: &str) -> Cow<'a, str> {
                Cow::Owned(format!("<{token}:{value}>"))
            }
        }
        let engine = Engine::new().with_appender(Brackets);
        assert_eq!(
            engine
                .render_str("a${ - ,movie.title,!}b", &model())
                .unwrap(),
            "a - <movie.title:Heat>!b"
        );
    }

    #[test]
    fn renders_a_large_list_into_a_writer() {
        let movies = Value::list((0..20_000).map(|i| Value::map([("n", Value::Int(i))])));
        let model = Value::map([("movies", movies)]);
        let engine = Engine::new();
        let template = Template::parse("${foreach movies m}${m.n},${end}").unwrap();
        struct Count(usize);
        impl fmt::Write for Count {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.0 += s.len();
                Ok(())
            }
        }
        let mut sink = Count(0);
        engine.render_to(&template, &model, &mut sink).unwrap();
        assert_eq!(sink.0, 108_890);
    }
}
