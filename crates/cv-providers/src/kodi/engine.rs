//! The ScraperXML interpreter: numbered text buffers and nested `<RegExp>` rules.
//!
//! A scraper function (`CreateSearchUrl`, `GetSearchResults`, `GetDetails`, ...) is a list of
//! `<RegExp>` elements. Every rule:
//!
//! 1. runs the rules nested inside it first, in order;
//! 2. reads its `input` (default `$$1`), where `$$n` is the text in buffer `n` (1 to 20);
//! 3. matches its `<expression>` against the input and builds its `output` for each match
//!    (once, or for every match with `repeat="yes"`), replacing `\1` to `\9` with the captured
//!    groups, `\0` with the whole match, `$$n` with buffers and `$INFO[name]` with settings;
//! 4. stores the result in the buffer named by `dest` (default 1); `dest="n+"` appends instead
//!    of replacing.
//!
//! Expression options: `noclean="1,2"` keeps the listed groups as captured (all others have
//! their HTML tags removed and are trimmed), `trim="1"`, `fixchars="1"` (decode HTML entities)
//! and `encode="1"` (URL-encode) apply to the listed groups, `cs="yes"` makes the match case
//! sensitive, `optional="n"` keeps an `\(...\)` part of the output only when group `n` is not
//! empty, and `compare="n"` keeps a result only when it contains the text of buffer `n`. On a
//! rule, `clear="yes"` empties the destination even when nothing matches, and
//! `conditional="name"` (or `"!name"`) runs the rule only when the setting is (is not) `true`.
//!
//! A function's `dest` attribute names the buffer its answer is read from. Unless the function
//! says `clearbuffers="no"`, all buffers are emptied after it ran.

use fancy_regex::Regex;

use super::pattern;
use super::scraper::{KodiError, KodiSettings, ScraperDef};
use super::text::{decode_html, strip_tags, url_encode};
use super::xml::Element;

/// Number of buffers (`$$1` to `$$20`).
pub(crate) const BUFFERS: usize = 20;
/// How deeply `<RegExp>` rules may nest.
const MAX_DEPTH: usize = 32;
/// Matches one `repeat="yes"` expression may take from one input.
const MAX_MATCHES: usize = 100_000;
/// The largest a buffer may grow.
const MAX_BUFFER_BYTES: usize = 32 * 1024 * 1024;

pub(crate) struct Interpreter<'a> {
    def: &'a ScraperDef,
    settings: &'a KodiSettings,
    buffers: Vec<String>,
}

/// Which captured groups get which clean-up. Index 0 is the whole match.
struct GroupFlags {
    clean: [bool; 10],
    trim: [bool; 10],
    fixchars: [bool; 10],
    encode: [bool; 10],
}

fn flag_list(attr: Option<&str>, default: bool) -> [bool; 10] {
    let mut flags = [default; 10];
    for index in attr.unwrap_or_default().split(',') {
        if let Ok(n) = index.trim().parse::<usize>()
            && n < flags.len()
        {
            flags[n] = !default;
        }
    }
    flags
}

impl GroupFlags {
    fn of(expression: Option<&Element>) -> Self {
        let attr = |name: &str| expression.and_then(|e| e.attr(name));
        Self {
            clean: flag_list(attr("noclean"), true),
            trim: flag_list(attr("trim"), false),
            fixchars: flag_list(attr("fixchars"), false),
            encode: flag_list(attr("encode"), false),
        }
    }

    fn apply(&self, n: usize, captured: &str) -> String {
        let mut value = captured.to_owned();
        if self.clean[n] {
            value = strip_tags(&value).trim().to_owned();
        }
        if self.trim[n] {
            value = value.trim().to_owned();
        }
        if self.fixchars[n] {
            value = decode_html(&value);
        }
        if self.encode[n] {
            value = url_encode(&value);
        }
        value
    }
}

fn is_yes(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.eq_ignore_ascii_case("yes") || v.eq_ignore_ascii_case("true"))
}

/// Reads `3` or `3+` from a `dest` attribute: the 0-based buffer and whether to append.
fn parse_dest(value: Option<&str>) -> Option<(usize, bool)> {
    let value = value.unwrap_or("1").trim();
    let append = value.ends_with('+');
    let n: usize = value.trim_end_matches('+').trim().parse().ok()?;
    (1..=BUFFERS).contains(&n).then_some((n - 1, append))
}

impl<'a> Interpreter<'a> {
    pub fn new(def: &'a ScraperDef, settings: &'a KodiSettings) -> Self {
        Self {
            def,
            settings,
            buffers: vec![String::new(); BUFFERS],
        }
    }

    /// Puts `values` into buffers 1, 2, ... (the fetched pages and extra parameters of a call).
    pub fn set_inputs(&mut self, values: &[String]) {
        for (slot, value) in self.buffers.iter_mut().zip(values) {
            slot.clone_from(value);
        }
    }

    /// Runs a function and returns the buffer its `dest` names.
    pub fn run(&mut self, function: &str) -> Result<String, KodiError> {
        let def = self.def;
        let element = def
            .root
            .first(function)
            .ok_or_else(|| KodiError::MissingFunction(function.to_owned()))?;
        let (dest, _) = parse_dest(element.attr("dest")).unwrap_or((0, false));
        self.run_rules(element, 0)?;
        let answer = std::mem::take(&mut self.buffers[dest]);
        let keep = element
            .attr("clearbuffers")
            .is_some_and(|v| v.eq_ignore_ascii_case("no"));
        if keep {
            self.buffers[dest].clone_from(&answer);
        } else {
            self.buffers.iter_mut().for_each(String::clear);
        }
        Ok(answer)
    }

    fn run_rules(&mut self, parent: &Element, depth: usize) -> Result<(), KodiError> {
        if depth > MAX_DEPTH {
            return Err(KodiError::Limit("rules nested too deeply".to_owned()));
        }
        for rule in parent.elements("RegExp") {
            if let Some(condition) = rule.attr("conditional")
                && !self.condition_holds(condition)
            {
                continue;
            }
            self.run_rules(rule, depth + 1)?;
            self.run_rule(rule)?;
        }
        Ok(())
    }

    fn condition_holds(&self, condition: &str) -> bool {
        let (negated, name) = match condition.trim().strip_prefix('!') {
            Some(name) => (true, name),
            None => (false, condition.trim()),
        };
        let is_true = self.settings.get(name) == Some("true");
        is_true != negated
    }

    fn run_rule(&mut self, rule: &Element) -> Result<(), KodiError> {
        let Some((dest, append)) = parse_dest(rule.attr("dest")) else {
            tracing::warn!("a scraper rule names a buffer outside 1 to {BUFFERS}; skipped");
            return Ok(());
        };
        let input = self.expand(rule.attr("input").unwrap_or("$$1"), None);
        let output = rule.attr("output").unwrap_or("\\1");
        self.apply_expression(&input, dest, append, rule, output)
    }

    fn apply_expression(
        &mut self,
        input: &str,
        dest: usize,
        mut append: bool,
        rule: &Element,
        output: &str,
    ) -> Result<(), KodiError> {
        let expression = rule.first("expression");
        let pattern_text = expression
            .map(Element::text)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "(.*)".to_owned());
        let pattern_text = self.expand(&pattern_text, None);
        let case_sensitive = is_yes(expression.and_then(|e| e.attr("cs")));
        let regex: Regex = match pattern::compile(&pattern_text, case_sensitive) {
            Ok(regex) => regex,
            Err(error) => {
                tracing::warn!(%error, "a scraper expression does not compile; the rule is skipped");
                return Ok(());
            }
        };
        let repeat = is_yes(expression.and_then(|e| e.attr("repeat")));
        if is_yes(rule.attr("clear")) {
            self.buffers[dest].clear();
        }
        let flags = GroupFlags::of(expression);
        let optional: Option<usize> = expression
            .and_then(|e| e.attr("optional"))
            .and_then(|v| v.trim().parse().ok());
        let compare: Option<String> = expression
            .and_then(|e| e.attr("compare"))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|n| (1..=BUFFERS).contains(n))
            .map(|n| self.buffers[n - 1].to_lowercase());

        let mut current = input;
        for _ in 0..MAX_MATCHES {
            let captures = match regex.captures(current) {
                Ok(Some(captures)) => captures,
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(%error, "a scraper expression gave up while matching");
                    break;
                }
            };
            let Some(whole) = captures.get(0) else {
                break;
            };
            if whole.start() >= current.len() && !current.is_empty() {
                break;
            }
            if !append {
                append = true;
                self.buffers[dest].clear();
            }
            let group = |n: usize| -> String {
                let captured = captures.get(n).map_or("", |m| m.as_str());
                flags.apply(n.min(9), captured)
            };
            let template = match optional {
                Some(n) => apply_optional(output, !group(n).is_empty()),
                None => output.to_owned(),
            };
            let result = self.expand(&template, Some(&group));
            if !result.is_empty() {
                let keep = compare
                    .as_ref()
                    .is_none_or(|wanted| result.to_lowercase().contains(wanted.as_str()));
                if keep {
                    let buffer = &mut self.buffers[dest];
                    buffer.push_str(&result);
                    if buffer.len() > MAX_BUFFER_BYTES {
                        return Err(KodiError::Limit("a buffer grew too large".to_owned()));
                    }
                }
            }
            if repeat && !whole.as_str().is_empty() {
                current = &current[whole.end()..];
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Replaces `$$n`, `$INFO[name]`, `$LOCALIZE[n]` and, when `groups` is given, `\0` to `\9`.
    /// Replaced text is not scanned again.
    fn expand(&self, template: &str, groups: Option<&dyn Fn(usize) -> String>) -> String {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(c) = rest.chars().next() {
            if let Some(groups) = groups
                && c == '\\'
                && let Some(d) = rest[1..].chars().next().and_then(|d| d.to_digit(10))
            {
                out.push_str(&groups(d as usize));
                rest = &rest[2..];
            } else if let Some(after) = rest.strip_prefix("$$") {
                match buffer_reference(after) {
                    Some((n, len)) => {
                        out.push_str(&self.buffers[n - 1]);
                        rest = &after[len..];
                    }
                    None => {
                        out.push_str("$$");
                        rest = after;
                    }
                }
            } else if let Some(after) = rest.strip_prefix("$INFO[")
                && let Some(end) = after.find(']')
            {
                let mut parts = after[..end].splitn(3, ',');
                let name = parts.next().unwrap_or_default();
                let value = self.settings.get(name).unwrap_or_default();
                if !value.is_empty() {
                    out.push_str(parts.next().unwrap_or_default());
                    out.push_str(value);
                    out.push_str(parts.next().unwrap_or_default());
                }
                rest = &after[end + 1..];
            } else if let Some(after) = rest.strip_prefix("$LOCALIZE[")
                && let Some(end) = after.find(']')
            {
                rest = &after[end + 1..];
            } else {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        out
    }
}

/// The buffer number at the start of `text` (after `$$`) and the digits it used. Two digits
/// count when they name a buffer (10 to 20).
fn buffer_reference(text: &str) -> Option<(usize, usize)> {
    let digits: String = text
        .chars()
        .take(2)
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.len() == 2
        && let Ok(n) = digits.parse::<usize>()
        && (1..=BUFFERS).contains(&n)
    {
        return Some((n, 2));
    }
    let first = digits.chars().next()?;
    let n = first.to_digit(10)? as usize;
    (1..=9).contains(&n).then_some((n, 1))
}

/// Resolves the `\(` ... `\)` parts of an output template: kept (without the markers) when
/// `present`, dropped with their content otherwise.
fn apply_optional(template: &str, present: bool) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("\\(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("\\)") else {
            out.push_str(&rest[start..]);
            return out;
        };
        if present {
            out.push_str(&after[..end]);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(scraper: &str, function: &str, inputs: &[&str]) -> String {
        run_with(scraper, function, inputs, &KodiSettings::new())
    }

    fn run_with(scraper: &str, function: &str, inputs: &[&str], settings: &KodiSettings) -> String {
        let def = ScraperDef::parse(&format!("<scraper>{scraper}</scraper>")).unwrap();
        let mut interpreter = Interpreter::new(&def, settings);
        let inputs: Vec<String> = inputs.iter().map(|s| (*s).to_owned()).collect();
        interpreter.set_inputs(&inputs);
        interpreter.run(function).unwrap()
    }

    #[test]
    fn a_rule_builds_its_output_from_groups() {
        let out = run(
            r#"<F dest="2"><RegExp input="$$1" output="[\1|\2]" dest="2"><expression>(\w+)-(\w+)</expression></RegExp></F>"#,
            "F",
            &["alpha-beta"],
        );
        assert_eq!(out, "[alpha|beta]");
    }

    #[test]
    fn nested_rules_run_first_and_feed_the_outer_rule() {
        let out = run(
            r#"<F dest="3">
                 <RegExp input="$$2" output="&lt;all&gt;\1&lt;/all&gt;" dest="3">
                   <RegExp input="$$1" output="&lt;i&gt;\1&lt;/i&gt;" dest="2+">
                     <expression repeat="yes">\[(\w+)\]</expression>
                   </RegExp>
                   <expression noclean="1"/>
                 </RegExp>
               </F>"#,
            "F",
            &["[a] and [b] and [c]"],
        );
        assert_eq!(out, "<all><i>a</i><i>b</i><i>c</i></all>");
    }

    #[test]
    fn a_non_appending_destination_is_replaced_on_the_first_match_only() {
        let out = run(
            r#"<F dest="2"><RegExp input="$$1" output="\1;" dest="2"><expression repeat="yes">(\d)</expression></RegExp></F>"#,
            "F",
            &["1 2 3"],
        );
        assert_eq!(out, "1;2;3;");
    }

    #[test]
    fn no_match_leaves_the_destination_unless_clear_is_set() {
        let keep = run(
            r#"<F dest="2"><RegExp input="$$1" output="x" dest="2"><expression>nope</expression></RegExp></F>"#,
            "F",
            &["abc", "old"],
        );
        assert_eq!(keep, "old");
        let cleared = run(
            r#"<F dest="2"><RegExp input="$$1" output="x" dest="2" clear="yes"><expression>nope</expression></RegExp></F>"#,
            "F",
            &["abc", "old"],
        );
        assert_eq!(cleared, "");
    }

    #[test]
    fn group_clean_up_options() {
        let page = "<b> Tom &amp; Jerry </b> a b";
        let cleaned = run(
            r#"<F dest="2"><RegExp input="$$1" output="\1" dest="2"><expression>(.*) a b</expression></RegExp></F>"#,
            "F",
            &[page],
        );
        assert_eq!(cleaned, "Tom &amp; Jerry");
        let decoded = run(
            r#"<F dest="2"><RegExp input="$$1" output="\1" dest="2"><expression fixchars="1">(.*) a b</expression></RegExp></F>"#,
            "F",
            &[page],
        );
        assert_eq!(decoded, "Tom & Jerry");
        let raw = run(
            r#"<F dest="2"><RegExp input="$$1" output="\1" dest="2"><expression noclean="1">(.*) a b</expression></RegExp></F>"#,
            "F",
            &[page],
        );
        assert_eq!(raw, "<b> Tom &amp; Jerry </b>");
        let encoded = run(
            r#"<F dest="2"><RegExp input="$$1" output="q=\1" dest="2"><expression encode="1">(.*)</expression></RegExp></F>"#,
            "F",
            &["a b&c"],
        );
        assert_eq!(encoded, "q=a+b%26c");
    }

    #[test]
    fn an_ampersand_in_the_output_is_literal() {
        let out = run(
            r#"<F dest="2"><RegExp input="$$1" output="a=\1&amp;b=2" dest="2"><expression>(.*)</expression></RegExp></F>"#,
            "F",
            &["1"],
        );
        assert_eq!(out, "a=1&b=2");
    }

    #[test]
    fn buffers_in_output_and_two_digit_references() {
        let out = run(
            r#"<F dest="3"><RegExp input="$$1" output="$$2-\1-$$10" dest="3"><expression>(.)</expression></RegExp></F>"#,
            "F",
            &["x", "two", "", "", "", "", "", "", "", "ten"],
        );
        assert_eq!(out, "two-x-ten");
    }

    #[test]
    fn settings_and_conditionals() {
        let mut settings = KodiSettings::new();
        settings.set("lang", "de");
        settings.set("fast", "true");
        let scraper = r#"<F dest="2">
              <RegExp input="$$1" output="A" dest="2"><expression>.*</expression></RegExp>
              <RegExp conditional="fast" input="$$1" output="B" dest="2+"><expression>.*</expression></RegExp>
              <RegExp conditional="!fast" input="$$1" output="C" dest="2+"><expression>.*</expression></RegExp>
              <RegExp input="$$1" output="$INFO[lang,-,!]" dest="2+"><expression>.*</expression></RegExp>
              <RegExp input="$$1" output="$INFO[missing,-,!]" dest="2+"><expression>.*</expression></RegExp>
            </F>"#;
        assert_eq!(run_with(scraper, "F", &["x"], &settings), "AB-de!");
    }

    #[test]
    fn optional_parts_and_compare() {
        let scraper = r#"<F dest="2">
              <RegExp input="$$1" output="t=\1\(&amp;y=\2\)" dest="2"><expression optional="2">(\w+)(?: (\d+))?</expression></RegExp>
            </F>"#;
        assert_eq!(run(scraper, "F", &["heat 1995"]), "t=heat&y=1995");
        assert_eq!(run(scraper, "F", &["heat"]), "t=heat");

        let compared = r#"<F dest="2">
              <RegExp input="$$1" output="&lt;m&gt;\1&lt;/m&gt;" dest="2"><expression repeat="yes" compare="3">(\w+)</expression></RegExp>
            </F>"#;
        assert_eq!(
            run(compared, "F", &["Alpha Beta Gamma", "", "BETA"]),
            "<m>Beta</m>"
        );
    }

    #[test]
    fn functions_clear_buffers_unless_told_not_to() {
        let def = ScraperDef::parse(
            r#"<scraper>
                 <A dest="2" clearbuffers="no"><RegExp input="$$1" output="kept" dest="5"><expression>.*</expression></RegExp>
                   <RegExp input="$$1" output="a" dest="2"><expression>.*</expression></RegExp></A>
                 <B dest="2"><RegExp input="$$5" output="\1" dest="2"><expression>(.*)</expression></RegExp></B>
               </scraper>"#,
        )
        .unwrap();
        let settings = KodiSettings::new();
        let mut interpreter = Interpreter::new(&def, &settings);
        interpreter.set_inputs(&["x".to_owned()]);
        assert_eq!(interpreter.run("A").unwrap(), "a");
        assert_eq!(interpreter.run("B").unwrap(), "kept");
        // B cleared everything, so a second run sees nothing.
        assert_eq!(interpreter.run("B").unwrap(), "");
    }

    #[test]
    fn a_missing_function_is_an_error() {
        let def = ScraperDef::parse("<scraper/>").unwrap();
        let settings = KodiSettings::new();
        let mut interpreter = Interpreter::new(&def, &settings);
        assert_eq!(
            interpreter.run("GetDetails"),
            Err(KodiError::MissingFunction("GetDetails".to_owned()))
        );
    }

    #[test]
    fn a_bad_expression_skips_its_rule_and_the_rest_still_runs() {
        let out = run(
            r#"<F dest="2">
                 <RegExp input="$$1" output="never" dest="2"><expression>(broken</expression></RegExp>
                 <RegExp input="$$1" output="ok" dest="2+"><expression>.*</expression></RegExp>
               </F>"#,
            "F",
            &["x"],
        );
        assert_eq!(out, "ok");
    }

    #[test]
    fn runaway_nesting_is_refused() {
        let mut nested = String::from("<F dest=\"2\">");
        for _ in 0..40 {
            nested.push_str("<RegExp input=\"$$1\" output=\"x\" dest=\"2\">");
        }
        for _ in 0..40 {
            nested.push_str("</RegExp>");
        }
        nested.push_str("</F>");
        let def = ScraperDef::parse(&format!("<scraper>{nested}</scraper>")).unwrap();
        let settings = KodiSettings::new();
        let mut interpreter = Interpreter::new(&def, &settings);
        assert!(matches!(interpreter.run("F"), Err(KodiError::Limit(_))));
    }
}
