//! Judging merged releases: is it the title searched for, where does it sort, what
//! should be said about it, and is it better than the copy in the library.
//!
//! Pure functions over [`ParsedRelease`], [`Release`] and [`TorrentQuery`].

use std::cmp::Reverse;

use cv_domain::MediaSource;

use crate::model::{Hdr, LibraryCopy, Release, Resolution, Scope, TorrentQuery};
use crate::query::{accepted_keys, match_key};
use crate::release::{EpisodeSpan, LowQuality, ParsedRelease, parse_release};

/// Why a release is not the title searched for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// The title is another one.
    Title,
    /// A movie from another year.
    Year,
    /// A show release for a movie search, or a movie for a show search.
    Kind,
    /// Another season, or a different episode.
    Scope,
}

/// Whether a release is the title (and scope) searched for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relevance {
    Match,
    /// Hidden by default and counted; "Show hidden" brings it back, marked as not matching.
    Hidden(Mismatch),
}

impl Relevance {
    #[must_use]
    pub fn is_match(self) -> bool {
        self == Self::Match
    }
}

/// What kind of pack a release is, when it is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    /// Several episodes of one season.
    Episodes,
    Season,
    Seasons,
    Complete,
}

/// What a flag is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    NoSeeders,
    TooSmall,
    Payload,
    LowQuality(LowQuality),
}

/// A warning about a release, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub kind: FlagKind,
    pub text: String,
}

/// How a release compares with the library copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upgrade {
    /// Higher resolution, or the same with a better source or HDR added.
    Better,
    /// Resolution, source and HDR all match.
    Same,
    Worse,
}

/// Everything judged about one release for one query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub parsed: ParsedRelease,
    pub relevance: Relevance,
    pub flags: Vec<Flag>,
    /// `None` when the title is not in the library (or has no video there).
    pub upgrade: Option<Upgrade>,
    pub pack: Option<PackKind>,
}

/// The lowest plausible video bit rate in Mbit/s for each resolution. A pack is not size
/// checked: how many episodes it holds is not known from the name.
const MBIT_PER_SECOND: [(Resolution, f64); 4] = [
    (Resolution::R480, 0.5),
    (Resolution::R720, 1.0),
    (Resolution::R1080, 2.0),
    (Resolution::R2160, 6.0),
];

fn pack_of(span: EpisodeSpan) -> Option<PackKind> {
    match span {
        EpisodeSpan::Multi { .. } => Some(PackKind::Episodes),
        EpisodeSpan::SeasonPack(_) => Some(PackKind::Season),
        EpisodeSpan::SeasonRange { .. } => Some(PackKind::Seasons),
        EpisodeSpan::Complete => Some(PackKind::Complete),
        EpisodeSpan::None | EpisodeSpan::Single { .. } => None,
    }
}

/// Whether the parsed release is the title searched for: the title (or original or alternative
/// title) matches, a movie is from the same year (one year either way, or no year), and the
/// season or episode is the one asked for or a pack that holds it.
#[must_use]
pub fn relevance(parsed: &ParsedRelease, query: &TorrentQuery) -> Relevance {
    if !accepted_keys(query).contains(&match_key(&parsed.title)) {
        return Relevance::Hidden(Mismatch::Title);
    }
    match query.scope {
        Scope::Movie => {
            if parsed.span.is_tv() {
                return Relevance::Hidden(Mismatch::Kind);
            }
            if let (Some(want), Some(got)) = (query.year, parsed.year)
                && want.abs_diff(got) > 1
            {
                return Relevance::Hidden(Mismatch::Year);
            }
            Relevance::Match
        }
        Scope::Show => Relevance::Match,
        Scope::Season(season) => {
            let holds = match parsed.span {
                EpisodeSpan::SeasonPack(s) => s == season,
                EpisodeSpan::SeasonRange { first, last } => (first..=last).contains(&season),
                EpisodeSpan::Complete => true,
                _ => false,
            };
            if holds {
                Relevance::Match
            } else {
                Relevance::Hidden(Mismatch::Scope)
            }
        }
        Scope::Episode(season, episode) => {
            let holds = match parsed.span {
                EpisodeSpan::Single {
                    season: s,
                    episode: e,
                } => s == season && e == episode,
                EpisodeSpan::Multi {
                    season: s,
                    first,
                    last,
                } => s == season && (first..=last).contains(&episode),
                EpisodeSpan::SeasonPack(s) => s == season,
                EpisodeSpan::SeasonRange { first, last } => (first..=last).contains(&season),
                EpisodeSpan::Complete => true,
                EpisodeSpan::None => false,
            };
            if holds {
                Relevance::Match
            } else {
                Relevance::Hidden(Mismatch::Scope)
            }
        }
    }
}

/// The resolution group a release is listed under (2160p, 1080p, 720p, 480p, unknown).
#[must_use]
pub fn group_of(parsed: &ParsedRelease) -> Resolution {
    parsed.resolution
}

/// The default order: resolution group first (2160p first, unknown last), then the most seeded,
/// then the largest, then the name so the order never wobbles.
pub type SortKey = (Resolution, Reverse<u32>, Reverse<u64>, String);

/// The key to sort a release by in the default order.
#[must_use]
pub fn order(release: &Release, parsed: &ParsedRelease) -> SortKey {
    (
        group_of(parsed),
        Reverse(release.seeders.unwrap_or(0)),
        Reverse(release.size.unwrap_or(0)),
        release.name.to_lowercase(),
    )
}

fn resolution_label(resolution: Resolution) -> &'static str {
    match resolution {
        Resolution::R2160 => "2160p",
        Resolution::R1080 => "1080p",
        Resolution::R720 => "720p",
        Resolution::R480 => "480p",
        Resolution::Unknown => "this resolution",
    }
}

/// The smallest plausible size in bytes for one title of `runtime_minutes` at `resolution`.
fn minimum_size(resolution: Resolution, runtime_minutes: u32) -> Option<u64> {
    let mbps = MBIT_PER_SECOND
        .iter()
        .find(|(r, _)| *r == resolution)
        .map(|(_, mbps)| *mbps)?;
    let seconds = f64::from(runtime_minutes) * 60.0;
    // Truncating to whole bytes is exactly what a threshold needs.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((mbps * 1_000_000.0 / 8.0 * seconds) as u64)
}

/// The warnings for a release, in plain words: an executable or archive, a cam or screener
/// recording, an implausibly small size for its resolution and runtime, no seeders.
#[must_use]
pub fn flags(release: &Release, parsed: &ParsedRelease, query: &TorrentQuery) -> Vec<Flag> {
    let mut out = Vec::new();
    if parsed.payload_warning {
        out.push(Flag {
            kind: FlagKind::Payload,
            text: "Contains an executable or archive file".to_owned(),
        });
    }
    if let Some(low) = parsed.low_quality {
        let text = match low {
            LowQuality::Cam => "Camera recording",
            LowQuality::Telesync => "Telesync recording",
            LowQuality::Telecine => "Telecine recording",
            LowQuality::Screener => "Screener copy",
        };
        out.push(Flag {
            kind: FlagKind::LowQuality(low),
            text: text.to_owned(),
        });
    }
    if let (Some(size), Some(runtime)) = (release.size, query.runtime_minutes) {
        let episodes = match parsed.span {
            EpisodeSpan::None | EpisodeSpan::Single { .. } => Some(1),
            EpisodeSpan::Multi { first, last, .. } => Some(u64::from(last - first) + 1),
            _ => None,
        };
        if let Some(episodes) = episodes
            && let Some(minimum) = minimum_size(parsed.resolution, runtime)
            && size < minimum.saturating_mul(episodes)
        {
            out.push(Flag {
                kind: FlagKind::TooSmall,
                text: format!("Too small for {}", resolution_label(parsed.resolution)),
            });
        }
    }
    if release.seeders == Some(0) {
        out.push(Flag {
            kind: FlagKind::NoSeeders,
            text: "No seeders".to_owned(),
        });
    }
    out
}

/// How good a source is, higher is better: remux, disc, web download, web rip, TV capture,
/// DVD, anything else.
fn source_rank(source: &MediaSource, remux: bool) -> u8 {
    if remux {
        return 6;
    }
    match source {
        MediaSource::UhdBluray | MediaSource::Bluray => 5,
        MediaSource::WebDl => 4,
        MediaSource::WebRip | MediaSource::Stream => 3,
        MediaSource::Tv | MediaSource::HdRip => 2,
        MediaSource::Dvd | MediaSource::HdDvd => 1,
        _ => 0,
    }
}

fn resolution_rank(resolution: Resolution) -> u8 {
    match resolution {
        Resolution::R2160 => 4,
        Resolution::R1080 => 3,
        Resolution::R720 => 2,
        Resolution::R480 => 1,
        Resolution::Unknown => 0,
    }
}

/// A release against the copy in the library. A cam or screener is never an upgrade. When the
/// resolution of either is unknown it counts as equal, so the source and HDR decide.
#[must_use]
pub fn upgrade(parsed: &ParsedRelease, copy: &LibraryCopy) -> Upgrade {
    if parsed.low_quality.is_some() {
        return Upgrade::Worse;
    }
    let (mine, theirs) =
        if parsed.resolution == Resolution::Unknown || copy.resolution == Resolution::Unknown {
            (0, 0)
        } else {
            (
                resolution_rank(parsed.resolution),
                resolution_rank(copy.resolution),
            )
        };
    let source = source_rank(&parsed.source, parsed.remux);
    let held = source_rank(&copy.source, false);
    let hdr_added = parsed.hdr != Hdr::None && copy.hdr == Hdr::None;
    let same_hdr = (parsed.hdr != Hdr::None) == (copy.hdr != Hdr::None);
    if mine > theirs || (mine == theirs && (source > held || (source == held && hdr_added))) {
        Upgrade::Better
    } else if mine == theirs && source == held && same_hdr {
        Upgrade::Same
    } else {
        Upgrade::Worse
    }
}

/// Parses and judges one merged release for a query.
#[must_use]
pub fn assess(release: &Release, query: &TorrentQuery) -> Assessment {
    let parsed = parse_release(&release.name, &release.files);
    let relevance = relevance(&parsed, query);
    let flags = flags(release, &parsed, query);
    let upgrade = query
        .library_copy
        .as_ref()
        .map(|copy| upgrade(&parsed, copy));
    let pack = pack_of(parsed.span);
    Assessment {
        parsed,
        relevance,
        flags,
        upgrade,
        pack,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaKind, TitleIds};

    fn query(title: &str, year: Option<u16>, scope: Scope) -> TorrentQuery {
        TorrentQuery {
            kind: if matches!(scope, Scope::Movie) {
                MediaKind::Movie
            } else {
                MediaKind::Show
            },
            ids: TitleIds::default(),
            title: title.to_owned(),
            original_title: None,
            alternative_titles: Vec::new(),
            year,
            scope,
            runtime_minutes: None,
            anime: false,
            library_copy: None,
        }
    }

    fn rel(name: &str, size: Option<u64>, seeders: Option<u32>) -> Release {
        Release {
            id: name.to_owned(),
            name: name.to_owned(),
            size,
            seeders,
            ..Release::default()
        }
    }

    fn judge(name: &str, q: &TorrentQuery) -> Relevance {
        relevance(&parse_release(name, &[]), q)
    }

    #[test]
    fn same_words_other_film_is_hidden() {
        let q = query("Dune", Some(2021), Scope::Movie);
        assert_eq!(
            judge("Dune.2021.2160p.WEB-DL.DDP5.1.H.265", &q),
            Relevance::Match
        );
        assert_eq!(judge("Dune.2021.1080p.BluRay.x264", &q), Relevance::Match);
        assert_eq!(
            judge("Dune.1984.1080p.BluRay.x264", &q),
            Relevance::Hidden(Mismatch::Year)
        );
        assert_eq!(
            judge("Dune.Part.Two.2024.2160p.WEB-DL.H.265", &q),
            Relevance::Hidden(Mismatch::Title)
        );
    }

    #[test]
    fn a_movie_year_may_be_one_off_or_missing() {
        let q = query("Heat", Some(1995), Scope::Movie);
        assert_eq!(judge("Heat.1996.720p", &q), Relevance::Match);
        assert_eq!(judge("Heat.1994.720p", &q), Relevance::Match);
        assert_eq!(judge("Heat.720p.BluRay", &q), Relevance::Match);
        assert_eq!(
            judge("Heat.1997.720p", &q),
            Relevance::Hidden(Mismatch::Year)
        );
        let no_year = query("Heat", None, Scope::Movie);
        assert_eq!(judge("Heat.1997.720p", &no_year), Relevance::Match);
    }

    #[test]
    fn the_release_spelling_of_an_apostrophe_title_matches() {
        let q = query("Widow's Bay", Some(2019), Scope::Movie);
        assert_eq!(judge("Widow.s.Bay.2019.1080p.WEB-DL", &q), Relevance::Match);
        assert_eq!(judge("Widows.Bay.2019.1080p.WEB-DL", &q), Relevance::Match);
    }

    #[test]
    fn original_and_alternative_titles_are_accepted() {
        let mut q = query("Amélie", Some(2001), Scope::Movie);
        q.original_title = Some("Le Fabuleux Destin d'Amélie Poulain".into());
        q.alternative_titles = vec!["Amelie from Montmartre".into()];
        assert_eq!(judge("Amelie.2001.1080p", &q), Relevance::Match);
        assert_eq!(
            judge("Le.Fabuleux.Destin.d.Amelie.Poulain.2001.1080p", &q),
            Relevance::Match
        );
        assert_eq!(
            judge("Amelie.From.Montmartre.2001.720p", &q),
            Relevance::Match
        );
    }

    #[test]
    fn a_tv_release_is_not_a_movie() {
        let q = query("Dune", Some(2021), Scope::Movie);
        assert_eq!(
            judge("Dune.S01E01.1080p", &q),
            Relevance::Hidden(Mismatch::Kind)
        );
    }

    #[test]
    fn a_season_pack_is_listed_for_an_episode() {
        let q = query("Show Name", None, Scope::Episode(2, 5));
        assert_eq!(judge("Show.Name.S02E05.1080p", &q), Relevance::Match);
        assert_eq!(judge("Show.Name.S02.Complete.1080p", &q), Relevance::Match);
        assert_eq!(judge("Show.Name.S01-S03.1080p", &q), Relevance::Match);
        assert_eq!(
            judge("Show.Name.Complete.Series.720p", &q),
            Relevance::Match
        );
        assert_eq!(judge("Show.Name.S02E04-E06.720p", &q), Relevance::Match);
        assert_eq!(
            judge("Show.Name.S03E05.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
        assert_eq!(
            judge("Show.Name.S02E06.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
        assert_eq!(
            judge("Show.Name.S03.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
        assert_eq!(
            judge("Show.Name.S01-S01.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
        assert_eq!(
            judge("Show.Name.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
        assert_eq!(
            judge("Other.Show.S02E05.1080p", &q),
            Relevance::Hidden(Mismatch::Title)
        );
    }

    #[test]
    fn a_season_scope_wants_that_season_or_a_pack_holding_it() {
        let q = query("Show Name", None, Scope::Season(2));
        assert_eq!(judge("Show.Name.S02.1080p", &q), Relevance::Match);
        assert_eq!(judge("Show.Name.Seasons.1-3.720p", &q), Relevance::Match);
        assert_eq!(
            judge("Show.Name.Complete.Series.720p", &q),
            Relevance::Match
        );
        assert_eq!(
            judge("Show.Name.S03.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
        assert_eq!(
            judge("Show.Name.S02E01.1080p", &q),
            Relevance::Hidden(Mismatch::Scope)
        );
    }

    #[test]
    fn a_whole_show_wants_anything_of_it() {
        let q = query("Show Name", None, Scope::Show);
        for name in [
            "Show.Name.S01E01.1080p",
            "Show.Name.S09.720p",
            "Show.Name.Complete.Series",
            "Show.Name.1080p.WEB",
        ] {
            assert_eq!(judge(name, &q), Relevance::Match, "{name}");
        }
        assert_eq!(
            judge("Another.Show.S01E01", &q),
            Relevance::Hidden(Mismatch::Title)
        );
    }

    #[test]
    fn the_default_order_is_group_then_seeders_then_size() {
        let mut rows: Vec<(Release, ParsedRelease)> = [
            ("Movie.2020.1080p.WEB-DL", 50, 3),
            ("Movie.2020.2160p.WEB-DL", 5, 9),
            ("Movie.2020.1080p.BluRay", 200, 5),
            ("Movie.2020.720p.HDTV", 900, 1),
            ("Movie.2020.2160p.BluRay", 5, 12),
            ("Movie.2020.WEB", 70, 2),
            ("Movie.2020.1080p.WEBRip", 50, 8),
        ]
        .into_iter()
        .map(|(name, seeders, gb)| {
            let release = rel(name, Some(gb * 1_000_000_000), Some(seeders));
            let parsed = parse_release(name, &[]);
            (release, parsed)
        })
        .collect();
        rows.sort_by_key(|(release, parsed)| order(release, parsed));
        let names: Vec<_> = rows.iter().map(|(r, _)| r.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Movie.2020.2160p.BluRay",
                "Movie.2020.2160p.WEB-DL",
                "Movie.2020.1080p.BluRay",
                "Movie.2020.1080p.WEBRip",
                "Movie.2020.1080p.WEB-DL",
                "Movie.2020.720p.HDTV",
                "Movie.2020.WEB",
            ]
        );
    }

    #[test]
    fn a_small_2160p_release_is_flagged_too_small() {
        let mut q = query("Movie", Some(2020), Scope::Movie);
        q.runtime_minutes = Some(100);
        let release = rel("Movie.2020.2160p.WEB-DL", Some(300_000_000), Some(10));
        let parsed = parse_release(&release.name, &[]);
        let found = flags(&release, &parsed, &q);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, FlagKind::TooSmall);
        assert_eq!(found[0].text, "Too small for 2160p");

        let big = rel("Movie.2020.2160p.WEB-DL", Some(15_000_000_000), Some(10));
        assert!(flags(&big, &parsed, &q).is_empty());
    }

    #[test]
    fn the_size_table_is_per_resolution() {
        let mut q = query("Movie", None, Scope::Movie);
        q.runtime_minutes = Some(100);
        // 100 minutes: 480p 375 MB, 720p 750 MB, 1080p 1.5 GB, 2160p 4.5 GB.
        for (name, just_under, just_over) in [
            ("Movie.480p", 374_000_000, 376_000_000),
            ("Movie.720p", 749_000_000, 751_000_000),
            ("Movie.1080p", 1_499_000_000, 1_501_000_000),
            ("Movie.2160p", 4_499_000_000, 4_501_000_000),
        ] {
            let parsed = parse_release(name, &[]);
            let small = flags(&rel(name, Some(just_under), Some(1)), &parsed, &q);
            assert!(
                small.iter().any(|f| f.kind == FlagKind::TooSmall),
                "{name} under"
            );
            let fine = flags(&rel(name, Some(just_over), Some(1)), &parsed, &q);
            assert!(
                fine.iter().all(|f| f.kind != FlagKind::TooSmall),
                "{name} over"
            );
        }
    }

    #[test]
    fn no_runtime_size_or_resolution_means_no_size_flag() {
        let mut q = query("Movie", None, Scope::Movie);
        let tiny = rel("Movie.2160p", Some(1000), Some(1));
        let parsed = parse_release(&tiny.name, &[]);
        assert!(flags(&tiny, &parsed, &q).is_empty(), "no runtime");
        q.runtime_minutes = Some(100);
        let unknown = rel("Movie.WEB", Some(1000), Some(1));
        assert!(
            flags(&unknown, &parse_release(&unknown.name, &[]), &q).is_empty(),
            "no resolution"
        );
        let unsized_release = rel("Movie.2160p", None, Some(1));
        assert!(flags(&unsized_release, &parsed, &q).is_empty(), "no size");
    }

    #[test]
    fn episodes_are_checked_per_episode_and_packs_are_not_checked() {
        let mut q = query("Show", None, Scope::Show);
        q.runtime_minutes = Some(45);
        // 45 minutes of 1080p: 675 MB.
        let one = rel("Show.S01E01.1080p", Some(600_000_000), Some(1));
        assert!(
            flags(&one, &parse_release(&one.name, &[]), &q)
                .iter()
                .any(|f| f.kind == FlagKind::TooSmall)
        );
        let two = rel("Show.S01E01-E02.1080p", Some(1_000_000_000), Some(1));
        assert!(
            flags(&two, &parse_release(&two.name, &[]), &q)
                .iter()
                .any(|f| f.kind == FlagKind::TooSmall)
        );
        let pack = rel("Show.S01.1080p", Some(1_000_000), Some(1));
        assert!(flags(&pack, &parse_release(&pack.name, &[]), &q).is_empty());
    }

    #[test]
    fn seeders_payload_and_recordings_are_flagged_in_plain_words() {
        let q = query("Movie", None, Scope::Movie);
        let release = rel("Movie.2020.CAM.x264.exe", None, Some(0));
        let parsed = parse_release(&release.name, &[]);
        let texts: Vec<_> = flags(&release, &parsed, &q)
            .into_iter()
            .map(|f| f.text)
            .collect();
        assert_eq!(
            texts,
            vec![
                "Contains an executable or archive file",
                "Camera recording",
                "No seeders"
            ]
        );
        let unknown_seeders = rel("Movie.2020", None, None);
        assert!(
            flags(
                &unknown_seeders,
                &parse_release(&unknown_seeders.name, &[]),
                &q
            )
            .is_empty()
        );
    }

    fn copy(resolution: Resolution, source: MediaSource, hdr: Hdr) -> LibraryCopy {
        LibraryCopy {
            resolution,
            source,
            hdr,
        }
    }

    #[test]
    fn the_upgrade_marker_compares_resolution_source_and_hdr() {
        let mine = copy(Resolution::R1080, MediaSource::WebDl, Hdr::None);
        let mark = |name: &str| upgrade(&parse_release(name, &[]), &mine);
        assert_eq!(mark("Movie.2020.2160p.BluRay.x265"), Upgrade::Better);
        assert_eq!(mark("Movie.2020.1080p.WEB-DL.x264"), Upgrade::Same);
        assert_eq!(mark("Movie.2020.1080p.BluRay.x264"), Upgrade::Better);
        assert_eq!(mark("Movie.2020.1080p.BluRay.REMUX"), Upgrade::Better);
        assert_eq!(mark("Movie.2020.1080p.WEB-DL.HDR10"), Upgrade::Better);
        assert_eq!(mark("Movie.2020.1080p.WEBRip.x264"), Upgrade::Worse);
        assert_eq!(mark("Movie.2020.720p.BluRay"), Upgrade::Worse);
        assert_eq!(mark("Movie.2020.1080p.HDTV"), Upgrade::Worse);
    }

    #[test]
    fn a_hdr_copy_is_matched_by_any_hdr_release() {
        let mine = copy(Resolution::R2160, MediaSource::UhdBluray, Hdr::Hdr10);
        let mark = |name: &str| upgrade(&parse_release(name, &[]), &mine);
        assert_eq!(mark("Movie.2020.2160p.BluRay.DV"), Upgrade::Same);
        assert_eq!(mark("Movie.2020.2160p.BluRay.REMUX.HDR"), Upgrade::Better);
        assert_eq!(mark("Movie.2020.2160p.BluRay"), Upgrade::Worse);
    }

    #[test]
    fn a_cam_is_never_an_upgrade_and_an_unknown_resolution_counts_as_equal() {
        let mine = copy(Resolution::R720, MediaSource::Dvd, Hdr::None);
        assert_eq!(
            upgrade(&parse_release("Movie.2025.2160p.CAM", &[]), &mine),
            Upgrade::Worse
        );
        let unknown = copy(Resolution::Unknown, MediaSource::WebDl, Hdr::None);
        assert_eq!(
            upgrade(&parse_release("Movie.2020.1080p.WEB-DL", &[]), &unknown),
            Upgrade::Same
        );
        assert_eq!(
            upgrade(&parse_release("Movie.2020.1080p.BluRay", &[]), &unknown),
            Upgrade::Better
        );
    }

    #[test]
    fn the_spec_example_marks_one_better_and_one_same() {
        let mut q = query("Movie", Some(2020), Scope::Movie);
        q.library_copy = Some(copy(Resolution::R1080, MediaSource::WebDl, Hdr::None));
        let better = assess(
            &rel(
                "Movie.2020.2160p.BluRay.x265",
                Some(30_000_000_000),
                Some(5),
            ),
            &q,
        );
        let same = assess(
            &rel("Movie.2020.1080p.WEB-DL.x264", Some(3_000_000_000), Some(5)),
            &q,
        );
        assert_eq!(better.upgrade, Some(Upgrade::Better));
        assert_eq!(same.upgrade, Some(Upgrade::Same));
        q.library_copy = None;
        assert_eq!(
            assess(&rel("Movie.2020.1080p", None, None), &q).upgrade,
            None
        );
    }

    #[test]
    fn assess_reports_the_pack_kind() {
        let q = query("Show", None, Scope::Show);
        let kind = |name: &str| assess(&rel(name, None, None), &q).pack;
        assert_eq!(kind("Show.S01.1080p"), Some(PackKind::Season));
        assert_eq!(kind("Show.S01-S03.1080p"), Some(PackKind::Seasons));
        assert_eq!(kind("Show.Complete.Series"), Some(PackKind::Complete));
        assert_eq!(kind("Show.S01E01-E02.720p"), Some(PackKind::Episodes));
        assert_eq!(kind("Show.S01E01.720p"), None);
    }
}
