//! On a 20,000-title synthetic library, the first page of a filtered,
//! sorted query (and of a full-text search) comes back in under 100 ms.

use std::time::{Duration, Instant};

use cv_db::Database;
use cv_db::repo::{
    self, Direction, FacetCondition, Filter, FilterId, IntRange, ItemFlag, ItemQuery, MatchMode,
    NumOp, SortField, SortKey, fts,
};
use cv_domain::{
    AudioStream, CalendarDate, Credit, CreditKind, Genre, Item, ItemDetail, ItemKind, MediaFile,
    MediaFileType, MediaSource, Rating, SubtitleStream,
};

const LIBRARY_SIZE: usize = 20_000;
const BUDGET: Duration = Duration::from_millis(100);
const PAGE: usize = 60;

/// A small deterministic generator, so the library is the same on every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const ADJECTIVES: &[&str] = &[
    "Silent", "Last", "Dark", "Golden", "Hidden", "Broken", "Eternal", "Crimson", "Lost", "Final",
    "Wild", "Frozen", "Burning", "Secret", "Distant", "Hollow",
];
const NOUNS: &[&str] = &[
    "Harbor", "Empire", "Garden", "Signal", "Horizon", "Kingdom", "Machine", "Voyage", "Shadow",
    "Archive", "Frontier", "Mirror", "Orchard", "Station", "Verdict", "Tide",
];
const FIRST_NAMES: &[&str] = &[
    "Anna", "Marcus", "Elena", "Tobias", "Sofia", "Daniel", "Mira", "Lucas", "Nadia", "Oscar",
    "Ines", "Felix", "Clara", "Hugo", "Lena", "Viktor", "Amara", "Jonas", "Rosa", "Ivan",
];
const LAST_NAMES: &[&str] = &[
    "Novak",
    "Ferrari",
    "Lindqvist",
    "Okafor",
    "Moreau",
    "Kowalski",
    "Tanaka",
    "Haddad",
    "Silva",
    "Bergman",
    "Duarte",
    "Petrov",
    "Larsen",
    "Costa",
    "Weber",
    "Ibarra",
    "Nakamura",
    "Rossi",
    "Keller",
    "Mendes",
];
const GENRES: &[Genre] = &[
    Genre::Action,
    Genre::Comedy,
    Genre::Drama,
    Genre::Horror,
    Genre::ScienceFiction,
    Genre::Thriller,
    Genre::Romance,
    Genre::Documentary,
    Genre::Animation,
    Genre::Western,
];
const TAGS: &[&str] = &[
    "favourite",
    "kids",
    "classic",
    "date night",
    "rewatch",
    "festival",
];
const SOURCES: &[&str] = &["D:\\Movies", "E:\\Films", "\\\\nas\\media\\movies"];
const HEIGHTS: &[u32] = &[480, 576, 720, 1080, 1080, 1080, 2160, 2160];
const CODECS: &[&str] = &["H.264", "HEVC", "AV1", "VC-1", "MPEG-2"];
const HDR: &[&[&str]] = &[
    &[],
    &[],
    &[],
    &["HDR10"],
    &["Dolby Vision", "HDR10"],
    &["HLG"],
];
const LANGS: &[&str] = &["eng", "fra", "deu", "spa", "jpn", "ita"];

fn synthetic_movie(rng: &mut Lcg, n: usize, people: &[String]) -> Item {
    let title = format!("{} {} {}", rng.pick(ADJECTIVES), rng.pick(NOUNS), n);
    let mut item = Item::movie(&title);
    let year = 1950 + rng.below(76) as u16;
    item.core.year = year;
    item.core.data_source = (*rng.pick(SOURCES)).to_owned();
    item.core.path = format!("{}\\{title} ({year})", item.core.data_source);
    item.core.plot = format!(
        "A {} {} in the year {year}, remembered by {}.",
        rng.pick(ADJECTIVES).to_lowercase(),
        rng.pick(NOUNS).to_lowercase(),
        rng.pick(people)
    );
    item.core.date_added_ms = 1_600_000_000_000 + (rng.next() % 100_000_000_000) as i64;
    item.core.ids.set("tmdb", (100_000 + n).to_string());
    item.core.ids.set("imdb", format!("tt{:07}", 1_000_000 + n));
    item.core.ratings.insert(Rating::new(
        "imdb",
        3.0 + (rng.below(60) as f32) / 10.0,
        1000,
        10,
    ));
    for _ in 0..rng.below(3) {
        item.core.tags.add(rng.pick(TAGS));
    }
    for (i, kind) in [CreditKind::Director, CreditKind::Actor, CreditKind::Actor]
        .into_iter()
        .enumerate()
    {
        item.core
            .credits
            .push(Credit::new(kind, rng.pick(people), format!("Role {i}")));
    }

    let mut video = MediaFile::new(
        MediaFileType::Video,
        item.core.path.clone(),
        format!("{title} ({year}).mkv"),
    );
    video.height = *rng.pick(HEIGHTS);
    video.width = video.height * 16 / 9;
    video.video_codec = (*rng.pick(CODECS)).to_owned();
    video.container_format = "Matroska".into();
    video.size_bytes = 1_000_000_000 + rng.next() % 40_000_000_000;
    video.hdr_formats = rng.pick(HDR).iter().map(|s| (*s).to_owned()).collect();
    video.audio_streams.push(AudioStream {
        codec: "AC3".into(),
        language: (*rng.pick(LANGS)).to_owned(),
        channels: 6,
        ..AudioStream::default()
    });
    video.subtitles.push(SubtitleStream {
        codec: "SRT".into(),
        language: (*rng.pick(LANGS)).to_owned(),
        ..SubtitleStream::default()
    });
    item.core.media_files.push(video);

    if let ItemDetail::Movie(m) = &mut item.detail {
        m.runtime_minutes = 70 + rng.below(110) as u32;
        m.watched = rng.below(3) == 0;
        m.release_date = CalendarDate::new(
            i32::from(year),
            1 + rng.below(12) as u8,
            1 + rng.below(28) as u8,
        );
        m.media_source = if rng.below(2) == 0 {
            MediaSource::Bluray
        } else {
            MediaSource::WebDl
        };
        for _ in 0..1 + rng.below(3) {
            let g = rng.pick(GENRES).clone();
            if !m.genres.contains(&g) {
                m.genres.push(g);
            }
        }
    }
    item
}

fn build_library(db: &Database) -> usize {
    let mut rng = Lcg(42);
    let people: Vec<String> = FIRST_NAMES
        .iter()
        .flat_map(|f| LAST_NAMES.iter().map(move |l| format!("{f} {l}")))
        .collect();
    let mut movies: Vec<Item> = (0..LIBRARY_SIZE)
        .map(|n| synthetic_movie(&mut rng, n, &people))
        .collect();
    // A known director, credited on a small, fixed share of the library.
    let mut villeneuve = 0;
    for movie in movies.iter_mut().skip(7).step_by(1_500) {
        movie.core.credits[0] = Credit::new(CreditKind::Director, "Denis Villeneuve", "");
        villeneuve += 1;
    }
    db.write(move |tx| {
        for m in &movies {
            repo::insert_item(tx, m)?;
        }
        // What the app does after a bulk import, so the planner has statistics.
        tx.execute_batch("ANALYZE")?;
        Ok(())
    })
    .unwrap();
    villeneuve
}

fn timed<T>(what: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    eprintln!("{what}: {took:?}");
    (out, took)
}

#[test]
fn first_page_of_filtered_sorted_and_searched_queries_is_fast_on_20k_items() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("library.db")).unwrap();
    let (villeneuve, built) = timed("build 20k library", || build_library(&db));
    eprintln!("(built in {built:?})");
    let total = db
        .read(|c| repo::count_items(c, &ItemQuery::new(ItemKind::Movie)))
        .unwrap();
    assert_eq!(total, LIBRARY_SIZE as u64);

    // The task's query: filtered by year, genre and 4K video, sorted by year then title.
    let filtered_sorted = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::and([
            Filter::Year {
                range: IntRange::between(1990, 2020),
            },
            Filter::Genre {
                names: vec!["science_fiction".into()],
                mode: MatchMode::Any,
            },
            Filter::VideoHeight {
                range: IntRange::at_least(2160),
            },
        ]))
        .sort_by(SortKey::desc(SortField::Year))
        .sort_by(SortKey::asc(SortField::Title));
    let (page, took) = timed("filtered + sorted, first page", || {
        db.read(|c| repo::list_items(c, &filtered_sorted, None, PAGE))
            .unwrap()
    });
    assert_eq!(page.items.len(), PAGE, "the filter leaves plenty of rows");
    assert!(page.next.is_some());
    assert!(page.items.windows(2).all(|w| w[0].year >= w[1].year));
    assert!(took < BUDGET, "took {took:?}");

    // The following page continues where the first stopped.
    let (second, took) = timed("filtered + sorted, second page", || {
        db.read(|c| repo::list_items(c, &filtered_sorted, page.next.as_ref(), PAGE))
            .unwrap()
    });
    assert_eq!(second.items.len(), PAGE);
    assert!(
        second
            .items
            .iter()
            .all(|i| !page.items.iter().any(|p| p.id == i.id))
    );
    assert!(took < BUDGET, "took {took:?}");

    // Default title sort with an unselective filter (most rows match).
    let by_title = ItemQuery::new(ItemKind::Movie).filter(Filter::Flag {
        flag: ItemFlag::Watched,
        value: false,
    });
    let (page, took) = timed("title sort, unwatched, first page", || {
        db.read(|c| repo::list_items(c, &by_title, None, PAGE))
            .unwrap()
    });
    assert_eq!(page.items.len(), PAGE);
    assert!(took < BUDGET, "took {took:?}");

    // A selective filter that must scan: rating and a data source, newest first.
    let newest = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::and([
            Filter::DataSource {
                values: vec!["E:\\Films".into()],
            },
            Filter::Rating {
                source: "imdb".into(),
                min: Some(8.0),
                max: None,
            },
            Filter::Genre {
                names: vec!["Drama".into()],
                mode: MatchMode::Any,
            },
        ]))
        .sort_by(SortKey {
            field: SortField::DateAdded,
            direction: Direction::Desc,
        });
    let (page, took) = timed("source + rating + genre, newest first", || {
        db.read(|c| repo::list_items(c, &newest, None, PAGE))
            .unwrap()
    });
    assert!(!page.items.is_empty());
    assert!(took < BUDGET, "took {took:?}");

    // Full-text search: every movie crediting the director, ranked.
    let (hits, took) = timed("full-text 'villeneuve' (ranked)", || {
        db.read(|c| fts::search(c, "villeneuve", 1000)).unwrap()
    });
    assert_eq!(hits.len(), villeneuve);
    assert!(took < BUDGET, "took {took:?}");

    // The same search as a filter, sorted by title.
    let search = ItemQuery::new(ItemKind::Movie).filter(Filter::Search {
        text: "villeneuve".into(),
    });
    let (page, took) = timed("full-text filter, title sort", || {
        db.read(|c| repo::list_items(c, &search, None, PAGE))
            .unwrap()
    });
    assert_eq!(page.items.len(), villeneuve);
    assert!(took < BUDGET, "took {took:?}");

    // Search narrowed by a filter and sorted by a different column, partial last word.
    let narrowed = ItemQuery::new(ItemKind::Movie)
        .filter(Filter::and([
            Filter::Search {
                text: "silent harb".into(),
            },
            Filter::Year {
                range: IntRange::at_least(1980),
            },
        ]))
        .sort_by(SortKey::desc(SortField::DateAdded));
    let (_, took) = timed("search + filter, newest first", || {
        db.read(|c| repo::list_items(c, &narrowed, None, PAGE))
            .unwrap()
    });
    assert!(took < BUDGET, "took {took:?}");

    // The quick search as the title bar sends it: ranked by relevance, then the typo fallback
    // (nothing matches "spiderman" or "silnt harbr" exactly, so the whole library is read for
    // near misses; that scan is several times slower in a debug build).
    let typo_budget = if cfg!(debug_assertions) {
        BUDGET * 4
    } else {
        BUDGET
    };
    for text in ["silent harb", "spiderman", "silnt harbr"] {
        let ranked = ItemQuery::new(ItemKind::Movie)
            .filter(Filter::Search { text: text.into() })
            .title_first(text);
        let (_, took) = timed(&format!("quick search '{text}', ranked"), || {
            db.read(|c| {
                repo::count_items(c, &ranked)?;
                repo::list_items(c, &ranked, None, PAGE)
            })
            .unwrap()
        });
        let budget = if text == "silent harb" {
            BUDGET
        } else {
            typo_budget
        };
        assert!(took < budget, "'{text}' took {took:?}");
    }

    // The poster grid reads random-access windows. A window deep into the library (the
    // scrollbar dragged near the end) costs about as much as the first one, in either direction.
    let windows = ItemQuery::new(ItemKind::Movie)
        .sort_by(SortKey::desc(SortField::Year))
        .sort_by(SortKey::asc(SortField::Title));
    for offset in [0, 10_000, LIBRARY_SIZE - 120] {
        let (rows, took) = timed(&format!("window at offset {offset}"), || {
            db.read(|c| repo::list_items_at(c, &windows, offset, 120))
                .unwrap()
        });
        assert_eq!(rows.len(), 120.min(LIBRARY_SIZE - offset));
        assert!(took < BUDGET, "offset {offset} took {took:?}");
    }

    // Catalogue facets compile to the same indexed lookups, so a chip does not slow
    // the first page down. Each query below is a different shape of facet.
    let facet = |condition: FacetCondition| Filter::Facet(condition);
    let facet_queries: Vec<(&str, Filter)> = vec![
        (
            "resolution + HDR + year range",
            Filter::and([
                facet(FacetCondition::new(FilterId::VideoFormat).with_values(["2160p"])),
                facet(FacetCondition::new(FilterId::HdrFormat).with_values(["Dolby Vision"])),
                facet(FacetCondition::new(FilterId::Year).between(1990.0, 2020.0)),
            ]),
        ),
        (
            "audio language ALL + subtitle count",
            Filter::and([
                facet(
                    FacetCondition::new(FilterId::AudioLanguage)
                        .with_values(["eng", "fra"])
                        .with_mode(MatchMode::All),
                ),
                facet(FacetCondition::new(FilterId::SubtitleCount).with_number(NumOp::Ge, 1.0)),
            ]),
        ),
        (
            "negated genre ANY + file size",
            Filter::and([
                facet(
                    FacetCondition::new(FilterId::Genre)
                        .with_values(["comedy", "horror"])
                        .negate(),
                ),
                facet(
                    FacetCondition::new(FilterId::VideoFilesize).with_number(NumOp::Gt, 20_000.0),
                ),
            ]),
        ),
        (
            "any-text term",
            facet(FacetCondition::new(FilterId::AllInOne).with_values(["silent harbor"])),
        ),
        (
            "missing metadata (full scan, no match)",
            facet(FacetCondition::new(FilterId::MissingMetadata).with_values(["Cast"])),
        ),
        (
            "missing artwork (every row matches)",
            facet(FacetCondition::new(FilterId::MissingArtwork).with_values(["Poster"])),
        ),
        (
            "duplicates",
            facet(FacetCondition::new(FilterId::Duplicate)),
        ),
    ];
    for (name, filter) in facet_queries {
        let query = ItemQuery::new(ItemKind::Movie)
            .filter(filter)
            .sort_by(SortKey::asc(SortField::Title));
        let (page, took) = timed(&format!("facet: {name}"), || {
            db.read(|c| repo::list_items(c, &query, None, PAGE))
                .unwrap()
        });
        assert!(page.items.len() <= PAGE);
        assert!(took < BUDGET, "facet {name} took {took:?}");
    }
}
