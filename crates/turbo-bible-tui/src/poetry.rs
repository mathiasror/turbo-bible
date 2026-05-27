//! Classify which passages the reading view sets as poetry, for the
//! cosmetic verse indent in [`crate::render`].
//!
//! This is a *whole-passage* classification keyed by OSIS book code (and
//! chapter, where a book mixes prose and poetry). We have no intra-verse
//! line-break data — the scrollmapper source the pipeline ingests is
//! verse-level prose, with no `\q` markers or embedded newlines — so we
//! only set known-poetic passages apart with a flat left indent; we do
//! *not* reconstruct poetic line layout (Hebrew parallelism). That would
//! need a different data source (USFM/USX) and a schema change.
//!
//! The set is the high-confidence "this whole passage is poetry" core.
//! Ecclesiastes and the prophets (Isaiah, Jeremiah, the minor prophets,
//! …) are deliberately excluded: they interleave prose and poetry *within*
//! chapters, so a chapter-granularity indent would mis-indent their prose.

/// Whether the given book/chapter is rendered as poetry (and therefore
/// indented) in the reading view. `book_code` is the OSIS code (`PSA`,
/// `GEN`, …); `chapter` is 1-based.
#[must_use]
pub fn is_poetic(book_code: &str, chapter: i64) -> bool {
    match book_code {
        // Wholly poetic books.
        "PSA" | "PRO" | "SNG" | "LAM" => true,
        // Job: the prose prologue (1–2) stays flush. The poetic dialogue runs
        // 3:1–42:6, but chapter 42 also carries the prose epilogue (42:7–17),
        // so at chapter granularity we keep all of 42 flush — better to drop
        // the indent on Job's 6-verse reply than to indent the epilogue.
        "JOB" => (3..=41).contains(&chapter),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_poetic;

    #[test]
    fn wholly_poetic_books_are_poetic_in_every_chapter() {
        // (book, last chapter) — exercise the first and last real chapter of
        // each so the classifier holds across the whole book, not just ch 1.
        for (code, last) in [("PSA", 150), ("PRO", 31), ("SNG", 8), ("LAM", 5)] {
            assert!(is_poetic(code, 1), "{code} 1 should be poetic");
            assert!(is_poetic(code, last), "{code} {last} should be poetic");
        }
    }

    #[test]
    fn narrative_books_are_never_poetic() {
        for code in ["GEN", "EXO", "MAT", "ROM", "REV"] {
            assert!(!is_poetic(code, 1), "{code} should be prose");
        }
    }

    #[test]
    fn job_prose_frame_is_not_poetic() {
        // Prologue.
        assert!(!is_poetic("JOB", 1));
        assert!(!is_poetic("JOB", 2));
        // Narrative close.
        assert!(!is_poetic("JOB", 42));
    }

    #[test]
    fn job_dialogue_is_poetic() {
        assert!(is_poetic("JOB", 3));
        assert!(is_poetic("JOB", 20));
        assert!(is_poetic("JOB", 41));
    }

    #[test]
    fn unknown_book_code_is_not_poetic() {
        assert!(!is_poetic("", 1));
        assert!(!is_poetic("XYZ", 1));
    }
}
