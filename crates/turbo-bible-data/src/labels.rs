//! Per-translation book labels. Copied from
//! `crates/turbo-bible-tui/src/import.rs` for the three translations
//! that already ship; new translations (en-asv/ylt/drc/bsb,
//! de-menge, fr-crampon, pt-blivre, la-clementine) fall back to
//! `KJV_LABELS` until a localised table is curated.

type Label = (&'static str, &'static str, &'static str);

#[rustfmt::skip]
pub const KJV_LABELS: &[Label] = &[
    ("GEN", "Genesis", "Gen"), ("EXO", "Exodus", "Exo"),
    ("LEV", "Leviticus", "Lev"), ("NUM", "Numbers", "Num"),
    ("DEU", "Deuteronomy", "Deut"), ("JOS", "Joshua", "Josh"),
    ("JDG", "Judges", "Judg"), ("RUT", "Ruth", "Ruth"),
    ("1SA", "1 Samuel", "1 Sam"), ("2SA", "2 Samuel", "2 Sam"),
    ("1KI", "1 Kings", "1 Kgs"), ("2KI", "2 Kings", "2 Kgs"),
    ("1CH", "1 Chronicles", "1 Chr"), ("2CH", "2 Chronicles", "2 Chr"),
    ("EZR", "Ezra", "Ezra"), ("NEH", "Nehemiah", "Neh"),
    ("EST", "Esther", "Esth"), ("JOB", "Job", "Job"),
    ("PSA", "Psalms", "Ps"), ("PRO", "Proverbs", "Prov"),
    ("ECC", "Ecclesiastes", "Eccl"), ("SNG", "Song of Solomon", "Song"),
    ("ISA", "Isaiah", "Isa"), ("JER", "Jeremiah", "Jer"),
    ("LAM", "Lamentations", "Lam"), ("EZK", "Ezekiel", "Ezek"),
    ("DAN", "Daniel", "Dan"), ("HOS", "Hosea", "Hos"),
    ("JOL", "Joel", "Joel"), ("AMO", "Amos", "Amos"),
    ("OBA", "Obadiah", "Obad"), ("JON", "Jonah", "Jonah"),
    ("MIC", "Micah", "Mic"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habakkuk", "Hab"), ("ZEP", "Zephaniah", "Zeph"),
    ("HAG", "Haggai", "Hag"), ("ZEC", "Zechariah", "Zech"),
    ("MAL", "Malachi", "Mal"), ("MAT", "Matthew", "Matt"),
    ("MRK", "Mark", "Mark"), ("LUK", "Luke", "Luke"),
    ("JHN", "John", "John"), ("ACT", "Acts", "Acts"),
    ("ROM", "Romans", "Rom"), ("1CO", "1 Corinthians", "1 Cor"),
    ("2CO", "2 Corinthians", "2 Cor"), ("GAL", "Galatians", "Gal"),
    ("EPH", "Ephesians", "Eph"), ("PHP", "Philippians", "Phil"),
    ("COL", "Colossians", "Col"), ("1TH", "1 Thessalonians", "1 Thess"),
    ("2TH", "2 Thessalonians", "2 Thess"), ("1TI", "1 Timothy", "1 Tim"),
    ("2TI", "2 Timothy", "2 Tim"), ("TIT", "Titus", "Titus"),
    ("PHM", "Philemon", "Phlm"), ("HEB", "Hebrews", "Heb"),
    ("JAS", "James", "Jas"), ("1PE", "1 Peter", "1 Pet"),
    ("2PE", "2 Peter", "2 Pet"), ("1JN", "1 John", "1 John"),
    ("2JN", "2 John", "2 John"), ("3JN", "3 John", "3 John"),
    ("JUD", "Jude", "Jude"), ("REV", "Revelation", "Rev"),
];

#[rustfmt::skip]
pub const NB_1930_LABELS: &[Label] = &[
    ("GEN", "Første Mosebok", "1 Mos"), ("EXO", "Andre Mosebok", "2 Mos"),
    ("LEV", "Tredje Mosebok", "3 Mos"), ("NUM", "Fjerde Mosebok", "4 Mos"),
    ("DEU", "Femte Mosebok", "5 Mos"), ("JOS", "Josva", "Jos"),
    ("JDG", "Dommerne", "Dom"), ("RUT", "Rut", "Rut"),
    ("1SA", "Første Samuelsbok", "1 Sam"), ("2SA", "Andre Samuelsbok", "2 Sam"),
    ("1KI", "Første Kongebok", "1 Kong"), ("2KI", "Andre Kongebok", "2 Kong"),
    ("1CH", "Første Krønikebok", "1 Krøn"), ("2CH", "Andre Krønikebok", "2 Krøn"),
    ("EZR", "Esra", "Esra"), ("NEH", "Nehemja", "Neh"),
    ("EST", "Ester", "Est"), ("JOB", "Job", "Job"),
    ("PSA", "Salmene", "Sal"), ("PRO", "Ordspråkene", "Ordsp"),
    ("ECC", "Forkynneren", "Fork"), ("SNG", "Høysangen", "Høys"),
    ("ISA", "Jesaja", "Jes"), ("JER", "Jeremia", "Jer"),
    ("LAM", "Klagesangene", "Klag"), ("EZK", "Esekiel", "Esek"),
    ("DAN", "Daniel", "Dan"), ("HOS", "Hosea", "Hos"),
    ("JOL", "Joel", "Joel"), ("AMO", "Amos", "Am"),
    ("OBA", "Obadja", "Obad"), ("JON", "Jona", "Jona"),
    ("MIC", "Mika", "Mi"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habakkuk", "Hab"), ("ZEP", "Sefanja", "Sef"),
    ("HAG", "Haggai", "Hag"), ("ZEC", "Sakarja", "Sak"),
    ("MAL", "Malaki", "Mal"), ("MAT", "Matteus", "Matt"),
    ("MRK", "Markus", "Mark"), ("LUK", "Lukas", "Luk"),
    ("JHN", "Johannes", "Joh"), ("ACT", "Apostlenes gjerninger", "Apg"),
    ("ROM", "Romerne", "Rom"), ("1CO", "Første Korinterbrev", "1 Kor"),
    ("2CO", "Andre Korinterbrev", "2 Kor"), ("GAL", "Galaterne", "Gal"),
    ("EPH", "Efeserne", "Ef"), ("PHP", "Filipperne", "Fil"),
    ("COL", "Kolosserne", "Kol"), ("1TH", "Første Tessalonikerbrev", "1 Tess"),
    ("2TH", "Andre Tessalonikerbrev", "2 Tess"), ("1TI", "Første Timoteusbrev", "1 Tim"),
    ("2TI", "Andre Timoteusbrev", "2 Tim"), ("TIT", "Titus", "Tit"),
    ("PHM", "Filemon", "Filem"), ("HEB", "Hebreerne", "Hebr"),
    ("JAS", "Jakob", "Jak"), ("1PE", "Første Petersbrev", "1 Pet"),
    ("2PE", "Andre Petersbrev", "2 Pet"), ("1JN", "Første Johannesbrev", "1 Joh"),
    ("2JN", "Andre Johannesbrev", "2 Joh"), ("3JN", "Tredje Johannesbrev", "3 Joh"),
    ("JUD", "Judas", "Jud"), ("REV", "Johannes' åpenbaring", "Åp"),
];

#[rustfmt::skip]
pub const ES_RV1909_LABELS: &[Label] = &[
    ("GEN", "Génesis", "Gn"), ("EXO", "Éxodo", "Ex"),
    ("LEV", "Levítico", "Lv"), ("NUM", "Números", "Nm"),
    ("DEU", "Deuteronomio", "Dt"), ("JOS", "Josué", "Jos"),
    ("JDG", "Jueces", "Jue"), ("RUT", "Rut", "Rt"),
    ("1SA", "1 Samuel", "1 S"), ("2SA", "2 Samuel", "2 S"),
    ("1KI", "1 Reyes", "1 R"), ("2KI", "2 Reyes", "2 R"),
    ("1CH", "1 Crónicas", "1 Cr"), ("2CH", "2 Crónicas", "2 Cr"),
    ("EZR", "Esdras", "Esd"), ("NEH", "Nehemías", "Neh"),
    ("EST", "Ester", "Est"), ("JOB", "Job", "Job"),
    ("PSA", "Salmos", "Sal"), ("PRO", "Proverbios", "Pr"),
    ("ECC", "Eclesiastés", "Ec"), ("SNG", "Cantares", "Cnt"),
    ("ISA", "Isaías", "Is"), ("JER", "Jeremías", "Jer"),
    ("LAM", "Lamentaciones", "Lm"), ("EZK", "Ezequiel", "Ez"),
    ("DAN", "Daniel", "Dn"), ("HOS", "Oseas", "Os"),
    ("JOL", "Joel", "Jl"), ("AMO", "Amós", "Am"),
    ("OBA", "Abdías", "Abd"), ("JON", "Jonás", "Jon"),
    ("MIC", "Miqueas", "Mi"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habacuc", "Hab"), ("ZEP", "Sofonías", "Sof"),
    ("HAG", "Hageo", "Hag"), ("ZEC", "Zacarías", "Zac"),
    ("MAL", "Malaquías", "Mal"), ("MAT", "Mateo", "Mt"),
    ("MRK", "Marcos", "Mr"), ("LUK", "Lucas", "Lc"),
    ("JHN", "Juan", "Jn"), ("ACT", "Hechos", "Hch"),
    ("ROM", "Romanos", "Ro"), ("1CO", "1 Corintios", "1 Co"),
    ("2CO", "2 Corintios", "2 Co"), ("GAL", "Gálatas", "Gá"),
    ("EPH", "Efesios", "Ef"), ("PHP", "Filipenses", "Fil"),
    ("COL", "Colosenses", "Col"), ("1TH", "1 Tesalonicenses", "1 Ts"),
    ("2TH", "2 Tesalonicenses", "2 Ts"), ("1TI", "1 Timoteo", "1 Ti"),
    ("2TI", "2 Timoteo", "2 Ti"), ("TIT", "Tito", "Tit"),
    ("PHM", "Filemón", "Flm"), ("HEB", "Hebreos", "He"),
    ("JAS", "Santiago", "Stg"), ("1PE", "1 Pedro", "1 P"),
    ("2PE", "2 Pedro", "2 P"), ("1JN", "1 Juan", "1 Jn"),
    ("2JN", "2 Juan", "2 Jn"), ("3JN", "3 Juan", "3 Jn"),
    ("JUD", "Judas", "Jud"), ("REV", "Apocalipsis", "Ap"),
];

/// Best labels for a translation code, with English KJV fallback.
///
/// New translations without a curated table fall back to English book
/// names; a follow-up pass should add localised tables for de-menge,
/// fr-crampon, pt-blivre, and la-clementine.
pub fn labels_for(code: &str) -> &'static [Label] {
    match code {
        "nb-1930" => NB_1930_LABELS,
        "es-rv1909" => ES_RV1909_LABELS,
        _ => KJV_LABELS,
    }
}

/// `(name, abbreviation)` for a given OSIS code within a label table.
pub fn lookup(labels: &'static [Label], osis: &str) -> Option<(&'static str, &'static str)> {
    labels
        .iter()
        .find(|(o, _, _)| *o == osis)
        .map(|(_, n, a)| (*n, *a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::osis::BOOKS;

    fn osis_set() -> HashSet<&'static str> {
        BOOKS.iter().map(|(o, _, _)| *o).collect()
    }

    #[test]
    fn kjv_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = KJV_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    #[test]
    fn nb_1930_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = NB_1930_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    #[test]
    fn es_rv1909_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = ES_RV1909_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    #[test]
    fn labels_for_known_dispatch() {
        // Probe the dispatch via a known-distinct GEN label per table —
        // `std::ptr::eq` on slice references compares fat pointers and the
        // compiler is free to materialise separate references to a const.
        assert_eq!(
            lookup(labels_for("nb-1930"), "GEN").unwrap().0,
            "Første Mosebok"
        );
        assert_eq!(lookup(labels_for("es-rv1909"), "GEN").unwrap().0, "Génesis");
        assert_eq!(lookup(labels_for("en-kjv"), "GEN").unwrap().0, "Genesis");
        // Unknown / new translation → KJV fallback.
        assert_eq!(lookup(labels_for("en-bsb"), "GEN").unwrap().0, "Genesis");
        assert_eq!(
            lookup(labels_for("la-clementine"), "GEN").unwrap().0,
            "Genesis"
        );
    }

    #[test]
    fn lookup_returns_localised_name() {
        let (name, abbr) = lookup(NB_1930_LABELS, "JHN").expect("JHN in nb-1930");
        assert_eq!(name, "Johannes");
        assert_eq!(abbr, "Joh");
    }
}
