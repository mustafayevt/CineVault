//! The scenarios of the template-engine spec, run through the public API.

use std::sync::Arc;

use cv_template::{
    DateTime, Engine, Error, HtmlEncoder, JsonEncoder, Object, ParseOptions, Template, Value,
    XmlEncoder,
};

fn movie(title: &str, year: i64, edition: &str) -> Value {
    Value::map([
        ("title", Value::text(title)),
        ("year", Value::Int(year)),
        ("edition", Value::text(edition)),
        ("seasonNr", Value::Int(3)),
    ])
}

fn render(engine: &Engine, template: &str, model: &Value) -> String {
    engine.render_str(template, model).expect("renders")
}

#[test]
fn conditional_segment_leaves_no_dangling_separator() {
    let engine = Engine::standard().zero_is_empty(true);
    let template = "${title}${ - ,edition,} (${year})";
    assert_eq!(
        render(&engine, template, &movie("Heat", 1995, "")),
        "Heat (1995)"
    );
    assert_eq!(
        render(&engine, template, &movie("Heat", 1995, "Final Cut")),
        "Heat - Final Cut (1995)"
    );
}

#[test]
fn foreach_in_an_export_template() {
    let model = Value::map([(
        "movies",
        Value::list([movie("Heat", 1995, ""), movie("Arrival", 2016, "")]),
    )]);
    let engine = Engine::standard();
    assert_eq!(
        render(
            &engine,
            "${foreach movies movie}${movie.title};${end}",
            &model
        ),
        "Heat;Arrival;"
    );
}

#[test]
fn first_letter_bucket() {
    let engine = Engine::standard();
    assert_eq!(
        render(&engine, "${title;first}", &movie("The Matrix", 1999, "")),
        "T"
    );
    assert_eq!(
        render(
            &engine,
            "${title;first}",
            &movie("2001: A Space Odyssey", 1968, "")
        ),
        "#"
    );
}

#[test]
fn zero_padded_number() {
    let engine = Engine::standard();
    assert_eq!(
        render(&engine, "${seasonNr;number(%02d)}", &movie("x", 1, "")),
        "03"
    );
}

#[test]
fn zero_values_render_empty_in_the_renamer_engine() {
    let engine = Engine::standard().zero_is_empty(true);
    assert_eq!(
        render(&engine, "${title} (${year})", &movie("Title", 0, "")),
        "Title ()"
    );
    assert_eq!(
        render(&engine, "${title}${ (,year,)}", &movie("Title", 0, "")),
        "Title"
    );
    // The export engine prints the zero.
    assert_eq!(
        render(
            &Engine::standard(),
            "${title} (${year})",
            &movie("Title", 0, "")
        ),
        "Title (0)"
    );
}

#[test]
fn syntax_errors_report_where() {
    let Err(error) = Template::parse("one\ntwo ${if a}x") else {
        panic!("expected a syntax error")
    };
    assert_eq!((error.line, error.column), (2, 5));
    assert!(error.to_string().contains("line 2, column 5"));
    let engine = Engine::standard();
    let Err(Error::Syntax(error)) = engine.render_str("${title", &Value::Null) else {
        panic!("expected a syntax error")
    };
    assert_eq!(error.offset, 0);
}

#[test]
fn renamer_templates_keep_typed_backslashes() {
    let renamer = ParseOptions {
        backslash_escapes: false,
    };
    let model = movie("Heat", 1995, "");
    let engine = Engine::standard();
    let render_plain = |source: &str| {
        let template = Template::parse_with(source, renamer).unwrap();
        engine.render(&template, &model).unwrap()
    };
    assert_eq!(render_plain(r"\${title}"), r"\Heat");
    assert_eq!(render_plain(r"${title}\${year}"), r"Heat\1995");
    assert_eq!(render_plain(r"a\\b"), r"a\\b");
    // Export templates treat them as escapes.
    assert_eq!(render(&engine, r"\${title} a\\b", &model), r"${title} a\b");
}

#[test]
fn encoders_escape_tokens_only() {
    let model = Value::map([("t", Value::text("Müller & <Söhne> \"x\" / \u{2013}"))]);
    let template = "<b>${t}</b>";
    let html = Engine::standard().with_appender(HtmlEncoder);
    assert_eq!(
        render(&html, template, &model),
        "<b>M&uuml;ller &amp; &lt;S&ouml;hne&gt; &quot;x&quot; / &ndash;</b>"
    );
    let xml = Engine::standard().with_appender(XmlEncoder);
    assert_eq!(
        render(&xml, template, &model),
        "<b>Müller &amp; &lt;Söhne&gt; &quot;x&quot; / –</b>"
    );
    let json = Engine::standard().with_appender(JsonEncoder);
    let unit = |hex: &str| format!("{}u{hex}", '\\');
    assert_eq!(
        render(&json, template, &model),
        format!(
            r#"<b>M{}ller & <S{}hne> \"x\" \/ {}</b>"#,
            unit("00FC"),
            unit("00F6"),
            unit("2013")
        )
    );
}

#[test]
fn dates_use_the_pattern_letters() {
    let model = Value::map([(
        "d",
        Value::DateTime(DateTime::parse("2024-02-29 13:05:09", 0).unwrap()),
    )]);
    let engine = Engine::standard();
    assert_eq!(
        render(&engine, "${d;date(yyyy-MM-dd)}", &model),
        "2024-02-29"
    );
    assert_eq!(
        render(&engine, "${d;date}", &model),
        "29.02.2024 13:05:09 +0000"
    );
}

#[test]
fn chained_renderers_apply_in_order() {
    let engine = Engine::standard();
    let model = movie("the matrix", 1999, "");
    assert_eq!(render(&engine, "${title;upper;first}", &model), "T");
    assert_eq!(
        render(&engine, "${title;title;replace(Matrix,Vault)}", &model),
        "The Vault"
    );
}

#[derive(Debug)]
struct LazyMovie(i64);

impl Object for LazyMovie {
    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "title" => Some(Value::text(format!("Movie {}", self.0))),
            "id" => Some(Value::Int(self.0)),
            _ => None,
        }
    }

    fn display(&self) -> String {
        format!("movie-{}", self.0)
    }
}

#[test]
fn model_objects_are_read_lazily_and_print_themselves() {
    let model = Value::map([(
        "movies",
        Value::list((1..=3).map(|i| Value::Object(Arc::new(LazyMovie(i))))),
    )]);
    let engine = Engine::standard();
    assert_eq!(
        render(
            &engine,
            "${foreach movies m , }${m.title}/${m}${end}",
            &model
        ),
        "Movie 1/movie-1, Movie 2/movie-2, Movie 3/movie-3"
    );
}

#[test]
fn twenty_thousand_items_stream_into_a_writer() {
    let movies = Value::list((0..20_000).map(|i| movie(&format!("Movie {i}"), 2000 + i % 25, "")));
    let model = Value::map([("movies", movies)]);
    let template =
        Template::parse("${foreach movies m}${m.title} (${m.year})${if m.edition}!${end}\n${end}")
            .unwrap();
    let engine = Engine::standard();
    let mut out = String::new();
    engine.render_to(&template, &model, &mut out).unwrap();
    assert_eq!(out.lines().count(), 20_000);
    assert!(out.starts_with("Movie 0 (2000)\n"));
    assert!(out.ends_with("Movie 19999 (2024)\n"));
}

// ---- renamer token aliases -------------------------------------------------------

mod aliases {
    use cv_domain::{
        AudioStream, Edition, Item, ItemDetail, MediaFile, MediaFileType, MediaSource, id_key,
    };
    use cv_template::tokens::{
        ModelOptions, RenamerOptions, movie_aliases, movie_model, render_name,
    };

    /// A 2160p WEB-DL Dolby Vision HDR10 movie with TrueHD Atmos 7.1 audio.
    fn dune() -> Item {
        let mut item = Item::movie("Dune: Part Two");
        item.core.year = 2024;
        item.core.ids.set(id_key::TMDB, "693134");
        let mut video = MediaFile::new(MediaFileType::Video, "/m", "Dune.mkv");
        video.width = 3840;
        video.height = 2160;
        video.hdr_formats = vec!["Dolby Vision".into(), "HDR10".into()];
        video.audio_streams = vec![AudioStream {
            codec: "TrueHD Atmos".into(),
            channels: 8,
            language: "en".into(),
            ..AudioStream::default()
        }];
        item.core.media_files.push(video);
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.media_source = MediaSource::WebDl;
            movie.edition = Edition::ExtendedEdition;
        }
        item
    }

    fn name(template: &str, item: &Item) -> String {
        let model = movie_model(item, None, &ModelOptions::default());
        render_name(
            template,
            &movie_aliases(),
            &model,
            &RenamerOptions::default(),
        )
        .expect("renders")
    }

    const DETAILED: &str = "${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrFormat}] [${audioCodec} ${audioChannelsDot}]";

    #[test]
    fn detailed_template_renders_exactly() {
        assert_eq!(
            name(DETAILED, &dune()),
            "Dune - Part Two (2024) [2160p] [Web-DL] [Dolby Vision, HDR10] [TrueHD Atmos 7.1]"
        );
    }

    #[test]
    fn detailed_template_leaves_empty_brackets_for_missing_data() {
        // The renamer's clean-up removes these; the engine prints nothing for what is missing.
        let mut item = Item::movie("Heat");
        item.core.year = 0;
        assert_eq!(name(DETAILED, &item), "Heat () [] [] [] [ ]");
    }

    #[test]
    fn short_aliases_cover_the_spec_list() {
        let item = dune();
        let render = |t: &str| name(t, &item);
        assert_eq!(render("${title}"), "Dune - Part Two");
        assert_eq!(render("${year}"), "2024");
        assert_eq!(render("${videoFormat}"), "2160p");
        assert_eq!(render("${hdrformat}"), "Dolby Vision, HDR10");
        assert_eq!(render("${audioCodec}"), "TrueHD Atmos");
        assert_eq!(render("${audioChannelsDot}"), "7.1");
        assert_eq!(render("${edition}"), "Extended Edition");
        assert_eq!(render("${mediaSource}"), "Web-DL");
        assert_eq!(render("${imdb}"), "");
        assert_eq!(render("${tmdb}"), "693134");
        assert_eq!(
            render("${title}${ - ,edition,} (${year})"),
            "Dune - Part Two - Extended Edition (2024)"
        );
        assert_eq!(
            render("${title} (${year}) {tmdb-${tmdb}}"),
            "Dune - Part Two (2024) {tmdb-693134}"
        );
    }

    #[test]
    fn the_top_250_position_is_a_token_that_prints_nothing_when_unranked() {
        let mut item = dune();
        assert_eq!(name("${title}${ [#,top250,]}", &item), "Dune - Part Two");
        assert_eq!(name("${top250}", &item), "");
        if let ItemDetail::Movie(movie) = &mut item.detail {
            movie.top250 = 42;
        }
        assert_eq!(name("${top250}", &item), "42");
        assert_eq!(
            name("${title}${ [#,top250,]}", &item),
            "Dune - Part Two [#42]"
        );
        assert_eq!(name("${movie.top250}", &item), "42");
    }

    #[test]
    fn missing_year_renders_empty_brackets() {
        let item = Item::movie("Title");
        assert_eq!(name("${title} (${year})", &item), "Title ()");
        assert_eq!(name("${title}${ (,year,)}", &item), "Title");
    }
}

// ---- validation -------------------------------------------------------------------

mod validation {
    use cv_template::tokens::{movie_validator, tv_validator};
    use cv_template::{DiagnosticKind, Engine, Severity};

    #[test]
    fn typo_in_token_is_marked_and_corrected() {
        let engine = Engine::standard();
        let source = "${titel} (${year})";
        let report = movie_validator(&engine).validate(source);
        assert_eq!(report.diagnostics.len(), 1);
        let unknown = &report.diagnostics[0];
        assert_eq!(unknown.kind, DiagnosticKind::UnknownToken);
        assert_eq!(
            &source[unknown.offset..unknown.offset + unknown.len],
            "titel"
        );
        assert_eq!(
            unknown.suggestions.first().map(String::as_str),
            Some("title")
        );
    }

    #[test]
    fn syntax_errors_say_where() {
        let engine = Engine::standard();
        let report = movie_validator(&engine).validate("${title}\n${year");
        let error = &report.diagnostics[0];
        assert_eq!(error.kind, DiagnosticKind::Syntax);
        assert_eq!((error.line, error.column, error.offset), (2, 1, 9));
        assert_eq!(error.severity, Severity::Error);
    }

    #[test]
    fn tv_file_pattern_rules() {
        let engine = Engine::standard();
        let validator = tv_validator(&engine);
        let ok = validator.check_tv_patterns(
            Some("Season ${seasonNr}"),
            "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}",
        );
        assert!(ok.is_recommended());

        let no_episode = validator.check_tv_patterns(None, "${showTitle} S${seasonNr2}");
        assert_eq!(
            no_episode.file.diagnostics[0].kind,
            DiagnosticKind::TvEpisodeToken
        );
        let wrong_order = validator.check_tv_patterns(None, "E${episodeNr2} S${seasonNr2}");
        assert_eq!(
            wrong_order.file.diagnostics[0].kind,
            DiagnosticKind::TvTokenOrder
        );
        assert!(!wrong_order.is_recommended());
    }
}
