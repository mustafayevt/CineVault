//! What a copy's rename label is made of, read from stored media info as the Compare copies page
//! reads it.

use cv_domain::{Item, ItemDetail};
use cv_media::resolution::{Resolution, resolution_bucket};
use cv_rename::conflict::copies::CopyAttributes;
use cv_template::tokens::audio_channels_dot;

use super::rank::hdr_rank;
use super::service::source_label;
use crate::hero_audio::hero_audio;
use crate::nfo::model::main_video;

/// The label facts of a movie, or `None` while its media info has not been read (no frame size
/// known), so a label is never made from missing data.
#[must_use]
pub fn label_attributes(item: &Item) -> Option<CopyAttributes> {
    let video = main_video(item)?;
    let bucket = resolution_bucket(video.width, video.height).filter(|_| video.height > 0)?;
    let resolution = if bucket >= Resolution::P720 {
        bucket.label()
    } else {
        "SD"
    };
    let hdr = match hdr_rank(&video.hdr_formats) {
        4 => "DV",
        3 => "HDR10+",
        2 => "HDR10",
        1 => "HLG",
        _ => "SDR",
    };
    let source = match &item.detail {
        ItemDetail::Movie(movie) => source_label(&movie.media_source),
        _ => String::new(),
    };
    let audio = hero_audio(&video.audio_streams, &item.core.original_language).map_or_else(
        String::new,
        |track| {
            if track.codec.to_ascii_lowercase().contains("atmos") {
                "Atmos".to_owned()
            } else if track.codec.eq_ignore_ascii_case("DTS-HD MA") {
                track.codec.clone()
            } else {
                let layout = audio_channels_dot(track.channels);
                format!("{} {layout}", track.codec).trim().to_owned()
            }
        },
    );
    Some(CopyAttributes {
        resolution: resolution.to_owned(),
        hdr: hdr.to_owned(),
        source,
        audio,
    })
}

#[cfg(test)]
mod tests {
    use cv_domain::{AudioStream, MediaFile, MediaFileType, MediaSource};

    use super::*;

    fn movie(width: u32, height: u32, hdr: &[&str], codec: &str, channels: u32) -> Item {
        let mut item = Item::movie("Example Movie");
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.media_source = MediaSource::WebDl;
        }
        let mut video = MediaFile::new(MediaFileType::Video, "D:/Movies/a", "a.mkv");
        video.width = width;
        video.height = height;
        video.hdr_formats = hdr.iter().map(|s| (*s).to_owned()).collect();
        video.audio_streams.push(AudioStream {
            codec: codec.into(),
            channels,
            ..AudioStream::default()
        });
        item.core.media_files.push(video);
        item
    }

    #[test]
    fn the_label_facts_use_short_names() {
        let uhd = label_attributes(&movie(
            3840,
            1600,
            &["Dolby Vision", "HDR10"],
            "TrueHD Atmos",
            8,
        ))
        .unwrap();
        assert_eq!(
            (
                uhd.resolution.as_str(),
                uhd.hdr.as_str(),
                uhd.source.as_str(),
                uhd.audio.as_str()
            ),
            ("2160p", "DV", "WEB-DL", "Atmos")
        );
        let hd = label_attributes(&movie(1920, 800, &[], "AAC", 6)).unwrap();
        assert_eq!((hd.resolution.as_str(), hd.hdr.as_str()), ("1080p", "SDR"));
        assert_eq!(hd.audio, "AAC 5.1");
        let dts = label_attributes(&movie(1280, 720, &["HDR10+"], "DTS-HD MA", 8)).unwrap();
        assert_eq!(
            (dts.hdr.as_str(), dts.audio.as_str()),
            ("HDR10+", "DTS-HD MA")
        );
        assert_eq!(
            label_attributes(&movie(720, 576, &[], "AC3", 2))
                .unwrap()
                .resolution,
            "SD"
        );
    }

    #[test]
    fn unread_media_info_gives_no_label() {
        assert_eq!(label_attributes(&movie(0, 0, &[], "", 0)), None);
        assert_eq!(label_attributes(&Item::movie("No video")), None);
    }
}
