//! German names the German-language providers (OFDb, fernsehserien.de) use for genres and
//! countries, mapped to the names and codes CineVault stores.

/// German genre names to the names CineVault's genres use. Others pass through unchanged.
const GENRES: &[(&str, &str)] = &[
    ("komödie", "Comedy"),
    ("krimi", "Crime"),
    ("kriegsfilm", "War"),
    ("abenteuer", "Adventure"),
    ("familie", "Family"),
    ("dokumentation", "Documentary"),
    ("dokumentarfilm", "Documentary"),
    ("trickfilm", "Animation"),
    ("animation", "Animation"),
    ("liebe/romantik", "Romance"),
    ("liebesfilm", "Romance"),
    ("romantik", "Romance"),
    ("musikfilm", "Music"),
    ("musik", "Music"),
    ("biographie", "Biography"),
    ("biografie", "Biography"),
    ("historienfilm", "History"),
    ("historisch", "History"),
    ("erotik", "Erotic"),
    ("tierfilm", "Animal"),
    ("kurzfilm", "Short"),
    ("katastrophenfilm", "Disaster"),
    ("heimatfilm", "Drama"),
    ("science-fiction", "Science Fiction"),
];

/// German country names to the two-letter codes CineVault stores.
const COUNTRIES: &[(&str, &str)] = &[
    ("usa", "US"),
    ("deutschland", "DE"),
    ("bundesrepublik deutschland", "DE"),
    ("brd", "DE"),
    ("ddr", "DE"),
    ("großbritannien", "GB"),
    ("vereinigtes königreich", "GB"),
    ("frankreich", "FR"),
    ("italien", "IT"),
    ("spanien", "ES"),
    ("kanada", "CA"),
    ("japan", "JP"),
    ("österreich", "AT"),
    ("schweiz", "CH"),
    ("australien", "AU"),
    ("niederlande", "NL"),
    ("belgien", "BE"),
    ("hongkong", "HK"),
    ("china", "CN"),
    ("südkorea", "KR"),
    ("indien", "IN"),
    ("russland", "RU"),
    ("schweden", "SE"),
    ("dänemark", "DK"),
    ("norwegen", "NO"),
    ("finnland", "FI"),
    ("irland", "IE"),
    ("polen", "PL"),
    ("tschechien", "CZ"),
    ("ungarn", "HU"),
    ("mexiko", "MX"),
    ("brasilien", "BR"),
    ("argentinien", "AR"),
    ("neuseeland", "NZ"),
    ("türkei", "TR"),
    ("griechenland", "GR"),
    ("portugal", "PT"),
    ("israel", "IL"),
];

/// The names CineVault's genres use for OFDb's German ones.
pub(crate) fn genres(names: &[String]) -> Vec<cv_domain::Genre> {
    let mut out: Vec<cv_domain::Genre> = Vec::new();
    for name in names {
        let lower = name.trim().to_lowercase();
        let english = GENRES
            .iter()
            .find(|(german, _)| *german == lower)
            .map_or(name.trim(), |(_, english)| english);
        if let Some(genre) = cv_domain::Genre::parse(english)
            && !out.contains(&genre)
        {
            out.push(genre);
        }
    }
    out
}

/// The codes of OFDb's German country names; a name outside the table stays as written.
pub(crate) fn countries(names: &[String]) -> Vec<String> {
    let mapped: Vec<String> = names
        .iter()
        .map(|name| {
            let lower = name.trim().to_lowercase();
            COUNTRIES
                .iter()
                .find(|(german, _)| *german == lower)
                .map_or_else(|| name.clone(), |(_, code)| (*code).to_owned())
        })
        .collect();
    crate::shared::country_codes(&mapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_names_map_to_english_genres_and_country_codes() {
        let names = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let genres = genres(&names(&["Komödie", "Drama", "Krimi", "Komödie"]));
        assert_eq!(genres.len(), 3);
        assert_eq!(genres[0], cv_domain::Genre::parse("Comedy").unwrap());
        assert_eq!(
            countries(&names(&["USA", "Deutschland", "Narnia"])),
            ["US", "DE", "Narnia"]
        );
    }
}
