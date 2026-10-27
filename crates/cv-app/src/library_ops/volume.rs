//! Whether two paths are on the same volume, as a hint for the preview ("copied, then removed"
//! instead of "renamed"). The executor does not rely on it: it tries the rename and falls back to
//! a verified copy when the system says the paths are on different volumes.

use std::path::Path;

/// A guess that two paths share a volume. Paths that do not exist yet are judged by the nearest
/// folder that does.
pub(crate) fn same_volume(a: &Path, b: &Path) -> bool {
    match (volume_key(a), volume_key(b)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

#[cfg(windows)]
fn volume_key(path: &Path) -> Option<String> {
    use std::path::Component;
    match path.components().next()? {
        Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().to_lowercase()),
        _ => None,
    }
}

#[cfg(unix)]
fn volume_key(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let mut probe = path.to_path_buf();
    loop {
        if let Ok(meta) = std::fs::metadata(&probe) {
            return Some(meta.dev().to_string());
        }
        if !probe.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_on_the_same_volume_as_its_neighbour() {
        let dir = tempfile::tempdir().unwrap();
        assert!(same_volume(&dir.path().join("a"), &dir.path().join("b/c")));
    }
}
