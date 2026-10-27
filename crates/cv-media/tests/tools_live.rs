//! Opt-in test against the real download servers. It skips unless `CINEVAULT_LIVE_TOOLS=1`
//! (it downloads the MediaInfo library, a few megabytes, and checks the pinned hash).

use cv_media::tools::{Cancel, Package, ToolId, ToolSource, ToolsManager};

#[test]
fn the_pinned_mediainfo_download_installs_from_the_real_server() {
    if std::env::var_os("CINEVAULT_LIVE_TOOLS").is_none() {
        eprintln!("skipped: set CINEVAULT_LIVE_TOOLS=1 to download the pinned MediaInfo build");
        return;
    }
    let tools = tempfile::tempdir().expect("temp dir");
    let manager = ToolsManager::new(tools.path());
    manager
        .install(Package::MediaInfo, &Cancel::new(), &mut |_| {})
        .expect("the pinned download matches its checksum");
    let library = manager.resolve(ToolId::MediaInfo).expect("library");
    assert_eq!(library.source, ToolSource::Managed);
    assert!(library.path.is_file());
    assert!(manager.version_of(&library).is_some());
}
