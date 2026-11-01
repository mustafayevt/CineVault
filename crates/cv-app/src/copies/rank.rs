//! The "Best quality" suggestion of the Compare copies page.
//!
//! Each copy's main video gives a [`Quality`] compared in this order: the resolution label, the
//! best HDR format, the lead audio track's channels and then its codec, and the video bitrate.
//! Source and size are shown but never rank: a bigger file is not a better one, and the source is
//! read from names. [`best`] marks a copy only when it is strictly ahead of every other copy and
//! every copy's media info has been read.

use cv_domain::MediaFile;
use cv_media::resolution::{Resolution, resolution_bucket};

use crate::hero_audio::hero_audio;

/// What one copy is ranked by. Fields are in the order they are compared.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quality {
    pub resolution: Option<Resolution>,
    /// Dolby Vision 4, HDR10+ 3, HDR10 2, HLG 1, SDR 0.
    pub hdr: u8,
    pub audio_channels: u32,
    /// See [`audio_codec_rank`].
    pub audio_codec: u8,
    pub video_bitrate_kbps: u32,
}

/// The rank of the best HDR format among `formats`.
#[must_use]
pub fn hdr_rank(formats: &[String]) -> u8 {
    formats
        .iter()
        .map(|format| match format.trim().to_ascii_lowercase().as_str() {
            "dolby vision" => 4,
            "hdr10+" => 3,
            "hdr10" => 2,
            "hlg" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// The rank of an audio codec as the library names it: TrueHD Atmos and DTS:X, then TrueHD and
/// DTS-HD MA, E-AC-3 Atmos, DTS-HD HRA, E-AC-3, the other DTS kinds, AC3, AAC, and anything else.
#[must_use]
pub fn audio_codec_rank(codec: &str) -> u8 {
    let codec = codec.trim().to_ascii_uppercase();
    match codec.as_str() {
        "TRUEHD ATMOS" => 8,
        c if c.starts_with("DTS-X") => 8,
        "TRUEHD" | "DTS-HD MA" => 7,
        "E-AC-3 ATMOS" => 6,
        "DTS-HD HRA" => 5,
        "E-AC-3" => 4,
        c if c.starts_with("DTS") => 3,
        c if c.starts_with("AC3") => 2,
        "AAC" => 1,
        _ => 0,
    }
}

/// The quality of a copy from its main video, or `None` while its media info has not been read
/// (no frame size known).
#[must_use]
pub fn quality_of(video: &MediaFile, original_language: &str) -> Option<Quality> {
    if video.width == 0 || video.height == 0 {
        return None;
    }
    let audio = hero_audio(&video.audio_streams, original_language);
    Some(Quality {
        resolution: resolution_bucket(video.width, video.height),
        hdr: hdr_rank(&video.hdr_formats),
        audio_channels: audio.map_or(0, |a| a.channels),
        audio_codec: audio.map_or(0, |a| audio_codec_rank(&a.codec)),
        video_bitrate_kbps: video.video_bitrate_kbps,
    })
}

/// The copy to mark "Best quality", by position: the one strictly ahead of every other. `None`
/// on a tie at the top, when a copy's media info has not been read, or with fewer than two.
#[must_use]
pub fn best(copies: &[Option<Quality>]) -> Option<usize> {
    if copies.len() < 2 {
        return None;
    }
    let read: Vec<Quality> = copies.iter().copied().collect::<Option<_>>()?;
    let top = read.iter().max()?;
    let mut leaders = read.iter().enumerate().filter(|(_, q)| *q == top);
    let (index, _) = leaders.next()?;
    leaders.next().is_none().then_some(index)
}

#[cfg(test)]
mod tests {
    use cv_domain::{AudioStream, MediaFileType};

    use super::*;

    fn video(width: u32, height: u32, hdr: &[&str], audio: (&str, u32), kbps: u32) -> MediaFile {
        let mut file = MediaFile::new(MediaFileType::Video, "D:/Movies", "film.mkv");
        file.width = width;
        file.height = height;
        file.hdr_formats = hdr.iter().map(|s| (*s).to_owned()).collect();
        file.video_bitrate_kbps = kbps;
        file.audio_streams.push(AudioStream {
            codec: audio.0.into(),
            channels: audio.1,
            ..AudioStream::default()
        });
        file
    }

    fn marked(files: &[MediaFile]) -> Option<usize> {
        let qualities: Vec<Option<Quality>> = files.iter().map(|f| quality_of(f, "")).collect();
        best(&qualities)
    }

    #[test]
    fn resolution_decides_first() {
        let uhd_sdr = video(3840, 2160, &[], ("AAC", 2), 10_000);
        let hd_dv = video(1920, 1080, &["Dolby Vision"], ("TrueHD Atmos", 8), 30_000);
        assert_eq!(marked(&[hd_dv, uhd_sdr]), Some(1));
    }

    #[test]
    fn a_cropped_film_has_the_resolution_of_its_frame_width() {
        let cropped = video(1920, 800, &[], ("AAC", 2), 8_000);
        let full = video(1920, 1080, &[], ("AAC", 2), 20_000);
        // Same resolution label, so the bitrate decides.
        assert_eq!(marked(&[cropped, full]), Some(1));
    }

    #[test]
    fn hdr_decides_after_resolution() {
        let hdr10 = video(3840, 2160, &["HDR10"], ("TrueHD Atmos", 8), 60_000);
        let dv = video(3840, 2160, &["Dolby Vision", "HDR10"], ("AAC", 2), 15_000);
        assert_eq!(marked(&[hdr10, dv]), Some(1));
        assert_eq!(hdr_rank(&["HLG".into()]), 1);
        assert_eq!(hdr_rank(&["HDR10+".into(), "HDR10".into()]), 3);
        assert_eq!(hdr_rank(&[]), 0);
    }

    #[test]
    fn audio_channels_then_codec_decide_after_hdr() {
        let stereo = video(1920, 1080, &[], ("TrueHD Atmos", 2), 20_000);
        let surround = video(1920, 1080, &[], ("AAC", 6), 10_000);
        assert_eq!(marked(&[stereo, surround]), Some(1));

        let ac3 = video(1920, 1080, &[], ("AC3", 6), 20_000);
        let dts_hd = video(1920, 1080, &[], ("DTS-HD MA", 6), 10_000);
        assert_eq!(marked(&[ac3, dts_hd]), Some(1));
    }

    #[test]
    fn codecs_rank_in_the_stated_order() {
        let order = [
            "Opus",
            "AAC",
            "AC3",
            "DTS",
            "E-AC-3",
            "DTS-HD HRA",
            "E-AC-3 Atmos",
            "DTS-HD MA",
            "TrueHD Atmos",
        ];
        for pair in order.windows(2) {
            assert!(
                audio_codec_rank(pair[0]) < audio_codec_rank(pair[1]),
                "{} < {}",
                pair[0],
                pair[1]
            );
        }
        assert_eq!(audio_codec_rank("TrueHD"), audio_codec_rank("DTS-HD MA"));
        assert_eq!(audio_codec_rank("DTS-X"), audio_codec_rank("TrueHD Atmos"));
        assert_eq!(audio_codec_rank("AC3 EX"), audio_codec_rank("AC3"));
    }

    #[test]
    fn bitrate_decides_last() {
        let low = video(1920, 1080, &[], ("AAC", 6), 8_000);
        let high = video(1920, 1080, &[], ("AAC", 6), 20_000);
        assert_eq!(marked(&[low, high]), Some(1));
    }

    #[test]
    fn a_tie_marks_nothing() {
        let a = video(1920, 1080, &[], ("AAC", 6), 8_000);
        let b = video(1920, 1080, &[], ("AAC", 6), 8_000);
        let worse = video(1280, 720, &[], ("AAC", 6), 8_000);
        assert_eq!(marked(&[a, b, worse]), None);
    }

    #[test]
    fn an_unread_copy_marks_nothing() {
        let read = video(3840, 2160, &[], ("AAC", 6), 8_000);
        let unread = MediaFile::new(MediaFileType::Video, "D:/Movies", "new.mkv");
        assert_eq!(marked(&[read, unread]), None);
    }

    #[test]
    fn one_copy_is_never_marked() {
        assert_eq!(marked(&[video(1920, 1080, &[], ("AAC", 6), 8_000)]), None);
    }
}
