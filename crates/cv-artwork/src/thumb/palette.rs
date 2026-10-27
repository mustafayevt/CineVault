//! Vibrant/muted palette extraction.
//!
//! The image is reduced to a 4-bit-per-channel histogram. Every populated bin becomes a
//! candidate swatch, and six swatch roles (vibrant, muted, each in light and dark) pick the
//! candidate that best matches a target saturation and lightness, weighted by how much of
//! the image the bin covers. The method is a small, deterministic scoring scheme written for
//! this project.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB colour, serialised as `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// Lower-case `#rrggbb`.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }

    /// Parse `#rrggbb` (the leading `#` is optional).
    pub fn from_hex(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#').unwrap_or(text);
        if digits.len() != 6 || !digits.is_ascii() {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        Some(Self([channel(0)?, channel(2)?, channel(4)?]))
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HexVisitor;
        impl Visitor<'_> for HexVisitor {
            type Value = Rgb;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a #rrggbb colour")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Rgb, E> {
                Rgb::from_hex(v).ok_or_else(|| E::custom("invalid colour"))
            }
        }
        deserializer.deserialize_str(HexVisitor)
    }
}

/// The colours extracted from one image.
///
/// Swatch roles that the image has no suitable colour for are `None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    /// The most common colour of the image.
    pub dominant: Rgb,
    /// The colour used to tint a poster's shadow and page glow: vibrant, else muted, else dominant.
    pub ambient: Rgb,
    pub vibrant: Option<Rgb>,
    pub light_vibrant: Option<Rgb>,
    pub dark_vibrant: Option<Rgb>,
    pub muted: Option<Rgb>,
    pub light_muted: Option<Rgb>,
    pub dark_muted: Option<Rgb>,
}

/// Role target: wanted saturation and lightness, and the range a candidate must fall in.
struct Target {
    saturation: f32,
    sat_min: f32,
    sat_max: f32,
    lightness: f32,
    light_min: f32,
    light_max: f32,
}

const VIBRANT: Target = Target {
    saturation: 1.0,
    sat_min: 0.35,
    sat_max: 1.0,
    lightness: 0.5,
    light_min: 0.3,
    light_max: 0.7,
};
const LIGHT_VIBRANT: Target = Target {
    saturation: 1.0,
    sat_min: 0.35,
    sat_max: 1.0,
    lightness: 0.74,
    light_min: 0.55,
    light_max: 1.0,
};
const DARK_VIBRANT: Target = Target {
    saturation: 1.0,
    sat_min: 0.35,
    sat_max: 1.0,
    lightness: 0.26,
    light_min: 0.0,
    light_max: 0.45,
};
const MUTED: Target = Target {
    saturation: 0.3,
    sat_min: 0.0,
    sat_max: 0.4,
    lightness: 0.5,
    light_min: 0.3,
    light_max: 0.7,
};
const LIGHT_MUTED: Target = Target {
    saturation: 0.3,
    sat_min: 0.0,
    sat_max: 0.4,
    lightness: 0.74,
    light_min: 0.55,
    light_max: 1.0,
};
const DARK_MUTED: Target = Target {
    saturation: 0.3,
    sat_min: 0.0,
    sat_max: 0.4,
    lightness: 0.26,
    light_min: 0.0,
    light_max: 0.45,
};

/// Colours this close to white or black carry no hue information for swatches.
const EXTREME_LIGHTNESS: f32 = 0.05;
const WEIGHT_SATURATION: f32 = 0.24;
const WEIGHT_LIGHTNESS: f32 = 0.52;
const WEIGHT_POPULATION: f32 = 0.24;

#[derive(Clone, Copy, Default)]
struct Bin {
    count: u32,
    sum: [u64; 3],
}

#[derive(Clone, Copy)]
struct Candidate {
    rgb: Rgb,
    saturation: f32,
    lightness: f32,
    count: u32,
}

fn to_hsl(rgb: [u8; 3]) -> (f32, f32) {
    let r = f32::from(rgb[0]) / 255.0;
    let g = f32::from(rgb[1]) / 255.0;
    let b = f32::from(rgb[2]) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    let saturation = if delta <= f32::EPSILON {
        0.0
    } else {
        delta / (1.0 - (2.0 * lightness - 1.0).abs())
    };
    (saturation.clamp(0.0, 1.0), lightness)
}

fn bin_index(pixel: &[u8]) -> usize {
    (usize::from(pixel[0] >> 4) << 8)
        | (usize::from(pixel[1] >> 4) << 4)
        | usize::from(pixel[2] >> 4)
}

fn mean_rgb(bin: &Bin) -> Rgb {
    let count = u64::from(bin.count.max(1));
    Rgb([
        ((bin.sum[0] + count / 2) / count) as u8,
        ((bin.sum[1] + count / 2) / count) as u8,
        ((bin.sum[2] + count / 2) / count) as u8,
    ])
}

fn pick(target: &Target, candidates: &[Candidate], max_count: u32, taken: &[Rgb]) -> Option<Rgb> {
    let mut best: Option<(f32, Rgb)> = None;
    for c in candidates {
        let in_range = c.saturation >= target.sat_min
            && c.saturation <= target.sat_max
            && c.lightness >= target.light_min
            && c.lightness <= target.light_max;
        if !in_range || taken.contains(&c.rgb) {
            continue;
        }
        let score = WEIGHT_SATURATION * (1.0 - (c.saturation - target.saturation).abs())
            + WEIGHT_LIGHTNESS * (1.0 - (c.lightness - target.lightness).abs())
            + WEIGHT_POPULATION * (c.count as f32 / max_count as f32);
        // Ties keep the first candidate, which is the lowest bin index: deterministic.
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, c.rgb));
        }
    }
    best.map(|(_, rgb)| rgb)
}

/// Extract a palette from tightly packed RGBA pixels.
///
/// Transparent pixels (alpha below 128) are ignored. Returns `None` when no opaque pixel exists.
/// Callers should pass a small image (around 128 px on the long side); cost is linear in pixels.
pub fn extract_palette(rgba: &[u8]) -> Option<Palette> {
    let mut bins = vec![Bin::default(); 4096];
    let mut any = false;
    for pixel in rgba.as_chunks::<4>().0 {
        if pixel[3] < 128 {
            continue;
        }
        any = true;
        let bin = &mut bins[bin_index(pixel)];
        bin.count += 1;
        for (sum, channel) in bin.sum.iter_mut().zip(&pixel[..3]) {
            *sum += u64::from(*channel);
        }
    }
    if !any {
        return None;
    }

    let dominant_bin = bins
        .iter()
        .enumerate()
        // `max_by_key` keeps the last maximum; reverse the index so the lowest bin wins ties.
        .max_by_key(|(i, b)| (b.count, std::cmp::Reverse(*i)))
        .map(|(_, b)| *b)?;
    let dominant = mean_rgb(&dominant_bin);

    let candidates: Vec<Candidate> = bins
        .iter()
        .filter(|b| b.count > 0)
        .map(|b| {
            let rgb = mean_rgb(b);
            let (saturation, lightness) = to_hsl(rgb.0);
            Candidate {
                rgb,
                saturation,
                lightness,
                count: b.count,
            }
        })
        .filter(|c| c.lightness > EXTREME_LIGHTNESS && c.lightness < 1.0 - EXTREME_LIGHTNESS)
        .collect();
    let max_count = candidates.iter().map(|c| c.count).max().unwrap_or(1);

    let mut taken: Vec<Rgb> = Vec::new();
    let mut role = |target: &Target| {
        let chosen = pick(target, &candidates, max_count, &taken);
        if let Some(rgb) = chosen {
            taken.push(rgb);
        }
        chosen
    };
    let vibrant = role(&VIBRANT);
    let light_vibrant = role(&LIGHT_VIBRANT);
    let dark_vibrant = role(&DARK_VIBRANT);
    let muted = role(&MUTED);
    let light_muted = role(&LIGHT_MUTED);
    let dark_muted = role(&DARK_MUTED);

    let ambient = vibrant.or(muted).unwrap_or(dominant);
    Some(Palette {
        dominant,
        ambient,
        vibrant,
        light_vibrant,
        dark_vibrant,
        muted,
        light_muted,
        dark_muted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(rgb: [u8; 3], pixels: usize) -> Vec<u8> {
        (0..pixels)
            .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
            .collect()
    }

    #[test]
    fn hex_round_trip() {
        let c = Rgb([1, 171, 255]);
        assert_eq!(c.hex(), "#01abff");
        assert_eq!(Rgb::from_hex("#01abff"), Some(c));
        assert_eq!(Rgb::from_hex("01ABFF"), Some(c));
        assert_eq!(Rgb::from_hex("#01ab"), None);
        assert_eq!(Rgb::from_hex("#zzzzzz"), None);
    }

    #[test]
    fn saturated_red_is_vibrant_and_dominant() {
        let palette = extract_palette(&solid([220, 20, 30], 400)).unwrap();
        assert_eq!(palette.dominant, Rgb([220, 20, 30]));
        assert_eq!(palette.vibrant, Some(Rgb([220, 20, 30])));
        assert_eq!(palette.ambient, Rgb([220, 20, 30]));
        assert_eq!(palette.muted, None);
    }

    #[test]
    fn grey_is_muted_not_vibrant() {
        let palette = extract_palette(&solid([120, 125, 130], 400)).unwrap();
        assert_eq!(palette.vibrant, None);
        assert_eq!(palette.muted, Some(Rgb([120, 125, 130])));
        assert_eq!(palette.ambient, Rgb([120, 125, 130]));
    }

    #[test]
    fn mixed_image_separates_roles() {
        // Mostly dark navy, a smaller bright orange area and a mid grey strip.
        let mut px = solid([15, 25, 70], 600);
        px.extend(solid([240, 140, 20], 250));
        px.extend(solid([128, 128, 128], 150));
        let palette = extract_palette(&px).unwrap();
        assert_eq!(palette.dominant, Rgb([15, 25, 70]));
        assert_eq!(palette.vibrant, Some(Rgb([240, 140, 20])));
        assert_eq!(palette.dark_vibrant, Some(Rgb([15, 25, 70])));
        assert_eq!(palette.muted, Some(Rgb([128, 128, 128])));
        assert_eq!(palette.ambient, Rgb([240, 140, 20]));
    }

    #[test]
    fn black_image_has_dominant_but_no_swatches() {
        let palette = extract_palette(&solid([0, 0, 0], 100)).unwrap();
        assert_eq!(palette.dominant, Rgb([0, 0, 0]));
        assert_eq!(palette.vibrant, None);
        assert_eq!(palette.ambient, Rgb([0, 0, 0]));
    }

    #[test]
    fn transparent_pixels_are_ignored() {
        let mut px = vec![0u8; 4 * 100];
        px.extend(solid([20, 200, 60], 10));
        let palette = extract_palette(&px).unwrap();
        assert_eq!(palette.dominant, Rgb([20, 200, 60]));
        assert!(extract_palette(&[0u8; 40]).is_none());
    }

    #[test]
    fn serde_uses_hex_strings() {
        let palette = extract_palette(&solid([220, 20, 30], 16)).unwrap();
        let json = serde_json::to_string(&palette).unwrap();
        assert!(json.contains("\"#dc141e\""), "{json}");
        let back: Palette = serde_json::from_str(&json).unwrap();
        assert_eq!(back, palette);
    }
}
