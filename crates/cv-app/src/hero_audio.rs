//! The audio track to show first: the one a viewer would expect to hear.
//!
//! The detail page's audio badge, the Inspector and the library table's Audio column all name one
//! track of the main video. [`hero_audio`] picks it the same way everywhere:
//!
//! 1. a track the container marks as the **original** version,
//! 2. else a track in the title's **original language**,
//! 3. else the track marked **default**,
//! 4. else the **first** track.
//!
//! A **commentary** track is skipped at every step while another track exists, so a director's
//! commentary never stands for the film. Languages are compared through the ISO 639 tables, so
//! `en`, `eng` and `en-US` are one language, and a French dub of an English film loses to the
//! English track even when the dub is the default.

use cv_domain::{AudioStream, StreamFlag, same_language};

/// The track to show first among `streams` (in file order), given the title's original language
/// (empty when unknown). `None` only when there are no streams.
#[must_use]
pub fn hero_audio<'a>(
    streams: &'a [AudioStream],
    original_language: &str,
) -> Option<&'a AudioStream> {
    hero_audio_index(streams, original_language).map(|index| &streams[index])
}

/// [`hero_audio`] as a position in `streams`.
#[must_use]
pub fn hero_audio_index(streams: &[AudioStream], original_language: &str) -> Option<usize> {
    let is_comment = |stream: &AudioStream| stream.flags.contains(&StreamFlag::Comment);
    let all: Vec<usize> = (0..streams.len()).collect();
    let main: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| !is_comment(&streams[i]))
        .collect();
    // Commentary only counts when nothing else is there.
    let pool = if main.is_empty() { all } else { main };

    let first =
        |test: &dyn Fn(&AudioStream) -> bool| pool.iter().copied().find(|&i| test(&streams[i]));
    first(&|s| s.flags.contains(&StreamFlag::Original))
        .or_else(|| {
            // Of several tracks in the original language, one the container calls default wins,
            // and a dub (a track marked as one) comes last.
            let matching: Vec<usize> = pool
                .iter()
                .copied()
                .filter(|&i| same_language(&streams[i].language, original_language))
                .collect();
            matching
                .iter()
                .copied()
                .find(|&i| {
                    streams[i].flags.contains(&StreamFlag::Default)
                        && !streams[i].flags.contains(&StreamFlag::Dub)
                })
                .or_else(|| {
                    matching
                        .iter()
                        .copied()
                        .find(|&i| !streams[i].flags.contains(&StreamFlag::Dub))
                })
                .or_else(|| matching.first().copied())
        })
        .or_else(|| first(&|s| s.flags.contains(&StreamFlag::Default)))
        .or_else(|| pool.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(language: &str, flags: &[StreamFlag]) -> AudioStream {
        AudioStream {
            codec: "AC3".into(),
            language: language.into(),
            flags: flags.iter().copied().collect(),
            ..AudioStream::default()
        }
    }

    fn pick(streams: &[AudioStream], original: &str) -> Option<usize> {
        hero_audio_index(streams, original)
    }

    #[test]
    fn nothing_to_pick_from_gives_nothing() {
        assert_eq!(pick(&[], "en"), None);
        assert!(hero_audio(&[], "en").is_none());
    }

    #[test]
    fn the_original_flag_wins_over_everything() {
        let streams = [
            track("fr", &[StreamFlag::Default]),
            track("de", &[]),
            track("ja", &[StreamFlag::Original]),
        ];
        assert_eq!(pick(&streams, "en"), Some(2));
    }

    #[test]
    fn the_french_dub_is_passed_over_for_the_original_language() {
        // An English film whose French dub is the default track.
        let streams = [
            track("fr", &[StreamFlag::Default, StreamFlag::Dub]),
            track("en", &[]),
        ];
        assert_eq!(pick(&streams, "en"), Some(1));
        // Without a known original language the default track stays.
        assert_eq!(pick(&streams, ""), Some(0));
        // A dub that is the only track in the original language still counts.
        let dub_only = [track("fr", &[]), track("en", &[StreamFlag::Dub])];
        assert_eq!(pick(&dub_only, "en"), Some(1));
    }

    #[test]
    fn two_letter_and_three_letter_codes_are_the_same_language() {
        let streams = [track("fra", &[StreamFlag::Default]), track("eng", &[])];
        assert_eq!(pick(&streams, "en"), Some(1));
        let streams = [track("en", &[]), track("fr", &[StreamFlag::Default])];
        assert_eq!(pick(&streams, "fre"), Some(1));
        assert_eq!(pick(&[track("en-US", &[])], "eng"), Some(0));
    }

    #[test]
    fn among_tracks_in_the_original_language_the_default_one_wins() {
        let streams = [
            track("en", &[]),
            track("en", &[StreamFlag::Default]),
            track("fr", &[]),
        ];
        assert_eq!(pick(&streams, "en"), Some(1));
    }

    #[test]
    fn a_commentary_is_skipped_while_another_track_exists() {
        // The commentary is the default and in the original language.
        let streams = [
            track("en", &[StreamFlag::Default, StreamFlag::Comment]),
            track("de", &[]),
        ];
        assert_eq!(pick(&streams, "en"), Some(1));
        // Even an Original-flagged commentary does not stand for the film.
        let streams = [
            track("en", &[StreamFlag::Original, StreamFlag::Comment]),
            track("en", &[]),
        ];
        assert_eq!(pick(&streams, "en"), Some(1));
        // When the commentary is all there is, it is shown.
        let only = [track("en", &[StreamFlag::Comment])];
        assert_eq!(pick(&only, "en"), Some(0));
    }

    #[test]
    fn the_default_then_the_first_track_decide_without_a_language() {
        let streams = [track("de", &[]), track("fr", &[StreamFlag::Default])];
        assert_eq!(pick(&streams, ""), Some(1));
        assert_eq!(pick(&streams, "ja"), Some(1));
        let streams = [track("de", &[]), track("fr", &[])];
        assert_eq!(pick(&streams, "ja"), Some(0));
    }

    #[test]
    fn a_track_without_a_language_never_matches_one() {
        let streams = [track("", &[]), track("en", &[])];
        assert_eq!(pick(&streams, "en"), Some(1));
        assert_eq!(pick(&[track("", &[])], ""), Some(0));
    }
}
