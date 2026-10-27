use super::*;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Sample {
    theme: String,
    grid_size: u32,
    renamer_template: String,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            grid_size: 3,
            renamer_template: "{title} ({year})".into(),
        }
    }
}

fn store(dir: &Path) -> SettingsStore {
    SettingsStore::new(dir.join("settings.json"))
}

fn corrupt_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("settings.json.corrupt-")
        })
        .collect();
    v.sort();
    v
}

#[test]
fn missing_file_gives_defaults_without_notice_or_writes() {
    let dir = tempfile::tempdir().unwrap();
    let loaded: Loaded<Sample> = store(dir.path()).load();
    assert_eq!(loaded.settings, Sample::default());
    assert!(loaded.notice.is_none());
    assert!(!dir.path().join("settings.json").exists());
}

#[test]
fn save_then_load_round_trips_with_version_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    let value = Sample {
        theme: "dark".into(),
        grid_size: 5,
        renamer_template: "{title}".into(),
    };
    s.save(&value).unwrap();
    let text = fs::read_to_string(s.path()).unwrap();
    assert!(text.contains("\"version\": 2"), "{text}");
    assert!(text.contains('\n'), "file must be human-readable");
    let loaded: Loaded<Sample> = s.load();
    assert_eq!(loaded.settings, value);
    assert!(loaded.notice.is_none());
}

#[test]
fn save_creates_missing_folders_and_leaves_no_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let s = SettingsStore::new(dir.path().join("a").join("b").join("settings.json"));
    s.save(&Sample::default()).unwrap();
    s.save(&Sample::default()).unwrap();
    let names: Vec<String> = fs::read_dir(dir.path().join("a").join("b"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["settings.json".to_string()]);
}

#[test]
fn save_replaces_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    s.save(&Sample::default()).unwrap();
    let changed = Sample {
        grid_size: 9,
        ..Sample::default()
    };
    s.save(&changed).unwrap();
    assert_eq!(s.load::<Sample>().settings, changed);
}

#[test]
fn corrupt_file_starts_with_defaults_and_preserves_corrupt_copy() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    let broken = b"{ \"version\": 2, \"settings\": { \"theme\": \"dar";
    fs::write(s.path(), broken).unwrap();

    let loaded: Loaded<Sample> = s.load();
    assert_eq!(loaded.settings, Sample::default());

    let kept = corrupt_files(dir.path());
    assert_eq!(kept.len(), 1, "exactly one quarantined file");
    assert_eq!(fs::read(&kept[0]).unwrap(), broken, "bytes preserved");
    assert!(
        !s.path().exists(),
        "broken file is moved, not left in place"
    );

    let Some(SettingsNotice::Quarantined {
        original,
        quarantined,
        reason,
    }) = loaded.notice
    else {
        panic!("a notice is required");
    };
    assert_eq!(original, s.path());
    assert_eq!(quarantined.as_deref(), Some(kept[0].as_path()));
    assert!(matches!(reason, QuarantineReason::Unparseable(_)));

    // The suffix is a UTC timestamp.
    let name = kept[0].file_name().unwrap().to_string_lossy().into_owned();
    let stamp = name.strip_prefix("settings.json.corrupt-").unwrap();
    assert_eq!(stamp.len(), 16, "{stamp}");
    assert!(stamp.ends_with('Z') && stamp.as_bytes()[8] == b'T');
}

#[test]
fn quarantine_notice_message_names_the_kept_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    fs::write(s.path(), "not json").unwrap();
    let notice = s.load::<Sample>().notice.unwrap();
    let msg = notice.message();
    assert!(msg.contains("defaults"), "{msg}");
    assert!(msg.contains(".corrupt-"), "{msg}");
}

#[test]
fn saving_after_quarantine_does_not_touch_the_corrupt_copy() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    fs::write(s.path(), "garbage").unwrap();
    let loaded: Loaded<Sample> = s.load();
    s.save(&loaded.settings).unwrap();
    let kept = corrupt_files(dir.path());
    assert_eq!(kept.len(), 1);
    assert_eq!(fs::read(&kept[0]).unwrap(), b"garbage");
    assert!(s.load::<Sample>().notice.is_none());
}

#[test]
fn repeated_corruption_in_the_same_second_keeps_every_copy() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    for i in 0..3 {
        fs::write(s.path(), format!("bad {i}")).unwrap();
        assert!(s.load::<Sample>().notice.is_some());
    }
    let kept = corrupt_files(dir.path());
    assert_eq!(kept.len(), 3);
    let mut contents: Vec<String> = kept
        .iter()
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    contents.sort();
    assert_eq!(contents, vec!["bad 0", "bad 1", "bad 2"]);
}

#[test]
fn wrong_envelope_or_content_shape_is_quarantined() {
    let cases = [
        ("[]", "array top level"),
        ("{}", "no version"),
        ("{\"version\":2}", "no settings"),
        ("{\"version\":0,\"settings\":{}}", "version zero"),
        (
            "{\"version\":2,\"settings\":{\"grid_size\":\"big\"}}",
            "wrong field type",
        ),
    ];
    for (text, label) in cases {
        let dir = tempfile::tempdir().unwrap();
        let s = store(dir.path());
        fs::write(s.path(), text).unwrap();
        let loaded: Loaded<Sample> = s.load();
        assert_eq!(loaded.settings, Sample::default(), "{label}");
        assert!(loaded.notice.is_some(), "{label}");
        assert_eq!(corrupt_files(dir.path()).len(), 1, "{label}");
    }
}

#[test]
fn newer_version_is_quarantined_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    let future = "{\"version\":7,\"settings\":{\"theme\":\"x\"}}";
    fs::write(s.path(), future).unwrap();
    let loaded: Loaded<Sample> = s.load();
    assert_eq!(loaded.settings, Sample::default());
    assert!(matches!(
        loaded.notice,
        Some(SettingsNotice::Quarantined {
            reason: QuarantineReason::NewerVersion {
                found: 7,
                supported: 2
            },
            ..
        })
    ));
    assert_eq!(
        fs::read_to_string(&corrupt_files(dir.path())[0]).unwrap(),
        future
    );
}

fn v1_to_v2(mut v: Value) -> Result<Value, String> {
    let obj = v.as_object_mut().ok_or("not an object")?;
    if let Some(old) = obj.remove("template") {
        obj.insert("renamer_template".into(), old);
    }
    Ok(v)
}

fn v2_to_v3(mut v: Value) -> Result<Value, String> {
    let obj = v.as_object_mut().ok_or("not an object")?;
    let size = obj.get("grid_size").and_then(Value::as_u64).unwrap_or(3);
    obj.insert("grid_size".into(), Value::from(size + 1));
    Ok(v)
}

#[test]
fn older_versions_are_migrated_in_order_and_written_back_at_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path()).with_migrations(vec![v1_to_v2, v2_to_v3]);
    assert_eq!(s.current_version(), 4);
    fs::write(
        s.path(),
        "{\"version\":2,\"settings\":{\"template\":\"{title}\",\"grid_size\":4}}",
    )
    .unwrap();
    let loaded: Loaded<Sample> = s.load();
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.settings.renamer_template, "{title}");
    assert_eq!(loaded.settings.grid_size, 5);
    assert!(corrupt_files(dir.path()).is_empty());

    s.save(&loaded.settings).unwrap();
    assert!(
        fs::read_to_string(s.path())
            .unwrap()
            .contains("\"version\": 4")
    );
}

#[test]
fn failed_migration_is_quarantined() {
    fn boom(_: Value) -> Result<Value, String> {
        Err("cannot upgrade".into())
    }
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path()).with_migrations(vec![boom]);
    let original = "{\"version\":2,\"settings\":{}}";
    fs::write(s.path(), original).unwrap();
    let loaded: Loaded<Sample> = s.load();
    assert_eq!(loaded.settings, Sample::default());
    assert!(matches!(
        loaded.notice,
        Some(SettingsNotice::Quarantined {
            reason: QuarantineReason::MigrationFailed {
                from_version: 2,
                ..
            },
            ..
        })
    ));
    assert_eq!(
        fs::read_to_string(&corrupt_files(dir.path())[0]).unwrap(),
        original
    );
}

#[test]
fn stale_temp_files_from_a_crash_are_cleaned_and_never_read() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    s.save(&Sample::default()).unwrap();
    let stale = dir.path().join("settings.json.tmp-1-0");
    fs::write(&stale, "half written").unwrap();
    let loaded: Loaded<Sample> = s.load();
    assert!(loaded.notice.is_none());
    assert!(!stale.exists());
}

#[test]
fn failed_save_keeps_previous_file_and_removes_temp() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    let good = Sample {
        grid_size: 8,
        ..Sample::default()
    };
    s.save(&good).unwrap();
    // Make the rename target a directory so the final replace fails.
    let blocked = SettingsStore::new(dir.path().join("blocked"));
    fs::create_dir(blocked.path()).unwrap();
    assert!(blocked.save(&good).is_err());
    assert!(
        !fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".tmp-"))
    );
    assert_eq!(s.load::<Sample>().settings, good);
}

#[test]
fn utc_stamp_formats_known_instants() {
    assert_eq!(utc_stamp(UNIX_EPOCH), "19700101T000000Z");
    // 2024-02-29 12:34:56 UTC
    let t = UNIX_EPOCH + Duration::from_secs(1_709_210_096);
    assert_eq!(utc_stamp(t), "20240229T123456Z");
    // 2000-03-01 00:00:00 UTC
    let t = UNIX_EPOCH + Duration::from_secs(951_868_800);
    assert_eq!(utc_stamp(t), "20000301T000000Z");
}

#[test]
fn an_unreadable_file_gives_defaults_and_is_set_aside_before_the_first_save() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    // A directory where the file should be cannot be read as a file.
    let path = dir.path().join("settings.json");
    fs::create_dir(&path).unwrap();
    let loaded: Loaded<Sample> = s.load();
    assert_eq!(loaded.settings, Sample::default());
    assert!(matches!(
        loaded.notice,
        Some(SettingsNotice::Unreadable { ref original, .. }) if *original == path
    ));
    assert!(s.has_unread_file());
    assert!(path.is_dir(), "loading leaves the file alone");

    s.save(&Sample::default()).unwrap();
    assert!(!s.has_unread_file());
    let aside: Vec<PathBuf> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains("settings.json.unreadable-"))
        .collect();
    assert_eq!(aside.len(), 1);
    assert!(aside[0].is_dir());
    assert!(path.is_file());
}

#[cfg(windows)]
#[test]
fn a_file_held_by_another_program_for_a_moment_is_read_after_a_retry() {
    use std::os::windows::fs::OpenOptionsExt;

    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    let value = Sample {
        theme: "dark".into(),
        ..Sample::default()
    };
    s.save(&value).unwrap();
    // No sharing at all: reads fail with a sharing violation while this handle is open.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.path().join("settings.json"))
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        drop(held);
    });
    let loaded: Loaded<Sample> = s.load();
    release.join().unwrap();
    assert_eq!(loaded.settings, value);
    assert!(loaded.notice.is_none());
    assert!(!s.has_unread_file());
}

#[test]
fn a_file_of_an_earlier_version_is_set_aside_unread_and_defaults_are_used() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path());
    let old = "{\"version\":1,\"settings\":{\"theme\":\"light\",\"grid_size\":9}}";
    fs::write(s.path(), old).unwrap();
    assert_eq!(s.earlier_version().unwrap(), Some(1));

    let loaded: Loaded<Sample> = s.load();

    assert_eq!(loaded.settings, Sample::default());
    assert!(loaded.notice.is_none());
    assert!(corrupt_files(dir.path()).is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("settings-old.json")).unwrap(),
        old
    );
    assert!(!s.path().exists());
    assert_eq!(s.earlier_version().unwrap(), None);
    s.save(&loaded.settings).unwrap();
    assert!(
        fs::read_to_string(s.path())
            .unwrap()
            .contains("\"version\": 2")
    );
    assert_eq!(s.set_aside_earlier().unwrap(), None);
}
