//! Per-translation book labels. Localised tables exist for every
//! non-English edition (nb-1930, es-rv1909, de-menge, fr-crampon,
//! pt-blivre, la-clementine); the remaining English editions
//! (en-asv, en-ylt, en-drc, en-bsb) fall back to `KJV_LABELS`.

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

#[rustfmt::skip]
pub const DE_MENGE_LABELS: &[Label] = &[
    ("GEN", "1. Mose", "1. Mose"), ("EXO", "2. Mose", "2. Mose"),
    ("LEV", "3. Mose", "3. Mose"), ("NUM", "4. Mose", "4. Mose"),
    ("DEU", "5. Mose", "5. Mose"), ("JOS", "Josua", "Jos"),
    ("JDG", "Richter", "Ri"), ("RUT", "Rut", "Rut"),
    ("1SA", "1. Samuel", "1. Sam"), ("2SA", "2. Samuel", "2. Sam"),
    ("1KI", "1. Könige", "1. Kön"), ("2KI", "2. Könige", "2. Kön"),
    ("1CH", "1. Chronik", "1. Chr"), ("2CH", "2. Chronik", "2. Chr"),
    ("EZR", "Esra", "Esr"), ("NEH", "Nehemia", "Neh"),
    ("EST", "Ester", "Est"), ("JOB", "Hiob", "Hiob"),
    ("PSA", "Psalmen", "Ps"), ("PRO", "Sprüche", "Spr"),
    ("ECC", "Prediger", "Pred"), ("SNG", "Hoheslied", "Hld"),
    ("ISA", "Jesaja", "Jes"), ("JER", "Jeremia", "Jer"),
    ("LAM", "Klagelieder", "Klgl"), ("EZK", "Hesekiel", "Hes"),
    ("DAN", "Daniel", "Dan"), ("HOS", "Hosea", "Hos"),
    ("JOL", "Joel", "Joel"), ("AMO", "Amos", "Am"),
    ("OBA", "Obadja", "Obd"), ("JON", "Jona", "Jona"),
    ("MIC", "Micha", "Mi"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habakuk", "Hab"), ("ZEP", "Zephanja", "Zef"),
    ("HAG", "Haggai", "Hag"), ("ZEC", "Sacharja", "Sach"),
    ("MAL", "Maleachi", "Mal"), ("MAT", "Matthäus", "Mt"),
    ("MRK", "Markus", "Mk"), ("LUK", "Lukas", "Lk"),
    ("JHN", "Johannes", "Joh"), ("ACT", "Apostelgeschichte", "Apg"),
    ("ROM", "Römer", "Röm"), ("1CO", "1. Korinther", "1. Kor"),
    ("2CO", "2. Korinther", "2. Kor"), ("GAL", "Galater", "Gal"),
    ("EPH", "Epheser", "Eph"), ("PHP", "Philipper", "Phil"),
    ("COL", "Kolosser", "Kol"), ("1TH", "1. Thessalonicher", "1. Thess"),
    ("2TH", "2. Thessalonicher", "2. Thess"), ("1TI", "1. Timotheus", "1. Tim"),
    ("2TI", "2. Timotheus", "2. Tim"), ("TIT", "Titus", "Tit"),
    ("PHM", "Philemon", "Phlm"), ("HEB", "Hebräer", "Hebr"),
    ("JAS", "Jakobus", "Jak"), ("1PE", "1. Petrus", "1. Petr"),
    ("2PE", "2. Petrus", "2. Petr"), ("1JN", "1. Johannes", "1. Joh"),
    ("2JN", "2. Johannes", "2. Joh"), ("3JN", "3. Johannes", "3. Joh"),
    ("JUD", "Judas", "Jud"), ("REV", "Offenbarung", "Offb"),
];

#[rustfmt::skip]
pub const FR_CRAMPON_LABELS: &[Label] = &[
    ("GEN", "Genèse", "Gn"), ("EXO", "Exode", "Ex"),
    ("LEV", "Lévitique", "Lv"), ("NUM", "Nombres", "Nb"),
    ("DEU", "Deutéronome", "Dt"), ("JOS", "Josué", "Jos"),
    ("JDG", "Juges", "Jg"), ("RUT", "Ruth", "Rt"),
    ("1SA", "1 Samuel", "1 S"), ("2SA", "2 Samuel", "2 S"),
    ("1KI", "1 Rois", "1 R"), ("2KI", "2 Rois", "2 R"),
    ("1CH", "1 Chroniques", "1 Ch"), ("2CH", "2 Chroniques", "2 Ch"),
    ("EZR", "Esdras", "Esd"), ("NEH", "Néhémie", "Ne"),
    ("EST", "Esther", "Est"), ("JOB", "Job", "Jb"),
    ("PSA", "Psaumes", "Ps"), ("PRO", "Proverbes", "Pr"),
    ("ECC", "Ecclésiaste", "Ec"), ("SNG", "Cantique des Cantiques", "Ct"),
    ("ISA", "Isaïe", "Is"), ("JER", "Jérémie", "Jr"),
    ("LAM", "Lamentations", "Lm"), ("EZK", "Ézéchiel", "Ez"),
    ("DAN", "Daniel", "Dn"), ("HOS", "Osée", "Os"),
    ("JOL", "Joël", "Jl"), ("AMO", "Amos", "Am"),
    ("OBA", "Abdias", "Ab"), ("JON", "Jonas", "Jon"),
    ("MIC", "Michée", "Mi"), ("NAM", "Nahum", "Na"),
    ("HAB", "Habacuc", "Ha"), ("ZEP", "Sophonie", "So"),
    ("HAG", "Aggée", "Ag"), ("ZEC", "Zacharie", "Za"),
    ("MAL", "Malachie", "Ml"), ("MAT", "Matthieu", "Mt"),
    ("MRK", "Marc", "Mc"), ("LUK", "Luc", "Lc"),
    ("JHN", "Jean", "Jn"), ("ACT", "Actes des Apôtres", "Ac"),
    ("ROM", "Romains", "Rm"), ("1CO", "1 Corinthiens", "1 Co"),
    ("2CO", "2 Corinthiens", "2 Co"), ("GAL", "Galates", "Ga"),
    ("EPH", "Éphésiens", "Ep"), ("PHP", "Philippiens", "Ph"),
    ("COL", "Colossiens", "Col"), ("1TH", "1 Thessaloniciens", "1 Th"),
    ("2TH", "2 Thessaloniciens", "2 Th"), ("1TI", "1 Timothée", "1 Tm"),
    ("2TI", "2 Timothée", "2 Tm"), ("TIT", "Tite", "Tt"),
    ("PHM", "Philémon", "Phm"), ("HEB", "Hébreux", "He"),
    ("JAS", "Jacques", "Jc"), ("1PE", "1 Pierre", "1 P"),
    ("2PE", "2 Pierre", "2 P"), ("1JN", "1 Jean", "1 Jn"),
    ("2JN", "2 Jean", "2 Jn"), ("3JN", "3 Jean", "3 Jn"),
    ("JUD", "Jude", "Jude"), ("REV", "Apocalypse", "Ap"),
];

#[rustfmt::skip]
pub const PT_BLIVRE_LABELS: &[Label] = &[
    ("GEN", "Gênesis", "Gn"), ("EXO", "Êxodo", "Êx"),
    ("LEV", "Levítico", "Lv"), ("NUM", "Números", "Nm"),
    ("DEU", "Deuteronômio", "Dt"), ("JOS", "Josué", "Js"),
    ("JDG", "Juízes", "Jz"), ("RUT", "Rute", "Rt"),
    ("1SA", "1 Samuel", "1 Sm"), ("2SA", "2 Samuel", "2 Sm"),
    ("1KI", "1 Reis", "1 Rs"), ("2KI", "2 Reis", "2 Rs"),
    ("1CH", "1 Crônicas", "1 Cr"), ("2CH", "2 Crônicas", "2 Cr"),
    ("EZR", "Esdras", "Ed"), ("NEH", "Neemias", "Ne"),
    ("EST", "Ester", "Et"), ("JOB", "Jó", "Jó"),
    ("PSA", "Salmos", "Sl"), ("PRO", "Provérbios", "Pv"),
    ("ECC", "Eclesiastes", "Ec"), ("SNG", "Cânticos", "Ct"),
    ("ISA", "Isaías", "Is"), ("JER", "Jeremias", "Jr"),
    ("LAM", "Lamentações", "Lm"), ("EZK", "Ezequiel", "Ez"),
    ("DAN", "Daniel", "Dn"), ("HOS", "Oseias", "Os"),
    ("JOL", "Joel", "Jl"), ("AMO", "Amós", "Am"),
    ("OBA", "Obadias", "Ob"), ("JON", "Jonas", "Jn"),
    ("MIC", "Miqueias", "Mq"), ("NAM", "Naum", "Na"),
    ("HAB", "Habacuque", "Hc"), ("ZEP", "Sofonias", "Sf"),
    ("HAG", "Ageu", "Ag"), ("ZEC", "Zacarias", "Zc"),
    ("MAL", "Malaquias", "Ml"), ("MAT", "Mateus", "Mt"),
    ("MRK", "Marcos", "Mc"), ("LUK", "Lucas", "Lc"),
    ("JHN", "João", "Jo"), ("ACT", "Atos", "At"),
    ("ROM", "Romanos", "Rm"), ("1CO", "1 Coríntios", "1 Co"),
    ("2CO", "2 Coríntios", "2 Co"), ("GAL", "Gálatas", "Gl"),
    ("EPH", "Efésios", "Ef"), ("PHP", "Filipenses", "Fp"),
    ("COL", "Colossenses", "Cl"), ("1TH", "1 Tessalonicenses", "1 Ts"),
    ("2TH", "2 Tessalonicenses", "2 Ts"), ("1TI", "1 Timóteo", "1 Tm"),
    ("2TI", "2 Timóteo", "2 Tm"), ("TIT", "Tito", "Tt"),
    ("PHM", "Filemom", "Fm"), ("HEB", "Hebreus", "Hb"),
    ("JAS", "Tiago", "Tg"), ("1PE", "1 Pedro", "1 Pe"),
    ("2PE", "2 Pedro", "2 Pe"), ("1JN", "1 João", "1 Jo"),
    ("2JN", "2 João", "2 Jo"), ("3JN", "3 João", "3 Jo"),
    ("JUD", "Judas", "Jd"), ("REV", "Apocalipse", "Ap"),
];

#[rustfmt::skip]
pub const LA_CLEMENTINE_LABELS: &[Label] = &[
    ("GEN", "Genesis", "Gen"), ("EXO", "Exodus", "Ex"),
    ("LEV", "Leviticus", "Lev"), ("NUM", "Numeri", "Num"),
    ("DEU", "Deuteronomium", "Deut"), ("JOS", "Josue", "Jos"),
    ("JDG", "Judices", "Jdc"), ("RUT", "Ruth", "Ruth"),
    ("1SA", "I Samuelis", "1 Sam"), ("2SA", "II Samuelis", "2 Sam"),
    ("1KI", "I Regum", "1 Reg"), ("2KI", "II Regum", "2 Reg"),
    ("1CH", "I Paralipomenon", "1 Par"), ("2CH", "II Paralipomenon", "2 Par"),
    ("EZR", "Esdras", "Esd"), ("NEH", "Nehemias", "Neh"),
    ("EST", "Esther", "Esth"), ("JOB", "Job", "Job"),
    ("PSA", "Psalmi", "Ps"), ("PRO", "Proverbia", "Prov"),
    ("ECC", "Ecclesiastes", "Eccl"), ("SNG", "Canticum Canticorum", "Cant"),
    ("ISA", "Isaias", "Is"), ("JER", "Jeremias", "Jer"),
    ("LAM", "Lamentationes", "Lam"), ("EZK", "Ezechiel", "Ez"),
    ("DAN", "Daniel", "Dan"), ("HOS", "Osee", "Os"),
    ("JOL", "Joel", "Joel"), ("AMO", "Amos", "Am"),
    ("OBA", "Abdias", "Abd"), ("JON", "Jonas", "Jon"),
    ("MIC", "Michaeas", "Mich"), ("NAM", "Nahum", "Nah"),
    ("HAB", "Habacuc", "Hab"), ("ZEP", "Sophonias", "Soph"),
    ("HAG", "Aggaeus", "Agg"), ("ZEC", "Zacharias", "Zach"),
    ("MAL", "Malachias", "Mal"), ("MAT", "Matthaeus", "Matt"),
    ("MRK", "Marcus", "Marc"), ("LUK", "Lucas", "Luc"),
    ("JHN", "Joannes", "Joa"), ("ACT", "Actus Apostolorum", "Act"),
    ("ROM", "Ad Romanos", "Rom"), ("1CO", "I ad Corinthios", "1 Cor"),
    ("2CO", "II ad Corinthios", "2 Cor"), ("GAL", "Ad Galatas", "Gal"),
    ("EPH", "Ad Ephesios", "Eph"), ("PHP", "Ad Philippenses", "Phil"),
    ("COL", "Ad Colossenses", "Col"), ("1TH", "I ad Thessalonicenses", "1 Thess"),
    ("2TH", "II ad Thessalonicenses", "2 Thess"), ("1TI", "I ad Timotheum", "1 Tim"),
    ("2TI", "II ad Timotheum", "2 Tim"), ("TIT", "Ad Titum", "Tit"),
    ("PHM", "Ad Philemonem", "Philem"), ("HEB", "Ad Hebraeos", "Hebr"),
    ("JAS", "Jacobi", "Jac"), ("1PE", "I Petri", "1 Pet"),
    ("2PE", "II Petri", "2 Pet"), ("1JN", "I Joannis", "1 Joa"),
    ("2JN", "II Joannis", "2 Joa"), ("3JN", "III Joannis", "3 Joa"),
    ("JUD", "Judae", "Jud"), ("REV", "Apocalypsis", "Apoc"),
];

/// Best labels for a translation code, with English KJV fallback.
///
/// Editions without a curated table — the remaining English versions
/// (en-asv, en-ylt, en-drc, en-bsb) — fall back to `KJV_LABELS`.
pub fn labels_for(code: &str) -> &'static [Label] {
    match code {
        "nb-1930" => NB_1930_LABELS,
        "es-rv1909" => ES_RV1909_LABELS,
        "de-menge" => DE_MENGE_LABELS,
        "fr-crampon" => FR_CRAMPON_LABELS,
        "pt-blivre" => PT_BLIVRE_LABELS,
        "la-clementine" => LA_CLEMENTINE_LABELS,
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
    fn de_menge_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = DE_MENGE_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    #[test]
    fn fr_crampon_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = FR_CRAMPON_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    #[test]
    fn pt_blivre_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = PT_BLIVRE_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    #[test]
    fn la_clementine_labels_cover_all_books() {
        let osis = osis_set();
        let labelled: HashSet<&str> = LA_CLEMENTINE_LABELS.iter().map(|(o, _, _)| *o).collect();
        assert_eq!(osis, labelled);
    }

    /// Each table's abbreviations must be unique so the Goto longest-match
    /// can't be steered onto the wrong book.
    #[test]
    fn abbreviations_unique_within_each_table() {
        for (code, table) in [
            ("en-kjv", KJV_LABELS),
            ("nb-1930", NB_1930_LABELS),
            ("es-rv1909", ES_RV1909_LABELS),
            ("de-menge", DE_MENGE_LABELS),
            ("fr-crampon", FR_CRAMPON_LABELS),
            ("pt-blivre", PT_BLIVRE_LABELS),
            ("la-clementine", LA_CLEMENTINE_LABELS),
        ] {
            let abbrs: HashSet<&str> = table.iter().map(|(_, _, a)| *a).collect();
            assert_eq!(abbrs.len(), table.len(), "duplicate abbreviation in {code}");
        }
    }

    #[test]
    fn labels_for_known_dispatch() {
        // Probe the dispatch via a known-distinct label per table —
        // `std::ptr::eq` on slice references compares fat pointers and the
        // compiler is free to materialise separate references to a const.
        assert_eq!(
            lookup(labels_for("nb-1930"), "GEN").unwrap().0,
            "Første Mosebok"
        );
        assert_eq!(lookup(labels_for("es-rv1909"), "GEN").unwrap().0, "Génesis");
        assert_eq!(lookup(labels_for("de-menge"), "GEN").unwrap().0, "1. Mose");
        assert_eq!(lookup(labels_for("fr-crampon"), "GEN").unwrap().0, "Genèse");
        assert_eq!(lookup(labels_for("pt-blivre"), "GEN").unwrap().0, "Gênesis");
        // Latin GEN is "Genesis" as in English, so probe a book that differs.
        assert_eq!(
            lookup(labels_for("la-clementine"), "JHN").unwrap().0,
            "Joannes"
        );
        assert_eq!(lookup(labels_for("en-kjv"), "GEN").unwrap().0, "Genesis");
        // Other English edition with no table → KJV fallback.
        assert_eq!(lookup(labels_for("en-bsb"), "GEN").unwrap().0, "Genesis");
    }

    #[test]
    fn lookup_returns_localised_name() {
        let (name, abbr) = lookup(NB_1930_LABELS, "JHN").expect("JHN in nb-1930");
        assert_eq!(name, "Johannes");
        assert_eq!(abbr, "Joh");
    }
}
