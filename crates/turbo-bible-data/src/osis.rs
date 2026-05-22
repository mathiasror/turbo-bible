//! Canonical Protestant 66-book metadata and the scrollmapper book-name
//! → OSIS lookup. Copied verbatim from
//! `crates/turbo-bible-tui/src/import.rs` (`BOOKS`, `SCROLLMAPPER_NAME_TO_OSIS`,
//! `lookup_osis`). The duplication is intentional during Phase B so the
//! TUI's legacy importer stays runnable; final dedup happens with the
//! Phase C runtime ATTACH refactor.

/// `(osis, testament, ord)` for all 66 protocanonical books.
#[rustfmt::skip]
pub const BOOKS: &[(&str, &str, i64)] = &[
    ("GEN", "OT", 1), ("EXO", "OT", 2), ("LEV", "OT", 3), ("NUM", "OT", 4),
    ("DEU", "OT", 5), ("JOS", "OT", 6), ("JDG", "OT", 7), ("RUT", "OT", 8),
    ("1SA", "OT", 9), ("2SA", "OT", 10), ("1KI", "OT", 11), ("2KI", "OT", 12),
    ("1CH", "OT", 13), ("2CH", "OT", 14), ("EZR", "OT", 15), ("NEH", "OT", 16),
    ("EST", "OT", 17), ("JOB", "OT", 18), ("PSA", "OT", 19), ("PRO", "OT", 20),
    ("ECC", "OT", 21), ("SNG", "OT", 22), ("ISA", "OT", 23), ("JER", "OT", 24),
    ("LAM", "OT", 25), ("EZK", "OT", 26), ("DAN", "OT", 27), ("HOS", "OT", 28),
    ("JOL", "OT", 29), ("AMO", "OT", 30), ("OBA", "OT", 31), ("JON", "OT", 32),
    ("MIC", "OT", 33), ("NAM", "OT", 34), ("HAB", "OT", 35), ("ZEP", "OT", 36),
    ("HAG", "OT", 37), ("ZEC", "OT", 38), ("MAL", "OT", 39),
    ("MAT", "NT", 40), ("MRK", "NT", 41), ("LUK", "NT", 42), ("JHN", "NT", 43),
    ("ACT", "NT", 44), ("ROM", "NT", 45), ("1CO", "NT", 46), ("2CO", "NT", 47),
    ("GAL", "NT", 48), ("EPH", "NT", 49), ("PHP", "NT", 50), ("COL", "NT", 51),
    ("1TH", "NT", 52), ("2TH", "NT", 53), ("1TI", "NT", 54), ("2TI", "NT", 55),
    ("TIT", "NT", 56), ("PHM", "NT", 57), ("HEB", "NT", 58), ("JAS", "NT", 59),
    ("1PE", "NT", 60), ("2PE", "NT", 61), ("1JN", "NT", 62), ("2JN", "NT", 63),
    ("3JN", "NT", 64), ("JUD", "NT", 65), ("REV", "NT", 66),
];

/// Scrollmapper's English book names → OSIS. Bail loudly on any unknown
/// name so we never silently accept a non-Protestant canon.
#[rustfmt::skip]
const SCROLLMAPPER_NAME_TO_OSIS: &[(&str, &str)] = &[
    ("Genesis", "GEN"), ("Exodus", "EXO"), ("Leviticus", "LEV"), ("Numbers", "NUM"),
    ("Deuteronomy", "DEU"), ("Joshua", "JOS"), ("Judges", "JDG"), ("Ruth", "RUT"),
    ("I Samuel", "1SA"), ("II Samuel", "2SA"), ("I Kings", "1KI"), ("II Kings", "2KI"),
    ("I Chronicles", "1CH"), ("II Chronicles", "2CH"), ("Ezra", "EZR"),
    ("Nehemiah", "NEH"), ("Esther", "EST"), ("Job", "JOB"), ("Psalms", "PSA"),
    ("Proverbs", "PRO"), ("Ecclesiastes", "ECC"), ("Song of Solomon", "SNG"),
    ("Isaiah", "ISA"), ("Jeremiah", "JER"), ("Lamentations", "LAM"), ("Ezekiel", "EZK"),
    ("Daniel", "DAN"), ("Hosea", "HOS"), ("Joel", "JOL"), ("Amos", "AMO"),
    ("Obadiah", "OBA"), ("Jonah", "JON"), ("Micah", "MIC"), ("Nahum", "NAM"),
    ("Habakkuk", "HAB"), ("Zephaniah", "ZEP"), ("Haggai", "HAG"), ("Zechariah", "ZEC"),
    ("Malachi", "MAL"), ("Matthew", "MAT"), ("Mark", "MRK"), ("Luke", "LUK"),
    ("John", "JHN"), ("Acts", "ACT"), ("Romans", "ROM"), ("I Corinthians", "1CO"),
    ("II Corinthians", "2CO"), ("Galatians", "GAL"), ("Ephesians", "EPH"),
    ("Philippians", "PHP"), ("Colossians", "COL"), ("I Thessalonians", "1TH"),
    ("II Thessalonians", "2TH"), ("I Timothy", "1TI"), ("II Timothy", "2TI"),
    ("Titus", "TIT"), ("Philemon", "PHM"), ("Hebrews", "HEB"), ("James", "JAS"),
    ("I Peter", "1PE"), ("II Peter", "2PE"), ("I John", "1JN"), ("II John", "2JN"),
    ("III John", "3JN"), ("Jude", "JUD"), ("Revelation of John", "REV"),
];

/// Resolve a scrollmapper English book name to its OSIS code, or `None`
/// if unknown. Use this for `formats/json/<ABBR>.json` parsing.
pub fn lookup_osis(name: &str) -> Option<&'static str> {
    SCROLLMAPPER_NAME_TO_OSIS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, o)| *o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn books_has_66_entries() {
        assert_eq!(BOOKS.len(), 66);
    }

    #[test]
    fn books_ord_is_dense_1_to_66() {
        let mut ords: Vec<i64> = BOOKS.iter().map(|(_, _, o)| *o).collect();
        ords.sort_unstable();
        assert_eq!(ords, (1i64..=66).collect::<Vec<_>>());
    }

    #[test]
    fn scrollmapper_name_table_covers_every_osis_code_exactly_once() {
        let osis: HashSet<&str> = BOOKS.iter().map(|(o, _, _)| *o).collect();
        let mapped: HashSet<&str> = SCROLLMAPPER_NAME_TO_OSIS.iter().map(|(_, o)| *o).collect();
        assert_eq!(osis, mapped);
        assert_eq!(SCROLLMAPPER_NAME_TO_OSIS.len(), 66);
    }

    #[test]
    fn lookup_osis_known_names() {
        assert_eq!(lookup_osis("Genesis"), Some("GEN"));
        assert_eq!(lookup_osis("I Samuel"), Some("1SA"));
        assert_eq!(lookup_osis("Revelation of John"), Some("REV"));
    }

    #[test]
    fn lookup_osis_unknown() {
        assert_eq!(lookup_osis("Tobit"), None);
        assert_eq!(lookup_osis(""), None);
    }
}
