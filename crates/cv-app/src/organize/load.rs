//! Loading the items in scope, with their files shaped for the planner.

use std::collections::BTreeSet;

use cv_db::repo::{ItemQuery, children_of, get_availability, get_item, get_summary, list_items};
use cv_db::{Database, DbError};
use cv_domain::{
    Availability, Item, ItemDetail, ItemId, ItemKind, MediaFile, MediaFileType, StreamFlag,
};
use cv_media::external::subtitle_file_stream;
use cv_rename::{PlanFile, SeasonInput, SubtitleTag};
use cv_template::tokens::MovieSetContext;

use super::model::Scope;

/// A movie with what the planner needs.
pub(crate) struct LoadedMovie {
    pub id: ItemId,
    pub item: Item,
    pub set: Option<MovieSetContext>,
    /// The data source root as stored.
    pub root: String,
    /// The movie's folder relative to the root.
    pub folder: String,
    pub files: Vec<PlanFile>,
}

/// A show with its seasons and episodes.
pub(crate) struct LoadedShow {
    pub id: ItemId,
    pub show: Item,
    pub root: String,
    pub folder: String,
    pub files: Vec<PlanFile>,
    pub seasons: Vec<SeasonInput>,
    pub episodes: Vec<(Item, Vec<PlanFile>)>,
}

/// An item in scope.
pub(crate) enum Loaded {
    Movie(LoadedMovie),
    Show(LoadedShow),
    /// It cannot be planned; the text says why.
    Unplannable {
        id: ItemId,
        kind: ItemKind,
        title: String,
        year: u16,
        reason: String,
    },
}

/// The movies and shows `scope` stands for, in a stable order, without duplicates.
pub(crate) fn scope_ids(db: &Database, scope: &Scope) -> Result<Vec<ItemId>, DbError> {
    match scope {
        Scope::Everything => {
            let mut ids = all_ids(db, ItemKind::Movie)?;
            ids.extend(all_ids(db, ItemKind::Show)?);
            Ok(ids)
        }
        Scope::Items(wanted) => {
            let mut seen: BTreeSet<ItemId> = BTreeSet::new();
            let mut ids = Vec::new();
            for id in wanted {
                for resolved in expand(db, *id)? {
                    if seen.insert(resolved) {
                        ids.push(resolved);
                    }
                }
            }
            Ok(ids)
        }
    }
}

fn all_ids(db: &Database, kind: ItemKind) -> Result<Vec<ItemId>, DbError> {
    let query = ItemQuery::new(kind);
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = db.read(|c| list_items(c, &query, cursor.as_ref(), 500))?;
        ids.extend(page.items.iter().map(|s| s.id));
        match page.next {
            Some(next) => cursor = Some(next),
            None => return Ok(ids),
        }
    }
}

/// The movie or show ids one selected item stands for.
fn expand(db: &Database, id: ItemId) -> Result<Vec<ItemId>, DbError> {
    let Some(summary) = db.read(|c| get_summary(c, id))? else {
        return Ok(Vec::new());
    };
    Ok(match summary.kind {
        ItemKind::Movie | ItemKind::Show => vec![id],
        ItemKind::Set => db.read(|c| {
            let mut movies = Vec::new();
            for child in children_of(c, id)? {
                if get_summary(c, child)?.is_some_and(|s| s.kind == ItemKind::Movie) {
                    movies.push(child);
                }
            }
            Ok(movies)
        })?,
        ItemKind::Season | ItemKind::Episode => summary.parent_id.into_iter().collect(),
    })
}

/// Loads the items of `ids`. An id that is gone is left out. An item whose files are missing or
/// whose library folder cannot be reached is not planned: its stored paths say nothing about
/// what is on disk, and another row may hold the same files.
pub(crate) fn load(db: &Database, ids: &[ItemId]) -> Result<Vec<Loaded>, DbError> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(item) = db.read(|c| get_item(c, *id))? else {
            continue;
        };
        let (availability, _) = db.read(|c| get_availability(c, *id))?;
        let noun = if matches!(item.detail, ItemDetail::Show(_)) {
            "show"
        } else {
            "movie"
        };
        match availability {
            Availability::Missing => {
                out.push(unplannable(
                    *id,
                    &item,
                    format!("The files of the {noun} are missing. Locate or remove it first."),
                ));
                continue;
            }
            Availability::Offline => {
                out.push(unplannable(
                    *id,
                    &item,
                    format!("The library folder of the {noun} cannot be reached."),
                ));
                continue;
            }
            Availability::Online | Availability::Invalid => {}
        }
        match &item.detail {
            ItemDetail::Movie(_) => out.push(load_movie(db, *id, item)?),
            ItemDetail::Show(_) => out.push(load_show(db, *id, item)?),
            _ => {}
        }
    }
    Ok(out)
}

fn unplannable(id: ItemId, item: &Item, reason: impl Into<String>) -> Loaded {
    Loaded::Unplannable {
        id,
        kind: item.detail.kind(),
        title: item.core.title.clone(),
        year: item.core.year,
        reason: reason.into(),
    }
}

fn load_movie(db: &Database, id: ItemId, item: Item) -> Result<Loaded, DbError> {
    let root = trim_root(&item.core.data_source);
    if root.is_empty() {
        return Ok(unplannable(
            id,
            &item,
            "The movie is not in a library folder.",
        ));
    }
    let Some(files) = plan_files(&root, &item.core.media_files) else {
        return Ok(unplannable(
            id,
            &item,
            "A file of the movie is outside its library folder.",
        ));
    };
    let folder = match relative_to(&root, &item.core.path) {
        Some(folder) if !item.core.path.is_empty() => folder,
        _ => files
            .iter()
            .find(|f| f.kind == MediaFileType::Video)
            .map(|f| f.parent().to_owned())
            .unwrap_or_default(),
    };
    let set = match item.parent_id {
        Some(parent) => set_context(db, parent)?,
        None => None,
    };
    Ok(Loaded::Movie(LoadedMovie {
        id,
        item,
        set,
        root,
        folder,
        files,
    }))
}

/// The collection a movie belongs to, when its parent is a set.
pub(crate) fn set_context(
    db: &Database,
    parent: ItemId,
) -> Result<Option<MovieSetContext>, DbError> {
    db.read(|conn| {
        let Some(set) = get_item(conn, parent)? else {
            return Ok(None);
        };
        let ItemDetail::Set(fields) = &set.detail else {
            return Ok(None);
        };
        // Release order, with a place for each released movie the library lacks, so that
        // `${movieSetIndex}` and `${movieSetIndex2}` count like the collection page does.
        let (movies, movies_with_dummy) =
            match crate::collections::play_order(conn, parent, crate::collections::today()) {
                Ok(order) => order,
                Err(crate::collections::CollectionError::Db(error)) => return Err(error),
                Err(other) => return Err(DbError::InvalidData(other.to_string())),
            };
        Ok(Some(MovieSetContext {
            title: set.core.title.clone(),
            sort_title: if fields.sort_title.is_empty() {
                set.core.title.clone()
            } else {
                fields.sort_title.clone()
            },
            movies_with_dummy,
            movies,
        }))
    })
}

fn load_show(db: &Database, id: ItemId, show: Item) -> Result<Loaded, DbError> {
    let root = trim_root(&show.core.data_source);
    if root.is_empty() {
        return Ok(unplannable(
            id,
            &show,
            "The show is not in a library folder.",
        ));
    }
    let Some(files) = plan_files(&root, &show.core.media_files) else {
        return Ok(unplannable(
            id,
            &show,
            "A file of the show is outside its library folder.",
        ));
    };
    let Some(folder) = relative_to(&root, &show.core.path).filter(|_| !show.core.path.is_empty())
    else {
        return Ok(unplannable(id, &show, "The show has no folder."));
    };
    let children = db.read(|c| {
        let mut out = Vec::new();
        for child in children_of(c, id)? {
            if let Some(item) = get_item(c, child)? {
                out.push(item);
            }
        }
        Ok(out)
    })?;
    let mut seasons = Vec::new();
    let mut episodes = Vec::new();
    for child in children {
        let Some(child_files) = plan_files(&root, &child.core.media_files) else {
            return Ok(unplannable(
                id,
                &show,
                "A file of the show is outside its library folder.",
            ));
        };
        match &child.detail {
            ItemDetail::Season(s) => seasons.push(SeasonInput {
                season: s.season,
                files: child_files,
            }),
            ItemDetail::Episode(_) => episodes.push((child, child_files)),
            _ => {}
        }
    }
    Ok(Loaded::Show(LoadedShow {
        id,
        show,
        root,
        folder,
        files,
        seasons,
        episodes,
    }))
}

/// The files of an item for the planner, or `None` when one lies outside `root`.
fn plan_files(root: &str, files: &[MediaFile]) -> Option<Vec<PlanFile>> {
    let video_stem = files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .min_by(|a, b| (a.stacking, &a.filename).cmp(&(b.stacking, &b.filename)))
        .map(|f| {
            f.filename
                .rsplit_once('.')
                .map_or(f.filename.as_str(), |(stem, _)| stem)
        })
        .unwrap_or_default();
    files
        .iter()
        .map(|f| plan_file(root, f, video_stem))
        .collect()
}

/// What a subtitle file says about itself: its stream as the library stores it, with what its
/// name says filling the gaps. An external subtitle often has no stream details at all (nothing
/// read it), and its language, forced and SDH marks then live only in its name
/// (`Heat (1995).eng.forced.srt`), which the rename must keep.
fn subtitle_tag(file: &MediaFile, video_stem: &str) -> SubtitleTag {
    let stream = subtitle_file_stream(file, video_stem);
    SubtitleTag {
        forced: stream.flags.contains(&StreamFlag::Forced),
        sdh: stream.flags.contains(&StreamFlag::HearingImpaired),
        language: stream.language,
        title: stream.title,
    }
}

fn plan_file(root: &str, file: &MediaFile, video_stem: &str) -> Option<PlanFile> {
    let dir = relative_to(root, &file.directory)?;
    let path = if dir.is_empty() {
        file.filename.clone()
    } else {
        format!("{dir}/{}", file.filename)
    };
    let mut plan = PlanFile::new(path, file.kind).modified(file.modified_ms);
    if file.stacking > 0 {
        plan = plan.stacked(file.stacking_marker.clone(), file.stacking);
    }
    if file.kind == MediaFileType::Subtitle {
        plan = plan.subtitle(subtitle_tag(file, video_stem));
    }
    Some(plan)
}

/// The root without a trailing separator.
pub(crate) fn trim_root(root: &str) -> String {
    root.trim_end_matches(['\\', '/']).to_owned()
}

/// `path` below `root`, relative and with `/`, or `None` when it is elsewhere. Case is ignored in
/// the comparison (the supported volumes are case-insensitive) and kept in the answer.
pub(crate) fn relative_to(root: &str, path: &str) -> Option<String> {
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let path = path.replace('\\', "/");
    let head = path.get(..root.len())?;
    if !head.eq_ignore_ascii_case(root) {
        return None;
    }
    let rest = &path[root.len()..];
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    Some(rest.trim_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use cv_domain::SubtitleStream;

    use super::*;

    fn subtitle(name: &str) -> MediaFile {
        MediaFile::new(MediaFileType::Subtitle, "D:/Movies/Heat", name)
    }

    #[test]
    fn a_subtitle_without_stream_details_keeps_the_language_and_flags_of_its_name() {
        let files = [
            MediaFile::new(MediaFileType::Video, "D:/Movies/Heat", "heat.mkv"),
            subtitle("heat.eng.srt"),
            subtitle("heat.deu.forced.srt"),
            subtitle("Heat (1995).jpn.sdh.srt"),
            subtitle("heat.srt"),
        ];
        let planned = plan_files("D:/Movies", &files).unwrap();
        let tags: Vec<(String, bool, bool)> = planned[1..]
            .iter()
            .map(|f| {
                let tag = f.subtitle.clone().unwrap();
                (tag.language, tag.forced, tag.sdh)
            })
            .collect();
        assert_eq!(
            tags,
            [
                ("en".to_owned(), false, false),
                ("de".to_owned(), true, false),
                ("ja".to_owned(), false, true),
                (String::new(), false, false),
            ]
        );
    }

    #[test]
    fn the_stream_details_win_over_the_name_when_they_are_known() {
        let mut file = subtitle("heat.eng.srt");
        file.subtitles.push(SubtitleStream {
            language: "fr".to_owned(),
            ..SubtitleStream::default()
        });
        let tag = subtitle_tag(&file, "heat");
        assert_eq!(tag.language, "fr");
    }

    #[test]
    fn relative_paths_ignore_case_and_separator_style() {
        assert_eq!(
            relative_to("D:\\Movies", "d:\\movies\\Heat (1995)\\x").as_deref(),
            Some("Heat (1995)/x")
        );
        assert_eq!(
            relative_to("D:\\Movies\\", "D:\\Movies").as_deref(),
            Some("")
        );
        assert_eq!(relative_to("D:\\Movies", "D:\\Movies2\\a"), None);
        assert_eq!(relative_to("D:\\Movies", "E:\\Movies\\a"), None);
        assert_eq!(
            relative_to("/mnt/media", "/mnt/media/Heat").as_deref(),
            Some("Heat")
        );
    }
}
