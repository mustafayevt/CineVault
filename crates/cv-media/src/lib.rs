//! Technical media information: MediaInfo, FFmpeg, aspect ratio detection and checksums.
//!
//! * [`tools`]: locating and downloading the external tools these features need.
//! * [`aspect`]: aspect-ratio detection with FFmpeg (black-bar crop samples, rounding list).
//! * [`checksum`]: CRC-32 and perceptual-hash checksums of video files.
//! * [`disc`]: main title and streams of DVD, Blu-ray and HD-DVD folders and ISO images.
//! * [`framegrab`]: still frames grabbed from a video with FFmpeg as artwork candidates.
//! * [`external`]: language and flags of external audio and subtitle files, from their names.
//! * [`mediainfo`]: the dynamically loaded MediaInfo library and its parallel reader pool.
//! * [`resolution`]: resolution buckets (96p to 4320p) and the LD, SD, HD and UHD categories.
//! * [`runtime`]: runtime from the media files, summing stacked parts.

pub mod aspect;
pub mod checksum;
pub mod disc;
pub mod external;
pub mod framegrab;
pub mod mediainfo;
pub mod resolution;
pub mod runtime;
pub mod tools;
