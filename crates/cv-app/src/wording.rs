//! Small helpers for the sentences the app writes in reports and messages.

/// A count with the noun that agrees with it: `1 title`, `3 titles`, `0 files`.
pub fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The names of background jobs, as Activity shows them. They use the three verbs of the app
/// (Scan, Scrape, Rename) and the plain words around them, never an internal step name:
/// `Scanning 4 folders`, `Scraping 12 titles`, `Downloading artwork for 8 titles`. (Renames and
/// moves are not background jobs: Organize and the move workspace show their own progress.)
pub mod job {
    use super::counted;

    pub use cv_artwork::thumb_job::job_name as preparing_thumbnails;
    pub use cv_scan::rescan::job::job_name as scanning_folders;

    /// A scrape of `titles` titles, with the provider it runs on when it is one provider's batch.
    #[must_use]
    pub fn scraping_titles(titles: usize, provider: Option<&str>) -> String {
        let base = format!("Scraping {}", counted(titles, "title", "titles"));
        match provider {
            Some(provider) => format!("{base} ({provider})"),
            None => base,
        }
    }

    /// Fetching the artwork the scrape of `titles` titles is missing: the after-scrape artwork
    /// job (`ScrapeFollowUp`).
    #[must_use]
    pub fn downloading_artwork(titles: usize) -> String {
        format!(
            "Downloading artwork for {}",
            counted(titles, "title", "titles")
        )
    }

    /// Reading the video and audio details of `files` files; `reload` for a forced second read.
    #[must_use]
    pub fn reading_media_info(files: usize, reload: bool) -> String {
        format!(
            "{} media info of {}",
            if reload { "Reloading" } else { "Reading" },
            counted(files, "file", "files")
        )
    }

    /// Reading the NFO files a scan found next to the videos.
    pub const READING_NFO_FILES: &str = "Reading NFO files";

    /// The background check of which titles already have the naming style's names. Activity
    /// shows it at the Technical level only.
    pub const CHECKING_NAMES: &str = "Checking names";

    /// The check of `titles` titles' names: `Checking names of 12 titles`.
    #[must_use]
    pub fn checking_names(titles: usize) -> String {
        format!("{CHECKING_NAMES} of {}", counted(titles, "title", "titles"))
    }
}

#[cfg(test)]
mod tests {
    use super::{counted, job};

    #[test]
    fn the_noun_agrees_with_the_count() {
        assert_eq!(counted(1, "title", "titles"), "1 title");
        assert_eq!(counted(0, "title", "titles"), "0 titles");
        assert_eq!(counted(29, "title", "titles"), "29 titles");
        assert_eq!(counted(2, "data source", "data sources"), "2 data sources");
    }

    /// Every job name is built from the three verbs and plain words: none of the old step names
    /// (Match, Identify, Enrich, Rescan, Rescrape, Build, Reload cache) or the internal word
    /// "batch" without the API it came from.
    #[test]
    fn job_names_use_the_three_verbs() {
        let names = [
            job::scanning_folders(4),
            job::scanning_folders(1),
            job::scraping_titles(12, None),
            job::scraping_titles(1, Some("TMDB")),
            job::downloading_artwork(8),
            job::reading_media_info(5, false),
            job::reading_media_info(5, true),
            job::preparing_thumbnails(40),
            job::READING_NFO_FILES.to_owned(),
            job::checking_names(12),
        ];
        let banned = [
            "Match", "Identify", "Enrich", "Rescan", "Rescrape", "Build", "Search",
        ];
        for name in &names {
            assert!(
                banned.iter().all(|word| !name.contains(word)),
                "{name} uses an old word"
            );
            assert!(
                name.split_whitespace()
                    .next()
                    .is_some_and(|word| word.ends_with("ing")),
                "{name} starts with a verb"
            );
        }
        assert_eq!(job::scanning_folders(4), "Scanning 4 folders");
        assert_eq!(job::scanning_folders(1), "Scanning 1 folder");
        assert_eq!(job::scraping_titles(12, None), "Scraping 12 titles");
        assert_eq!(
            job::scraping_titles(1, Some("TMDB")),
            "Scraping 1 title (TMDB)"
        );
        assert_eq!(
            job::downloading_artwork(8),
            "Downloading artwork for 8 titles"
        );
        assert_eq!(job::checking_names(12), "Checking names of 12 titles");
        assert_eq!(job::preparing_thumbnails(1), "Preparing 1 thumbnail");
    }
}
