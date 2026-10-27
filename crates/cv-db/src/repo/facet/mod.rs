//! The filter catalogue as SQL-backed facets.
//!
//! The library views have a catalogue of filters for movies, TV shows and movie sets.
//! [`catalogue`] lists them; a [`FacetCondition`] is one chip with
//! its value(s), and it compiles inside a [`Filter`](super::Filter) to SQL over the item table,
//! so it composes with the quick-search filters, sorts and keyset paging of [`super::query`].
//!
//! A condition carries what a chip offers:
//!
//! - **negation**, which is exactly the inverse: an item without any value for the facet matches
//!   a negated condition;
//! - **ANY / ALL** for list and text filters ([`MatchMode`]);
//! - a **numeric operator** (`<`, `<=`, `=`, `>=`, `>`, between) for number and date filters.
//!
//! A condition with nothing to test yet (a list without values, a number without a number)
//! restricts nothing, like an empty quick-search text, whatever its negation.
//!
//! A show matches a filter about files, streams, editions, sources or episode IDs when any of
//! its episodes does; show-level facts (genre, cast, artwork, IDs, certification) are the show's
//! own. [`facet_values`] lists the values a list filter has in the library with their item
//! counts, for the live counts of the "+ Filter" picker.

mod ids;
mod spec;
#[cfg(test)]
mod tests;

use cv_domain::ItemKind;
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};

use ids::Scope;
pub use ids::{FilterDef, FilterGroup, FilterId, FilterKind, catalogue};
use spec::{Spec, spec};

use super::codec::enum_text;
use super::filter::{Filter, MatchMode, Sql, distinct_lowercase};
use crate::error::{DbError, DbResult};

/// How a number or date filter compares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumOp {
    Lt,
    Le,
    #[default]
    Eq,
    Ge,
    Gt,
    /// Between `number` and `number2` (or the first two `values` for a date), inclusive.
    Between,
}

impl NumOp {
    fn sql(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Eq => "=",
            Self::Ge => ">=",
            Self::Gt => ">",
            Self::Between => "BETWEEN",
        }
    }
}

/// One catalogue filter with its input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FacetCondition {
    pub id: FilterId,
    /// Keep what the filter would leave out.
    #[serde(default)]
    pub negated: bool,
    /// ANY or ALL, for list and text filters.
    #[serde(default)]
    pub mode: MatchMode,
    /// The comparison of a number or date filter.
    #[serde(default)]
    pub op: NumOp,
    /// The values of a list filter, the terms of a text filter, or the date(s) of a date filter
    /// (`YYYY`, `YYYY-MM` or `YYYY-MM-DD`).
    #[serde(default)]
    pub values: Vec<String>,
    /// The number of a number filter, or its lower bound for [`NumOp::Between`].
    #[serde(default)]
    pub number: Option<f64>,
    /// The upper bound of [`NumOp::Between`].
    #[serde(default)]
    pub number2: Option<f64>,
}

impl FacetCondition {
    pub fn new(id: FilterId) -> Self {
        Self {
            id,
            negated: false,
            mode: MatchMode::Any,
            op: NumOp::Eq,
            values: Vec::new(),
            number: None,
            number2: None,
        }
    }

    /// A list or text condition on these values.
    #[must_use]
    pub fn with_values<S: Into<String>>(mut self, values: impl IntoIterator<Item = S>) -> Self {
        self.values = values.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_mode(mut self, mode: MatchMode) -> Self {
        self.mode = mode;
        self
    }

    /// A number condition: `value` compared with `op`.
    #[must_use]
    pub fn with_number(mut self, op: NumOp, value: f64) -> Self {
        self.op = op;
        self.number = Some(value);
        self
    }

    /// A number condition between two bounds, inclusive.
    #[must_use]
    pub fn between(mut self, low: f64, high: f64) -> Self {
        self.op = NumOp::Between;
        self.number = Some(low);
        self.number2 = Some(high);
        self
    }

    /// A date condition: the first value compared with `op`.
    #[must_use]
    pub fn with_date(mut self, op: NumOp, date: &str) -> Self {
        self.op = op;
        self.values = vec![date.to_owned()];
        self
    }

    /// A date condition between two dates, inclusive.
    #[must_use]
    pub fn between_dates(mut self, from: &str, to: &str) -> Self {
        self.op = NumOp::Between;
        self.values = vec![from.to_owned(), to.to_owned()];
        self
    }

    #[must_use]
    pub fn negate(mut self) -> Self {
        self.negated = true;
        self
    }

    fn terms(&self) -> Vec<String> {
        self.values
            .iter()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect()
    }

    fn number_bounds(&self) -> Option<(f64, f64)> {
        let first = self.number.filter(|n| n.is_finite())?;
        if self.op == NumOp::Between {
            let second = self.number2.filter(|n| n.is_finite())?;
            Some((first.min(second), first.max(second)))
        } else {
            Some((first, first))
        }
    }

    fn date_bounds(&self) -> Option<(String, String)> {
        let terms = self.terms();
        let first = terms.first()?.clone();
        if self.op == NumOp::Between {
            let second = terms.get(1)?.clone();
            Some(if first <= second {
                (first, second)
            } else {
                (second, first)
            })
        } else {
            Some((first.clone(), first))
        }
    }

    /// Whether the condition has input to test. An inactive condition restricts nothing.
    fn is_active(&self, spec: &Spec) -> bool {
        match spec {
            Spec::Flag(_) => true,
            Spec::Multi(_) | Spec::Text(_) => !self.terms().is_empty(),
            Spec::Number(_) => self.number_bounds().is_some(),
            Spec::Date(_) => self.date_bounds().is_some(),
        }
    }

    /// Whether the condition has input to test for the filter it names.
    pub fn has_input(&self) -> bool {
        match self.id.def().kind {
            FilterKind::Flag => true,
            FilterKind::Multi | FilterKind::Text => !self.terms().is_empty(),
            FilterKind::Number => self.number_bounds().is_some(),
            FilterKind::Date => self.date_bounds().is_some(),
        }
    }

    /// Appends the SQL condition on `item` for the library `out.kind`.
    pub(crate) fn write(&self, out: &mut Sql) -> DbResult<()> {
        let kind = out
            .kind
            .ok_or_else(|| DbError::InvalidFilter("a catalogue filter needs a library".into()))?;
        let scope = Scope::of(kind)?;
        let spec = spec(scope, self.id).ok_or_else(|| {
            DbError::InvalidFilter(format!("{:?} is not a filter of {kind:?} items", self.id))
        })?;
        if !self.is_active(&spec) {
            out.push("1");
            return Ok(());
        }
        if self.negated {
            out.push("NOT (");
        }
        match &spec {
            Spec::Flag(condition) => {
                out.push("(");
                out.push(condition);
                out.push(")");
            }
            Spec::Multi(source) => self.write_multi(out, source),
            Spec::Text(source) => self.write_text(out, source),
            Spec::Number(source) => self.write_number(out, source),
            Spec::Date(source) => self.write_date(out, source),
        }
        if self.negated {
            out.push(")");
        }
        Ok(())
    }

    fn write_multi(&self, out: &mut Sql, source: &str) {
        let names = distinct_lowercase(&self.values);
        let list = |out: &mut Sql| {
            out.push("v.value COLLATE NOCASE IN (");
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    out.push(", ");
                }
                out.bind(name.clone());
            }
            out.push(")");
        };
        match self.mode {
            MatchMode::Any => exists(out, source, list),
            MatchMode::All => {
                out.push("(SELECT count(DISTINCT lower(v.value)) FROM (");
                out.push(source);
                out.push(") v WHERE v.owner = item.id AND ");
                list(out);
                out.push(&format!(") = {}", names.len()));
            }
        }
    }

    fn write_text(&self, out: &mut Sql, source: &str) {
        let like = |out: &mut Sql, term: &str| {
            out.push("v.value LIKE ");
            out.bind(format!("%{}%", escape_like(term)));
            out.push(" ESCAPE '\\'");
        };
        let terms = self.terms();
        match self.mode {
            MatchMode::Any => exists(out, source, |out| {
                out.push("(");
                for (i, term) in terms.iter().enumerate() {
                    if i > 0 {
                        out.push(" OR ");
                    }
                    like(out, term);
                }
                out.push(")");
            }),
            MatchMode::All => {
                out.push("(");
                for (i, term) in terms.iter().enumerate() {
                    if i > 0 {
                        out.push(" AND ");
                    }
                    exists(out, source, |out| like(out, term));
                }
                out.push(")");
            }
        }
    }

    fn write_number(&self, out: &mut Sql, source: &str) {
        let Some((low, high)) = self.number_bounds() else {
            out.push("1");
            return;
        };
        exists(out, source, |out| {
            out.push("v.value ");
            out.push(self.op.sql());
            out.push(" ");
            out.bind(low);
            if self.op == NumOp::Between {
                out.push(" AND ");
                out.bind(high);
            }
        });
    }

    fn write_date(&self, out: &mut Sql, source: &str) {
        let Some((low, high)) = self.date_bounds() else {
            out.push("1");
            return;
        };
        exists(out, source, |out| {
            out.push("v.value ");
            out.push(self.op.sql());
            out.push(" ");
            out.bind(low);
            if self.op == NumOp::Between {
                out.push(" AND ");
                out.bind(high);
            }
        });
    }
}

/// `EXISTS (SELECT 1 FROM (source) v WHERE v.owner = item.id AND <condition>)`.
fn exists(out: &mut Sql, source: &str, condition: impl FnOnce(&mut Sql)) {
    out.push("EXISTS (SELECT 1 FROM (");
    out.push(source);
    out.push(") v WHERE v.owner = item.id AND ");
    condition(out);
    out.push(")");
}

/// Escapes the wildcards of a `LIKE` pattern (the escape character is `\`).
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// A value of a list filter and how many items of the library have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetValue {
    pub value: String,
    pub count: u64,
}

/// The values of the list filter `id` in the library of `kind`, with the number of items that
/// have each one among those matching `base`. Most frequent first, then by name; values differing
/// only in case are one value. Pass [`Filter::All`] for the counts over the whole library, and
/// the other chips for the counts the "+ Filter" picker shows next to the values.
pub fn facet_values(
    conn: &Connection,
    kind: ItemKind,
    id: FilterId,
    base: &Filter,
) -> DbResult<Vec<FacetValue>> {
    let scope = Scope::of(kind)?;
    let Some(Spec::Multi(source)) = spec(scope, id) else {
        return Err(DbError::InvalidFilter(format!(
            "{id:?} has no list of values for {kind:?} items"
        )));
    };
    let mut sql = Sql {
        kind: Some(kind),
        ..Sql::default()
    };
    sql.push("SELECT min(v.value), count(DISTINCT v.owner) FROM (");
    sql.push(&source);
    sql.push(
        ") v JOIN item ON item.id = v.owner WHERE item.trashed_entry IS NULL AND item.kind = ",
    );
    sql.bind(enum_text(&kind)?);
    sql.push(" AND v.value IS NOT NULL AND v.value <> '' AND (");
    base.write(&mut sql)?;
    sql.push(") GROUP BY lower(v.value) ORDER BY 2 DESC, 1 COLLATE NOCASE");

    let mut stmt = conn.prepare_cached(&sql.text)?;
    let rows = stmt.query_map(params_from_iter(sql.params.iter()), |row| {
        Ok(FacetValue {
            value: row.get(0)?,
            count: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
