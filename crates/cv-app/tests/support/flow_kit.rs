//! Shared pieces of the end-to-end flow tests (`upgrade_flow.rs`, `redesign_flow.rs`): a bin in a
//! folder, a web server that only counts connections, snapshots of folders and a few waiting
//! helpers. Everything lives in temporary folders and every file is a few bytes.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use cv_app::data_location::{DataLocation, LocationInputs};
use cv_app::library_ops::{BinnedItem, SendOutcome, SystemBin, SystemTrash};
use cv_app::services::AppServices;
use cv_db::repo::{ItemQuery, get_item, list_items};
use cv_domain::{Item, ItemId, ItemKind};
use sha2::{Digest, Sha256};

/// The smallest valid PNG, standing in for a poster.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// A web server that only counts the connections made to it: every remote URL in the NFOs points
/// here, so any download a flow tried would show.
pub struct FakeWeb {
    port: u16,
    hits: Arc<AtomicUsize>,
}

impl FakeWeb {
    pub fn start() -> Self {
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

    pub fn url(&self, name: &str) -> String {
        format!("http://127.0.0.1:{}/{name}", self.port)
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// A movie NFO as an earlier CineVault wrote it (the header comment marks it as ours).
pub fn cinevault_nfo(title: &str, year: u16, tmdb: Option<&str>, web: &FakeWeb) -> String {
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

pub fn put(path: &Path, content: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// What a file is, for comparing folders: size, modification time and hash.
pub type Stamp = (u64, SystemTime, [u8; 32]);

/// Size, modification time and hash of every file below `root` (empty when it is not there).
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, Stamp> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
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

/// The files below `root` as `relative path -> content hash` (what a move or a rename may not
/// change, whatever happens to the modification times).
pub fn contents(root: &Path) -> BTreeMap<String, [u8; 32]> {
    snapshot(root)
        .into_iter()
        .map(|(path, (_, _, hash))| {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, hash)
        })
        .collect()
}

/// The files and folders below `root` as sorted `/`-separated relative paths (folders end in
/// `/`); empty when `root` is not there.
pub fn tree(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let mut rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                rel.push('/');
                stack.push(path);
            }
            out.push(rel);
        }
    }
    out.sort();
    out
}

pub fn hash_of(path: &Path) -> [u8; 32] {
    Sha256::digest(std::fs::read(path).unwrap()).into()
}

pub fn location(root: &Path) -> DataLocation {
    let inputs = LocationInputs {
        exe_dir: root.join("app"),
        user_data_base: root.join("userdata"),
        user_config_base: root.join("userconfig"),
    };
    std::fs::create_dir_all(&inputs.exe_dir).unwrap();
    DataLocation::resolve(&inputs)
}

pub async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Every movie of the library, in full.
pub fn movies(services: &AppServices) -> Vec<Item> {
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

pub fn item(services: &AppServices, id: ItemId) -> Item {
    services
        .db()
        .read(move |c| get_item(c, id))
        .unwrap()
        .unwrap()
}

/// The title whose folder ends in `folder`.
pub fn by_folder<'a>(items: &'a [Item], folder: &str) -> &'a Item {
    items
        .iter()
        .find(|i| i.core.path.ends_with(folder))
        .unwrap_or_else(|| panic!("no title in {folder}"))
}

/// A bin in a folder: items are renamed into it and can be put back. Stands in for the Recycle
/// Bin / Trash, which these tests never touch.
pub struct FakeBin {
    dir: PathBuf,
    sends: AtomicUsize,
    next: AtomicUsize,
    fail: AtomicBool,
    log: Mutex<Vec<String>>,
}

impl FakeBin {
    pub fn new(parent: &Path) -> Arc<Self> {
        let dir = parent.join("fake-bin");
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Self {
            dir,
            sends: AtomicUsize::new(0),
            next: AtomicUsize::new(1),
            fail: AtomicBool::new(false),
            log: Mutex::new(Vec::new()),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// How many times something was sent to the bin.
    pub fn sends(&self) -> usize {
        self.sends.load(Ordering::SeqCst)
    }

    /// The names of what is in the bin, without the numbering, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .unwrap()
            .map(|e| {
                let name = e.unwrap().file_name().to_string_lossy().into_owned();
                name.split_once('-')
                    .map_or(name.clone(), |(_, rest)| rest.to_owned())
            })
            .collect();
        names.sort();
        names
    }

    pub fn is_empty(&self) -> bool {
        self.names().is_empty()
    }

    /// Makes every later send fail, as a full bin does.
    pub fn fail_sends(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }

    /// The paths sent so far, in order.
    pub fn sent_paths(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = path.symlink_metadata() else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| size_of(&entry.path()))
                .sum()
        })
        .unwrap_or(0)
}

impl SystemTrash for FakeBin {
    fn supports(&self, _path: &Path) -> bool {
        true
    }

    fn send(&self, paths: &[PathBuf]) -> Result<(), String> {
        self.send_tracked(paths).error.map_or(Ok(()), Err)
    }
}

impl SystemBin for FakeBin {
    fn send_tracked(&self, paths: &[PathBuf]) -> SendOutcome {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return SendOutcome {
                sent: Vec::new(),
                error: Some("the bin is full".to_owned()),
            };
        }
        let mut sent = Vec::new();
        for path in paths {
            let size = size_of(path);
            let n = self.next.fetch_add(1, Ordering::SeqCst);
            let name = path.file_name().unwrap().to_string_lossy();
            let target = self.dir.join(format!("{n}-{name}"));
            if let Err(error) = std::fs::rename(path, &target) {
                return SendOutcome {
                    sent,
                    error: Some(error.to_string()),
                };
            }
            self.log
                .lock()
                .unwrap()
                .push(path.to_string_lossy().into_owned());
            sent.push(BinnedItem {
                original: path.to_string_lossy().into_owned(),
                bin_id: target.to_string_lossy().into_owned(),
                size,
            });
        }
        SendOutcome { sent, error: None }
    }

    fn present(&self, items: &[BinnedItem]) -> Vec<bool> {
        items
            .iter()
            .map(|i| Path::new(&i.bin_id).symlink_metadata().is_ok())
            .collect()
    }

    fn restore(&self, items: &[BinnedItem]) -> Result<(), Vec<(String, String)>> {
        let mut problems = Vec::new();
        for item in items {
            if Path::new(&item.original).symlink_metadata().is_ok() {
                problems.push((
                    item.original.clone(),
                    "Something else is at this path now; nothing was replaced".to_owned(),
                ));
            } else if Path::new(&item.bin_id).symlink_metadata().is_err() {
                problems.push((
                    item.original.clone(),
                    "It is no longer in the Recycle Bin or Trash".to_owned(),
                ));
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        for item in items {
            let to = Path::new(&item.original);
            if let Some(parent) = to.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(error) = std::fs::rename(&item.bin_id, to) {
                problems.push((item.original.clone(), error.to_string()));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}
