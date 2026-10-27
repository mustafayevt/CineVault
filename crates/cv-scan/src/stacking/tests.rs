//! Stacking rules. The byte-exact fixtures live in `tests/compat` (`stacking`
//! family) and run through `cv-compat`; these tests pin the rules and the edge cases next to the
//! code.

use super::*;

fn file(name: &str) -> (String, u32, String, String) {
    let s = parse_file_name(name);
    (s.marker, s.number, s.prefix, s.cleaned)
}

fn tuple(marker: &str, number: u32, prefix: &str, cleaned: &str) -> (String, u32, String, String) {
    (marker.into(), number, prefix.into(), cleaned.into())
}

#[test]
fn words_with_digits_in_any_delimiter() {
    assert_eq!(file("Movie.cd1.avi"), tuple("cd1", 1, "cd", "Movie.avi"));
    assert_eq!(file("Movie-CD2.avi"), tuple("CD2", 2, "CD", "Movie.avi"));
    assert_eq!(file("Movie_dvd1.iso"), tuple("dvd1", 1, "dvd", "Movie.iso"));
    assert_eq!(
        file("Movie part10.avi"),
        tuple("part10", 10, "part", "Movie.avi")
    );
    assert_eq!(file("Movie.pt3.mkv"), tuple("pt3", 3, "pt", "Movie.mkv"));
    assert_eq!(
        file("Movie.disk2.avi"),
        tuple("disk2", 2, "disk", "Movie.avi")
    );
}

#[test]
fn marker_text_is_removed_with_one_delimiter_run_before_it() {
    assert_eq!(file("Movie (2010) cd1.avi").3, "Movie (2010).avi");
    assert_eq!(file("Movie (2010) - cd1.avi").3, "Movie (2010) -.avi");
    assert_eq!(
        file("Movie (2010) part1 - Subtitle.avi").3,
        "Movie (2010) - Subtitle.avi"
    );
    assert_eq!(file("Movie.cd1.en.srt").3, "Movie.en.srt");
    assert_eq!(file("Movie-part1-poster.jpg").3, "Movie-poster.jpg");
}

#[test]
fn marker_at_the_start_of_the_name() {
    assert_eq!(file("cd1.avi"), tuple("cd1", 1, "cd", ".avi"));
    assert_eq!(file("dvd1 - something.iso").3, " - something.iso");
}

#[test]
fn digits_are_one_to_ninety_nine_without_a_leading_zero() {
    assert!(!parse_file_name("Movie.cd0.avi").is_stacked());
    assert!(!parse_file_name("Movie.cd100.avi").is_stacked());
    assert!(!parse_file_name("Movie.Disc03.avi").is_stacked());
    assert_eq!(parse_file_name("Movie.cd10.avi").number, 10);
}

#[test]
fn letter_markers_after_a_word_have_no_number() {
    assert_eq!(file("Movie.cda.avi"), tuple("cda", 0, "cda", "Movie.avi"));
    assert_eq!(
        file("Movie.disca.iso"),
        tuple("disca", 0, "disca", "Movie.iso")
    );
}

#[test]
fn single_letter_a_to_d_before_the_extension() {
    assert_eq!(file("Movie-a.avi"), tuple("a", 1, "", "Movie.avi"));
    assert_eq!(file("Movie_c.avi"), tuple("c", 3, "", "Movie.avi"));
    assert_eq!(file("Movie.d.avi"), tuple("d", 4, "", "Movie.avi"));
    assert_eq!(file("Plan-B.avi"), tuple("B", 2, "", "Plan.avi"));
    assert!(!parse_file_name("Movie-e.avi").is_stacked());
    assert!(
        !parse_file_name("Movie a.avi").is_stacked(),
        "a space is not a delimiter here"
    );
    assert!(
        !parse_file_name("Movie-b.en.srt").is_stacked(),
        "only one extension may follow"
    );
}

#[test]
fn x_of_y_forms() {
    assert_eq!(file("Movie-1of2.avi"), tuple("1of2", 1, "", "Movie.avi"));
    assert_eq!(
        file("Movie (1 of 2).avi"),
        tuple("1 of 2", 1, "", "Movie.avi")
    );
    assert_eq!(
        file("Movie.1.of.2.avi"),
        tuple("1.of.2", 1, "", "Movie.avi")
    );
    assert_eq!(
        file("Movie (2of3) - Subtitle.avi").3,
        "Movie - Subtitle.avi"
    );
    assert_eq!(file("Movie_1of3_x.avi").3, "Movie_x.avi");
    assert_eq!(
        file("Movie-12of14.avi"),
        tuple("12of14", 12, "", "Movie.avi")
    );
    assert_eq!(file("Movie 2\u{29F8}3.avi").1, 2);
    assert_eq!(file("Movie 2/3.avi").1, 2);
}

#[test]
fn names_without_a_marker_come_back_unchanged() {
    for name in [
        "",
        "Movie.avi",
        "Movie (2010).mkv",
        "Part of Me.avi",
        "The Part Time Hero.avi",
        "Discovery 2.avi",
        "Movie-2.avi",
        "Movie 3D.avi",
        "Movie part a.avi",
        "Movie cd1",
    ] {
        let s = parse_file_name(name);
        assert!(!s.is_stacked(), "{name}");
        assert_eq!(
            (s.number, s.prefix.as_str(), s.cleaned.as_str()),
            (0, "", name),
            "{name}"
        );
    }
}

#[test]
fn folder_markers_sit_at_the_end() {
    let f = |n: &str| {
        let s = parse_folder_name(n);
        (s.marker, s.cleaned)
    };
    assert_eq!(f("Movie CD1"), ("CD1".into(), "Movie".into()));
    assert_eq!(f("Movie (2010) cd1"), ("cd1".into(), "Movie (2010)".into()));
    assert_eq!(f("Movie.2010.part2"), ("part2".into(), "Movie.2010".into()));
    assert_eq!(f("Movie-dvd2"), ("dvd2".into(), "Movie".into()));
    assert_eq!(f("Movie_disc1"), ("disc1".into(), "Movie".into()));
    assert_eq!(f("CD10"), ("CD10".into(), String::new()));
    assert_eq!(f("cd1"), ("cd1".into(), String::new()));
}

#[test]
fn folder_names_that_are_not_markers() {
    for name in [
        "",
        "Movie",
        "Movie (2010)",
        "Movie part",
        "Movie cd 1",
        "Movie disk 1",
        "Movie.cd01",
        "Movie.cd1.avi",
    ] {
        let s = parse_folder_name(name);
        assert!(!s.is_stacked(), "{name}");
        assert_eq!(s.cleaned, name);
    }
}

#[test]
fn number_and_prefix_helpers() {
    assert_eq!(stacking_number("cd3"), 3);
    assert_eq!(stacking_number("B"), 2);
    assert_eq!(stacking_number("cdz"), 0);
    assert_eq!(stacking_number("z"), 0);
    assert_eq!(stacking_number("1 of 2"), 1);
    assert_eq!(stacking_prefix("part10"), "part");
    assert_eq!(stacking_prefix("a"), "");
    assert_eq!(stacking_prefix("1.of.2"), "");
    assert_eq!(stacking_prefix("cda"), "cda");
}
