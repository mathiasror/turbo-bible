//! Locale-aware Scripture reference formatting.
//!
//! The chapter–verse separator follows each translation's own typographic
//! convention: a colon for English / Spanish / Portuguese Bibles
//! (`John 3:16`, `Juan 3:16`, `João 3:16`), a comma for the
//! continental-European ones — Norwegian, German, French, Latin
//! (`Johannes 3,16`, `Jean 3,16`, `Genesis 1,1`).

/// The chapter–verse separator for a translation code (`en-kjv`, `nb-1930`,
/// …). Falls back to a comma for unknown languages — the European default the
/// corpus skews toward — so a missing entry never silently emits the English
/// colon for a non-English text.
pub fn separator(translation_code: &str) -> char {
    match translation_code.split('-').next().unwrap_or("") {
        "en" | "es" | "pt" => ':',
        _ => ',',
    }
}

/// Format a single-verse reference, e.g. `John 3:16` / `Johannes 3,16`.
pub fn format(book: &str, chapter: i64, verse: i64, translation_code: &str) -> String {
    format!("{book} {chapter}{}{verse}", separator(translation_code))
}

/// Format a verse-range reference, e.g. `John 3:16-18` / `Johannes 3,16-18`.
pub fn format_range(
    book: &str,
    chapter: i64,
    start: i64,
    end: i64,
    translation_code: &str,
) -> String {
    let sep = separator(translation_code);
    format!("{book} {chapter}{sep}{start}-{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separator_is_colon_for_colon_locales() {
        for code in ["en-kjv", "en-asv", "es-rv1909", "pt-blivre"] {
            assert_eq!(separator(code), ':', "{code} should use a colon");
        }
    }

    #[test]
    fn separator_is_comma_for_comma_locales_and_unknown() {
        for code in [
            "nb-1930",
            "de-menge",
            "fr-crampon",
            "la-clementine",
            "xx-???",
        ] {
            assert_eq!(separator(code), ',', "{code} should use a comma");
        }
    }

    #[test]
    fn formats_match_locale() {
        assert_eq!(format("John", 3, 16, "en-kjv"), "John 3:16");
        assert_eq!(format("Johannes", 3, 16, "nb-1930"), "Johannes 3,16");
        assert_eq!(format_range("John", 3, 16, 18, "en-kjv"), "John 3:16-18");
        assert_eq!(format_range("Salme", 23, 1, 3, "nb-1930"), "Salme 23,1-3");
    }
}
