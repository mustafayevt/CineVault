//! Where in the video the samples are taken.

use serde::{Deserialize, Serialize};

/// How thoroughly the picture is sampled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArdMode {
    /// 4 samples of 1 s, at most 1800 s apart.
    Fast,
    /// 6 samples of 2 s, at most 900 s apart.
    #[default]
    #[serde(rename = "default")]
    Standard,
    /// 30 samples of 2 s, at most 900 s apart. Only this mode looks for a second format.
    Accurate,
}

impl ArdMode {
    /// Length of one sample in seconds.
    #[must_use]
    pub const fn sample_secs(self) -> f64 {
        match self {
            Self::Fast => 1.0,
            Self::Standard | Self::Accurate => 2.0,
        }
    }

    /// The least number of samples taken (fewer only when the video is too short).
    #[must_use]
    pub const fn min_samples(self) -> u32 {
        match self {
            Self::Fast => 4,
            Self::Standard => 6,
            Self::Accurate => 30,
        }
    }

    /// The largest distance between two samples, in seconds.
    #[must_use]
    pub const fn max_gap_secs(self) -> f64 {
        match self {
            Self::Fast => 1800.0,
            Self::Standard | Self::Accurate => 900.0,
        }
    }
}

/// One stretch of video to look at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Index of the part (file) the sample is in.
    pub part: usize,
    /// Seconds from the start of that part.
    pub start: f64,
    /// Seconds to read.
    pub length: f64,
}

/// Plans the samples over the parts of a video (one file, or the stacked parts in order).
///
/// The parts form one timeline. The first `begin_pct` and the last `end_pct` percent of it are
/// left out (titles, credits). What remains is cut into equal slots, one per sample, each
/// sample sitting in the middle of its slot, so the gap between samples never exceeds the
/// mode's maximum. A video too short for the mode's sample count gets as many whole samples as
/// fit, at least one.
#[must_use]
pub fn plan_samples(durations: &[f64], mode: ArdMode, begin_pct: f64, end_pct: f64) -> Vec<Sample> {
    let durations: Vec<f64> = durations.iter().map(|d| d.max(0.0)).collect();
    let total: f64 = durations.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }
    // Each end is capped below half, so something always remains.
    let begin_pct = begin_pct.clamp(0.0, 45.0);
    let end_pct = end_pct.clamp(0.0, 45.0);
    let window_start = total * begin_pct / 100.0;
    let usable = total - window_start - total * end_pct / 100.0;
    let sample_secs = mode.sample_secs();

    let wanted = mode
        .min_samples()
        .max(ceil_to_u32(usable / mode.max_gap_secs()));
    let fits = floor_to_u32(usable / sample_secs).max(1);
    let count = wanted.min(fits);
    let slot = usable / f64::from(count);
    let length = sample_secs.min(slot);

    (0..count)
        .map(|i| {
            let at = window_start + f64::from(i) * slot + (slot - length) / 2.0;
            locate(&durations, at, length)
        })
        .collect()
}

/// Finds the part holding the timeline position `at` and keeps the sample inside it.
fn locate(durations: &[f64], at: f64, length: f64) -> Sample {
    let mut offset = 0.0;
    for (part, duration) in durations.iter().enumerate() {
        let last = part + 1 == durations.len();
        if at < offset + duration || last {
            let length = length.min(*duration);
            let start = (at - offset).clamp(0.0, (duration - length).max(0.0));
            return Sample {
                part,
                start,
                length,
            };
        }
        offset += duration;
    }
    // Only reached for an empty list, which the caller has already ruled out.
    Sample {
        part: 0,
        start: 0.0,
        length,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn ceil_to_u32(value: f64) -> u32 {
    value.ceil().clamp(0.0, f64::from(u32::MAX)) as u32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn floor_to_u32(value: f64) -> u32 {
    value.floor().clamp(0.0, f64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode_row(mode: ArdMode) -> (f64, u32, f64) {
        (mode.sample_secs(), mode.min_samples(), mode.max_gap_secs())
    }

    #[test]
    fn modes_match_the_documented_table() {
        assert_eq!(mode_row(ArdMode::Fast), (1.0, 4, 1800.0));
        assert_eq!(mode_row(ArdMode::Standard), (2.0, 6, 900.0));
        assert_eq!(mode_row(ArdMode::Accurate), (2.0, 30, 900.0));
        assert_eq!(
            serde_json::to_string(&ArdMode::Standard).unwrap(),
            "\"default\""
        );
        assert_eq!(
            serde_json::from_str::<ArdMode>("\"fast\"").unwrap(),
            ArdMode::Fast
        );
    }

    #[test]
    fn samples_stay_inside_the_window_and_inside_the_video() {
        let total = 7200.0;
        for mode in [ArdMode::Fast, ArdMode::Standard, ArdMode::Accurate] {
            let samples = plan_samples(&[total], mode, 2.0, 8.0);
            let by_gap = (total * 0.90 / mode.max_gap_secs()).ceil() as usize;
            assert_eq!(samples.len(), (mode.min_samples() as usize).max(by_gap));
            for s in &samples {
                assert!(s.start >= total * 0.02, "{s:?}");
                assert!(s.start + s.length <= total * 0.92 + 1e-6, "{s:?}");
                assert!((s.length - mode.sample_secs()).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn the_gap_between_samples_never_exceeds_the_maximum() {
        // A 10 hour video needs more than 6 samples to stay under 900 s.
        let total = 36_000.0;
        let samples = plan_samples(&[total], ArdMode::Standard, 2.0, 8.0);
        assert_eq!(samples.len(), 36); // ceil(32400 / 900)
        for pair in samples.windows(2) {
            assert!(pair[1].start - pair[0].start <= 900.0 + 1e-6);
        }
    }

    #[test]
    fn samples_are_evenly_spaced() {
        let samples = plan_samples(&[1000.0], ArdMode::Standard, 2.0, 8.0);
        let step = samples[1].start - samples[0].start;
        for pair in samples.windows(2) {
            assert!((pair[1].start - pair[0].start - step).abs() < 1e-9);
        }
    }

    #[test]
    fn a_short_video_gets_fewer_whole_samples() {
        // 10 s: window 0.2 to 9.2 s = 9 s, which holds four 2 s samples.
        let samples = plan_samples(&[10.0], ArdMode::Accurate, 2.0, 8.0);
        assert_eq!(samples.len(), 4);
        for s in &samples {
            assert!((s.length - 2.0).abs() < 1e-9);
            assert!(s.start + s.length <= 10.0);
        }
        // 1.5 s: one sample, no longer than the video.
        let samples = plan_samples(&[1.5], ArdMode::Standard, 2.0, 8.0);
        assert_eq!(samples.len(), 1);
        assert!(samples[0].length <= 1.5);
    }

    #[test]
    fn no_duration_means_no_samples() {
        assert!(plan_samples(&[], ArdMode::Fast, 2.0, 8.0).is_empty());
        assert!(plan_samples(&[0.0], ArdMode::Fast, 2.0, 8.0).is_empty());
    }

    #[test]
    fn stacked_parts_form_one_timeline() {
        // Parts of 3000 s and 1000 s: the window is 80 s to 3680 s.
        let samples = plan_samples(&[3000.0, 1000.0], ArdMode::Standard, 2.0, 8.0);
        assert_eq!(samples.len(), 6);
        let parts: Vec<usize> = samples.iter().map(|s| s.part).collect();
        assert_eq!(parts, vec![0, 0, 0, 0, 0, 1]);
        for s in &samples {
            let duration = if s.part == 0 { 3000.0 } else { 1000.0 };
            assert!(s.start >= 0.0 && s.start + s.length <= duration + 1e-9);
        }
        // The last slot is 3080..3680 on the timeline, which is 80..680 in the second part.
        assert!(samples[5].start > 80.0 && samples[5].start < 680.0);
    }

    #[test]
    fn a_sample_crossing_a_part_boundary_is_pulled_back_into_its_part() {
        let samples = plan_samples(&[3.0, 3.0], ArdMode::Fast, 0.0, 0.0);
        for s in &samples {
            assert!(s.start >= 0.0 && s.start + s.length <= 3.0 + 1e-9, "{s:?}");
        }
    }

    #[test]
    fn silly_percentages_cannot_remove_the_whole_video() {
        let samples = plan_samples(&[100.0], ArdMode::Fast, 80.0, 80.0);
        assert!(!samples.is_empty());
        for s in &samples {
            assert!(s.start >= 0.0 && s.start + s.length <= 100.0);
        }
    }
}
