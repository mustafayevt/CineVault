//! The first scan of a fresh library (spec `library-scanning` "The first scan of a
//! fresh library reads local files first"): the existing NFO and artwork files decide each
//! title's stage, without any network access and without changing a file in the media folders.

mod support;

use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::lifecycle::Stage;
use cv_app::local_library::LOCAL_IMPORT_JOB;
use cv_app::nfo::follow::owned_by_cinevault;
use cv_app::provider_credentials::ProviderCredentialsService;
use cv_app::secrets::SecretStore;
use cv_app::services::{AppServices, BootOptions};
use cv_db::repo::data_sources::insert_source;
use cv_db::repo::{ItemQuery, get_item, list_items};
use cv_domain::{Item, ItemId, ItemKind, MediaFileType, SourceKind};
use cv_providers::{CredentialResolver, MemoryUserCredentials, ProviderRegistry};
use sha2::{Digest, Sha256};
use support::{Entry, Scripted, arc};
use tempfile::TempDir;

/// The smallest valid PNG, standing in for a poster.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// A web server that only counts the connections made to it: every remote URL in the NFOs points
/// here, so any download the first scan tried would show.
struct FakeWeb {
    port: u16,
    hits: Arc<AtomicUsize>,
}

impl FakeWeb {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                counter.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut stream) = stream {
                    let mut buf = [0u8; 512];
                    let _ = stream.read(&mut buf);
                }
            }
        });
        Self { port, hits }
    }

    fn url(&self, name: &str) -> String {
        format!("http://127.0.0.1:{}/{name}", self.port)
    }
}

/// A movie NFO as an earlier CineVault wrote it (the header comment marks it as ours).
fn cinevault_nfo(title: &str, year: u16, tmdb: Option<&str>, web: &FakeWeb) -> String {
    let id = tmdb.map_or_else(String::new, |id| {
        format!(
            "  <uniqueid type=\"tmdb\" default=\"true\">{id}</uniqueid>\n  <tmdbid>{id}</tmdbid>\n"
        )
    });
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <!--created on 2026-09-01 - by CineVault 0.2.5 for Kodi-->\n\
         <movie>\n  <title>{title}</title>\n  <year>{year}</year>\n  \
         <plot>The plot of {title}.</plot>\n{id}  \
         <thumb aspect=\"poster\">{}</thumb>\n</movie>\n",
        web.url(&format!("{year}.jpg"))
    )
}

fn put(path: &Path, content: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Size, modification time and hash of every file below `root`.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime, [u8; 32])> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let meta = std::fs::metadata(&path).unwrap();
                let hash: [u8; 32] = Sha256::digest(std::fs::read(&path).unwrap()).into();
                out.insert(path, (meta.len(), meta.modified().unwrap(), hash));
            }
        }
    }
    out
}

fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn movies(services: &AppServices) -> Vec<Item> {
    services
        .db()
        .read(|c| {
            let page = list_items(c, &ItemQuery::new(ItemKind::Movie), None, 100)?;
            page.items
                .into_iter()
                .map(|row| get_item(c, row.id).map(Option::unwrap))
                .collect()
        })
        .unwrap()
}

fn by_folder<'a>(items: &'a [Item], folder: &str) -> &'a Item {
    items
        .iter()
        .find(|i| i.core.path.ends_with(folder))
        .unwrap_or_else(|| panic!("no title in {folder}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_first_scan_reads_local_files_only_and_sets_each_stage() {
    let temp = TempDir::new().unwrap();
    let web = FakeWeb::start();
    let media = temp.path().join("movies");
    // An NFO with a TMDB ID in a folder that matches the naming style.
    put(&media.join("Alien (1979)/Alien (1979).mkv"), "alien");
    put(
        &media.join("Alien (1979)/Alien (1979).nfo"),
        cinevault_nfo("Alien", 1979, Some("348"), &web),
    );
    put(&media.join("Alien (1979)/Alien (1979)-poster.png"), PNG);
    // An NFO with an ID in a folder that does not match.
    put(&media.join("heat.1995.1080p/heat.1995.1080p.mkv"), "heat");
    put(
        &media.join("heat.1995.1080p/heat.1995.1080p.nfo"),
        cinevault_nfo("Heat", 1995, Some("949"), &web),
    );
    // An NFO with a title and year but no ID: its title is used, the title stays New.
    put(&media.join("solyaris/solyaris.mkv"), "solaris");
    put(
        &media.join("solyaris/solyaris.nfo"),
        cinevault_nfo("Solaris", 1972, None, &web),
    );
    // A malformed NFO.
    put(&media.join("Broken (2001)/Broken (2001).mkv"), "broken");
    put(
        &media.join("Broken (2001)/Broken (2001).nfo"),
        "<movie><year>20",
    );
    // No NFO at all.
    put(&media.join("Plain (2010)/Plain (2010).mkv"), "plain");
    let before = snapshot(&media);

    // A provider that would answer any scrape, with a key: nothing may ask it.
    let tripwire = arc(Scripted::new(
        "tmdb",
        vec![
            Entry::new("348", "Alien", 1979),
            Entry::new("949", "Heat", 1995),
        ],
    ));
    let mut registry = ProviderRegistry::new();
    registry.register_metadata(Arc::clone(&tripwire) as Arc<dyn cv_providers::MetadataProvider>);
    let resolver = CredentialResolver::new(Arc::new(MemoryUserCredentials::default()));
    resolver.set_user("tmdb", "api_key", "cvtest-key").unwrap();
    let options = BootOptions::new(location(temp.path()), Arc::new(SecretStore::in_memory()))
        .with_providers(ProviderCredentialsService::with_resolver(
            resolver, registry,
        ));
    let services = AppServices::boot(options).expect("boot");
    let source = media.to_string_lossy().into_owned();
    services
        .db()
        .write(move |tx| insert_source(tx, SourceKind::Movie, &source))
        .unwrap();

    // The first start: the scan on start, then the NFO import and the name check.
    services.start();
    until("the five titles", || {
        (movies(&services).len() == 5).then_some(())
    })
    .await;
    until("the NFO import", || {
        services
            .jobs()
            .list()
            .iter()
            .find(|j| j.name == LOCAL_IMPORT_JOB)
            .filter(|j| j.state.is_terminal())
            .map(|_| ())
    })
    .await;
    let items = movies(&services);
    let ids: Vec<ItemId> = items.iter().filter_map(|i| i.id).collect();
    let stage_of = |folder: &str| {
        let id = by_folder(&items, folder).id.unwrap();
        services.lifecycle().read(&[id]).unwrap()[&id].stage
    };
    until("the name check of every title", || {
        (stage_of("Alien (1979)") == Stage::Renamed).then_some(())
    })
    .await;

    assert_eq!(stage_of("Alien (1979)"), Stage::Renamed);
    assert_eq!(stage_of("heat.1995.1080p"), Stage::Scraped);
    assert_eq!(stage_of("solyaris"), Stage::New);
    assert_eq!(stage_of("Broken (2001)"), Stage::New);
    assert_eq!(stage_of("Plain (2010)"), Stage::New);

    let items = movies(&services);
    let alien = by_folder(&items, "Alien (1979)");
    assert_eq!(alien.core.title, "Alien");
    assert_eq!(alien.core.plot, "The plot of Alien.");
    assert_eq!(alien.core.ids.get("tmdb"), Some("348"));
    // The poster from the folder shows: it is one of the title's files.
    assert!(
        alien
            .core
            .media_files
            .iter()
            .any(|f| f.kind == MediaFileType::Poster && f.filename == "Alien (1979)-poster.png"),
        "{:?}",
        alien.core.media_files
    );
    let solaris = by_folder(&items, "solyaris");
    assert_eq!(
        (solaris.core.title.as_str(), solaris.core.year),
        ("Solaris", 1972)
    );
    assert!(solaris.core.ids.is_empty());
    let broken = by_folder(&items, "Broken (2001)");
    assert_eq!(broken.core.title, "Broken");

    // NFOs an earlier CineVault wrote are CineVault's to keep current, so a later edit rewrites
    // them; a title without an ID is not.
    let owned = services
        .db()
        .read(move |c| owned_by_cinevault(c, &ids))
        .unwrap();
    let alien_id = alien.id.unwrap();
    let heat_id = by_folder(&items, "heat.1995.1080p").id.unwrap();
    assert!(owned.contains(&alien_id), "{owned:?}");
    assert!(owned.contains(&heat_id), "{owned:?}");
    assert!(!owned.contains(&solaris.id.unwrap()));

    // No network: no provider was asked and no remote image was fetched.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(Scripted::count(&tripwire.searches), 0);
    assert_eq!(Scripted::count(&tripwire.lookups), 0);
    assert_eq!(Scripted::count(&tripwire.details), 0);
    assert_eq!(
        web.hits.load(Ordering::SeqCst),
        0,
        "a remote URL was fetched"
    );

    // Nothing in the media folders changed.
    services.shutdown();
    assert_eq!(snapshot(&media), before);
}
