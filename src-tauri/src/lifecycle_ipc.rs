//! The Tauri surface of the lifecycle commands.
//!
//! The rules, the DTOs and the command bodies live in [`cv_ipc::lifecycle`].

use tauri::State;

use crate::library_ipc::{LibraryCommandError, LibraryKindDto};
use crate::sources_ipc::LibraryState;

pub use cv_ipc::lifecycle::*;

/// The pipeline counts of the movies or the shows: titles per stage, and Needs review, Not
/// matched, Working, Rename pending, Locked and Missing. Every screen that shows these numbers
/// reads this command.
#[tauri::command]
#[specta::specta]
pub async fn lifecycle_counts(
    library: State<'_, LibraryState>,
    kind: LibraryKindDto,
) -> Result<LifecycleCountsDto, LibraryCommandError> {
    cv_ipc::lifecycle::lifecycle_counts(&library, kind).await
}

/// The stage, flags and explanation lines of the titles `ids` (movies, shows, seasons and
/// episodes). A title without a lifecycle, such as a collection, is left out.
#[tauri::command]
#[specta::specta]
pub async fn get_lifecycles(
    library: State<'_, LibraryState>,
    ids: Vec<String>,
) -> Result<Vec<ItemLifecycleDto>, LibraryCommandError> {
    cv_ipc::lifecycle::get_lifecycles(&library, ids).await
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use cv_app::data_location::{DataLocation, LocationInputs};
    use cv_app::secrets::SecretStore;
    use cv_app::services::{AppServices, BootOptions};
    use cv_artwork::library_source::LibrarySources;
    use cv_domain::{Item, SourceKind};

    use super::*;
    use crate::detail_ipc::DetailReader;
    use crate::tv_detail_ipc::TvReader;

    fn services(root: &Path) -> Arc<AppServices> {
        let inputs = LocationInputs {
            exe_dir: root.join("app"),
            user_data_base: root.join("userdata"),
            user_config_base: root.join("userconfig"),
        };
        std::fs::create_dir_all(&inputs.exe_dir).unwrap();
        let options = BootOptions::new(
            DataLocation::resolve(&inputs),
            Arc::new(SecretStore::in_memory()),
        )
        .without_providers();
        AppServices::boot(options).expect("boot")
    }

    /// The movie, show and episode pages (and so the Inspector, which reads
    /// them) carry the stage and flags of the title and the label of its library folder.
    #[test]
    fn the_detail_and_tv_pages_carry_the_lifecycle_and_the_source_label() {
        let root = tempfile::tempdir().unwrap();
        let services = services(root.path());
        let (movies, shows) = (r"D:\Movies 4K", r"D:\Series");
        let (movie, show, episode) = services
            .db()
            .write(move |conn| {
                let source =
                    cv_db::repo::data_sources::insert_source(conn, SourceKind::Movie, movies)?;
                cv_db::repo::data_sources::set_source_label(conn, source.id, "NAS 4K")?;
                let source = cv_db::repo::data_sources::insert_source(conn, SourceKind::Tv, shows)?;
                cv_db::repo::data_sources::set_source_label(conn, source.id, "NAS TV")?;
                let mut heat = Item::movie("Heat");
                heat.core.data_source = movies.to_owned();
                heat.core.path = format!(r"{movies}\Heat (1995)");
                heat.core.ids.set("tmdb", "949");
                heat.core.last_scraper_id = "tmdb".to_owned();
                let movie = cv_db::repo::insert_item(conn, &heat)?;
                let mut dark = Item::show("Dark");
                dark.core.data_source = shows.to_owned();
                dark.core.path = format!(r"{shows}\Dark");
                let show = cv_db::repo::insert_item(conn, &dark)?;
                let mut secrets = Item::episode(show, "Secrets");
                secrets.core.data_source = shows.to_owned();
                let episode = cv_db::repo::insert_item(conn, &secrets)?;
                Ok((movie, show, episode))
            })
            .unwrap();
        let lifecycle = Some(Arc::clone(services.lifecycle()));
        let sources = Arc::new(LibrarySources::new());

        let page = DetailReader::new(Arc::clone(services.db()), Arc::clone(&sources), None)
            .with_lifecycle(lifecycle.clone())
            .movie(movie)
            .unwrap();
        assert_eq!(page.source_label, "NAS 4K");
        let stage = page.lifecycle.expect("the movie page has a lifecycle");
        assert_eq!(stage.stage, StageDto::Scraped);
        assert_eq!(stage.lines.len(), 3);

        let tv = TvReader::new(Arc::clone(services.db()), sources, None).with_lifecycle(lifecycle);
        let page = tv.show(show).unwrap();
        assert_eq!(page.source_label, "NAS TV");
        let stage = page.lifecycle.expect("the show page has a lifecycle");
        assert_eq!(stage.stage, StageDto::New);
        // No scrape was tried for it: just New, not Not matched (QA final-07).
        assert!(stage.flags.is_empty(), "{:?}", stage.flags);
        let page = tv.episode(episode).unwrap();
        assert_eq!(page.source_label, "NAS TV");
        assert_eq!(
            page.lifecycle.map(|l| l.stage),
            Some(StageDto::New),
            "an episode takes the lifecycle of its show"
        );
    }
}
