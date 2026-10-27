//! Looking at downloaded bytes: is it an image we can store, and how wide is it.
//!
//! A server that answers `200` with an HTML error page, a login wall or a truncated body must never
//! end up on disk as `poster.jpg`, so every download passes [`inspect`] first. The format found in
//! the bytes (not the URL, not the `Content-Type`) decides the file extension.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageFormat, ImageReader};

/// Why bytes cannot be stored as an image.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    /// The bytes are not a picture at all (an error page, an empty body).
    #[error("the answer is not an image")]
    NotAnImage,
    /// A picture in a format media centres do not read as artwork.
    #[error("{0} images are not supported")]
    Unsupported(String),
    /// The header is fine but the picture cannot be read.
    #[error("the image is damaged: {0}")]
    Damaged(String),
    /// Re-encoding a resized picture failed.
    #[error("the resized image could not be written: {0}")]
    Encode(String),
}

/// What [`inspect`] learned about a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    /// The extension of the format: `jpg`, `png` or `webp`.
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
}

/// The format and size of `bytes`. Only the header is read.
///
/// # Errors
/// The bytes are not a JPEG, PNG or WebP picture, or the picture has no size.
pub fn inspect(bytes: &[u8]) -> Result<ImageInfo, ImageError> {
    let format = image::guess_format(bytes).map_err(|_| ImageError::NotAnImage)?;
    let ext = match format {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        other => return Err(ImageError::Unsupported(format!("{other:?}"))),
    };
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|e| ImageError::Damaged(e.to_string()))?;
    if width == 0 || height == 0 {
        return Err(ImageError::Damaged("it has no size".to_owned()));
    }
    Ok(ImageInfo { ext, width, height })
}

/// A picture ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub bytes: Vec<u8>,
    pub info: ImageInfo,
}

/// The JPEG quality of a resized picture.
pub const RESIZE_QUALITY: u8 = 85;

/// `bytes` scaled down to at most `max_width` pixels wide, keeping the proportions, as a JPEG. A
/// picture that is not wider than that is returned as it is: nothing is ever scaled up, and a
/// small picture is not re-encoded.
///
/// # Errors
/// The bytes cannot be read as a picture, or the result cannot be encoded.
pub fn shrink_to_width(bytes: &[u8], max_width: u32) -> Result<Picture, ImageError> {
    let info = inspect(bytes)?;
    if max_width == 0 || info.width <= max_width {
        return Ok(Picture {
            bytes: bytes.to_vec(),
            info,
        });
    }
    if !crate::thumb::fits_decode_limits(info.width, info.height) {
        return Err(ImageError::Damaged(format!(
            "it is too large to resize ({} × {})",
            info.width, info.height
        )));
    }
    let format = image::guess_format(bytes).map_err(|_| ImageError::NotAnImage)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(crate::thumb::decode_limits());
    let decoded = reader
        .decode()
        .map_err(|e| ImageError::Damaged(e.to_string()))?;
    let height =
        u32::try_from((u64::from(info.height) * u64::from(max_width)) / u64::from(info.width))
            .unwrap_or(u32::MAX)
            .max(1);
    let resized = decoded
        .resize_exact(max_width, height, FilterType::Lanczos3)
        .to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, RESIZE_QUALITY)
        .encode(
            resized.as_raw(),
            resized.width(),
            resized.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| ImageError::Encode(e.to_string()))?;
    Ok(Picture {
        info: ImageInfo {
            ext: "jpg",
            width: resized.width(),
            height: resized.height(),
        },
        bytes: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    pub(crate) fn picture_bytes(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let image = RgbImage::from_pixel(width, height, Rgb([200, 30, 30]));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).expect("encode");
        out.into_inner()
    }

    #[test]
    fn the_format_in_the_bytes_decides_the_extension() {
        let jpeg = inspect(&picture_bytes(ImageFormat::Jpeg, 40, 60)).unwrap();
        assert_eq!((jpeg.ext, jpeg.width, jpeg.height), ("jpg", 40, 60));
        assert_eq!(
            inspect(&picture_bytes(ImageFormat::Png, 8, 8)).unwrap().ext,
            "png"
        );
    }

    #[test]
    fn pages_empty_bodies_and_other_formats_are_refused() {
        assert_eq!(
            inspect(b"<html><body>Not found</body></html>"),
            Err(ImageError::NotAnImage)
        );
        assert_eq!(inspect(&[]), Err(ImageError::NotAnImage));
        assert!(matches!(
            inspect(&picture_bytes(ImageFormat::Bmp, 4, 4)),
            Err(ImageError::Unsupported(_))
        ));
    }

    #[test]
    fn a_wide_picture_shrinks_in_proportion_and_a_small_one_is_left_alone() {
        let wide = picture_bytes(ImageFormat::Png, 1000, 500);
        let shrunk = shrink_to_width(&wide, 250).unwrap();
        assert_eq!(
            (shrunk.info.ext, shrunk.info.width, shrunk.info.height),
            ("jpg", 250, 125)
        );
        assert_eq!(inspect(&shrunk.bytes).unwrap().width, 250);

        let small = picture_bytes(ImageFormat::Jpeg, 200, 100);
        let same = shrink_to_width(&small, 250).unwrap();
        assert_eq!(same.bytes, small, "nothing is scaled up or re-encoded");
        assert_eq!(same.info.width, 200);
    }
}
