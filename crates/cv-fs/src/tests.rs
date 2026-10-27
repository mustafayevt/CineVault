use super::*;

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn an_atomic_write_replaces_the_file_and_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Heat.nfo");
    write_atomic(&target, b"first").unwrap();
    write_atomic(&target, b"second").unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"second");
    assert_eq!(names(dir.path()), ["Heat.nfo"]);
}

#[test]
fn temporary_names_are_unique_and_never_a_media_extension() {
    let target = Path::new("/m/Heat (1995)/poster.jpg");
    let (a, b) = (temp_path(target), temp_path(target));
    assert_ne!(a, b);
    assert_eq!(a.parent(), target.parent(), "beside the target");
    assert_eq!(a.extension().unwrap(), TEMP_EXTENSION);
    assert!(
        a.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("poster.jpg.")
    );
}

#[test]
fn a_leftover_from_a_crash_does_not_block_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("poster.jpg");
    // What older versions left behind after a crash, and what a crash of this one leaves.
    fs::create_dir(dir.path().join("poster.jpg.cvpart")).unwrap();
    fs::write(temp_path(&target), b"half").unwrap();
    write_atomic(&target, b"whole").unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"whole");
}

#[test]
fn a_write_that_cannot_be_put_in_place_keeps_the_target_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    // A folder is in the way of the name: the rename can never succeed.
    let target = dir.path().join("theme.mp3");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("inside"), b"keep").unwrap();
    assert!(write_atomic(&target, b"new").is_err());
    assert!(target.is_dir());
    assert_eq!(fs::read(target.join("inside")).unwrap(), b"keep");
    assert_eq!(names(dir.path()), ["theme.mp3"], "no temporary file stays");
}

#[test]
fn a_copy_to_a_temporary_file_is_complete_and_the_source_stays() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("dropped.mkv");
    fs::write(&source, vec![7u8; 100_000]).unwrap();
    let target = dir.path().join("Heat-trailer.mkv");
    let temp = copy_temp(&target, &source).unwrap();
    persist(&temp, &target).unwrap();
    assert_eq!(fs::read(&target).unwrap(), vec![7u8; 100_000]);
    assert!(source.is_file());
    assert!(copy_temp(&target, &dir.path().join("missing.mkv")).is_err());
    assert_eq!(names(dir.path()), ["Heat-trailer.mkv", "dropped.mkv"]);
}

#[test]
fn a_move_creates_the_folder_and_keeps_the_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("a.srt");
    fs::write(&from, b"subs").unwrap();
    let to = dir.path().join("trash").join("deep").join("a.srt");
    move_file(&from, &to).unwrap();
    assert!(!from.exists());
    assert_eq!(fs::read(&to).unwrap(), b"subs");
    assert!(move_file(&dir.path().join("missing"), &dir.path().join("x")).is_err());
}

#[test]
fn only_lock_errors_count_as_transient() {
    let lock = if cfg!(windows) { 32 } else { 16 };
    assert!(is_transient(&io::Error::from_raw_os_error(lock)));
    assert!(!is_transient(&io::Error::from(io::ErrorKind::NotFound)));
    assert!(!is_transient(&io::Error::other("disk full")));
    let mut calls = 0;
    let result: io::Result<()> = retry("test", Path::new("x"), || {
        calls += 1;
        Err(io::Error::from(io::ErrorKind::NotFound))
    });
    assert!(result.is_err());
    assert_eq!(calls, 1, "a lasting error is not retried");
}

/// Opens `path` the way a virus scanner or indexer does for a moment: no sharing at all, so the
/// file can be neither replaced, renamed nor deleted until the handle closes.
#[cfg(windows)]
fn hold(path: &Path) -> File {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn a_file_held_for_a_moment_is_still_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Heat.nfo");
    fs::write(&target, b"old").unwrap();
    let held = hold(&target);
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        drop(held);
    });
    write_atomic(&target, b"new").unwrap();
    release.join().unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(names(dir.path()), ["Heat.nfo"]);
}

#[cfg(windows)]
#[test]
fn a_file_held_for_long_fails_soon_and_stays_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Heat.nfo");
    fs::write(&target, b"old").unwrap();
    let held = hold(&target);
    let started = std::time::Instant::now();
    let error = write_atomic(&target, b"new").unwrap_err();
    assert!(is_transient(&error), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the retries are bounded"
    );
    drop(held);
    assert_eq!(fs::read(&target).unwrap(), b"old");
    assert_eq!(names(dir.path()), ["Heat.nfo"], "no temporary file stays");
}

#[cfg(windows)]
#[test]
fn a_file_held_for_a_moment_is_still_moved() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("poster.jpg");
    fs::write(&from, b"pixels").unwrap();
    let held = hold(&from);
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        drop(held);
    });
    let to = dir.path().join("trash").join("poster.jpg");
    move_file(&from, &to).unwrap();
    release.join().unwrap();
    assert!(!from.exists());
    assert_eq!(fs::read(&to).unwrap(), b"pixels");
}

#[test]
fn a_new_file_is_put_in_place_and_the_temporary_name_goes() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Heat.eng.srt");
    let temp = write_temp(&target, b"subtitles").unwrap();
    persist_new(&temp, &target).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"subtitles");
    assert_eq!(
        names(dir.path()),
        ["Heat.eng.srt"],
        "no temporary file stays"
    );
}

#[test]
fn persist_new_never_replaces_a_file_that_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Heat.eng.srt");
    fs::write(&target, b"the user's file").unwrap();
    let temp = write_temp(&target, b"new").unwrap();
    let error = persist_new(&temp, &target).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&target).unwrap(), b"the user's file");
    assert!(temp.exists(), "the caller removes its own temporary file");
}

#[test]
fn persist_new_into_a_missing_folder_fails_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("gone").join("a.srt");
    let temp = write_temp(&dir.path().join("a.srt"), b"x").unwrap();
    assert!(persist_new(&temp, &target).is_err());
    assert!(!target.exists());
    assert!(temp.exists());
}

/// A Blu-ray folder whose stream file a player has open (with every kind of sharing, as a
/// reader over the network does) cannot be renamed until the file is closed; a folder rename
/// waits a few seconds for that, where a file operation gives up after about one.
#[cfg(windows)]
#[test]
fn a_folder_held_by_an_open_file_below_it_is_renamed_once_the_file_is_closed() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let movie = dir.path().join("Bridge of Spies (2015) [AC3 2.0]");
    let stream = movie.join("BDMV").join("STREAM").join("00800.m2ts");
    fs::create_dir_all(stream.parent().unwrap()).unwrap();
    fs::write(&stream, b"stream").unwrap();
    let renamed = dir.path().join("Bridge of Spies (2015) [DTS-HD MA 7.1]");
    // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: sharing does not help a folder.
    let open = || {
        OpenOptions::new()
            .read(true)
            .share_mode(0x7)
            .open(&stream)
            .unwrap()
    };

    let held = open();
    let error = fs::rename(&movie, &renamed).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(5), "access denied: {error}");
    assert!(is_transient(&error));
    // The short retry of a file operation is not enough for a hold of two seconds ...
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2_000));
        drop(held);
    });
    assert!(retry("rename", &movie, || fs::rename(&movie, &renamed)).is_err());
    // ... the folder retry is.
    retry_folder("rename", &movie, || fs::rename(&movie, &renamed)).unwrap();
    release.join().unwrap();
    assert!(
        renamed
            .join("BDMV")
            .join("STREAM")
            .join("00800.m2ts")
            .is_file()
    );

    // A hold that never ends still fails, after about four seconds.
    let back = dir.path().join("back");
    let stream = renamed.join("BDMV").join("STREAM").join("00800.m2ts");
    let _held = OpenOptions::new()
        .read(true)
        .share_mode(0x7)
        .open(&stream)
        .unwrap();
    let started = std::time::Instant::now();
    assert!(retry_folder("rename", &renamed, || fs::rename(&renamed, &back)).is_err());
    assert!(started.elapsed() >= Duration::from_secs(3));
}
