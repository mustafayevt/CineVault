//! The filter catalogue for the library views: which filters exist for movies, shows and
//! collections, how a chip carries its value(s), and the values a list filter offers.
//!
//! The rules live in `cv_db::repo::facet`; this module only converts to and from the types the UI
//! sees. A filter id travels as its stable snake_case name (`video_format`, `hdr_format`), and an
//! unknown one is refused with the `bad_filter` code instead of being ignored, so a stale saved
//! view cannot silently show the wrong list.

use cv_db::repo::facet::{
    FacetCondition, FacetValue, FilterDef, FilterGroup, FilterId, FilterKind, NumOp, catalogue,
};
use std::collections::HashMap;

use cv_db::repo::filter::MatchMode;
use cv_domain::{Genre, ItemKind};
use cv_nfo::certification::{CertificationStyle, format_certification};
use cv_template::tokens::genre_label;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::library::{LibraryCommandError, LibraryKindDto};

/// How a multi-value or text filter combines its values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FacetModeDto {
    /// At least one of the values.
    #[default]
    Any,
    /// Every one of the values.
    All,
}

impl From<FacetModeDto> for MatchMode {
    fn from(mode: FacetModeDto) -> Self {
        match mode {
            FacetModeDto::Any => Self::Any,
            FacetModeDto::All => Self::All,
        }
    }
}

/// The comparison of a number or date filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FacetOpDto {
    Lt,
    Le,
    #[default]
    Eq,
    Ge,
    Gt,
    Between,
}

impl From<FacetOpDto> for NumOp {
    fn from(op: FacetOpDto) -> Self {
        match op {
            FacetOpDto::Lt => Self::Lt,
            FacetOpDto::Le => Self::Le,
            FacetOpDto::Eq => Self::Eq,
            FacetOpDto::Ge => Self::Ge,
            FacetOpDto::Gt => Self::Gt,
            FacetOpDto::Between => Self::Between,
        }
    }
}

/// One catalogue filter chip: the filter, its negation and its input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct LibraryFacetDto {
    /// A filter id from `library_filter_catalogue`, such as `video_format`.
    pub id: String,
    /// Keep what the filter would leave out.
    pub negated: bool,
    /// ANY or ALL, for list and text filters.
    pub mode: FacetModeDto,
    /// The comparison of a number or date filter.
    pub op: FacetOpDto,
    /// The values of a list filter, the terms of a text filter, or the date(s) of a date filter
    /// (`YYYY`, `YYYY-MM` or `YYYY-MM-DD`).
    pub values: Vec<String>,
    /// The number of a number filter, or its lower bound when `op` is `between`.
    pub number: Option<f64>,
    /// The upper bound when `op` is `between`.
    pub number2: Option<f64>,
}

/// The tab of the filter picker a filter belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FilterGroupDto {
    Details,
    Metadata,
    Video,
    Audio,
    Subtitles,
    Artwork,
    Files,
    Ids,
    Ratings,
}

impl From<FilterGroup> for FilterGroupDto {
    fn from(group: FilterGroup) -> Self {
        match group {
            FilterGroup::Details => Self::Details,
            FilterGroup::Metadata => Self::Metadata,
            FilterGroup::Video => Self::Video,
            FilterGroup::Audio => Self::Audio,
            FilterGroup::Subtitles => Self::Subtitles,
            FilterGroup::Artwork => Self::Artwork,
            FilterGroup::Files => Self::Files,
            FilterGroup::Ids => Self::Ids,
            FilterGroup::Ratings => Self::Ratings,
        }
    }
}

/// How a filter takes its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FilterKindDto {
    /// On or off; negation turns it round.
    Flag,
    /// Values present in the library, combined with ANY or ALL.
    Multi,
    /// Case-insensitive "contains" terms, combined with ANY or ALL.
    Text,
    /// A number compared with `<`, `<=`, `=`, `>=`, `>` or between.
    Number,
    /// An ISO date compared the same way.
    Date,
}

impl From<FilterKind> for FilterKindDto {
    fn from(kind: FilterKind) -> Self {
        match kind {
            FilterKind::Flag => Self::Flag,
            FilterKind::Multi => Self::Multi,
            FilterKind::Text => Self::Text,
            FilterKind::Number => Self::Number,
            FilterKind::Date => Self::Date,
        }
    }
}

/// One entry of the filter picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FilterDefDto {
    pub id: String,
    pub label: String,
    pub group: FilterGroupDto,
    pub kind: FilterKindDto,
    /// The unit of a number filter (`min`, `kbps`, `MiB`, `px`).
    pub unit: Option<String>,
}

/// A value of a list filter and how many items have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FacetValueDto {
    /// The stored value, which a chip filters by (`SCIENCE_FICTION`, `US_PG13`).
    pub value: String,
    /// The value as the user reads it (`Science Fiction`, `PG-13`).
    pub label: String,
    pub count: u32,
}

impl From<FacetValue> for FacetValueDto {
    fn from(value: FacetValue) -> Self {
        Self {
            label: value.value.clone(),
            value: value.value,
            count: u32::try_from(value.count).unwrap_or(u32::MAX),
        }
    }
}

/// A certification token as the user reads it: `US_PG13` is `PG-13`, `DE_FSK12` is `FSK 12`.
/// Text that is no token stays as it is.
pub fn certification_label(token: &str) -> String {
    let label = format_certification(token, CertificationStyle::Short);
    if label.is_empty() {
        token.to_owned()
    } else {
        label
    }
}

/// The values of filter `id` with readable labels: genres by name, certifications by their short
/// form, or with their country (`US: PG`, `GB: PG`) where two countries share a short form.
pub fn labelled_values(id: FilterId, values: Vec<FacetValue>) -> Vec<FacetValueDto> {
    let mut rows: Vec<FacetValueDto> = values.into_iter().map(FacetValueDto::from).collect();
    match id {
        FilterId::Genre => {
            for row in &mut rows {
                if let Some(genre) = Genre::parse(&row.value) {
                    row.label = genre_label(&genre);
                }
            }
        }
        FilterId::Certification => {
            for row in &mut rows {
                row.label = certification_label(&row.value);
            }
            let mut seen: HashMap<String, usize> = HashMap::new();
            for row in &rows {
                *seen.entry(row.label.clone()).or_default() += 1;
            }
            for row in &mut rows {
                if seen.get(&row.label).copied().unwrap_or(0) > 1 {
                    let medium = format_certification(&row.value, CertificationStyle::Medium);
                    if !medium.is_empty() {
                        row.label = medium;
                    }
                }
            }
        }
        _ => {}
    }
    rows
}

fn bad_filter(message: String) -> LibraryCommandError {
    LibraryCommandError {
        code: "bad_filter".to_owned(),
        message,
    }
}

/// The id behind a filter's stable name.
pub fn parse_filter_id(name: &str) -> Result<FilterId, LibraryCommandError> {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .map_err(|_| bad_filter(format!("There is no filter called `{name}`.")))
}

fn id_name(id: FilterId) -> String {
    serde_json::to_value(id)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl From<FilterDef> for FilterDefDto {
    fn from(def: FilterDef) -> Self {
        Self {
            id: id_name(def.id),
            label: def.label.to_owned(),
            group: def.group.into(),
            kind: def.kind.into(),
            unit: def.unit.map(str::to_owned),
        }
    }
}

impl LibraryFacetDto {
    /// The condition this chip stands for.
    pub fn to_condition(&self) -> Result<FacetCondition, LibraryCommandError> {
        let mut condition = FacetCondition::new(parse_filter_id(&self.id)?);
        condition.negated = self.negated;
        condition.mode = self.mode.into();
        condition.op = self.op.into();
        condition.values = self.values.clone();
        condition.number = self.number;
        condition.number2 = self.number2;
        Ok(condition)
    }
}

/// The filters that exist for a library, in catalogue order.
pub fn filter_catalogue(kind: LibraryKindDto) -> Vec<FilterDefDto> {
    let kind: ItemKind = kind.into();
    catalogue(kind)
        .unwrap_or_default()
        .into_iter()
        .map(FilterDefDto::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_counts_match_the_spec_and_ids_round_trip() {
        // The 64, 71 and 4 filters of the base catalogue without the tag filters (Tag for
        // movies and shows, Episode tag for shows), plus the rating filters of movies and shows
        // (per source and the Top 250 rank).
        let base = |kind| {
            filter_catalogue(kind)
                .iter()
                .filter(|def| def.group != FilterGroupDto::Ratings && def.id != "edition_lost")
                .count()
        };
        assert_eq!(base(LibraryKindDto::Movies), 63);
        assert_eq!(base(LibraryKindDto::Shows), 69);
        assert_eq!(base(LibraryKindDto::Collections), 4);
        // Movies also have CineVault's own "Edition only in the file name".
        assert_eq!(filter_catalogue(LibraryKindDto::Movies).len(), 63 + 12 + 1);
        assert_eq!(filter_catalogue(LibraryKindDto::Collections).len(), 4);
        for kind in [
            LibraryKindDto::Movies,
            LibraryKindDto::Shows,
            LibraryKindDto::Collections,
        ] {
            for def in filter_catalogue(kind) {
                let id = parse_filter_id(&def.id).unwrap();
                assert_eq!(id_name(id), def.id);
            }
        }
    }

    #[test]
    fn an_unknown_filter_id_is_refused_with_its_own_code() {
        let error = parse_filter_id("no_such_filter").unwrap_err();
        assert_eq!(error.code, "bad_filter");
    }

    #[test]
    fn genre_and_certification_values_carry_readable_labels() {
        let value = |value: &str, count: u64| FacetValue {
            value: value.to_owned(),
            count,
        };
        let genres = labelled_values(
            FilterId::Genre,
            vec![value("SCIENCE_FICTION", 2), value("Mockumentary", 1)],
        );
        let labels: Vec<&str> = genres.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["Science Fiction", "Mockumentary"]);
        assert_eq!(genres[0].value, "SCIENCE_FICTION");

        let ratings = labelled_values(
            FilterId::Certification,
            vec![
                value("US_PG13", 3),
                value("US_R", 2),
                value("US_PG", 1),
                value("GB_PG", 1),
                value("DE_FSK12", 1),
            ],
        );
        let labels: Vec<&str> = ratings.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["PG-13", "R", "US: PG", "GB: PG", "FSK 12"]);
        assert_eq!(certification_label("US_PG13"), "PG-13");
        assert_eq!(certification_label("PG-13"), "PG-13");
        assert_eq!(certification_label(""), "");
    }
}
