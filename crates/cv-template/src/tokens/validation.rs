//! Validators for naming templates: the alias maps and model fields of movies and TV.

use cv_domain::Item;

use super::{ModelOptions, MovieSetContext, movie_aliases, movie_model, tv_aliases, tv_model};
use crate::eval::Engine;
use crate::parse::ParseOptions;
use crate::validate::{ModelRoot, Validator};

/// The model roots a movie template reads: `movie` and `movieSet`, with the fields the movie
/// model has.
pub fn movie_roots() -> Vec<ModelRoot> {
    let model = movie_model(
        &Item::movie(""),
        Some(&MovieSetContext::default()),
        &ModelOptions::default(),
    );
    ["movie", "movieSet"]
        .into_iter()
        .map(|name| ModelRoot::from_value(name, model.get(name).as_ref()))
        .collect()
}

/// The model roots a TV template reads: `tvShow`, `season` and `episode`.
pub fn tv_roots() -> Vec<ModelRoot> {
    let show = Item::show("");
    let episode = Item::episode(cv_domain::ItemId(0), "");
    let model = tv_model(&show, &episode, &ModelOptions::default());
    ["tvShow", "season", "episode"]
        .into_iter()
        .map(|name| ModelRoot::from_value(name, model.get(name).as_ref()))
        .collect()
}

/// A validator for movie naming templates: the movie aliases, `movie.*` and `movieSet.*` paths,
/// and the renderers of `engine`. Naming templates are parsed the renamer's way (a typed
/// backslash stays a backslash).
pub fn movie_validator(engine: &Engine) -> Validator {
    Validator::new(engine, movie_roots())
        .with_aliases(movie_aliases())
        .with_parse_options(ParseOptions {
            backslash_escapes: false,
        })
}

/// A validator for TV naming templates (show folder, season folder and episode file). Use
/// [`Validator::check_tv_patterns`] for the episode token rules.
pub fn tv_validator(engine: &Engine) -> Validator {
    Validator::new(engine, tv_roots())
        .with_aliases(tv_aliases())
        .with_parse_options(ParseOptions {
            backslash_escapes: false,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::AliasMap;

    /// Every alias resolves to a model field that exists, so a template made only of known
    /// aliases and their full paths always validates.
    #[test]
    fn every_alias_and_its_target_validates() {
        let engine = crate::tokens::renamer_engine(&crate::tokens::RenamerOptions::default());
        for (validator, aliases) in [
            (movie_validator(&engine), movie_aliases()),
            (tv_validator(&engine), tv_aliases()),
        ] {
            check_all(&validator, &aliases);
        }
    }

    fn check_all(validator: &Validator, aliases: &AliasMap) {
        for (name, target) in aliases.entries() {
            for source in [format!("${{{name}}}"), format!("${{{target}}}")] {
                let report = validator.validate(&source);
                assert!(report.is_clean(), "{source}: {:?}", report.diagnostics);
            }
        }
    }

    #[test]
    fn roots_have_their_fields() {
        let movie = movie_roots();
        assert_eq!(movie.len(), 2);
        let fields = movie[0].fields.as_ref().unwrap();
        for field in ["title", "year", "mediaInfoVideoCodec", "movieSet"] {
            assert!(fields.iter().any(|f| f == field), "movie.{field}");
        }
        assert!(
            movie[1]
                .fields
                .as_ref()
                .unwrap()
                .iter()
                .any(|f| f == "title")
        );
        let tv = tv_roots();
        assert_eq!(
            tv.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["tvShow", "season", "episode"]
        );
        assert!(tv.iter().all(|root| root.fields.is_some()));
    }
}
