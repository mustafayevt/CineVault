//! How a report is printed: as plain text for a person, or as JSON for a script.

use std::fmt::Write as _;

use cv_app::automation::{BatchReport, CredentialSummary, StepReport, StepStatus};

/// The report as one JSON document.
#[must_use]
pub fn json(report: &BatchReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|error| {
        format!("{{\"error\":\"the report could not be written: {error}\"}}")
    })
}

/// The provider credential list as one JSON document.
#[must_use]
pub fn providers_json(providers: &[CredentialSummary]) -> String {
    serde_json::to_string_pretty(providers).unwrap_or_else(|_| "[]".to_owned())
}

/// The provider credential list as a table.
#[must_use]
pub fn providers_text(providers: &[CredentialSummary]) -> String {
    let width = providers
        .iter()
        .map(|p| p.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for provider in providers {
        let state = match provider.state.as_str() {
            "active" => "active",
            "rejected" => "rejected",
            _ => "missing",
        };
        let _ = writeln!(
            out,
            "{:<width$}  {:<12}  {state}",
            provider.name, provider.importance
        );
    }
    out
}

fn status_word(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Done => "done",
        StepStatus::Planned => "planned",
        StepStatus::Skipped => "skipped",
        StepStatus::Unavailable => "unavailable",
        StepStatus::Failed => "FAILED",
    }
}

fn step_text(out: &mut String, step: &StepReport) {
    let _ = writeln!(
        out,
        "[{}] {}: {}",
        step.step,
        status_word(step.status),
        step.message
    );
    for item in &step.items {
        let year = if item.year > 0 {
            format!(" ({})", item.year)
        } else {
            String::new()
        };
        let _ = writeln!(out, "    {:<12} {}{year}", item.status, item.title);
        if let (Some(from), Some(to)) = (&item.from, &item.to)
            && from != to
        {
            let _ = writeln!(out, "                 {from}  ->  {to}");
        }
        if let (Some(from), Some(to)) = (&item.from_file, &item.to_file)
            && from != to
        {
            let _ = writeln!(out, "                 {from}  ->  {to}");
        }
        if let Some(message) = &item.message {
            let _ = writeln!(out, "                 {message}");
        }
    }
    if let Some(undo) = &step.undo {
        let _ = writeln!(out, "    can be undone in the app (batch {undo})");
    }
}

/// The report as text for a terminal.
#[must_use]
pub fn text(report: &BatchReport) -> String {
    let mut out = String::new();
    if let Some(error) = &report.error {
        let _ = writeln!(out, "{error}");
        return out;
    }
    let _ = writeln!(
        out,
        "{} batch{}{}",
        report.kind.name(),
        if report.dry_run {
            " (dry run: nothing was changed)"
        } else {
            ""
        },
        if report.forwarded {
            ", run by the open CineVault app"
        } else {
            ""
        }
    );
    for step in &report.steps {
        step_text(&mut out, step);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cv_app::automation::{BatchRequest, EntityKind, ItemResult, SelectionSummary, Step};

    fn sample() -> BatchReport {
        let mut step = StepReport::new(Step::Rename, StepStatus::Planned, "Would rename: 1.");
        let mut item = ItemResult::new(Some(1), "Alien", 1979, "rename");
        item.from = Some("D:\\Movies\\alien".into());
        item.to = Some("D:\\Movies\\Alien (1979)".into());
        step.items.push(item);
        BatchReport {
            kind: EntityKind::Movie,
            dry_run: true,
            forwarded: false,
            selection: SelectionSummary::default(),
            steps: vec![step],
            credentials: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn the_text_report_shows_the_plan_and_says_nothing_changed() {
        let text = text(&sample());
        assert!(text.contains("dry run: nothing was changed"), "{text}");
        assert!(text.contains("[rename] planned"), "{text}");
        assert!(
            text.contains("D:\\Movies\\alien  ->  D:\\Movies\\Alien (1979)"),
            "{text}"
        );
    }

    #[test]
    fn a_batch_that_could_not_start_prints_only_the_reason() {
        let request = BatchRequest::new(EntityKind::Movie);
        let report = BatchReport::failed_to_start(&request, "No step was asked for.");
        assert_eq!(text(&report), "No step was asked for.\n");
        assert!(json(&report).contains("\"error\""));
    }

    #[test]
    fn the_json_report_carries_the_per_item_results() {
        let value: serde_json::Value = serde_json::from_str(&json(&sample())).unwrap();
        assert_eq!(value["steps"][0]["step"], "rename");
        assert_eq!(
            value["steps"][0]["items"][0]["to"],
            "D:\\Movies\\Alien (1979)"
        );
        assert_eq!(value["dry_run"], true);
    }
}
