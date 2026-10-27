//! A parsed scraper definition, its settings and the URLs its functions return.

use std::collections::BTreeMap;

use super::xml::{self, Element};

/// Why a scraper could not be loaded or run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KodiError {
    /// The file is not a scraper definition.
    #[error("not a scraper definition: {0}")]
    InvalidScraper(String),
    /// The scraper does not define the function that was asked for.
    #[error("the scraper has no {0} function")]
    MissingFunction(String),
    /// A scraper function produced more than the interpreter allows.
    #[error("the scraper exceeded a limit: {0}")]
    Limit(String),
    /// A scraper returned a URL CineVault will not fetch.
    #[error("the scraper returned a URL that cannot be fetched: {0}")]
    BadUrl(String),
}

/// What kind of titles a scraper handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KodiContent {
    Movies,
    TvShows,
}

impl KodiContent {
    /// Reads a scraper's `content` attribute (`movies`, `tvshows`).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "movies" | "movie" => Some(Self::Movies),
            "tvshows" | "tvshow" => Some(Self::TvShows),
            _ => None,
        }
    }
}

/// A scraper definition: the `<scraper>` element of an XML scraper file.
#[derive(Debug, Clone)]
pub struct ScraperDef {
    pub(super) root: Element,
    name: String,
    content: Option<KodiContent>,
    language: String,
    framework: String,
}

impl ScraperDef {
    /// Reads a scraper definition. The root element must be `<scraper>`.
    pub fn parse(text: &str) -> Result<Self, KodiError> {
        let document = xml::parse(text);
        let root = document
            .elements("scraper")
            .next()
            .cloned()
            .ok_or_else(|| KodiError::InvalidScraper("no <scraper> element".to_owned()))?;
        Ok(Self {
            name: root.attr("name").unwrap_or_default().to_owned(),
            content: root.attr("content").and_then(KodiContent::parse),
            language: root.attr("language").unwrap_or_default().to_owned(),
            framework: root.attr("framework").unwrap_or_default().to_owned(),
            root,
        })
    }

    /// The name the scraper gives itself (may be empty).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The content type the scraper declares, when it declares a known one.
    pub fn content(&self) -> Option<KodiContent> {
        self.content
    }

    /// The language the scraper declares (may be empty).
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The scraper framework version the file declares (may be empty).
    pub fn framework(&self) -> &str {
        &self.framework
    }

    /// Whether the scraper defines the function (a child element named like the function).
    pub fn has_function(&self, name: &str) -> bool {
        self.root.first(name).is_some()
    }
}

/// Setting values a scraper reads through `$INFO[name]` and `conditional="name"`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KodiSettings(BTreeMap<String, String>);

impl KodiSettings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_owned(), value.to_owned());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }

    /// The defaults an add-on declares in its `resources/settings.xml`: both the flat format
    /// (`<setting id="x" default="y"/>`) and the sectioned one
    /// (`<setting id="x"><default>y</default></setting>`).
    pub fn from_settings_xml(text: &str) -> Self {
        fn walk(element: &Element, out: &mut KodiSettings) {
            for child in element.child_elements() {
                if child.name == "setting"
                    && let Some(id) = child.attr("id").filter(|id| !id.is_empty())
                {
                    let default = child
                        .attr("default")
                        .map(str::to_owned)
                        .or_else(|| child.first("default").map(|d| d.text().trim().to_owned()))
                        .unwrap_or_default();
                    out.set(id, &default);
                }
                walk(child, out);
            }
        }
        let mut settings = Self::new();
        walk(&xml::parse(text), &mut settings);
        settings
    }

    /// Settings with `overrides` laid over `self`.
    #[must_use]
    pub fn with_overrides(mut self, overrides: &KodiSettings) -> Self {
        for (key, value) in &overrides.0 {
            self.0.insert(key.clone(), value.clone());
        }
        self
    }
}

/// One URL a scraper function returned, with the options Kodi's `<url>` element carries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScraperUrl {
    pub url: String,
    /// Send the query part of the URL as a form body.
    pub post: bool,
    /// A `Referer` to send.
    pub spoof: Option<String>,
    /// The scraper function to run on the page instead of the default one.
    pub function: Option<String>,
}

impl ScraperUrl {
    pub(super) fn from_element(element: &Element) -> Option<Self> {
        let url = element.text().trim().to_owned();
        if url.is_empty() {
            return None;
        }
        let yes = |name: &str| {
            element
                .attr(name)
                .is_some_and(|v| v.eq_ignore_ascii_case("yes") || v == "1" || v == "true")
        };
        Some(Self {
            url,
            post: yes("post"),
            spoof: element
                .attr("spoof")
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            function: element
                .attr("function")
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        })
    }

    /// The `<url>` elements in a function's output. A bare `http(s)` text counts as one URL.
    pub fn parse_all(output: &str) -> Vec<Self> {
        let document = xml::parse(output);
        let mut urls: Vec<Self> = document
            .elements("url")
            .filter_map(Self::from_element)
            .collect();
        if urls.is_empty() {
            let bare = output.trim();
            if bare.starts_with("http://") || bare.starts_with("https://") {
                urls.push(Self {
                    url: bare.to_owned(),
                    ..Self::default()
                });
            }
        }
        urls
    }

    /// Only plain web addresses are fetched; a scraper cannot make CineVault read a local file
    /// or call another scheme.
    pub(super) fn check(&self) -> Result<(), KodiError> {
        let lower = self.url.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            Ok(())
        } else {
            Err(KodiError::BadUrl(self.url.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRAPER: &str = r#"<?xml version="1.0"?>
<scraper framework="1.1" name="Sample" content="movies" language="en">
  <CreateSearchUrl dest="3"/>
</scraper>"#;

    #[test]
    fn reads_scraper_attributes_and_functions() {
        let def = ScraperDef::parse(SCRAPER).unwrap();
        assert_eq!(def.name(), "Sample");
        assert_eq!(def.content(), Some(KodiContent::Movies));
        assert_eq!(def.language(), "en");
        assert!(def.has_function("CreateSearchUrl"));
        assert!(!def.has_function("GetDetails"));
    }

    #[test]
    fn a_file_without_a_scraper_element_is_rejected() {
        assert!(matches!(
            ScraperDef::parse("<addon id='x'/>"),
            Err(KodiError::InvalidScraper(_))
        ));
    }

    #[test]
    fn settings_defaults_in_both_formats() {
        let flat = KodiSettings::from_settings_xml(
            r#"<settings><setting id="a" type="bool" default="true"/><setting id="b" default="x"/></settings>"#,
        );
        assert_eq!(flat.get("a"), Some("true"));
        let sectioned = KodiSettings::from_settings_xml(
            r#"<settings version="1"><section id="s"><category id="c"><group id="g">
               <setting id="lang"><default>de</default></setting></group></category></section></settings>"#,
        );
        assert_eq!(sectioned.get("lang"), Some("de"));
        let mut user = KodiSettings::new();
        user.set("a", "false");
        assert_eq!(flat.with_overrides(&user).get("a"), Some("false"));
    }

    #[test]
    fn urls_with_options() {
        let urls = ScraperUrl::parse_all(
            r#"<url post="yes" spoof="https://x.example/">https://x.example/s?q=a</url><url function="GetMore">https://x.example/m</url>"#,
        );
        assert_eq!(urls.len(), 2);
        assert!(urls[0].post);
        assert_eq!(urls[0].spoof.as_deref(), Some("https://x.example/"));
        assert_eq!(urls[1].function.as_deref(), Some("GetMore"));
        assert_eq!(
            ScraperUrl::parse_all("https://x.example/plain")[0].url,
            "https://x.example/plain"
        );
        assert!(ScraperUrl::parse_all("<results/>").is_empty());
    }

    #[test]
    fn only_web_urls_are_allowed() {
        let local = ScraperUrl {
            url: "file:///C:/secret".into(),
            ..ScraperUrl::default()
        };
        assert!(local.check().is_err());
        let web = ScraperUrl {
            url: "HTTPS://x.example".into(),
            ..ScraperUrl::default()
        };
        assert!(web.check().is_ok());
    }
}
