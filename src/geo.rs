//! Offline GeoIP country lookup from the bundled DB-IP database (Task 2).
//!
//! The DB-IP Lite country MMDB ships unmodified in `assets/geo/` and is compiled into the binary;
//! nothing is fetched at runtime. Only `country.iso_code` is read. A database that fails to load
//! makes every public address resolve to [`UNKNOWN`] instead of crashing.

use std::net::IpAddr;
use std::sync::LazyLock;

use crate::net;

/// Country code for addresses on the local network (the same address-class rule as the LAN total).
pub const LAN: &str = "LAN";
/// Country code for public addresses with no usable country (no data, reserved range, failed load).
pub const UNKNOWN: &str = "ZZ";

static GEO: LazyLock<Option<maxminddb::Reader<&'static [u8]>>> = LazyLock::new(|| {
    maxminddb::Reader::from_source(&include_bytes!("../assets/geo/dbip-country-lite-2026-10.mmdb")[..]).ok()
});

/// The country code for `ip`: [`LAN`] for local-network addresses, otherwise the ISO-2 code from the
/// bundled database, or [`UNKNOWN`].
pub fn country_of(ip: IpAddr) -> String {
    if net::is_private(&ip) {
        return LAN.to_string();
    }
    lookup_in(GEO.as_ref(), ip)
}

/// Keeps a code only when it is exactly two ASCII uppercase letters; anything else becomes [`UNKNOWN`].
pub fn normalize(code: Option<&str>) -> String {
    match code {
        Some(c) if c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()) => c.to_string(),
        _ => UNKNOWN.to_string(),
    }
}

/// English display name for a country code. Codes missing from the table come back unchanged.
pub fn name(code: &str) -> String {
    match code {
        LAN => "Local network".to_string(),
        UNKNOWN => "Unknown".to_string(),
        _ => match NAMES.binary_search_by_key(&code, |&(c, _)| c) {
            Ok(i) => NAMES[i].1.to_string(),
            Err(_) => code.to_string(),
        },
    }
}

/// Whether the bundled database opened successfully.
#[cfg(test)]
pub fn loaded() -> bool {
    GEO.is_some()
}

/// Looks up `ip` in `reader`; a missing reader, a failed lookup or a record without a valid
/// `country.iso_code` gives [`UNKNOWN`].
fn lookup_in(reader: Option<&maxminddb::Reader<&[u8]>>, ip: IpAddr) -> String {
    let code = reader.and_then(|r| r.lookup(ip).ok()).and_then(|found| {
        found
            .decode_path::<&str>(&maxminddb::path!["country", "iso_code"])
            .ok()
            .flatten()
    });
    normalize(code)
}

/// ISO 3166-1 alpha-2 codes (plus `XK`, Kosovo) and their common English names, sorted by code
/// for binary search.
static NAMES: &[(&str, &str)] = &[
    ("AD", "Andorra"),
    ("AE", "United Arab Emirates"),
    ("AF", "Afghanistan"),
    ("AG", "Antigua and Barbuda"),
    ("AI", "Anguilla"),
    ("AL", "Albania"),
    ("AM", "Armenia"),
    ("AO", "Angola"),
    ("AQ", "Antarctica"),
    ("AR", "Argentina"),
    ("AS", "American Samoa"),
    ("AT", "Austria"),
    ("AU", "Australia"),
    ("AW", "Aruba"),
    ("AX", "Åland Islands"),
    ("AZ", "Azerbaijan"),
    ("BA", "Bosnia and Herzegovina"),
    ("BB", "Barbados"),
    ("BD", "Bangladesh"),
    ("BE", "Belgium"),
    ("BF", "Burkina Faso"),
    ("BG", "Bulgaria"),
    ("BH", "Bahrain"),
    ("BI", "Burundi"),
    ("BJ", "Benin"),
    ("BL", "Saint Barthélemy"),
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
    ("CD", "DR Congo"),
    ("CF", "Central African Republic"),
    ("CG", "Republic of the Congo"),
    ("CH", "Switzerland"),
    ("CI", "Côte d'Ivoire"),
    ("CK", "Cook Islands"),
    ("CL", "Chile"),
    ("CM", "Cameroon"),
    ("CN", "China"),
    ("CO", "Colombia"),
    ("CR", "Costa Rica"),
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
    ("GS", "South Georgia and the South Sandwich Islands"),
    ("GT", "Guatemala"),
    ("GU", "Guam"),
    ("GW", "Guinea-Bissau"),
    ("GY", "Guyana"),
    ("HK", "Hong Kong"),
    ("HM", "Heard Island and McDonald Islands"),
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
    ("KN", "Saint Kitts and Nevis"),
    ("KP", "North Korea"),
    ("KR", "South Korea"),
    ("KW", "Kuwait"),
    ("KY", "Cayman Islands"),
    ("KZ", "Kazakhstan"),
    ("LA", "Laos"),
    ("LB", "Lebanon"),
    ("LC", "Saint Lucia"),
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
    ("MF", "Saint Martin"),
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
    ("PM", "Saint Pierre and Miquelon"),
    ("PN", "Pitcairn Islands"),
    ("PR", "Puerto Rico"),
    ("PS", "Palestine"),
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
    ("SH", "Saint Helena, Ascension and Tristan da Cunha"),
    ("SI", "Slovenia"),
    ("SJ", "Svalbard and Jan Mayen"),
    ("SK", "Slovakia"),
    ("SL", "Sierra Leone"),
    ("SM", "San Marino"),
    ("SN", "Senegal"),
    ("SO", "Somalia"),
    ("SR", "Suriname"),
    ("SS", "South Sudan"),
    ("ST", "São Tomé and Príncipe"),
    ("SV", "El Salvador"),
    ("SX", "Sint Maarten"),
    ("SY", "Syria"),
    ("SZ", "Eswatini"),
    ("TC", "Turks and Caicos Islands"),
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
    ("TR", "Turkey"),
    ("TT", "Trinidad and Tobago"),
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
    ("VC", "Saint Vincent and the Grenadines"),
    ("VE", "Venezuela"),
    ("VG", "British Virgin Islands"),
    ("VI", "U.S. Virgin Islands"),
    ("VN", "Vietnam"),
    ("VU", "Vanuatu"),
    ("WF", "Wallis and Futuna"),
    ("WS", "Samoa"),
    ("XK", "Kosovo"),
    ("YE", "Yemen"),
    ("YT", "Mayotte"),
    ("ZA", "South Africa"),
    ("ZM", "Zambia"),
    ("ZW", "Zimbabwe"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn private_is_lan() {
        assert_eq!(country_of(ip("192.168.1.5")), LAN);
    }

    #[test]
    fn ula_is_lan() {
        assert_eq!(country_of(ip("fd00::1")), LAN);
    }

    #[test]
    fn link_local_is_lan() {
        assert_eq!(country_of(ip("fe80::1")), LAN);
    }

    #[test]
    fn cgnat_is_not_lan() {
        assert_ne!(country_of(ip("100.64.1.1")), LAN);
    }

    #[test]
    fn public_lookup() {
        assert!(loaded());
        assert_eq!(country_of(ip("8.8.8.8")), "US");
    }

    #[test]
    fn corrupt_reader_is_unknown() {
        let bad = maxminddb::Reader::from_source(&b"garbage"[..]);
        assert!(bad.is_err());
        assert_eq!(lookup_in(bad.as_ref().ok(), ip("8.8.8.8")), "ZZ");
        assert_eq!(lookup_in(None, ip("8.8.8.8")), "ZZ");
    }

    #[test]
    fn normalize_rules() {
        assert_eq!(normalize(Some("IL")), "IL");
        assert_eq!(normalize(Some("il")), "ZZ");
        assert_eq!(normalize(Some("ISR")), "ZZ");
        assert_eq!(normalize(None), "ZZ");
    }

    #[test]
    fn names() {
        assert_eq!(name("IL"), "Israel");
        assert_eq!(name("XK"), "Kosovo");
        assert_ne!(name("XK"), "XK");
        assert_eq!(name(LAN), "Local network");
        assert_eq!(name(UNKNOWN), "Unknown");
        assert_eq!(name("QQ"), "QQ");
    }

    #[test]
    fn name_table_is_sorted_and_valid() {
        assert!(NAMES.len() >= 249);
        for pair in NAMES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} must sort before {}", pair[0].0, pair[1].0);
        }
        for &(code, label) in NAMES {
            assert_eq!(normalize(Some(code)), code);
            assert!(code != LAN && code != UNKNOWN && !label.is_empty());
        }
    }

    #[test]
    fn every_database_country_has_a_name() {
        let reader = GEO.as_ref().expect("bundled database loads");
        let mut seen = std::collections::BTreeSet::new();
        for found in reader.networks(Default::default()).unwrap() {
            let found = found.unwrap();
            let code: Option<&str> = found.decode_path(&maxminddb::path!["country", "iso_code"]).unwrap();
            if let Some(code) = code {
                seen.insert(code);
            }
        }
        assert!(seen.len() > 200, "only {} countries in the database", seen.len());
        let missing: Vec<&str> = seen.into_iter().filter(|&c| name(c) == c).collect();
        assert!(missing.is_empty(), "codes without a name: {missing:?}");
    }
}
