//! Copies of one movie in a rename: each keeps a folder of its own, told apart by a label from
//! its media info, and a labelled copy stays as it is until its twin is gone.

use cv_db::repo::organize_state::OrganizeStateKind;

use super::*;

/// A copy of Example Movie (2020) in `folder`, its video `base.mkv` of `width` pixels (0: media
/// info not read yet).
fn copy(lib: &Lib, folder: &str, base: &str, width: u32) -> ItemId {
    let id = lib.movie(folder, base, "Example Movie", 2020);
    let mut item = lib.item(id);
    for file in &mut item.core.media_files {
        if file.kind == MediaFileType::Video && width > 0 {
            file.container_format = "Matroska".to_owned();
            file.width = width;
            file.height = width * 9 / 16;
        }
    }
    lib.db
        .write(move |tx| cv_db::repo::update_item(tx, &item))
        .unwrap();
    id
}

fn row(preview: &Preview, id: ItemId) -> &PreviewRow {
    preview
        .rows
        .iter()
        .find(|r| r.id == id.0.to_string())
        .unwrap_or_else(|| panic!("no row for {id:?} in {preview:?}"))
}

#[test]
fn two_copies_get_a_label_each_and_undo_puts_both_back() {
    let lib = Lib::new();
    let hd = copy(&lib, "dl-1", "example.1080p", 1920);
    let uhd = copy(&lib, "dl-2", "example.2160p", 3840);
    let request = lib.request(Scope::Everything);

    let preview = lib.service.preview(&request).unwrap();
    assert_eq!(preview.counts.conflicts, 0, "{preview:?}");
    assert_eq!(preview.counts.change, 2);
    assert_eq!(row(&preview, hd).after, "Example Movie (2020) - 1080p");
    assert_eq!(
        row(&preview, hd).after_file,
        "Example Movie (2020) - 1080p.mkv"
    );
    assert_eq!(row(&preview, uhd).after, "Example Movie (2020) - 2160p");
    // The preview says which part of each name is a copy label, and which copy it is.
    let note = |id| row(&preview, id).copy.clone().expect("a copy note");
    assert_eq!(
        (note(hd).nth, note(hd).of, note(hd).label),
        (1, 2, "1080p".to_owned())
    );
    assert_eq!(
        (note(uhd).nth, note(uhd).of, note(uhd).label),
        (2, 2, "2160p".to_owned())
    );

    let applied = lib
        .service
        .apply(&ApplyRequest {
            preview: request.clone(),
            include: ids(&[hd, uhd]),
        })
        .unwrap();
    assert_eq!(applied.done(), 2, "{applied:?}");
    assert!(lib.exists("Example Movie (2020) - 1080p/Example Movie (2020) - 1080p.mkv"));
    assert!(lib.exists("Example Movie (2020) - 1080p/Example Movie (2020) - 1080p.nfo"));
    assert!(lib.exists("Example Movie (2020) - 2160p/Example Movie (2020) - 2160p.mkv"));

    // Renamed again with the same templates and media info: nothing to do.
    let again = lib.service.preview(&request).unwrap();
    assert_eq!(again.counts.tidy, 2, "{again:?}");
    assert!(again.rows.is_empty());

    lib.service.undo(&applied.batch_id.unwrap()).unwrap();
    assert!(lib.exists("dl-1/example.1080p.mkv"));
    assert!(lib.exists("dl-2/example.2160p.mkv"));
}

#[test]
fn a_copy_renamed_alone_is_labelled_next_to_its_plain_named_twin() {
    let lib = Lib::new();
    let twin = copy(&lib, "Example Movie (2020)", "Example Movie (2020)", 3840);
    let new = copy(&lib, "dl", "example", 1920);

    let preview = lib
        .service
        .preview(&lib.request(Scope::Items(vec![new])))
        .unwrap();

    assert_eq!(preview.counts.conflicts, 0, "{preview:?}");
    assert_eq!(row(&preview, new).after, "Example Movie (2020) - 1080p");
    assert!(preview.rows.iter().all(|r| r.id != twin.0.to_string()));
    // The twin that is not in the batch still counts for "Copy 2 of 2".
    let note = row(&preview, new).copy.clone().expect("a copy note");
    assert_eq!((note.nth, note.of, note.label.as_str()), (2, 2, "1080p"));
}

#[test]
fn a_copy_whose_twin_is_gone_is_offered_its_plain_name() {
    let lib = Lib::new();
    let uhd = copy(
        &lib,
        "Example Movie (2020) - 2160p",
        "Example Movie (2020) - 2160p",
        3840,
    );
    let hd = copy(
        &lib,
        "Example Movie (2020) - 1080p",
        "Example Movie (2020) - 1080p",
        1920,
    );
    let request = lib.request(Scope::Everything);
    assert_eq!(lib.service.preview(&request).unwrap().counts.tidy, 2);

    // The 1080p copy is deleted.
    lib.db
        .write(move |tx| {
            tx.execute("UPDATE item SET trashed_entry = 1 WHERE id = ?1", [hd.0])?;
            Ok(())
        })
        .unwrap();

    let preview = lib
        .service
        .preview(&lib.request(Scope::Items(vec![uhd])))
        .unwrap();
    assert_eq!(row(&preview, uhd).after, "Example Movie (2020)");
    let plain = lib
        .service
        .plan_plain(&request.profile, uhd)
        .unwrap()
        .unwrap();
    assert_eq!(plain.label_to_drop().as_deref(), Some("2160p"));
    assert!(lib.exists("Example Movie (2020) - 2160p/Example Movie (2020) - 2160p.mkv"));
}

#[test]
fn copies_wait_while_one_of_them_has_no_media_info() {
    let lib = Lib::new();
    let read = copy(&lib, "dl-1", "a", 1920);
    let unread = copy(&lib, "dl-2", "b", 0);

    let states = lib
        .service
        .item_states(&lib.request(Scope::Everything))
        .unwrap();

    for id in [read, unread] {
        let state = states.iter().find(|s| s.item == id).unwrap();
        assert_eq!(state.state, OrganizeStateKind::Unplanned, "{states:?}");
        assert_eq!(state.reason, cv_rename::MEDIA_HELD_REASON);
    }
}

#[test]
fn a_plain_name_has_no_label_to_drop() {
    let lib = Lib::new();
    let id = copy(&lib, "Example Movie (2020)", "Example Movie (2020)", 1920);
    let profile = lib.request(Scope::Everything).profile;

    let plain = lib.service.plan_plain(&profile, id).unwrap().unwrap();

    assert_eq!(plain.folder, "Example Movie (2020)");
    assert_eq!(plain.label_to_drop(), None);
}

#[test]
fn a_movie_without_copies_has_no_copy_note() {
    let lib = Lib::new();
    let alone = lib.movie("dl", "another.example", "Another Example", 2018);

    let preview = lib
        .service
        .preview(&lib.request(Scope::Everything))
        .unwrap();

    assert_eq!(row(&preview, alone).copy, None);
}
