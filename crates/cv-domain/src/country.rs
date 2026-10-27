//! Country codes and their English names.
//!
//! CineVault stores the countries of a title as ISO 3166-1 alpha-2 codes (`JP`, `GB`). NFO files
//! carry the English name instead (`Japan`, `United Kingdom`), as Kodi-format NFO files do, and
//! a name read back from an NFO is turned into its code again. The names are the common English
//! (CLDR) names; a few long official forms are shortened to the name people use. TMDB also uses
//! some codes of countries that no longer exist (`SU`, `XC`, `XG`, `YU`); they are listed too.

/// `(code, English name)`, sorted by code.
const COUNTRIES: &[(&str, &str)] = &[
    ("AD", "Andorra"),
    ("AE", "United Arab Emirates"),
    ("AF", "Afghanistan"),
    ("AG", "Antigua & Barbuda"),
    ("AI", "Anguilla"),
    ("AL", "Albania"),
    ("AM", "Armenia"),
    ("AN", "Netherlands Antilles"),
    ("AO", "Angola"),
    ("AQ", "Antarctica"),
    ("AR", "Argentina"),
    ("AS", "American Samoa"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("AW", "Aruba"),
    ("AX", "Åland Islands"),
    ("AZ", "Azerbaijan"),
    ("BA", "Bosnia & Herzegovina"),
    ("BB", "Barbados"),
    ("BD", "Bangladesh"),
    ("BE", "Belgium"),
    ("BF", "Burkina Faso"),
    ("BG", "Bulgaria"),
    ("BH", "Bahrain"),
    ("BI", "Burundi"),
    ("BJ", "Benin"),
    ("BL", "St. Barthélemy"),
    ("BM", "Bermuda"),
    ("BN", "Brunei"),
    ("BO", "Bolivia"),
    ("BQ", "Caribbean Netherlands"),
    ("BR", "Brazil"),
    ("BS", "Bahamas"),
    ("BT", "Bhutan"),
    ("BV", "Bouvet Island"),
    ("BW", "Botswana"),
    ("BY", "Belarus"),
    ("BZ", "Belize"),
    ("CA", "Canada"),
    ("CC", "Cocos (Keeling) Islands"),
    ("CD", "Congo - Kinshasa"),
    ("CF", "Central African Republic"),
    ("CG", "Congo - Brazzaville"),
    ("CH", "Switzerland"),
    ("CI", "Côte d’Ivoire"),
    ("CK", "Cook Islands"),
    ("CL", "Chile"),
    ("CM", "Cameroon"),
    ("CN", "China"),
    ("CO", "Colombia"),
    ("CR", "Costa Rica"),
    ("CS", "Serbia and Montenegro"),
    ("CU", "Cuba"),
    ("CV", "Cape Verde"),
    ("CW", "Curaçao"),
    ("CX", "Christmas Island"),
    ("CY", "Cyprus"),
    ("CZ", "Czechia"),
    ("DE", "Germany"),
    ("DJ", "Djibouti"),
    ("DK", "Denmark"),
    ("DM", "Dominica"),
    ("DO", "Dominican Republic"),
    ("DZ", "Algeria"),
    ("EC", "Ecuador"),
    ("EE", "Estonia"),
    ("EG", "Egypt"),
    ("EH", "Western Sahara"),
    ("ER", "Eritrea"),
    ("ES", "Spain"),
    ("ET", "Ethiopia"),
    ("FI", "Finland"),
    ("FJ", "Fiji"),
    ("FK", "Falkland Islands"),
    ("FM", "Micronesia"),
    ("FO", "Faroe Islands"),
    ("FR", "France"),
    ("GA", "Gabon"),
    ("GB", "United Kingdom"),
    ("GD", "Grenada"),
    ("GE", "Georgia"),
    ("GF", "French Guiana"),
    ("GG", "Guernsey"),
    ("GH", "Ghana"),
    ("GI", "Gibraltar"),
    ("GL", "Greenland"),
    ("GM", "Gambia"),
    ("GN", "Guinea"),
    ("GP", "Guadeloupe"),
    ("GQ", "Equatorial Guinea"),
    ("GR", "Greece"),
    ("GS", "South Georgia & South Sandwich Islands"),
    ("GT", "Guatemala"),
    ("GU", "Guam"),
    ("GW", "Guinea-Bissau"),
    ("GY", "Guyana"),
    ("HK", "Hong Kong"),
    ("HM", "Heard & McDonald Islands"),
    ("HN", "Honduras"),
    ("HR", "Croatia"),
    ("HT", "Haiti"),
    ("HU", "Hungary"),
    ("ID", "Indonesia"),
    ("IE", "Ireland"),
    ("IL", "Israel"),
    ("IM", "Isle of Man"),
    ("IN", "India"),
    ("IO", "British Indian Ocean Territory"),
    ("IQ", "Iraq"),
    ("IR", "Iran"),
    ("IS", "Iceland"),
    ("IT", "Italy"),
    ("JE", "Jersey"),
    ("JM", "Jamaica"),
    ("JO", "Jordan"),
    ("JP", "Japan"),
    ("KE", "Kenya"),
    ("KG", "Kyrgyzstan"),
    ("KH", "Cambodia"),
    ("KI", "Kiribati"),
    ("KM", "Comoros"),
    ("KN", "St. Kitts & Nevis"),
    ("KP", "North Korea"),
    ("KR", "South Korea"),
    ("KW", "Kuwait"),
    ("KY", "Cayman Islands"),
    ("KZ", "Kazakhstan"),
    ("LA", "Laos"),
    ("LB", "Lebanon"),
    ("LC", "St. Lucia"),
    ("LI", "Liechtenstein"),
    ("LK", "Sri Lanka"),
    ("LR", "Liberia"),
    ("LS", "Lesotho"),
    ("LT", "Lithuania"),
    ("LU", "Luxembourg"),
    ("LV", "Latvia"),
    ("LY", "Libya"),
    ("MA", "Morocco"),
    ("MC", "Monaco"),
    ("MD", "Moldova"),
    ("ME", "Montenegro"),
    ("MF", "St. Martin"),
    ("MG", "Madagascar"),
    ("MH", "Marshall Islands"),
    ("MK", "North Macedonia"),
    ("ML", "Mali"),
    ("MM", "Myanmar"),
    ("MN", "Mongolia"),
    ("MO", "Macao"),
    ("MP", "Northern Mariana Islands"),
    ("MQ", "Martinique"),
    ("MR", "Mauritania"),
    ("MS", "Montserrat"),
    ("MT", "Malta"),
    ("MU", "Mauritius"),
    ("MV", "Maldives"),
    ("MW", "Malawi"),
    ("MX", "Mexico"),
    ("MY", "Malaysia"),
    ("MZ", "Mozambique"),
    ("NA", "Namibia"),
    ("NC", "New Caledonia"),
    ("NE", "Niger"),
    ("NF", "Norfolk Island"),
    ("NG", "Nigeria"),
    ("NI", "Nicaragua"),
    ("NL", "Netherlands"),
    ("NO", "Norway"),
    ("NP", "Nepal"),
    ("NR", "Nauru"),
    ("NU", "Niue"),
    ("NZ", "New Zealand"),
    ("OM", "Oman"),
    ("PA", "Panama"),
    ("PE", "Peru"),
    ("PF", "French Polynesia"),
    ("PG", "Papua New Guinea"),
    ("PH", "Philippines"),
    ("PK", "Pakistan"),
    ("PL", "Poland"),
    ("PM", "St. Pierre & Miquelon"),
    ("PN", "Pitcairn Islands"),
    ("PR", "Puerto Rico"),
    ("PS", "Palestinian Territories"),
    ("PT", "Portugal"),
    ("PW", "Palau"),
    ("PY", "Paraguay"),
    ("QA", "Qatar"),
    ("RE", "Réunion"),
    ("RO", "Romania"),
    ("RS", "Serbia"),
    ("RU", "Russia"),
    ("RW", "Rwanda"),
    ("SA", "Saudi Arabia"),
    ("SB", "Solomon Islands"),
    ("SC", "Seychelles"),
    ("SD", "Sudan"),
    ("SE", "Sweden"),
    ("SG", "Singapore"),
    ("SH", "St. Helena"),
    ("SI", "Slovenia"),
    ("SJ", "Svalbard & Jan Mayen"),
    ("SK", "Slovakia"),
    ("SL", "Sierra Leone"),
    ("SM", "San Marino"),
    ("SN", "Senegal"),
    ("SO", "Somalia"),
    ("SR", "Suriname"),
    ("SS", "South Sudan"),
    ("ST", "São Tomé & Príncipe"),
    ("SU", "Soviet Union"),
    ("SV", "El Salvador"),
    ("SX", "Sint Maarten"),
    ("SY", "Syria"),
    ("SZ", "Eswatini"),
    ("TC", "Turks & Caicos Islands"),
    ("TD", "Chad"),
    ("TF", "French Southern Territories"),
    ("TG", "Togo"),
    ("TH", "Thailand"),
    ("TJ", "Tajikistan"),
    ("TK", "Tokelau"),
    ("TL", "Timor-Leste"),
    ("TM", "Turkmenistan"),
    ("TN", "Tunisia"),
    ("TO", "Tonga"),
    ("TR", "Türkiye"),
    ("TT", "Trinidad & Tobago"),
    ("TV", "Tuvalu"),
    ("TW", "Taiwan"),
    ("TZ", "Tanzania"),
    ("UA", "Ukraine"),
    ("UG", "Uganda"),
    ("UM", "U.S. Outlying Islands"),
    ("US", "United States"),
    ("UY", "Uruguay"),
    ("UZ", "Uzbekistan"),
    ("VA", "Vatican City"),
    ("VC", "St. Vincent & Grenadines"),
    ("VE", "Venezuela"),
    ("VG", "British Virgin Islands"),
    ("VI", "U.S. Virgin Islands"),
    ("VN", "Vietnam"),
    ("VU", "Vanuatu"),
    ("WF", "Wallis & Futuna"),
    ("WS", "Samoa"),
    ("XC", "Czechoslovakia"),
    ("XG", "East Germany"),
    ("XK", "Kosovo"),
    ("YE", "Yemen"),
    ("YT", "Mayotte"),
    ("YU", "Yugoslavia"),
    ("ZA", "South Africa"),
    ("ZM", "Zambia"),
    ("ZW", "Zimbabwe"),
];

/// Other English names a file or a provider may use for a country.
const ALIASES: &[(&str, &str)] = &[
    ("United States of America", "US"),
    ("USA", "US"),
    ("UK", "GB"),
    ("Great Britain", "GB"),
    ("Turkey", "TR"),
    ("Czech Republic", "CZ"),
    ("Korea", "KR"),
    ("Republic of Korea", "KR"),
    ("Russian Federation", "RU"),
    ("Hong Kong SAR China", "HK"),
    ("Macao SAR China", "MO"),
    ("Macau", "MO"),
    ("Myanmar (Burma)", "MM"),
    ("Burma", "MM"),
    ("Macedonia", "MK"),
    ("Ivory Coast", "CI"),
    ("Swaziland", "SZ"),
    ("East Timor", "TL"),
    ("Holy See", "VA"),
    ("Democratic Republic of the Congo", "CD"),
    ("Republic of the Congo", "CG"),
    ("USSR", "SU"),
    ("Palestine", "PS"),
];

/// The English name of a country code (`JP` gives `Japan`), ignoring case and surrounding white
/// space. `None` for anything that is not a known code.
pub fn country_name(code: &str) -> Option<&'static str> {
    let code = code.trim();
    if code.len() != 2 {
        return None;
    }
    COUNTRIES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(code))
        .map(|(_, name)| *name)
}

/// The code of a country written as its English name or as a code (`Japan` and `jp` give `JP`),
/// ignoring case and surrounding white space. `None` when the text is neither.
pub fn country_code(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let by_code = || {
        COUNTRIES
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(text))
            .map(|(code, _)| *code)
    };
    let by_name = || {
        COUNTRIES
            .iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(text))
            .or_else(|| {
                ALIASES
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(text))
            })
            .map(|(a, b)| if a.len() == 2 { *a } else { *b })
    };
    if text.len() == 2 {
        by_code()
    } else {
        by_name()
    }
}

/// The country as an NFO writes it: the English name of a known code, anything else unchanged
/// (a name stays a name).
pub fn country_display(value: &str) -> String {
    country_name(value).map_or_else(|| value.trim().to_owned(), str::to_owned)
}

/// The country as CineVault stores it: the code of a known name or code, anything else unchanged.
pub fn country_stored(value: &str) -> String {
    country_code(value).map_or_else(|| value.trim().to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_give_english_names() {
        assert_eq!(country_name("JP"), Some("Japan"));
        assert_eq!(country_name("gb"), Some("United Kingdom"));
        assert_eq!(country_name(" US "), Some("United States"));
        assert_eq!(country_name("SU"), Some("Soviet Union"));
        assert_eq!(country_name("Japan"), None);
        assert_eq!(country_name("ZZ"), None);
    }

    #[test]
    fn names_and_codes_give_codes() {
        assert_eq!(country_code("Japan"), Some("JP"));
        assert_eq!(country_code("united kingdom"), Some("GB"));
        assert_eq!(country_code("United States of America"), Some("US"));
        assert_eq!(country_code("Turkey"), Some("TR"));
        assert_eq!(country_code("de"), Some("DE"));
        assert_eq!(country_code("Mars"), None);
        assert_eq!(country_code(""), None);
    }

    #[test]
    fn display_and_stored_round_trip_and_keep_unknown_text() {
        for (code, _) in COUNTRIES {
            assert_eq!(country_stored(&country_display(code)), *code, "{code}");
        }
        assert_eq!(country_display("Mars"), "Mars");
        assert_eq!(country_stored("Mars"), "Mars");
    }

    #[test]
    fn the_table_is_sorted_and_has_no_repeats() {
        for pair in COUNTRIES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }
}
