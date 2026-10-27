//! Queries the source tests share.

use cv_torrent::model::{MediaKind, Scope, TitleIds, TorrentQuery};

pub fn query(kind: MediaKind, title: &str, imdb: Option<&str>, scope: Scope) -> TorrentQuery {
    TorrentQuery {
        kind,
        ids: TitleIds {
            tmdb: Some(1),
            imdb: imdb.map(str::to_owned),
            tvdb: None,
        },
        title: title.to_owned(),
        original_title: None,
        alternative_titles: Vec::new(),
        year: Some(1968),
        scope,
        runtime_minutes: Some(96),
        anime: false,
        library_copy: None,
    }
}

pub fn movie(title: &str, imdb: Option<&str>) -> TorrentQuery {
    query(MediaKind::Movie, title, imdb, Scope::Movie)
}

pub fn show(title: &str, imdb: Option<&str>, scope: Scope) -> TorrentQuery {
    query(MediaKind::Show, title, imdb, scope)
}
