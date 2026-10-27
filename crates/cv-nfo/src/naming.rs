//! NFO file names for movies.
//!
//! The setting lists one or more naming choices; every choice that applies gives a file and all
//! files get the same content:
//!
//! * `FILENAME_NFO`: `<video base>.nfo`. For a disc structure it is `<main disc file>.nfo` when
//!   "NFO inside disc folder" is on and `movie.nfo` otherwise.
//! * `MOVIE_NFO`: `movie.nfo`.
//!
//! A folder that holds several movies always uses `<video base>.nfo`, whatever the setting says.

/// A movie NFO naming choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MovieNfoNaming {
    /// `<video base>.nfo`.
    #[default]
    VideoBase,
    /// `movie.nfo`.
    MovieNfo,
}

impl MovieNfoNaming {
    /// Parses the setting name (`FILENAME_NFO` or `MOVIE_NFO`), ignoring case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_uppercase().as_str() {
            "FILENAME_NFO" => Some(Self::VideoBase),
            "MOVIE_NFO" => Some(Self::MovieNfo),
            _ => None,
        }
    }
}

/// What the NFO names depend on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovieNameContext {
    /// The video file name without extension and without a stacking marker.
    pub video_base_name: String,
    /// A disc structure.
    pub is_disc: bool,
    /// For a disc: the main disc file without extension (`VIDEO_TS`, `index`).
    pub disc_base_name: String,
    /// Several movies share the folder.
    pub multi_movie_dir: bool,
    /// The "NFO inside disc folder" option.
    pub nfo_in_disc_folder: bool,
}

/// The NFO file names of a movie, without duplicates, in the order of the choices.
pub fn movie_nfo_file_names(namings: &[MovieNfoNaming], context: &MovieNameContext) -> Vec<String> {
    let video_nfo = format!("{}.nfo", context.video_base_name);
    if context.multi_movie_dir {
        return vec![video_nfo];
    }
    let mut names: Vec<String> = Vec::new();
    for naming in namings {
        let name = match naming {
            MovieNfoNaming::VideoBase if context.is_disc && context.nfo_in_disc_folder => {
                format!("{}.nfo", context.disc_base_name)
            }
            MovieNfoNaming::VideoBase if context.is_disc => "movie.nfo".to_owned(),
            MovieNfoNaming::VideoBase => video_nfo.clone(),
            MovieNfoNaming::MovieNfo => "movie.nfo".to_owned(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::MovieNfoNaming::{MovieNfo, VideoBase};
    use super::*;

    fn context() -> MovieNameContext {
        MovieNameContext {
            video_base_name: "Heat (1995)".to_owned(),
            ..MovieNameContext::default()
        }
    }

    #[test]
    fn default_is_the_video_base() {
        assert_eq!(
            movie_nfo_file_names(&[VideoBase], &context()),
            ["Heat (1995).nfo"]
        );
    }

    #[test]
    fn several_choices_give_several_files() {
        assert_eq!(
            movie_nfo_file_names(&[VideoBase, MovieNfo], &context()),
            ["Heat (1995).nfo", "movie.nfo"]
        );
        assert_eq!(
            movie_nfo_file_names(&[MovieNfo, MovieNfo], &context()),
            ["movie.nfo"]
        );
    }

    #[test]
    fn discs_use_the_main_disc_file_or_movie_nfo() {
        let mut c = context();
        c.is_disc = true;
        c.disc_base_name = "VIDEO_TS".to_owned();
        assert_eq!(movie_nfo_file_names(&[VideoBase], &c), ["movie.nfo"]);
        c.nfo_in_disc_folder = true;
        assert_eq!(movie_nfo_file_names(&[VideoBase], &c), ["VIDEO_TS.nfo"]);
        assert_eq!(movie_nfo_file_names(&[MovieNfo], &c), ["movie.nfo"]);
    }

    #[test]
    fn a_shared_folder_always_uses_the_video_base() {
        let mut c = context();
        c.multi_movie_dir = true;
        assert_eq!(movie_nfo_file_names(&[MovieNfo], &c), ["Heat (1995).nfo"]);
    }

    #[test]
    fn setting_names_parse() {
        assert_eq!(MovieNfoNaming::parse("filename_nfo"), Some(VideoBase));
        assert_eq!(MovieNfoNaming::parse("MOVIE_NFO"), Some(MovieNfo));
        assert_eq!(MovieNfoNaming::parse("x"), None);
    }
}
