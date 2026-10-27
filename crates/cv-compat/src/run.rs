use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::Value;

use crate::family::Family;
use crate::fixture::{Case, Expected, Suite};

/// What a handler produced for a case; the shape must match the case `Expected`.
#[derive(Debug, Clone, PartialEq)]
pub enum Actual {
    Json(Value),
    Files(BTreeMap<String, Vec<u8>>),
}

/// Why a handler could not produce output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerError {
    /// The case needs something that does not exist yet. Counted as pending, not as a failure.
    Pending(String),
    /// The handler hit an error. Counted as a failure of this fixture only.
    Failed(String),
}

/// Produces CineVault output for the cases of one family.
pub trait Handler {
    fn family(&self) -> Family;
    fn run(&self, case: &Case) -> Result<Actual, HandlerError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail(String),
    Pending(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub family: Family,
    pub id: String,
    pub outcome: Outcome,
}

#[derive(Debug, Default)]
pub struct Report {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub pass: usize,
    pub fail: usize,
    pub pending: usize,
}

impl Counts {
    pub fn total(self) -> usize {
        self.pass + self.fail + self.pending
    }
}

impl Report {
    pub fn counts(&self) -> Counts {
        self.counts_where(|_| true)
    }

    pub fn counts_for(&self, family: Family) -> Counts {
        self.counts_where(|e| e.family == family)
    }

    fn counts_where(&self, keep: impl Fn(&Entry) -> bool) -> Counts {
        let mut c = Counts::default();
        for e in self.entries.iter().filter(|e| keep(e)) {
            match e.outcome {
                Outcome::Pass => c.pass += 1,
                Outcome::Fail(_) => c.fail += 1,
                Outcome::Pending(_) => c.pending += 1,
            }
        }
        c
    }

    pub fn outcome(&self, family: Family, id: &str) -> Option<&Outcome> {
        self.entries
            .iter()
            .find(|e| e.family == family && e.id == id)
            .map(|e| &e.outcome)
    }

    /// No fixture failed. Pending fixtures do not break this.
    pub fn is_green(&self) -> bool {
        self.counts().fail == 0
    }

    /// Green and nothing pending: every fixture has an implementation behind it.
    pub fn is_complete(&self) -> bool {
        let c = self.counts();
        c.fail == 0 && c.pending == 0
    }

    /// One line per fixture that ran (pass, fail) with details for failures, followed by
    /// the per-family table and the totals. `show_pending` adds a line per pending fixture.
    pub fn render(&self, show_pending: bool) -> String {
        let mut out = String::new();
        for e in &self.entries {
            let name = format!("{}/{}", e.family, e.id);
            match &e.outcome {
                Outcome::Pass => {
                    let _ = writeln!(out, "PASS     {name}");
                }
                Outcome::Fail(detail) => {
                    let _ = writeln!(out, "FAIL     {name}");
                    for line in detail.lines() {
                        let _ = writeln!(out, "           {line}");
                    }
                }
                Outcome::Pending(reason) if show_pending => {
                    let _ = writeln!(out, "PENDING  {name} ({reason})");
                }
                Outcome::Pending(_) => {}
            }
        }
        let _ = writeln!(
            out,
            "\n{:<24} {:>6} {:>6} {:>6} {:>8}",
            "family", "total", "pass", "fail", "pending"
        );
        for family in Family::ALL {
            let c = self.counts_for(family);
            let _ = writeln!(
                out,
                "{:<24} {:>6} {:>6} {:>6} {:>8}",
                family.name(),
                c.total(),
                c.pass,
                c.fail,
                c.pending
            );
        }
        let c = self.counts();
        let _ = writeln!(
            out,
            "\n{} fixtures: {} passed, {} failed, {} pending",
            c.total(),
            c.pass,
            c.fail,
            c.pending
        );
        out
    }
}

/// Runs every case of the suite through the handler registered for its family.
/// Families without a handler are reported as pending. A panic in a handler fails only its case.
pub fn run_suite(suite: &Suite, handlers: &[Box<dyn Handler>]) -> Report {
    let mut entries = Vec::with_capacity(suite.cases.len());
    for case in &suite.cases {
        let handler = handlers.iter().find(|h| h.family() == case.family);
        let outcome = match handler {
            None => Outcome::Pending("no handler is registered for this family".to_owned()),
            Some(handler) => judge(case, handler.as_ref()),
        };
        entries.push(Entry {
            family: case.family,
            id: case.id.clone(),
            outcome,
        });
    }
    Report { entries }
}

fn judge(case: &Case, handler: &dyn Handler) -> Outcome {
    let result = catch_unwind(AssertUnwindSafe(|| handler.run(case))).unwrap_or_else(|panic| {
        Err(HandlerError::Failed(format!(
            "handler panicked: {}",
            panic_text(&panic)
        )))
    });
    match result {
        Err(HandlerError::Pending(reason)) => Outcome::Pending(reason),
        Err(HandlerError::Failed(message)) => Outcome::Fail(message),
        Ok(actual) => compare(case, &actual).map_or(Outcome::Pass, Outcome::Fail),
    }
}

fn panic_text(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-text panic payload".to_owned()
    }
}

/// `None` when the output equals the expected output exactly.
fn compare(case: &Case, actual: &Actual) -> Option<String> {
    match (&case.expected, actual) {
        (Expected::Json(want), Actual::Json(got)) => {
            (want != got).then(|| format!("expected: {want}\nactual:   {got}"))
        }
        (Expected::Files(want), Actual::Files(got)) => diff_files(want, got),
        (Expected::Json(_), Actual::Files(_)) => {
            Some("handler returned files but the fixture expects a JSON result".to_owned())
        }
        (Expected::Files(_), Actual::Json(_)) => {
            Some("handler returned a JSON result but the fixture expects files".to_owned())
        }
    }
}

fn diff_files(want: &BTreeMap<String, Vec<u8>>, got: &BTreeMap<String, Vec<u8>>) -> Option<String> {
    let mut lines = Vec::new();
    for path in want.keys().filter(|p| !got.contains_key(*p)) {
        lines.push(format!("missing file: {path}"));
    }
    for path in got.keys().filter(|p| !want.contains_key(*p)) {
        lines.push(format!("unexpected file: {path}"));
    }
    for (path, want_bytes) in want {
        let Some(got_bytes) = got.get(path) else {
            continue;
        };
        if want_bytes != got_bytes {
            lines.push(format!(
                "{path}: {}",
                first_difference(want_bytes, got_bytes)
            ));
        }
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn first_difference(want: &[u8], got: &[u8]) -> String {
    let want = String::from_utf8_lossy(want);
    let got = String::from_utf8_lossy(got);
    let mut want_lines = want.split('\n');
    let mut got_lines = got.split('\n');
    let mut number = 0;
    loop {
        number += 1;
        match (want_lines.next(), got_lines.next()) {
            (Some(w), Some(g)) if w == g => {}
            (Some(w), Some(g)) => {
                return format!(
                    "line {number} differs\n  expected: {}\n  actual:   {}",
                    show(w),
                    show(g)
                );
            }
            (Some(w), None) => return format!("line {number} missing, expected {}", show(w)),
            (None, Some(g)) => return format!("line {number} unexpected: {}", show(g)),
            (None, None) => return "content differs".to_owned(),
        }
    }
}

fn show(line: &str) -> String {
    let line = line.replace('\r', "\\r");
    if line.chars().count() > 160 {
        let cut: String = line.chars().take(160).collect();
        format!("{cut}...")
    } else {
        line
    }
}
