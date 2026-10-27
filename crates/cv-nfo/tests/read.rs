//! The tolerant readers on the hand-written NFOs in `tests/nfo-read` (including broken files) and
//! on every NFO the writers produce for the compatibility fixtures in `tests/compat/expected`.
//!
//! `tests/nfo-read/cases.json` lists, per file and reader, the summary values to compare (see
//! `summary_*` below for the keys). Keys that are not listed are not checked.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use cv_domain::{AiredStatus, CreditKind};
use cv_nfo::{
    EpisodeRead, MovieNfo, MovieSetNfo, SeasonRead, ShowNfo, decode_nfo_bytes, parse_roots,
    read_episode_nfo, read_movie_nfo, read_movie_set_nfo, read_season_nfo, read_show_nfo,
};

type Summary = BTreeMap<String, String>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn kind_name(kind: CreditKind) -> &'static str {
    match kind {
        CreditKind::Actor => "actor",
        CreditKind::Director => "director",
        CreditKind::Writer => "writer",
        CreditKind::Producer => "producer",
        CreditKind::Guest => "guest",
        CreditKind::Composer => "composer",
        CreditKind::Editor => "editor",
        CreditKind::Camera => "camera",
        CreditKind::Other => "other",
    }
}

fn credits(list: &[cv_domain::Credit]) -> String {
    list.iter()
        .map(|c| format!("{}:{}:{}", c.person.name, kind_name(c.kind), c.role))
        .collect::<Vec<_>>()
        .join("|")
}

fn put(summary: &mut Summary, key: &str, value: impl ToString) {
    summary.insert(key.to_owned(), value.to_string());
}

fn opt(value: Option<impl ToString>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn ids_into(summary: &mut Summary, ids: &cv_domain::ProviderIds) {
    for key in ["imdb", "tmdb", "tvdb", "tmdbSet"] {
        put(summary, key, ids.get(key).unwrap_or_default());
    }
    put(summary, "ids", ids.len());
}

fn summary_movie(movie: &MovieNfo) -> Summary {
    let mut s = Summary::new();
    put(&mut s, "title", &movie.title);
    put(&mut s, "original_title", &movie.original_title);
    put(&mut s, "sort_title", &movie.sort_title);
    put(&mut s, "plot", &movie.plot);
    put(&mut s, "outline", &movie.outline);
    put(&mut s, "tagline", &movie.tagline);
    put(&mut s, "year", movie.year);
    put(&mut s, "runtime", movie.runtime_minutes);
    put(&mut s, "top250", movie.top250);
    ids_into(&mut s, &movie.ids);
    put(&mut s, "ratings", movie.ratings.len());
    for r in movie.ratings.iter() {
        put(
            &mut s,
            &format!("rating.{}", r.id),
            format!("{}/{}/{}", r.value, r.votes, r.max_value),
        );
    }
    put(&mut s, "certification", &movie.certification.0);
    let names = |list: &[cv_domain::Genre]| {
        list.iter()
            .map(|g| g.name().to_owned())
            .collect::<Vec<_>>()
            .join("|")
    };
    put(&mut s, "genres", names(&movie.genres));
    put(&mut s, "studios", movie.studios.join("|"));
    put(&mut s, "countries", movie.countries.join("|"));
    put(&mut s, "languages", movie.spoken_languages.join("|"));
    put(&mut s, "tags", movie.tags.join("|"));
    put(&mut s, "release_date", opt(movie.release_date));
    put(&mut s, "watched", movie.watched);
    put(&mut s, "play_count", movie.play_count);
    put(&mut s, "last_played", opt(movie.last_played.clone()));
    put(&mut s, "date_added", opt(movie.date_added.clone()));
    let set = movie.movie_set.as_ref();
    put(&mut s, "set", set.map_or("", |x| x.title.as_str()));
    put(
        &mut s,
        "set_overview",
        set.map_or("", |x| x.overview.as_str()),
    );
    put(&mut s, "set_tmdb", opt(set.and_then(|x| x.tmdb_id)));
    let others: Vec<&str> = movie.other_sets.iter().map(|x| x.title.as_str()).collect();
    put(&mut s, "other_sets", others.join("|"));
    put(&mut s, "poster", &movie.artwork.poster);
    put(&mut s, "clearlogo", &movie.artwork.clearlogo);
    put(&mut s, "landscape", &movie.artwork.thumb);
    put(&mut s, "fanart", &movie.artwork.fanart);
    put(&mut s, "extra_fanarts", movie.artwork.extra_fanarts.len());
    put(
        &mut s,
        "trailer",
        movie.trailers.first().map_or("", |t| t.url.as_str()),
    );
    put(&mut s, "credits", credits(&movie.credits));
    s
}

fn summary_set(set: &MovieSetNfo) -> Summary {
    let mut s = Summary::new();
    put(&mut s, "title", &set.title);
    put(&mut s, "sort_title", &set.sort_title);
    put(&mut s, "overview", &set.overview);
    put(&mut s, "tmdb", set.ids.get("tmdb").unwrap_or_default());
    put(&mut s, "tags", set.tags.join("|"));
    put(&mut s, "poster", &set.poster);
    put(&mut s, "fanart", &set.fanart);
    put(&mut s, "date_added", opt(set.date_added.clone()));
    s
}

fn summary_show(show: &ShowNfo) -> Summary {
    let mut s = Summary::new();
    put(&mut s, "title", &show.title);
    put(&mut s, "original_title", &show.original_title);
    put(&mut s, "year", show.year);
    put(&mut s, "runtime", show.runtime_minutes);
    put(&mut s, "certification", &show.certification.0);
    let status = match show.status {
        AiredStatus::Unknown => "Unknown",
        AiredStatus::Continuing => "Continuing",
        AiredStatus::Ended => "Ended",
        AiredStatus::Canceled => "Canceled",
    };
    put(&mut s, "status", status);
    put(&mut s, "first_aired", opt(show.first_aired));
    ids_into(&mut s, &show.ids);
    put(&mut s, "poster", &show.artwork.poster);
    put(&mut s, "banner", &show.artwork.banner);
    put(&mut s, "fanart", &show.artwork.fanart);
    let seasons: Vec<String> = show
        .seasons
        .iter()
        .map(|x| format!("{}:{}", x.number, x.title))
        .collect();
    put(&mut s, "seasons", seasons.join("|"));
    let posters: Vec<String> = show
        .seasons
        .iter()
        .filter(|x| !x.artwork.poster.is_empty())
        .map(|x| format!("{}:{}", x.number, x.artwork.poster))
        .collect();
    put(&mut s, "season_posters", posters.join("|"));
    put(&mut s, "credits", credits(&show.credits));
    put(&mut s, "studios", show.studios.join("|"));
    put(&mut s, "countries", show.countries.join("|"));
    s
}

fn summary_season(read: &SeasonRead) -> Summary {
    let mut s = Summary::new();
    put(&mut s, "number", opt(read.number));
    put(&mut s, "title", &read.season.title);
    put(&mut s, "show_title", &read.show_title);
    put(&mut s, "year", read.year);
    put(&mut s, "first_aired", opt(read.season.first_aired));
    put(&mut s, "poster", &read.season.artwork.poster);
    s
}

fn summary_episodes(episodes: &[EpisodeRead]) -> Summary {
    let mut s = Summary::new();
    put(&mut s, "count", episodes.len());
    for (i, read) in episodes.iter().enumerate() {
        let e = &read.episode;
        let key = |name: &str| format!("{i}.{name}");
        put(&mut s, &key("valid"), read.is_valid());
        put(&mut s, &key("title"), &e.title);
        put(&mut s, &key("show_title"), &e.show_title);
        put(&mut s, &key("season"), opt(read.season_number));
        put(&mut s, &key("episode"), opt(read.episode_number));
        put(&mut s, &key("aired"), opt(e.first_aired));
        put(&mut s, &key("runtime"), e.runtime_minutes);
        put(&mut s, &key("tvdb"), e.ids.get("tvdb").unwrap_or_default());
        put(&mut s, &key("thumb"), &e.thumb);
        put(&mut s, &key("watched"), e.watched);
        put(&mut s, &key("plot"), &e.plot);
        put(&mut s, &key("certification"), &e.certification.0);
        put(&mut s, &key("credits"), credits(&e.credits));
    }
    s
}

#[test]
fn the_reader_fixtures_read_as_listed() {
    let dir = repo_root().join("tests/nfo-read");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("cases.json")).unwrap()).unwrap();
    let cases = manifest["cases"].as_array().unwrap();
    assert!(cases.len() >= 30, "the fixture set shrank");

    let mut used = BTreeSet::new();
    for case in cases {
        let file = case["file"].as_str().unwrap();
        let kind = case["kind"].as_str().unwrap();
        used.insert(file.to_owned());
        let text = decode_nfo_bytes(&fs::read(dir.join(file)).unwrap());
        let label = format!("{file} as {kind}");

        let (valid, summary) = match kind {
            "movie" => read_movie_nfo(&text).map(|m| (m.is_valid(), summary_movie(&m))),
            "set" => read_movie_set_nfo(&text).map(|m| (m.is_valid(), summary_set(&m))),
            "show" => read_show_nfo(&text).map(|m| (m.is_valid(), summary_show(&m))),
            "season" => read_season_nfo(&text).map(|m| (m.is_valid(), summary_season(&m))),
            "episode" => {
                let episodes = read_episode_nfo(&text);
                let valid = episodes.iter().any(EpisodeRead::is_valid);
                Some((valid, summary_episodes(&episodes)))
            }
            other => panic!("unknown kind {other}"),
        }
        .map_or((None, Summary::new()), |(v, s)| (Some(v), s));

        if case["none"].as_bool().unwrap_or(false) {
            assert!(valid.is_none(), "{label}: expected no result");
            continue;
        }
        let valid = valid.unwrap_or_else(|| panic!("{label}: expected a result"));
        if let Some(want) = case["valid"].as_bool() {
            assert_eq!(valid, want, "{label}: validity");
        }
        for (key, want) in case["expect"].as_object().unwrap() {
            let got = summary
                .get(key)
                .unwrap_or_else(|| panic!("{label}: no summary key {key}"));
            assert_eq!(got, want.as_str().unwrap(), "{label}: {key}");
        }
    }

    let on_disk: BTreeSet<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".nfo"))
        .collect();
    assert_eq!(
        used, on_disk,
        "every fixture file must be listed in cases.json"
    );
}

fn nfo_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            nfo_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "nfo") {
            out.push(path);
        }
    }
}

#[test]
fn every_nfo_the_writers_produce_for_the_compat_fixtures_reads_back() {
    let mut files = Vec::new();
    nfo_files(&repo_root().join("tests/compat/expected/nfo"), &mut files);
    assert!(files.len() > 50, "found only {} NFOs", files.len());

    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for path in files {
        let text = decode_nfo_bytes(&fs::read(&path).unwrap());
        let root = parse_roots(&text)
            .first()
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let label = path.display().to_string();
        match root.as_str() {
            "movie" => {
                let movie = read_movie_nfo(&text).unwrap();
                assert!(movie.is_valid(), "{label}: no title");
            }
            "collection" => {
                let set = read_movie_set_nfo(&text).unwrap();
                assert!(set.is_valid(), "{label}: no title");
            }
            "tvshow" => {
                let show = read_show_nfo(&text).unwrap();
                assert!(show.is_valid(), "{label}: no title");
            }
            "season" => {
                let season = read_season_nfo(&text).unwrap();
                assert!(season.is_valid(), "{label}: no season number");
            }
            "episodedetails" => {
                let episodes = read_episode_nfo(&text);
                let blocks = parse_roots(&text).len();
                assert_eq!(episodes.len(), blocks, "{label}: block count");
                assert!(
                    episodes.iter().all(EpisodeRead::is_valid),
                    "{label}: invalid"
                );
            }
            other => panic!("{label}: unexpected root {other}"),
        }
        *seen.entry(root).or_default() += 1;
    }
    for root in ["movie", "collection", "tvshow", "season", "episodedetails"] {
        assert!(seen.contains_key(root), "no {root} NFO among the fixtures");
    }
}
