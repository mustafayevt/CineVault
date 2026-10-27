//! The reasons a rescan gives for marking a stored show missing although its files may still be
//! on disk. They are written here and read back here (the missing/duplicate review tells the
//! two kinds apart by [`parse`]), so the wording cannot drift from the reading.

use cv_domain::ItemId;

/// Why a rescan set a show row aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Superseded {
    /// The row's folder is part of another show now (an older scan read `Show/Season 1` as a
    /// show of its own).
    PartOfShow,
    /// A second row on the same folder; the row it duplicates is the one given.
    DuplicateOf(ItemId),
}

/// The reason of a show whose folder `item_path` lies inside the show at `owner_path`.
#[must_use]
pub fn part_of_show(item_path: &str, owner_path: &str) -> String {
    format!("the folder {item_path} is part of the show {owner_path} now")
}

/// The reason of a second row on the folder `folder`, which `kept` already holds.
#[must_use]
pub fn another_show(folder: &str, kept: ItemId) -> String {
    format!(
        "the folder {folder} belongs to another show of the library (#{})",
        kept.0
    )
}

/// What a stored availability reason says about the row, when a rescan wrote it.
#[must_use]
pub fn parse(reason: &str) -> Option<Superseded> {
    if let Some((_, tail)) = reason.rsplit_once("belongs to another show of the library (#")
        && let Some(id) = tail.strip_suffix(')').and_then(|n| n.parse::<i64>().ok())
    {
        return Some(Superseded::DuplicateOf(ItemId(id)));
    }
    (reason.starts_with("the folder ")
        && reason.ends_with(" now")
        && reason.contains(" is part of the show "))
    .then_some(Superseded::PartOfShow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reasons_read_back() {
        assert_eq!(
            parse(&another_show(r"D:\TV\Dark", ItemId(7))),
            Some(Superseded::DuplicateOf(ItemId(7)))
        );
        assert_eq!(
            parse(&part_of_show(r"D:\TV\Dark\Season 1", r"D:\TV\Dark")),
            Some(Superseded::PartOfShow)
        );
        assert_eq!(parse("the folder is gone"), None);
        assert_eq!(parse(""), None);
    }
}
