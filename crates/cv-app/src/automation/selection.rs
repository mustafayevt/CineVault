//! Which titles a batch works on: all of the library, or the ones a data source, a folder or a
//! quick-search query picks out. The selectors narrow each other.

use cv_db::repo::filter::Filter;
use cv_db::repo::{ItemQuery, data_sources::list_sources, list_items};
use cv_db::{Database, DbError};
use cv_domain::{DataSource, ItemId};

use super::model::{EntityKind, SelectionSpec, SelectionSummary};
use super::query::parse_quick_search;
use crate::app_settings::SettingsService;
use crate::ratings::rating_preference_from_values;

/// Why a selection could not be made.
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
    #[error("the library could not be read: {0}")]
    Db(#[from] DbError),
    #[error("there is no {kind} data source `{given}`; {choices}")]
    UnknownSource {
        kind: &'static str,
        given: String,
        choices: String,
    },
    #[error("`{0}` is not inside any data source of this library")]
    PathOutsideSources(String),
    #[error("the path `{0}` is not a full path")]
    PathNotAbsolute(String),
    #[error(
        "the search `{query}` was not understood: {problems}. Write a field and a value, such as \
         year:2010..2019, hdr:dv or -watched."
    )]
    BadQuery { query: String, problems: String },
}

/// One selected title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub id: ItemId,
    pub title: String,
    pub year: u16,
    /// The title's folder.
    pub path: String,
    /// The root of the data source it belongs to.
    pub data_source: String,
}

/// The titles of a selection.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub items: Vec<Picked>,
    /// A source, path or query narrowed the library.
    pub narrowed: bool,
    /// The data sources the selection covers: the chosen ones, the ones that hold the chosen
    /// folders, or all of the kind.
    pub sources: Vec<DataSource>,
}

impl Selection {
    /// The ids of the selected titles.
    #[must_use]
    pub fn ids(&self) -> Vec<ItemId> {
        self.items.iter().map(|item| item.id).collect()
    }

    #[must_use]
    pub fn summary(&self) -> SelectionSummary {
        SelectionSummary {
            titles: self.items.len(),
            narrowed: self.narrowed,
            sources: self.sources.iter().map(|s| s.path.clone()).collect(),
        }
    }
}

/// Lower-cases on systems whose file names ignore case.
fn fold(text: &str) -> String {
    let slashed = text.replace('\\', "/");
    let trimmed = slashed.trim_end_matches('/');
    if cfg!(any(windows, target_os = "macos")) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_owned()
    }
}

/// True when `child` is `parent` or lies below it.
#[must_use]
pub fn is_inside(child: &str, parent: &str) -> bool {
    let (child, parent) = (fold(child), fold(parent));
    child == parent
        || child
            .strip_prefix(&parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn describe(sources: &[DataSource]) -> String {
    if sources.is_empty() {
        return "none has been added yet".to_owned();
    }
    let list: Vec<String> = sources
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{} = {}", i + 1, s.path))
        .collect();
    format!("choose one of: {}", list.join(", "))
}

fn find_source<'a>(
    sources: &'a [DataSource],
    kind: EntityKind,
    token: &str,
) -> Result<&'a DataSource, SelectionError> {
    let token = token.trim();
    let found = match token.parse::<usize>() {
        Ok(n) => n.checked_sub(1).and_then(|i| sources.get(i)),
        Err(_) => sources.iter().find(|s| fold(&s.path) == fold(token)),
    };
    found.ok_or_else(|| SelectionError::UnknownSource {
        kind: kind.name(),
        given: token.to_owned(),
        choices: describe(sources),
    })
}

fn is_absolute(path: &str) -> bool {
    std::path::Path::new(path).is_absolute() || path.starts_with("\\\\") || path.starts_with('/')
}

/// Resolves a selection against the library.
///
/// # Errors
/// A source, path or query that cannot be used, or a library that cannot be read.
pub fn resolve(
    db: &Database,
    settings: &SettingsService,
    kind: EntityKind,
    spec: &SelectionSpec,
) -> Result<Selection, SelectionError> {
    let snapshot = settings.snapshot();
    let preference = rating_preference_from_values(&snapshot.values);
    let all_sources = db.read(|conn| list_sources(conn, Some(kind.source_kind())))?;

    // The data sources the selection covers.
    let mut covered: Vec<DataSource> = Vec::new();
    for token in &spec.sources {
        let source = find_source(&all_sources, kind, token)?;
        if !covered.iter().any(|s| s.id == source.id) {
            covered.push(source.clone());
        }
    }
    for path in &spec.paths {
        if !is_absolute(path) {
            return Err(SelectionError::PathNotAbsolute(path.clone()));
        }
        // Only the sources that hold the folder matter, unless sources were chosen as well.
        if spec.sources.is_empty() {
            let holders: Vec<&DataSource> = all_sources
                .iter()
                .filter(|s| is_inside(path, &s.path))
                .collect();
            if holders.is_empty() {
                return Err(SelectionError::PathOutsideSources(path.clone()));
            }
            for source in holders {
                if !covered.iter().any(|s| s.id == source.id) {
                    covered.push(source.clone());
                }
            }
        }
    }
    let sources = if covered.is_empty() {
        all_sources
    } else {
        covered
    };

    let mut filters: Vec<Filter> = Vec::new();
    if !spec.sources.is_empty() || !spec.paths.is_empty() {
        filters.push(Filter::DataSource {
            values: sources.iter().map(|s| s.path.clone()).collect(),
        });
    }
    if let Some(text) = &spec.query {
        let parsed = parse_quick_search(text);
        let mut problems: Vec<String> = parsed
            .rejected
            .iter()
            .map(|term| format!("`{term}` is not valid"))
            .collect();
        if parsed.open_quote {
            problems.push("a quote is not closed".to_owned());
        }
        if !problems.is_empty() {
            return Err(SelectionError::BadQuery {
                query: text.clone(),
                problems: problems.join(", "),
            });
        }
        filters.extend(parsed.filters(&preference));
    }

    let mut query = ItemQuery::new(kind.item_kind()).rating_preference(preference);
    if !filters.is_empty() {
        query = query.filter(Filter::and(filters));
    }

    let mut items = Vec::new();
    let mut cursor = None;
    loop {
        let page = db.read(|conn| list_items(conn, &query, cursor.as_ref(), 500))?;
        items.extend(page.items.into_iter().map(|summary| Picked {
            id: summary.id,
            title: summary.title,
            year: summary.year,
            path: summary.path,
            data_source: summary.data_source,
        }));
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    if !spec.paths.is_empty() {
        items.retain(|item| spec.paths.iter().any(|path| is_inside(&item.path, path)));
    }
    if !spec.ids.is_empty() {
        items.retain(|item| spec.ids.contains(&item.id.0));
    }

    Ok(Selection {
        items,
        narrowed: !spec.is_empty(),
        sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_inside_itself_and_below_but_not_a_sibling_with_the_same_prefix() {
        assert!(is_inside("D:\\Movies\\Heat (1995)", "D:\\Movies"));
        assert!(is_inside("D:/Movies/", "D:\\Movies"));
        assert!(is_inside("D:\\Movies", "D:\\Movies"));
        assert!(!is_inside("D:\\Movies2\\Heat", "D:\\Movies"));
        assert!(!is_inside("D:\\Movies", "D:\\Movies\\Heat"));
    }

    #[test]
    fn sources_are_found_by_position_or_path_with_a_helpful_error() {
        let sources = vec![
            DataSource {
                id: 1,
                kind: cv_domain::SourceKind::Movie,
                path: "D:\\Movies".into(),
                position: 0,
                label: String::new(),
                created_ms: 0,
            },
            DataSource {
                id: 2,
                kind: cv_domain::SourceKind::Movie,
                path: "E:\\More".into(),
                position: 1,
                label: String::new(),
                created_ms: 0,
            },
        ];
        assert_eq!(find_source(&sources, EntityKind::Movie, "2").unwrap().id, 2);
        assert_eq!(
            find_source(&sources, EntityKind::Movie, "D:/Movies/")
                .unwrap()
                .id,
            1
        );
        let error = find_source(&sources, EntityKind::Movie, "3").unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("1 = D:\\Movies") && text.contains("2 = E:\\More"),
            "{text}"
        );
        assert!(find_source(&sources, EntityKind::Movie, "0").is_err());
    }
}
