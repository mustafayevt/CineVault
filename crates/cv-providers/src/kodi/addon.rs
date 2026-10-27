//! Finding Kodi metadata add-ons on disk.
//!
//! An add-on is a folder with an `addon.xml`. Its `<extension point="xbmc.metadata.scraper.movies">`
//! (or `...tvshows`) elements name the scraper file in `library`. Only XML scraper files can be
//! run; an add-on whose library is Python (or missing) is listed as unsupported with the reason,
//! so the user sees why it is not offered.

use std::path::{Component, Path, PathBuf};

use super::scraper::{KodiContent, KodiError, KodiSettings, ScraperDef};
use super::xml::{self, Element};

/// The add-on that only reads local files; it is not a scraper to offer.
const LOCAL_ADDON_ID: &str = "metadata.local";
/// Folders are searched this many levels below a root.
const MAX_DEPTH: usize = 2;
/// The largest scraper or settings file that is read.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

const POINT_MOVIES: &str = "xbmc.metadata.scraper.movies";
const POINT_TVSHOWS: &str = "xbmc.metadata.scraper.tvshows";

/// Whether a scraper file can be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    /// An XML scraper, run by the interpreter.
    Xml,
    /// Not runnable, with the reason to show the user.
    Unsupported(String),
}

/// One scraper an add-on declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScraperEntry {
    pub content: KodiContent,
    /// The `library` attribute: the scraper file, relative to the add-on folder.
    pub library: String,
    /// The language the extension declares (may be empty).
    pub language: String,
    pub support: Support,
}

/// A metadata add-on found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KodiAddon {
    /// The add-on ID, for example `metadata.example.movies`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub provider_name: String,
    pub dir: PathBuf,
    pub scrapers: Vec<ScraperEntry>,
}

impl KodiAddon {
    /// The scrapers CineVault can run.
    pub fn supported(&self) -> impl Iterator<Item = &ScraperEntry> {
        self.scrapers.iter().filter(|s| s.support == Support::Xml)
    }

    /// The reasons for the scrapers it cannot run.
    pub fn unsupported_reasons(&self) -> Vec<&str> {
        self.scrapers
            .iter()
            .filter_map(|s| match &s.support {
                Support::Unsupported(reason) => Some(reason.as_str()),
                Support::Xml => None,
            })
            .collect()
    }

    /// Reads the scraper file of `entry` and the add-on's setting defaults.
    pub fn load(&self, entry: &ScraperEntry) -> Result<(ScraperDef, KodiSettings), KodiError> {
        if let Support::Unsupported(reason) = &entry.support {
            return Err(KodiError::InvalidScraper(reason.clone()));
        }
        let path = safe_join(&self.dir, &entry.library).ok_or_else(|| {
            KodiError::InvalidScraper(format!("bad library path {}", entry.library))
        })?;
        let text = read_limited(&path)
            .ok_or_else(|| KodiError::InvalidScraper(format!("cannot read {}", path.display())))?;
        let def = ScraperDef::parse(&text)?;
        let settings = read_limited(&self.dir.join("resources").join("settings.xml"))
            .map(|t| KodiSettings::from_settings_xml(&t))
            .unwrap_or_default();
        Ok((def, settings))
    }
}

/// Joins a relative path onto `base`, refusing absolute paths and `..`.
fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        .then(|| base.join(relative))
}

fn read_limited(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// The folders Kodi add-ons live in, in search order: CineVault's own `kodi_scraper` folder
/// under `app_dir`, the Kodi user-data add-ons folder, then the Kodi install's add-ons folder.
/// Folders that do not exist are skipped by [`discover`].
pub fn default_roots(app_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![app_dir.join("kodi_scraper")];
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if cfg!(target_os = "windows") {
        if let Some(appdata) = env("APPDATA") {
            roots.push(appdata.join("Kodi").join("addons"));
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = env(var) {
                roots.push(dir.join("Kodi").join("addons"));
            }
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = env("HOME") {
            roots.push(home.join("Library/Application Support/Kodi/addons"));
        }
        roots.push(PathBuf::from(
            "/Applications/Kodi.app/Contents/Resources/Kodi/addons",
        ));
    } else {
        if let Some(home) = env("HOME") {
            roots.push(home.join(".kodi/addons"));
        }
        roots.push(PathBuf::from("/usr/share/kodi/addons"));
    }
    roots
}

/// Finds the metadata add-ons under `roots`. A root may itself be an add-on folder or hold
/// add-on folders up to two levels down. An add-on found in an earlier root hides one with the
/// same ID in a later root. `metadata.local` and add-ons without a movie or TV scraper are left
/// out. The result is sorted by ID.
pub fn discover(roots: &[PathBuf]) -> Vec<KodiAddon> {
    let mut found: Vec<KodiAddon> = Vec::new();
    for root in roots {
        let mut folders = Vec::new();
        collect_folders(root, 0, &mut folders);
        for folder in folders {
            let Some(addon) = read_addon(&folder) else {
                continue;
            };
            if addon.id == LOCAL_ADDON_ID || found.iter().any(|a| a.id == addon.id) {
                continue;
            }
            found.push(addon);
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn collect_folders(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if dir.join("addon.xml").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    children.sort();
    for child in children {
        collect_folders(&child, depth + 1, out);
    }
}

/// Reads one add-on folder. `None` when `addon.xml` is unreadable or declares no movie or TV
/// metadata scraper.
fn read_addon(dir: &Path) -> Option<KodiAddon> {
    let text = read_limited(&dir.join("addon.xml"))?;
    let document = xml::parse(&text);
    let root = document.first("addon")?;
    let id = root.attr("id")?.trim().to_owned();
    if id.is_empty() {
        return None;
    }
    let requires_python = root.elements("requires").any(|requires| {
        requires.elements("import").any(|import| {
            import
                .attr("addon")
                .is_some_and(|a| a == "xbmc.python" || a.starts_with("script.module."))
        })
    });
    let scrapers: Vec<ScraperEntry> = root
        .elements("extension")
        .filter_map(|extension| scraper_entry(dir, extension, requires_python))
        .collect();
    if scrapers.is_empty() {
        return None;
    }
    Some(KodiAddon {
        name: root.attr("name").unwrap_or(&id).to_owned(),
        version: root.attr("version").unwrap_or_default().to_owned(),
        provider_name: root.attr("provider-name").unwrap_or_default().to_owned(),
        id,
        dir: dir.to_path_buf(),
        scrapers,
    })
}

fn scraper_entry(dir: &Path, extension: &Element, requires_python: bool) -> Option<ScraperEntry> {
    let content = match extension.attr("point")? {
        POINT_MOVIES => KodiContent::Movies,
        POINT_TVSHOWS => KodiContent::TvShows,
        _ => return None,
    };
    let library = extension
        .attr("library")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let lower = library.to_ascii_lowercase();
    let support = if library.is_empty() {
        Support::Unsupported("the add-on names no scraper file".to_owned())
    } else if lower.ends_with(".py") || (!lower.ends_with(".xml") && requires_python) {
        Support::Unsupported(
            "this add-on is written in Python; only XML scrapers are supported".to_owned(),
        )
    } else if !lower.ends_with(".xml") {
        Support::Unsupported(format!("{library} is not an XML scraper file"))
    } else {
        match safe_join(dir, &library) {
            None => {
                Support::Unsupported("the scraper file path is not inside the add-on".to_owned())
            }
            Some(path) if !path.is_file() => {
                Support::Unsupported(format!("the scraper file {library} is missing"))
            }
            Some(_) => Support::Xml,
        }
    };
    Some(ScraperEntry {
        content,
        library,
        language: extension.attr("language").unwrap_or_default().to_owned(),
        support,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn addon_xml(id: &str, extension: &str) -> String {
        format!(
            r#"<addon id="{id}" name="{id} name" version="1.2.3" provider-name="Someone">{extension}</addon>"#
        )
    }

    #[test]
    fn finds_xml_and_python_add_ons_and_skips_the_local_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join("metadata.a/addon.xml"),
            &addon_xml(
                "metadata.a",
                r#"<extension point="xbmc.metadata.scraper.movies" library="a.xml" language="en"/>"#,
            ),
        );
        write(
            &root.join("metadata.a/a.xml"),
            "<scraper name='A' content='movies'/>",
        );
        write(
            &root.join("metadata.py/addon.xml"),
            &addon_xml(
                "metadata.py",
                r#"<requires><import addon="xbmc.python" version="3.0.0"/></requires>
                   <extension point="xbmc.metadata.scraper.tvshows" library="python/main.py"/>"#,
            ),
        );
        write(
            &root.join("metadata.local/addon.xml"),
            &addon_xml(
                "metadata.local",
                r#"<extension point="xbmc.metadata.scraper.movies" library="l.xml"/>"#,
            ),
        );
        write(
            &root.join("script.other/addon.xml"),
            &addon_xml(
                "script.other",
                r#"<extension point="xbmc.python.script" library="x.py"/>"#,
            ),
        );

        let found = discover(&[root.to_path_buf()]);
        let ids: Vec<_> = found.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["metadata.a", "metadata.py"]);

        let a = &found[0];
        assert_eq!(a.name, "metadata.a name");
        assert_eq!(a.version, "1.2.3");
        assert_eq!(a.supported().count(), 1);
        assert_eq!(a.scrapers[0].content, KodiContent::Movies);
        assert_eq!(a.scrapers[0].language, "en");
        let (def, _) = a.load(&a.scrapers[0]).unwrap();
        assert_eq!(def.name(), "A");

        let py = &found[1];
        assert_eq!(py.supported().count(), 0);
        assert_eq!(py.unsupported_reasons().len(), 1);
        assert!(py.unsupported_reasons()[0].contains("Python"));
        assert!(py.load(&py.scrapers[0]).is_err());
    }

    #[test]
    fn a_missing_or_escaping_library_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("metadata.m/addon.xml"),
            &addon_xml(
                "metadata.m",
                r#"<extension point="xbmc.metadata.scraper.movies" library="gone.xml"/>
                   <extension point="xbmc.metadata.scraper.tvshows" library="../outside.xml"/>"#,
            ),
        );
        let found = discover(&[dir.path().to_path_buf()]);
        assert_eq!(found[0].supported().count(), 0);
        assert!(found[0].unsupported_reasons()[0].contains("missing"));
        assert!(found[0].unsupported_reasons()[1].contains("not inside"));
    }

    #[test]
    fn the_first_root_wins_and_missing_roots_are_fine() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for (root, version) in [(&first, "1.0.0"), (&second, "2.0.0")] {
            write(
                &root.path().join("metadata.dup/addon.xml"),
                &format!(
                    r#"<addon id="metadata.dup" version="{version}"><extension point="xbmc.metadata.scraper.movies" library="d.xml"/></addon>"#
                ),
            );
            write(&root.path().join("metadata.dup/d.xml"), "<scraper/>");
        }
        let found = discover(&[
            PathBuf::from("does/not/exist"),
            first.path().to_path_buf(),
            second.path().to_path_buf(),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version, "1.0.0");
    }

    #[test]
    fn a_root_can_be_an_add_on_folder() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("addon.xml"),
            &addon_xml(
                "metadata.direct",
                r#"<extension point="xbmc.metadata.scraper.movies" library="s.xml"/>"#,
            ),
        );
        write(&dir.path().join("s.xml"), "<scraper/>");
        assert_eq!(discover(&[dir.path().to_path_buf()]).len(), 1);
    }

    #[test]
    fn default_roots_start_with_the_project_folder() {
        let roots = default_roots(Path::new("app"));
        assert_eq!(roots[0], Path::new("app").join("kodi_scraper"));
        assert!(roots.len() >= 2);
    }
}
