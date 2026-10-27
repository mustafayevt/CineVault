//! Template engine used by the renamer.
//!
//! It uses a `${token}` template syntax compatible with common renamer patterns (see the
//! template-engine spec): `${token}`, dotted and indexed paths, `${prefix,token,suffix}` segments,
//! `${if}`/`${else}`/`${end}`, `${foreach list item [separator]}`, `${@regexp ...}` and named
//! renderers chained with `;`.
//!
//! ```
//! use cv_template::{Engine, Value};
//!
//! let model = Value::map([("title", Value::text("Heat")), ("year", Value::Int(1995))]);
//! let engine = Engine::standard();
//! let text = engine
//!     .render_str("${title;upper}${ - ,edition,} (${year})", &model)
//!     .unwrap();
//! assert_eq!(text, "HEAT (1995)");
//! ```
//!
//! Parse once with [`Template::parse`], render many times with an [`Engine`]. Output is written
//! to a [`std::fmt::Write`] (or an [`std::io::Write`]) as the template is walked, and every
//! render stays within the engine's [`Limits`] on output size, time and work.
//!
//! Editors check a template before it is used with a [`Validator`]: syntax errors with
//! positions, unknown tokens and renderers with suggestions, and the rules for TV file patterns.

pub mod datetime;
pub mod decimal;
pub mod encode;
pub mod eval;
pub mod parse;
pub mod renamer_renderers;
pub mod renderers;
pub mod tokens;
pub mod validate;
pub mod value;

pub use datetime::DateTime;
pub use encode::{HtmlEncoder, JsonEncoder, XmlEncoder, encoder_for_extension};
pub use eval::{
    Engine, Error, FnRenderer, Limits, NamedRenderer, OutputAppender, RenderContext, RenderError,
};
pub use parse::{ParseOptions, SyntaxError, Template};
pub use renamer_renderers::{
    RenamerRenderers, ReplaceTables, register_movie_set_index, register_renamer,
    register_replace_tables,
};
pub use validate::{
    Diagnostic, DiagnosticKind, ModelRoot, Severity, TvPatternReport, ValidationReport, Validator,
    suggest,
};
pub use value::{Object, Value};
