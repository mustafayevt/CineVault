//! Runtime from the media files: the length of the video instead of the scraped runtime.
//!
//! Spec: `media-info` (Runtime from media). The parts of a stacked movie are summed, so a movie
//! in two parts of 1 h 40 min and 1 h 32 min has a runtime of 3 h 12 min (192 minutes).

use cv_domain::{MediaFile, MediaFileType};

/// The runtime in whole minutes of the video files of one item, or `None` when no video file
/// has a known length (so the scraped runtime stays). Non-video files are ignored. The summed
/// seconds are rounded to the nearest minute, not each part, so rounding errors do not add up.
#[must_use]
pub fn runtime_minutes_from_media(files: &[MediaFile]) -> Option<u32> {
    let seconds: u64 = files
        .iter()
        .filter(|f| f.kind == MediaFileType::Video)
        .map(|f| u64::from(f.duration_secs))
        .sum();
    (seconds > 0).then(|| u32::try_from((seconds + 30) / 60).unwrap_or(u32::MAX))
}

/// Picks the runtime to use: the media runtime when `from_media` is on and it is known,
/// otherwise the `scraped` one.
#[must_use]
pub fn effective_runtime_minutes(from_media: bool, files: &[MediaFile], scraped: u32) -> u32 {
    if from_media {
        runtime_minutes_from_media(files).unwrap_or(scraped)
    } else {
        scraped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(secs: u32, stacking: u32) -> MediaFile {
        let mut f = MediaFile::new(
            MediaFileType::Video,
            "D:/Movies/Heat",
            format!("Heat cd{stacking}.mkv"),
        );
        f.duration_secs = secs;
        f.stacking = stacking;
        f
    }

    #[test]
    fn stacked_parts_are_summed() {
        let parts = [video(100 * 60, 1), video(92 * 60, 2)];
        assert_eq!(runtime_minutes_from_media(&parts), Some(192));
    }

    #[test]
    fn a_single_file_gives_its_own_length_rounded_to_the_minute() {
        assert_eq!(runtime_minutes_from_media(&[video(7020, 0)]), Some(117));
        assert_eq!(runtime_minutes_from_media(&[video(7049, 0)]), Some(117));
        assert_eq!(runtime_minutes_from_media(&[video(7050, 0)]), Some(118));
    }

    #[test]
    fn rounding_happens_on_the_sum() {
        // 40 s + 40 s = 80 s = 1 min 20 s -> 1 minute; rounding each part would give 2.
        assert_eq!(
            runtime_minutes_from_media(&[video(40, 1), video(40, 2)]),
            Some(1)
        );
    }

    #[test]
    fn unknown_lengths_and_other_file_types_are_ignored() {
        assert_eq!(runtime_minutes_from_media(&[]), None);
        assert_eq!(runtime_minutes_from_media(&[video(0, 0)]), None);
        let mut subtitle = MediaFile::new(MediaFileType::Subtitle, "D:/m", "a.srt");
        subtitle.duration_secs = 5000;
        assert_eq!(runtime_minutes_from_media(&[subtitle, video(0, 1)]), None);
    }

    #[test]
    fn the_setting_chooses_between_media_and_scraped_runtime() {
        let parts = [video(100 * 60, 1), video(92 * 60, 2)];
        assert_eq!(effective_runtime_minutes(true, &parts, 120), 192);
        assert_eq!(effective_runtime_minutes(false, &parts, 120), 120);
        assert_eq!(effective_runtime_minutes(true, &[], 120), 120);
    }
}
