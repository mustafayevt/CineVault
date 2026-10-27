//! Resolution buckets and quality categories.
//!
//! A video frame size maps to a standard label (`480p`, `1080p`, `2160p` ...) and to a category
//! (LD, SD, HD, UHD). Filters, the NFO writer and the renamer tokens use these.
//!
//! The rules:
//!
//! * buckets run from 96p to 4320p;
//! * a size reaches a bucket when its height is at least the bucket height minus 1 %, so a
//!   1,070-line frame is still 1080p; the highest bucket reached wins;
//! * **letterbox rule:** wide films are stored with the black bars cropped, so a 3840x1608 frame
//!   has far fewer lines than 2160. For the buckets with a common 16:9 frame width (720p, 1080p,
//!   2160p, 4320p) a width of at least the bucket width minus 1 % also reaches the bucket;
//! * the width rule is not used for 1440p (2560 wide is also the ultra-wide 1080p frame) or for
//!   the standard-definition buckets, whose widths are not unique.
//!
//! This is a pure function of the frame size; reading the size is the job of the MediaInfo
//! mapping.

use std::fmt;

/// Quality category of a resolution bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResolutionCategory {
    /// Low definition: 360p and below.
    Ld,
    /// Standard definition: 480p, 540p and 576p.
    Sd,
    /// High definition: 720p up to 1440p.
    Hd,
    /// Ultra high definition: 2160p and 4320p.
    Uhd,
}

impl ResolutionCategory {
    /// Short upper-case name: `LD`, `SD`, `HD` or `UHD`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ld => "LD",
            Self::Sd => "SD",
            Self::Hd => "HD",
            Self::Uhd => "UHD",
        }
    }
}

impl fmt::Display for ResolutionCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A standard resolution bucket, named by its number of lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resolution {
    P96,
    P120,
    P144,
    P240,
    P288,
    P360,
    P480,
    P540,
    P576,
    P720,
    P1080,
    P1440,
    P2160,
    P4320,
}

/// Bucket, height and the 16:9 frame width used by the letterbox rule, highest first.
const BUCKETS: [(Resolution, u32, Option<u32>); 14] = [
    (Resolution::P4320, 4320, Some(7680)),
    (Resolution::P2160, 2160, Some(3840)),
    (Resolution::P1440, 1440, None),
    (Resolution::P1080, 1080, Some(1920)),
    (Resolution::P720, 720, Some(1280)),
    (Resolution::P576, 576, None),
    (Resolution::P540, 540, None),
    (Resolution::P480, 480, None),
    (Resolution::P360, 360, None),
    (Resolution::P288, 288, None),
    (Resolution::P240, 240, None),
    (Resolution::P144, 144, None),
    (Resolution::P120, 120, None),
    (Resolution::P96, 96, None),
];

/// True when `value` is at least `target` less 1 %.
fn reaches(value: u32, target: u32) -> bool {
    u64::from(value) * 100 >= u64::from(target) * 99
}

impl Resolution {
    /// Number of lines of the bucket.
    pub fn height(self) -> u32 {
        match self {
            Self::P96 => 96,
            Self::P120 => 120,
            Self::P144 => 144,
            Self::P240 => 240,
            Self::P288 => 288,
            Self::P360 => 360,
            Self::P480 => 480,
            Self::P540 => 540,
            Self::P576 => 576,
            Self::P720 => 720,
            Self::P1080 => 1080,
            Self::P1440 => 1440,
            Self::P2160 => 2160,
            Self::P4320 => 4320,
        }
    }

    /// Label such as `1080p`.
    pub fn label(self) -> &'static str {
        match self {
            Self::P96 => "96p",
            Self::P120 => "120p",
            Self::P144 => "144p",
            Self::P240 => "240p",
            Self::P288 => "288p",
            Self::P360 => "360p",
            Self::P480 => "480p",
            Self::P540 => "540p",
            Self::P576 => "576p",
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::P2160 => "2160p",
            Self::P4320 => "4320p",
        }
    }

    /// Quality category of the bucket.
    pub fn category(self) -> ResolutionCategory {
        match self {
            Self::P96 | Self::P120 | Self::P144 | Self::P240 | Self::P288 | Self::P360 => {
                ResolutionCategory::Ld
            }
            Self::P480 | Self::P540 | Self::P576 => ResolutionCategory::Sd,
            Self::P720 | Self::P1080 | Self::P1440 => ResolutionCategory::Hd,
            Self::P2160 | Self::P4320 => ResolutionCategory::Uhd,
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The bucket for a frame size, or `None` when the height is zero or below the smallest bucket.
///
/// ```
/// use cv_media::resolution::{resolution_bucket, Resolution, ResolutionCategory};
/// let bucket = resolution_bucket(3840, 1608).unwrap();
/// assert_eq!(bucket, Resolution::P2160);
/// assert_eq!(bucket.category(), ResolutionCategory::Uhd);
/// ```
pub fn resolution_bucket(width: u32, height: u32) -> Option<Resolution> {
    BUCKETS
        .iter()
        .find(|(_, bucket_height, bucket_width)| {
            reaches(height, *bucket_height)
                || bucket_width.is_some_and(|w| width > 0 && reaches(width, w))
        })
        .map(|(bucket, ..)| *bucket)
}

/// The label for a frame size, for example `2160p`; `None` when it is below every bucket.
pub fn resolution_label(width: u32, height: u32) -> Option<&'static str> {
    resolution_bucket(width, height).map(Resolution::label)
}

/// The category for a frame size; `None` when it is below every bucket.
pub fn resolution_category(width: u32, height: u32) -> Option<ResolutionCategory> {
    resolution_bucket(width, height).map(Resolution::category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sizes_map_to_buckets_and_categories() {
        use ResolutionCategory::{Hd, Ld, Sd, Uhd};
        // (width, height, label, category)
        let table: &[(u32, u32, Option<&str>, Option<ResolutionCategory>)] = &[
            // The letterboxed UHD file from the spec.
            (3840, 1608, Some("2160p"), Some(Uhd)),
            (3840, 2160, Some("2160p"), Some(Uhd)),
            (4096, 2160, Some("2160p"), Some(Uhd)),
            (3840, 1600, Some("2160p"), Some(Uhd)),
            (7680, 4320, Some("4320p"), Some(Uhd)),
            (7680, 3200, Some("4320p"), Some(Uhd)),
            (2560, 1440, Some("1440p"), Some(Hd)),
            // 1 % tolerance on the height and the width.
            (1920, 1080, Some("1080p"), Some(Hd)),
            (1920, 1070, Some("1080p"), Some(Hd)),
            (1916, 1069, Some("1080p"), Some(Hd)),
            (1800, 1060, Some("720p"), Some(Hd)),
            (3800, 2140, Some("2160p"), Some(Uhd)),
            (3790, 1600, Some("1440p"), Some(Hd)),
            // Letterboxed or cropped HD.
            (1920, 800, Some("1080p"), Some(Hd)),
            (1920, 816, Some("1080p"), Some(Hd)),
            (1904, 800, Some("1080p"), Some(Hd)),
            (1280, 536, Some("720p"), Some(Hd)),
            (1280, 720, Some("720p"), Some(Hd)),
            // Ultra-wide 1080p is not read as 1440p.
            (2560, 1080, Some("1080p"), Some(Hd)),
            // Standard definition.
            (720, 576, Some("576p"), Some(Sd)),
            (720, 480, Some("480p"), Some(Sd)),
            (640, 480, Some("480p"), Some(Sd)),
            (960, 540, Some("540p"), Some(Sd)),
            (720, 400, Some("360p"), Some(Ld)),
            // Low definition.
            (640, 360, Some("360p"), Some(Ld)),
            (426, 240, Some("240p"), Some(Ld)),
            (320, 180, Some("144p"), Some(Ld)),
            (176, 96, Some("96p"), Some(Ld)),
            // Unknown.
            (0, 0, None, None),
            (1920, 0, Some("1080p"), Some(Hd)),
            (100, 50, None, None),
        ];
        for &(width, height, label, category) in table {
            assert_eq!(
                resolution_label(width, height),
                label,
                "label of {width}x{height}"
            );
            assert_eq!(
                resolution_category(width, height),
                category,
                "category of {width}x{height}"
            );
        }
    }

    #[test]
    fn every_bucket_has_its_own_height_and_label() {
        for (bucket, height, _) in BUCKETS {
            assert_eq!(bucket.height(), height);
            assert_eq!(bucket.label(), format!("{height}p"));
            assert_eq!(resolution_bucket(0, height), Some(bucket));
        }
    }

    #[test]
    fn categories_cover_the_documented_ranges() {
        assert_eq!(Resolution::P360.category(), ResolutionCategory::Ld);
        assert_eq!(Resolution::P480.category(), ResolutionCategory::Sd);
        assert_eq!(Resolution::P576.category(), ResolutionCategory::Sd);
        assert_eq!(Resolution::P720.category(), ResolutionCategory::Hd);
        assert_eq!(Resolution::P1440.category(), ResolutionCategory::Hd);
        assert_eq!(Resolution::P2160.category(), ResolutionCategory::Uhd);
        assert_eq!(Resolution::P4320.category(), ResolutionCategory::Uhd);
        assert_eq!(ResolutionCategory::Uhd.to_string(), "UHD");
    }
}
