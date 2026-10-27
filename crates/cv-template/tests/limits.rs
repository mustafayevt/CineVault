//! Bounded and streaming evaluation (template-engine spec, "Safe evaluation").

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cv_template::tokens::{RenamerOptions, renamer_engine};
use cv_template::{Engine, Limits, Object, RenderError, Template, Value};

/// A library item that builds its fields when a template asks for them, so a list of 20,000
/// of them costs nothing until it is walked.
#[derive(Debug)]
struct LazyMovie {
    n: i64,
    reads: Arc<AtomicUsize>,
}

impl Object for LazyMovie {
    fn get(&self, key: &str) -> Option<Value> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        match key {
            "title" => Some(Value::text(format!("Movie {}", self.n))),
            "year" => Some(Value::Int(1950 + self.n % 75)),
            "edition" => Some(Value::text(if self.n % 10 == 0 { "Extended" } else { "" })),
            _ => None,
        }
    }
}

fn library(count: i64) -> (Value, Arc<AtomicUsize>) {
    let reads = Arc::new(AtomicUsize::new(0));
    let movies = Value::list((0..count).map(|n| {
        Value::object(LazyMovie {
            n,
            reads: Arc::clone(&reads),
        })
    }));
    (Value::map([("movies", movies)]), reads)
}

const EXPORT_ROW: &str = "${foreach movies m}${m.title}${ (,m.year,)}${ - ,m.edition,}\n${end}";

/// Counts what is written and remembers the biggest single write; it never stores the text.
#[derive(Default)]
struct Counting {
    bytes: usize,
    writes: usize,
    biggest: usize,
}

impl fmt::Write for Counting {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.bytes += s.len();
        self.writes += 1;
        self.biggest = self.biggest.max(s.len());
        Ok(())
    }
}

#[test]
fn twenty_thousand_items_stream_without_collecting() {
    let (model, reads) = library(20_000);
    let template = Template::parse(EXPORT_ROW).unwrap();
    let mut sink = Counting::default();
    Engine::standard()
        .render_to(&template, &model, &mut sink)
        .unwrap();
    // 20,000 rows, each written in pieces: the engine never hands over the whole result.
    assert!(sink.writes > 20_000 * 3);
    assert!(sink.biggest < 64, "biggest write: {}", sink.biggest);
    assert!(sink.bytes > 20_000 * 15);
    // Every field was read lazily, once per use.
    assert!(reads.load(Ordering::Relaxed) >= 20_000 * 3);
}

#[test]
fn twenty_thousand_items_render_to_the_exact_text() {
    let (model, _) = library(20_000);
    let engine = Engine::standard().zero_is_empty(true);
    let template = Template::parse(EXPORT_ROW).unwrap();
    let text = engine.render(&template, &model).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("Movie 0 (1950) - Extended"));
    assert_eq!(lines.next(), Some("Movie 1 (1951)"));
    assert_eq!(text.lines().count(), 20_000);
    assert!(text.ends_with("Movie 19999 (1999)\n"));
}

#[test]
fn twenty_thousand_items_stream_into_an_io_writer() {
    struct Bytes(usize);
    impl std::io::Write for Bytes {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (model, _) = library(20_000);
    let template = Template::parse(EXPORT_ROW).unwrap();
    let mut sink = Bytes(0);
    Engine::standard()
        .render_to_io(&template, &model, &mut sink)
        .unwrap();
    assert!(sink.0 > 20_000 * 15);
}

#[test]
fn a_sink_that_refuses_stops_the_render_early() {
    struct Refuses(usize);
    impl fmt::Write for Refuses {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            if self.0 < s.len() {
                return Err(fmt::Error);
            }
            self.0 -= s.len();
            Ok(())
        }
    }
    let (model, reads) = library(20_000);
    let template = Template::parse(EXPORT_ROW).unwrap();
    let error = Engine::standard()
        .render_to(&template, &model, &mut Refuses(500))
        .unwrap_err();
    assert_eq!(error, RenderError::Write);
    assert!(reads.load(Ordering::Relaxed) < 1_000);
}

#[test]
fn output_size_is_bounded() {
    let (model, reads) = library(20_000);
    let template = Template::parse(EXPORT_ROW).unwrap();
    let engine = Engine::standard().with_limits(Limits {
        max_output_bytes: 1_000,
        ..Limits::UNBOUNDED
    });
    let mut sink = Counting::default();
    let error = engine.render_to(&template, &model, &mut sink).unwrap_err();
    assert_eq!(error, RenderError::OutputTooLarge { limit: 1_000 });
    assert!(sink.bytes <= 1_000);
    // It stopped at the limit; it did not walk the other 19,900 items first.
    assert!(reads.load(Ordering::Relaxed) < 1_000);
    // The one-call helpers report the same error.
    assert_eq!(
        engine.render(&template, &model).unwrap_err(),
        RenderError::OutputTooLarge { limit: 1_000 }
    );
    assert!(engine.render_str("short", &model).is_ok());
}

#[test]
fn work_is_bounded_even_when_nothing_is_written() {
    // 3,000 x 3,000 iterations that print nothing.
    let rows = Value::list((0..3_000).map(Value::Int));
    let model = Value::map([("rows", rows)]);
    let template = Template::parse("${foreach rows a}${foreach rows b}${end}${end}").unwrap();
    let engine = Engine::standard().with_limits(Limits {
        max_steps: 50_000,
        ..Limits::UNBOUNDED
    });
    assert_eq!(
        engine.render(&template, &model).unwrap_err(),
        RenderError::TooManySteps { limit: 50_000 }
    );
}

#[test]
fn time_is_bounded() {
    let rows = Value::list((0..3_000).map(Value::Int));
    let model = Value::map([("rows", rows)]);
    let template = Template::parse("${foreach rows a}${foreach rows b}${end}${end}").unwrap();
    let engine = Engine::standard().with_limits(Limits {
        max_duration: Some(Duration::ZERO),
        ..Limits::UNBOUNDED
    });
    assert_eq!(
        engine.render(&template, &model).unwrap_err(),
        RenderError::TimedOut
    );
    // A generous limit does not get in the way.
    let engine = Engine::standard().with_limits(Limits {
        max_duration: Some(Duration::from_secs(60)),
        ..Limits::UNBOUNDED
    });
    let small = Template::parse("${foreach rows a}x${end}").unwrap();
    assert_eq!(engine.render(&small, &model).unwrap().len(), 3_000);
}

#[test]
fn the_default_limits_allow_a_big_export_and_naming_is_tighter() {
    assert_eq!(Engine::new().limits(), Limits::EXPORT);
    let (model, _) = library(20_000);
    let template = Template::parse(EXPORT_ROW).unwrap();
    assert!(Engine::standard().render(&template, &model).is_ok());

    let naming = renamer_engine(&RenamerOptions::default());
    assert_eq!(naming.limits(), Limits::NAMING);
    assert!(naming.limits().max_output_bytes < Limits::EXPORT.max_output_bytes);
}

#[test]
fn parsing_depth_is_bounded_too() {
    let deep = "${if a}".repeat(1_000);
    assert!(Template::parse(&deep).is_err());
}
