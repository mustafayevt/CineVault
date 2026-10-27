//! The template engine of the renamer.
//!
//! A [`Namer`] holds an engine built for naming (zero and empty values print nothing, tokens are
//! safe for file names, `first`, `replace` and the movie-set index are registered) together with
//! the alias table of movies or TV. It is built once for a batch and renders any number of
//! templates.

use cv_nfo::{CertificationStyle, format_certification};
use cv_template::tokens::{
    AliasMap, RenamerOptions, movie_aliases, renamer_engine, render_name_with, tv_aliases,
};
use cv_template::{Engine, Error, Value};

use crate::options::NameOptions;

/// Renders naming templates against a model.
pub struct Namer {
    engine: Engine,
    aliases: AliasMap,
}

impl Namer {
    /// A namer for movie templates.
    pub fn movie(options: &NameOptions) -> Namer {
        Namer {
            engine: renamer_engine(&engine_options(options)),
            aliases: movie_aliases(),
        }
    }

    /// A namer for TV templates.
    pub fn tv(options: &NameOptions) -> Namer {
        Namer {
            engine: renamer_engine(&engine_options(options)),
            aliases: tv_aliases(),
        }
    }

    /// Renders `template` against `model`. The result is the raw name; clean it with
    /// [`crate::clean::create_destination`].
    pub fn render(&self, template: &str, model: &Value) -> Result<String, Error> {
        render_name_with(&self.engine, template, &self.aliases, model)
    }

    /// The template with every alias written as the model path it stands for.
    pub fn morph(&self, template: &str) -> String {
        self.aliases.morph(template)
    }

    pub fn aliases(&self) -> &AliasMap {
        &self.aliases
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

/// Replaces the certification token the model holds (`US_PG13`) with the text people read
/// (`PG-13`) in the map `root` of the model (`movie`, `tvShow`).
pub(crate) fn with_written_certification(model: Value, root: &str) -> Value {
    let Some(Value::Map(fields)) = model.get(root) else {
        return model;
    };
    let token = fields
        .get("certification")
        .map(|v| v.to_text(false))
        .unwrap_or_default();
    let written = format_certification(&token, CertificationStyle::Short);
    let Value::Map(top) = &model else {
        return model;
    };
    let mut fields = (*fields).clone();
    fields.insert("certification".to_owned(), Value::text(written));
    let mut top = (**top).clone();
    top.insert(root.to_owned(), Value::map(fields));
    Value::map(top)
}

fn engine_options(options: &NameOptions) -> RenamerOptions {
    RenamerOptions {
        colon_replacement: options.colon_replacement.clone(),
        number_replacement: options.first_character_number_replacement.clone(),
        data_dir: options.data_dir.clone(),
        unicode_replacement: options.unicode_replacement,
    }
}

#[cfg(test)]
mod tests {
    use cv_domain::Item;
    use cv_template::tokens::{ModelOptions, movie_model};

    use super::*;

    #[test]
    fn renders_aliases_and_makes_tokens_safe() {
        let mut item = Item::movie("Dune: Part Two");
        item.core.year = 2024;
        let model = movie_model(&item, None, &ModelOptions::default());
        let namer = Namer::movie(&NameOptions::default());
        assert_eq!(
            namer.render("${title} (${year})", &model).unwrap(),
            "Dune - Part Two (2024)"
        );
        let colon_free = NameOptions {
            colon_replacement: String::new(),
            ..NameOptions::default()
        };
        assert_eq!(
            Namer::movie(&colon_free)
                .render("${title}", &model)
                .unwrap(),
            "Dune Part Two"
        );
        assert_eq!(namer.morph("${title}"), "${movie.title}");
    }

    #[test]
    fn certification_is_written_as_people_read_it() {
        let mut item = Item::movie("Dune");
        if let cv_domain::ItemDetail::Movie(fields) = &mut item.detail {
            fields.certification = cv_domain::Certification("US_PG13".to_owned());
        }
        let model =
            with_written_certification(movie_model(&item, None, &ModelOptions::default()), "movie");
        let namer = Namer::movie(&NameOptions::default());
        assert_eq!(namer.render("${certification}", &model).unwrap(), "PG-13");
    }

    #[test]
    fn unicode_option_reaches_the_tokens() {
        let item = Item::movie("AC/DC");
        let model = movie_model(&item, None, &ModelOptions::default());
        let options = NameOptions {
            unicode_replacement: true,
            ..NameOptions::default()
        };
        assert_eq!(
            Namer::movie(&options).render("${title}", &model).unwrap(),
            "AC\u{2044}DC"
        );
    }
}
