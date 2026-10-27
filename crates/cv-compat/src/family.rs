use std::fmt;

/// A group of fixture cases that one handler produces output for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    Classification,
    TitleParsing,
    IdParsing,
    Stacking,
    Episodes,
    Nfo,
    RenameDestinations,
    RenameTrees,
    RenameDestinationsTv,
    RenameTreesTv,
}

/// Where a family's inputs and expected outputs live, relative to `fixtures/` and `expected/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One JSON file of cases and one JSON file of expected results with the same ids.
    Json {
        fixture: &'static str,
        expected: &'static str,
    },
    /// One JSON file of cases; the expected side is `<dir>/index.json` plus `<dir>/<id>/<file>`.
    Files {
        fixture: &'static str,
        expected_dir: &'static str,
    },
}

impl Family {
    pub const ALL: [Family; 10] = [
        Family::Classification,
        Family::TitleParsing,
        Family::IdParsing,
        Family::Stacking,
        Family::Episodes,
        Family::Nfo,
        Family::RenameDestinations,
        Family::RenameTrees,
        Family::RenameDestinationsTv,
        Family::RenameTreesTv,
    ];

    /// Stable name used in reports.
    pub fn name(self) -> &'static str {
        match self {
            Family::Classification => "classification",
            Family::TitleParsing => "title-parsing",
            Family::IdParsing => "id-parsing",
            Family::Stacking => "stacking",
            Family::Episodes => "episodes",
            Family::Nfo => "nfo",
            Family::RenameDestinations => "rename-destinations",
            Family::RenameTrees => "rename-trees",
            Family::RenameDestinationsTv => "rename-destinations-tv",
            Family::RenameTreesTv => "rename-trees-tv",
        }
    }

    pub fn from_name(name: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|f| f.name() == name)
    }

    pub fn layout(self) -> Layout {
        match self {
            Family::Classification => Layout::Json {
                fixture: "classification.json",
                expected: "classification.json",
            },
            Family::TitleParsing => Layout::Json {
                fixture: "title-parsing.json",
                expected: "title-parsing.json",
            },
            Family::IdParsing => Layout::Json {
                fixture: "id-parsing.json",
                expected: "id-parsing.json",
            },
            Family::Stacking => Layout::Json {
                fixture: "stacking.json",
                expected: "stacking.json",
            },
            Family::Episodes => Layout::Json {
                fixture: "episodes.json",
                expected: "episodes.json",
            },
            Family::Nfo => Layout::Files {
                fixture: "nfo/cases.json",
                expected_dir: "nfo",
            },
            Family::RenameDestinations => Layout::Json {
                fixture: "rename/destinations.json",
                expected: "rename/destinations.json",
            },
            Family::RenameTrees => Layout::Json {
                fixture: "rename/trees.json",
                expected: "rename/trees.json",
            },
            Family::RenameDestinationsTv => Layout::Json {
                fixture: "rename/destinations-tv.json",
                expected: "rename/destinations-tv.json",
            },
            Family::RenameTreesTv => Layout::Json {
                fixture: "rename/trees-tv.json",
                expected: "rename/trees-tv.json",
            },
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
