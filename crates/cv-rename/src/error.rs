//! Why a rename cannot be planned.

/// An error from planning a rename.
#[derive(Debug, thiserror::Error)]
pub enum RenameError {
    /// The item is not a movie (or not a show for the TV planner).
    #[error("the item is not a movie")]
    NotAMovie,
    #[error("the item is not a TV show")]
    NotAShow,
    /// A title is needed for naming; an item without one is left alone.
    #[error("the item has no title")]
    NoTitle,
    /// The pattern could not be parsed or rendered.
    #[error("the naming pattern is not valid: {0}")]
    Template(#[from] cv_template::Error),
    /// A TV file pattern without an episode token would give every episode of a season the
    /// same name.
    #[error("the episode file pattern has no episode token")]
    EpisodePatternWithoutEpisode,
    /// A name typed to resolve a conflict is empty once cleaned.
    #[error("the edited name for item {item} is empty")]
    EmptyEditedName { item: String },
    /// The resolution needs a video base name, and a show has none.
    #[error("the resolution does not apply to item {item}")]
    ResolutionNotApplicable { item: String },
}
