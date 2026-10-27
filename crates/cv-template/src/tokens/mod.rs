//! Token alias maps and the models they read (movie and TV).
//!
//! Naming templates use short tokens such as `${title}`, `${videoFormat}` or `${seasonNr2}`. Each
//! one is an alias for a model path with an optional renderer, for example `videoBitRate` for
//! `movie.mediaInfoVideoBitrate;bitrate`. The pieces:
//!
//! - [`AliasMap`] holds the aliases: [`movie_aliases`] and [`tv_aliases`];
//! - [`AliasMap::morph`] rewrites a template so that it only uses model paths;
//! - [`movie_model`] and [`tv_model`] build the model a morphed template reads from library
//!   items;
//! - [`renamer_engine`] builds an [`Engine`](crate::Engine) with the renamer's rules: zero and
//!   empty values print nothing, tokens are made safe for file names, and the renamer renderers
//!   are registered.
//! - [`render_name`] ties them together: morph, parse with the renamer's backslash rule, render;
//! - [`movie_validator`] and [`tv_validator`] check what the user typed against the alias maps
//!   and the model fields (see [`crate::validate`]).
//!
//! # Zero and empty values
//!
//! Numeric zero, null, empty text and empty lists print as empty text, so that
//! `${title} (${year})` for a movie without a year gives `Title ()`, which the renamer's
//! clean-up turns into `Title`, and `${ - ,edition,}` leaves no dangling separator. Numbers that
//! pass through a renderer (`seasonNr`, `videoBitRate`, ...) print as the renderer formats them,
//! so season 0 prints `0` with `${seasonNr}`.
//!
//! ```
//! use cv_domain::Item;
//! use cv_template::tokens::{ModelOptions, RenamerOptions, movie_model, render_name, movie_aliases};
//!
//! let mut item = Item::movie("Heat");
//! item.core.year = 1995;
//! let model = movie_model(&item, None, &ModelOptions::default());
//! let name = render_name(
//!     "${title} (${year})",
//!     &movie_aliases(),
//!     &model,
//!     &RenamerOptions::default(),
//! )
//! .unwrap();
//! assert_eq!(name, "Heat (1995)");
//! ```

mod aliases;
mod appender;
mod labels;
mod media;
mod morph;
mod movie;
mod tv;
mod validation;

pub use aliases::{AliasMap, movie_aliases, tv_aliases};
pub use appender::{
    NameTokenAppender, RenamerOptions, renamer_engine, render_name, render_name_with,
    unicode_lookalike,
};
pub use labels::{
    DEFAULT_TITLE_PREFIXES, audio_channels_dot, decade_long, decade_short, edition_label,
    genre_label, media_source_label, sortable_title, status_label,
};
pub use movie::{MovieSetContext, movie_model};
pub use tv::tv_model;
pub use validation::{movie_roots, movie_validator, tv_roots, tv_validator};

/// Settings the models need that are not part of a library item.
#[derive(Debug, Clone)]
pub struct ModelOptions {
    /// Leading words that move to the end of a sortable title (`Matrix, The`).
    pub title_prefixes: Vec<String>,
    /// The rating source `rating` reads (`imdb`, `themoviedb`, ...). When it is `None`, or the
    /// item has no rating from it, the first rating other than the user's own is used.
    pub rating_source: Option<String>,
}

impl Default for ModelOptions {
    fn default() -> Self {
        ModelOptions {
            title_prefixes: DEFAULT_TITLE_PREFIXES
                .iter()
                .map(|p| (*p).to_owned())
                .collect(),
            rating_source: None,
        }
    }
}
