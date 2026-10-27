//! Guard cases for data sources: nesting, system folders and path syntax. All checks
//! are text-only, so both path syntaxes run on every operating system.

use super::*;

fn windows() -> SourceGuard {
    SourceGuard::new(PathStyle::Windows, SystemFolders::new(PathStyle::Windows))
}

fn posix() -> SourceGuard {
    SourceGuard::new(PathStyle::Posix, SystemFolders::new(PathStyle::Posix))
}

fn src(id: i64, path: &str) -> ExistingSource {
    ExistingSource {
        id,
        path: path.to_owned(),
    }
}

fn code(result: Result<String, SourceError>) -> &'static str {
    result.expect_err("should be refused").code()
}

// ---------------------------------------------------------------------------------------------
// normalisation
// ---------------------------------------------------------------------------------------------

#[test]
fn windows_paths_are_cleaned() {
    let w = PathStyle::Windows;
    assert_eq!(w.normalize("d:/Movies/").unwrap(), "D:\\Movies");
    assert_eq!(
        w.normalize("  D:\\Movies\\\\Action\\.  ").unwrap(),
        "D:\\Movies\\Action"
    );
    assert_eq!(
        w.normalize("D:\\Movies\\Old\\..\\New").unwrap(),
        "D:\\Movies\\New"
    );
    assert_eq!(w.normalize("D:\\").unwrap(), "D:\\");
    assert_eq!(w.normalize("\\\\nas\\Movies\\").unwrap(), "\\\\nas\\Movies");
    assert_eq!(
        w.normalize("//nas/Movies/Action").unwrap(),
        "\\\\nas\\Movies\\Action"
    );
    assert_eq!(w.normalize("\\\\?\\D:\\Movies").unwrap(), "D:\\Movies");
    assert_eq!(
        w.normalize("\\\\?\\UNC\\nas\\Movies").unwrap(),
        "\\\\nas\\Movies"
    );
    // Windows ignores trailing dots and spaces, so they must not hide a folder.
    assert_eq!(w.normalize("D:\\Movies. \\").unwrap(), "D:\\Movies");
}

#[test]
fn posix_paths_are_cleaned() {
    let p = PathStyle::Posix;
    assert_eq!(p.normalize("/Volumes/Movies/").unwrap(), "/Volumes/Movies");
    assert_eq!(
        p.normalize("/Volumes//Movies/./Action").unwrap(),
        "/Volumes/Movies/Action"
    );
    assert_eq!(p.normalize("/Volumes/Movies/../TV").unwrap(), "/Volumes/TV");
    assert_eq!(p.normalize("/").unwrap(), "/");
}

#[test]
fn relative_empty_and_odd_paths_are_refused() {
    let w = PathStyle::Windows;
    assert_eq!(w.normalize("   ").unwrap_err(), SourceError::Empty);
    for bad in [
        "Movies",
        "..\\Movies",
        "D:Movies",
        "\\Movies",
        "\\\\server",
        "\\\\server\\",
    ] {
        assert_eq!(
            w.normalize(bad).unwrap_err().code(),
            "not_absolute",
            "{bad}"
        );
    }
    for bad in [
        "D:\\Mov*ies",
        "D:\\a|b",
        "D:\\Movies\\file:stream",
        "D:\\a?b",
        "D:\\a\0b",
    ] {
        assert_eq!(
            w.normalize(bad).unwrap_err().code(),
            "invalid_characters",
            "{bad:?}"
        );
    }
    let p = PathStyle::Posix;
    for bad in ["Movies", "./Movies", "~/Movies", "D:\\Movies"] {
        assert_eq!(
            p.normalize(bad).unwrap_err().code(),
            "not_absolute",
            "{bad}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// nested data sources
// ---------------------------------------------------------------------------------------------

#[test]
fn a_folder_inside_an_existing_source_is_refused_and_explained() {
    // The scenario of the spec.
    let existing = [src(1, "D:\\Media\\Movies")];
    let err = windows()
        .check("D:\\Media\\Movies\\Action", &existing, None)
        .unwrap_err();
    assert_eq!(err.code(), "inside_existing");
    let message = err.to_string();
    assert!(message.contains("already covered"), "{message}");
    assert!(message.contains("D:\\Media\\Movies\\Action"), "{message}");
    assert!(message.contains("D:\\Media\\Movies"), "{message}");
}

#[test]
fn a_deeply_nested_folder_is_refused() {
    let existing = [src(1, "D:\\Media")];
    assert_eq!(
        code(windows().check("D:\\Media\\Movies\\Action\\80s", &existing, None)),
        "inside_existing"
    );
}

#[test]
fn nesting_ignores_case_separators_and_trailing_slashes() {
    let existing = [src(1, "D:\\Media\\Movies\\")];
    for path in ["d:/media/MOVIES/action", "D:\\MEDIA\\Movies\\Action\\"] {
        assert_eq!(
            code(windows().check(path, &existing, None)),
            "inside_existing",
            "{path}"
        );
    }
    assert_eq!(
        code(posix().check("/volumes/movies/Action", &[src(1, "/Volumes/Movies")], None)),
        "inside_existing"
    );
}

#[test]
fn a_folder_containing_an_existing_source_is_refused() {
    let existing = [src(1, "D:\\Media\\Movies\\Action")];
    let err = windows().check("D:\\Media", &existing, None).unwrap_err();
    assert_eq!(err.code(), "contains_existing");
    assert!(err.to_string().contains("D:\\Media\\Movies\\Action"));
}

#[test]
fn the_same_folder_twice_is_refused() {
    let existing = [src(1, "D:\\Movies")];
    for path in ["D:\\Movies", "d:\\movies\\", "D:/Movies/."] {
        assert_eq!(
            code(windows().check(path, &existing, None)),
            "already_added",
            "{path}"
        );
    }
}

#[test]
fn a_sibling_with_a_shared_name_prefix_is_not_nested() {
    // `Movies2` starts with the text `Movies` but is not inside it.
    let existing = [src(1, "D:\\Movies")];
    assert_eq!(
        windows().check("D:\\Movies2", &existing, None).unwrap(),
        "D:\\Movies2"
    );
    assert_eq!(
        windows().check("D:\\Movies Old", &existing, None).unwrap(),
        "D:\\Movies Old"
    );
    assert_eq!(
        posix()
            .check("/Volumes/Movies2", &[src(1, "/Volumes/Movies")], None)
            .unwrap(),
        "/Volumes/Movies2"
    );
}

#[test]
fn different_drives_and_shares_never_nest() {
    let existing = [src(1, "D:\\Movies"), src(2, "\\\\nas\\Movies")];
    assert_eq!(
        windows().check("E:\\Movies", &existing, None).unwrap(),
        "E:\\Movies"
    );
    // A different share on the same server is a different root.
    assert_eq!(
        windows().check("\\\\nas\\Films", &existing, None).unwrap(),
        "\\\\nas\\Films"
    );
    assert_eq!(
        code(windows().check("\\\\NAS\\movies\\Action", &existing, None)),
        "inside_existing"
    );
}

#[test]
fn a_share_root_can_be_a_source() {
    assert_eq!(
        windows().check("\\\\nas\\Movies", &[], None).unwrap(),
        "\\\\nas\\Movies"
    );
}

#[test]
fn ignoring_a_source_allows_remapping_onto_a_path_near_its_own() {
    let existing = [src(1, "D:\\Movies"), src(2, "D:\\Shows")];
    // Remapping source 1 to a subfolder of itself would otherwise be "inside existing".
    assert_eq!(
        windows()
            .check("D:\\Movies\\Films", &existing, Some(1))
            .unwrap(),
        "D:\\Movies\\Films"
    );
    // Other sources still count.
    assert_eq!(
        code(windows().check("D:\\Shows\\Drama", &existing, Some(1))),
        "inside_existing"
    );
}

#[test]
fn an_unreadable_stored_path_never_blocks_a_new_source() {
    let existing = [src(1, "not a path")];
    assert!(windows().check("D:\\Movies", &existing, None).is_ok());
}

// ---------------------------------------------------------------------------------------------
// system folders
// ---------------------------------------------------------------------------------------------

#[test]
fn windows_system_folders_are_refused() {
    for path in [
        "C:\\",
        "C:\\Windows",
        "C:\\Windows\\System32",
        "C:\\windows\\system32\\drivers",
        "C:\\Program Files",
        "C:\\Program Files\\Some App",
        "C:\\Program Files (x86)\\Some App",
        "C:\\ProgramData\\Package Cache",
        "C:\\Users",
        "C:\\Users\\Alex",
        "C:\\Recovery",
        "C:\\$Recycle.Bin",
        "D:\\$RECYCLE.BIN",
        "D:\\System Volume Information",
        "E:\\Media\\System Volume Information\\x",
        "C:\\Windows.\\System32",
        "C:\\Windows \\",
        "c:/WINDOWS/",
        "C:\\Media\\..\\Windows",
    ] {
        assert_eq!(
            code(windows().check(path, &[], None)),
            "system_folder",
            "{path}"
        );
    }
    // A bare drive letter is not a folder path at all.
    assert_eq!(code(windows().check("c:", &[], None)), "not_absolute");
}

#[test]
fn windows_folders_next_to_system_folders_are_fine() {
    for path in [
        "D:\\",
        "D:\\Windows",
        "C:\\Users\\Alex\\Videos",
        "C:\\Users\\Alex\\Videos\\Movies",
        "C:\\Media\\Movies",
        "C:\\Windows Media",
        "C:\\Program Files Old",
        "E:\\Movies",
    ] {
        assert!(windows().check(path, &[], None).is_ok(), "{path}");
    }
}

#[test]
fn the_system_drive_follows_the_environment() {
    let mut system = SystemFolders::new(PathStyle::Windows);
    system.protect_exact("D:\\", "the system drive");
    system.protect_tree("D:\\WinNT", "the Windows folder");
    let guard = SourceGuard::new(PathStyle::Windows, system);
    assert_eq!(code(guard.check("D:\\", &[], None)), "system_folder");
    assert_eq!(
        code(guard.check("D:\\WinNT\\System32", &[], None)),
        "system_folder"
    );
    assert!(guard.check("D:\\Movies", &[], None).is_ok());
}

#[test]
fn macos_system_folders_are_refused() {
    for path in [
        "/",
        "/System",
        "/System/Library/CoreServices",
        "/Library",
        "/Library/Application Support",
        "/Applications",
        "/Applications/Some.app",
        "/usr",
        "/usr/local/bin",
        "/bin",
        "/etc",
        "/var/db",
        "/private/var",
        "/Users",
        "/Users/alex",
        "/Volumes",
        "/Volumes/Movies/.Trashes",
        "/Volumes/Movies/.Spotlight-V100",
        "/sys/../System",
    ] {
        assert_eq!(
            code(posix().check(path, &[], None)),
            "system_folder",
            "{path}"
        );
    }
}

#[test]
fn macos_folders_next_to_system_folders_are_fine() {
    for path in [
        "/Volumes/Movies",
        "/Volumes/Macintosh HD/Media",
        "/Users/alex/Movies",
        "/Users/alex/Movies/Films",
        "/Library2",
        "/mnt/nas/Movies",
        "/Systemx",
    ] {
        assert!(posix().check(path, &[], None).is_ok(), "{path}");
    }
}

#[test]
fn the_home_folder_and_app_data_folders_come_from_the_environment() {
    let mut system = SystemFolders::new(PathStyle::Windows);
    system
        .protect_exact("E:\\Home\\Alex", "your user profile folder")
        .protect_tree("E:\\Home\\Alex\\AppData", "your application data folder")
        .protect_tree("E:\\Data\\CineVault", "CineVault's own data folder");
    let guard = SourceGuard::new(PathStyle::Windows, system);
    assert_eq!(
        code(guard.check("E:\\Home\\Alex", &[], None)),
        "system_folder"
    );
    assert_eq!(
        code(guard.check("E:\\Home\\Alex\\AppData\\Local", &[], None)),
        "system_folder"
    );
    assert_eq!(
        code(guard.check("E:\\Data\\CineVault\\backups", &[], None)),
        "system_folder"
    );
    assert!(guard.check("E:\\Home\\Alex\\Videos", &[], None).is_ok());
}

#[test]
fn the_message_names_the_folder_and_what_it_is() {
    let err = windows().check("C:\\Windows", &[], None).unwrap_err();
    assert_eq!(
        err.to_string(),
        "C:\\Windows is the Windows folder, so it cannot be a library folder."
    );
}

#[test]
fn a_folder_inside_a_protected_folder_is_named_with_that_folder() {
    let mut system = SystemFolders::new(PathStyle::Windows);
    system.protect_tree(
        r"C:\Users\Alex\AppData\Local",
        "your application data folder",
    );
    let guard = SourceGuard::new(PathStyle::Windows, system);
    let path = r"C:\Users\Alex\AppData\Local\Temp\qa\media\Movies";
    let err = guard.check(path, &[], None).unwrap_err();
    assert_eq!(err.code(), "system_folder");
    assert_eq!(
        err.to_string(),
        format!(
            r"{path} is inside your application data folder (C:\Users\Alex\AppData\Local), so it cannot be a library folder."
        )
    );
    // The protected folder itself is still described as what it is.
    let err = guard
        .check(r"c:\users\alex\appdata\local", &[], None)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        r"C:\users\alex\appdata\local is your application data folder, so it cannot be a library folder."
    );
    // A folder below a recycle bin names the recycle bin.
    let err = windows()
        .check(r"E:\Media\$RECYCLE.BIN\S-1-5\Movies", &[], None)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        r"E:\Media\$RECYCLE.BIN\S-1-5\Movies is inside a Windows system folder (E:\Media\$RECYCLE.BIN), so it cannot be a library folder."
    );
}

#[test]
fn the_system_check_runs_before_the_nesting_check() {
    // Even when a system folder is "inside" an existing source, the system reason wins.
    let existing = [src(1, "C:\\")];
    assert_eq!(
        code(windows().check("C:\\Windows", &existing, None)),
        "system_folder"
    );
}

// ---------------------------------------------------------------------------------------------
// links (real file system)
// ---------------------------------------------------------------------------------------------

#[test]
fn check_with_links_accepts_an_ordinary_existing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Movies");
    std::fs::create_dir(&path).unwrap();
    let guard = SourceGuard::new(
        PathStyle::current(),
        SystemFolders::new(PathStyle::current()),
    );
    let text = path.to_string_lossy().into_owned();
    assert!(guard.check_with_links(&text, &[], None).is_ok());
}

#[test]
fn check_with_links_refuses_a_folder_that_resolves_into_a_protected_one() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("protected");
    let inner = real.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    // `inner` is protected through a path spelling the plain check cannot see: the protected
    // folder is registered by its resolved location, and the source reaches it by `..` that only
    // the file system resolves through a link.
    let link = dir.path().join("link");
    #[cfg(windows)]
    let linked = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&real)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&real, &link).is_ok();
    if !linked {
        // Creating links is not permitted here; the text-only checks above still cover the rule.
        return;
    }
    let mut system = SystemFolders::new(PathStyle::current());
    system.protect_tree(
        &std::fs::canonicalize(&real).unwrap().to_string_lossy(),
        "a system folder",
    );
    let guard = SourceGuard::new(PathStyle::current(), system);
    let through_link = link.join("inner").to_string_lossy().into_owned();
    // The text check alone does not know the link.
    assert!(guard.check(&through_link, &[], None).is_ok());
    assert_eq!(
        code(guard.check_with_links(&through_link, &[], None)),
        "system_folder"
    );
}
