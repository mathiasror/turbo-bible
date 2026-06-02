//! Small text-shaping helpers shared by the reading view and the splash.

use unicode_width::UnicodeWidthStr;

/// Terminal display width of `s`, in cells: wide (CJK/fullwidth) glyphs count
/// as 2 and zero-width/combining marks as 0. This is the correct measure when
/// laying text into a fixed-column grid — `str::chars().count()` over-counts
/// wide glyphs, so padding and wrapping derived from it misalign any non-Latin
/// text (e.g. an imported CJK translation). For the bundled Latin translations
/// `display_width == chars().count()`, so this is a no-op for shipped content.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Greedy whitespace-respecting word wrap. Splits `text` into lines no wider
/// than `max_width` display columns; a word wider than `max_width` becomes its
/// own (over-long) line rather than being broken.
///
/// `max_width == 0` returns a single line containing the input unchanged.
pub fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
            continue;
        }
        // +1 for the joining space.
        if display_width(&current) + 1 + display_width(word) <= max_width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Lowercase `s` and strip the Latin diacritics present in the corpus, so an
/// un-accented query resolves the same way FTS5 search does. The Find index is
/// built `tokenize='unicode61 remove_diacritics 1'`, which folds both case and
/// diacritics; Goto and the splash book filter call this so typing the plain
/// ASCII form (`genesis`, `joao`) reaches accented book names (`Génesis`,
/// `João`) instead of dead-ending where search succeeds.
///
/// Dependency-free by deliberate choice — the project keeps a minimal
/// dependency tree, so this is a hand-rolled `char` map over the Latin-1 /
/// Latin Extended marks the bundled translations actually carry rather than a
/// full Unicode normalization pass.
#[must_use]
pub fn fold_diacritics(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_diacritics_strips_latin_accents() {
        assert_eq!(fold_diacritics("Génesis"), "genesis");
        assert_eq!(fold_diacritics("João"), "joao");
        assert_eq!(fold_diacritics("Éxodo"), "exodo");
        assert_eq!(fold_diacritics("Números"), "numeros");
    }

    #[test]
    fn fold_diacritics_passes_ascii_through() {
        assert_eq!(fold_diacritics("John"), "john");
    }

    #[test]
    fn wraps_long_paragraph() {
        let out = word_wrap("the quick brown fox jumps", 10);
        assert!(out.iter().all(|l| l.chars().count() <= 10));
        assert_eq!(out.join(" "), "the quick brown fox jumps");
    }

    #[test]
    fn preserves_words_longer_than_width() {
        let out = word_wrap("supercalifragilistic", 5);
        assert_eq!(out, vec!["supercalifragilistic".to_string()]);
    }

    #[test]
    fn zero_width_returns_input() {
        let out = word_wrap("a b c", 0);
        assert_eq!(out, vec!["a b c".to_string()]);
    }

    #[test]
    fn display_width_counts_wide_glyphs_as_two() {
        assert_eq!(display_width("abc"), 3);
        // Two fullwidth CJK ideographs occupy four terminal cells.
        assert_eq!(display_width("世界"), 4);
    }

    #[test]
    fn wraps_by_display_width_not_char_count() {
        // "世界" and "你好" are width 4 each: 4 + 1 + 4 = 9 > 7, so they wrap.
        // A char-count metric would see 2 + 1 + 2 = 5 and wrongly keep them on
        // one (visually overflowing) line.
        let out = word_wrap("世界 你好", 7);
        assert_eq!(out, vec!["世界".to_string(), "你好".to_string()]);
    }
}
