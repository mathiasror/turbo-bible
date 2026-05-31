//! Cross-pane word-level diff for the compare view.
//!
//! When two or more translations are read side by side, this highlights the
//! words that diverge between them so the reader sees at a glance where the
//! wordings part company. It's a read-only reading aid layered on the existing
//! render — it never moves a cursor or syncs scroll.
//!
//! ## Model — cross-pane consensus (set intersection)
//!
//! Panes are grouped by `(language, book, chapter)`. Within each group of two
//! or more panes, a verse's word is **divergent** when its case-folded form
//! isn't shared by *every* pane in the group that carries that verse number. A
//! word every aligned pane agrees on stays calm; anything that varies is lit.
//!
//! This is deliberately positionless and order-independent — it compares the
//! *set* of words, not their alignment. That buys three things over a pairwise
//! diff: it needs no "reference" pane, it scales to any number of panes, and a
//! word that merely moved (same vocabulary, different order) doesn't flag, so
//! the highlight doesn't jitter as the focus moves between panes.
//!
//! The grouping key is doing real work in two directions:
//! * **Language** — word-by-word comparison across languages (KJV vs Bibelen
//!   1930) is meaningless, so panes only ever compare within their language.
//! * **Location** — compare panes are independent readers; one can be moved to
//!   a different book/chapter (or opened on a cross-reference). Panes that
//!   aren't on the same passage fall into singleton groups and never diff.
//!
//! Known limitation (see README "What's not in v1"): because the model is
//! positionless, a word that appears more than once in a verse where only one
//! occurrence differs, or a pure word-order change, is reported at the
//! vocabulary level rather than per-position.

use std::collections::{HashMap, HashSet};

use unicode_segmentation::UnicodeSegmentation;

/// One pane's result: verse number → the set of case-folded word keys that
/// diverge from the cross-pane consensus. A verse missing from the map (or
/// mapped to an empty set) means "nothing to highlight in this verse".
pub type PaneDiff = HashMap<i64, HashSet<String>>;

/// One pane's contribution to the diff: its `(language, book, chapter)`
/// grouping key plus its verses as `(number, text)` pairs.
pub struct DiffInput<'a> {
    pub language: &'a str,
    pub book_code: &'a str,
    pub chapter: i64,
    pub verses: &'a [(i64, &'a str)],
}

/// Case-folded word cores of `text` — UAX #29 word boundaries, with
/// punctuation and whitespace excluded (so `"loved,"` keys as `loved`) and
/// case folded (so `"The"` and `"the"` don't flag as different).
///
/// The renderer ([`crate::render`]) independently applies the same rule
/// (`unicode_word_indices()` + `to_lowercase()` — it needs the byte offsets,
/// which this iterator drops), so the keys it tests against these consensus
/// sets agree on what counts as a "word". This is the canonical definition.
fn word_keys(text: &str) -> impl Iterator<Item = String> + '_ {
    text.unicode_words().map(str::to_lowercase)
}

/// Compute the per-pane divergent-key maps. `inputs[i]` corresponds to
/// `result[i]`. Panes whose language is empty (unknown) are never grouped, and
/// a group of one yields an empty map — both render exactly as today.
#[must_use]
pub fn compute(inputs: &[DiffInput]) -> Vec<PaneDiff> {
    let mut out: Vec<PaneDiff> = vec![PaneDiff::new(); inputs.len()];

    // Bucket pane indices by their grouping key.
    let mut groups: HashMap<(&str, &str, i64), Vec<usize>> = HashMap::new();
    for (i, p) in inputs.iter().enumerate() {
        if p.language.is_empty() {
            continue; // unknown language — don't guess at alignment
        }
        groups
            .entry((p.language, p.book_code, p.chapter))
            .or_default()
            .push(i);
    }

    for members in groups.values() {
        if members.len() >= 2 {
            diff_group(inputs, members, &mut out);
        }
    }
    out
}

/// Fill `out` for one group of two or more aligned panes.
fn diff_group(inputs: &[DiffInput], members: &[usize], out: &mut [PaneDiff]) {
    // Per member: verse number → that verse's key set. Built once, in `members`
    // order — so `per_pane[k]` is the verse map for pane `members[k]`. The
    // write-back loop below relies on that alignment (`per_pane.iter().zip(members)`).
    let per_pane: Vec<HashMap<i64, HashSet<String>>> = members
        .iter()
        .map(|&i| {
            inputs[i]
                .verses
                .iter()
                .map(|&(n, t)| (n, word_keys(t).collect::<HashSet<String>>()))
                .collect()
        })
        .collect();

    // Every verse number present anywhere in the group.
    let mut verse_nums: HashSet<i64> = HashSet::new();
    for vp in &per_pane {
        verse_nums.extend(vp.keys().copied());
    }

    for n in verse_nums {
        // The panes that actually carry this verse (versification differs, so a
        // verse can be absent from some translations).
        let present: Vec<&HashSet<String>> = per_pane.iter().filter_map(|vp| vp.get(&n)).collect();
        if present.len() < 2 {
            continue; // only one pane has it — nothing to compare against
        }

        // How many of the present panes contain each key.
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for set in &present {
            for k in *set {
                *counts.entry(k.as_str()).or_default() += 1;
            }
        }
        let total = present.len();

        // A key is divergent when some present pane lacks it.
        for (slot, member) in per_pane.iter().zip(members) {
            let Some(set) = slot.get(&n) else { continue };
            let divergent: HashSet<String> = set
                .iter()
                .filter(|k| counts.get(k.as_str()).copied().unwrap_or(0) < total)
                .cloned()
                .collect();
            if !divergent.is_empty() {
                out[*member].insert(n, divergent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(language: &'a str, verses: &'a [(i64, &'a str)]) -> DiffInput<'a> {
        DiffInput {
            language,
            book_code: "JHN",
            chapter: 3,
            verses,
        }
    }

    /// Divergent keys for pane `i`, verse `n`, as a sorted Vec for assertions.
    fn keys(diffs: &[PaneDiff], i: usize, n: i64) -> Vec<String> {
        let mut v: Vec<String> = diffs[i]
            .get(&n)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn identical_verses_have_no_divergence() {
        let a = [(16i64, "For God so loved the world")];
        let b = [(16i64, "For God so loved the world")];
        let diffs = compute(&[input("en", &a), input("en", &b)]);
        assert!(keys(&diffs, 0, 16).is_empty());
        assert!(keys(&diffs, 1, 16).is_empty());
    }

    #[test]
    fn one_differing_word_is_flagged_in_both_panes() {
        let a = [(1i64, "and his raiment was white")];
        let b = [(1i64, "and his garments was white")];
        let diffs = compute(&[input("en", &a), input("en", &b)]);
        assert_eq!(keys(&diffs, 0, 1), vec!["raiment"]);
        assert_eq!(keys(&diffs, 1, 1), vec!["garments"]);
    }

    #[test]
    fn comparison_is_case_insensitive_but_punctuation_folds_out() {
        // "The"/"the" must agree; "loved," and "loved" must agree (the trailing
        // comma is not part of the key).
        let a = [(1i64, "The Lord loved, truly")];
        let b = [(1i64, "the Lord loved truly")];
        let diffs = compute(&[input("en", &a), input("en", &b)]);
        assert!(
            keys(&diffs, 0, 1).is_empty(),
            "case + trailing punctuation should not count as divergence: {:?}",
            keys(&diffs, 0, 1)
        );
    }

    #[test]
    fn n_way_flags_a_word_missing_from_any_pane() {
        // "begotten" is in two of three panes → not shared by all → divergent
        // in the panes that carry it.
        let a = [(16i64, "his only begotten son")];
        let b = [(16i64, "his only begotten son")];
        let c = [(16i64, "his only son")];
        let diffs = compute(&[input("en", &a), input("en", &b), input("en", &c)]);
        assert_eq!(keys(&diffs, 0, 16), vec!["begotten"]);
        assert_eq!(keys(&diffs, 1, 16), vec!["begotten"]);
        assert!(
            keys(&diffs, 2, 16).is_empty(),
            "the pane lacking the word has nothing extra to light"
        );
    }

    #[test]
    fn different_languages_never_diff() {
        let en = [(16i64, "For God so loved the world")];
        let nb = [(16i64, "For so har Gud elsket verden")];
        let diffs = compute(&[input("en", &en), input("nb", &nb)]);
        assert!(keys(&diffs, 0, 16).is_empty());
        assert!(keys(&diffs, 1, 16).is_empty());
    }

    #[test]
    fn different_chapters_never_diff() {
        let a_verses = [(16i64, "wholly unrelated wording here")];
        let b_verses = [(16i64, "entirely separate text indeed")];
        let a = DiffInput {
            language: "en",
            book_code: "JHN",
            chapter: 3,
            verses: &a_verses,
        };
        let b = DiffInput {
            language: "en",
            book_code: "JHN",
            chapter: 4, // different chapter → different group
            verses: &b_verses,
        };
        let diffs = compute(&[a, b]);
        assert!(keys(&diffs, 0, 16).is_empty());
        assert!(keys(&diffs, 1, 16).is_empty());
    }

    #[test]
    fn empty_language_is_left_ungrouped() {
        let a = [(1i64, "alpha beta")];
        let b = [(1i64, "alpha gamma")];
        let diffs = compute(&[input("", &a), input("", &b)]);
        assert!(keys(&diffs, 0, 1).is_empty());
        assert!(keys(&diffs, 1, 1).is_empty());
    }

    #[test]
    fn single_pane_group_has_no_diff() {
        let a = [(1i64, "alpha beta gamma")];
        let diffs = compute(&[input("en", &a)]);
        assert!(keys(&diffs, 0, 1).is_empty());
    }

    #[test]
    fn versification_gap_compares_per_verse_not_per_group() {
        // Verse 2 exists in panes 0 and 2 but not pane 1 (a real versification
        // gap). Consensus for verse 2 is over the panes that carry it, so the
        // denominator is 2, not the group size of 3.
        let a = [(1i64, "shared word"), (2i64, "alpha beta")];
        let b = [(1i64, "shared word")]; // no verse 2
        let c = [(1i64, "shared word"), (2i64, "alpha gamma")];
        let diffs = compute(&[input("en", &a), input("en", &b), input("en", &c)]);
        // Verse 1: identical across all three → nothing.
        assert!(keys(&diffs, 0, 1).is_empty());
        // Verse 2: "alpha" is in both present panes; "beta"/"gamma" diverge.
        assert_eq!(keys(&diffs, 0, 2), vec!["beta"]);
        assert_eq!(keys(&diffs, 2, 2), vec!["gamma"]);
        // Pane 1 has no verse 2 → no entry at all.
        assert!(keys(&diffs, 1, 2).is_empty());
    }

    #[test]
    fn verse_present_in_only_one_pane_is_never_flagged() {
        // Verse 2 exists only in pane 0 → nothing to compare against.
        let a = [(1i64, "shared"), (2i64, "lonely verse text")];
        let b = [(1i64, "shared")];
        let diffs = compute(&[input("en", &a), input("en", &b)]);
        assert!(keys(&diffs, 0, 2).is_empty());
    }

    #[test]
    fn duplicate_word_in_one_pane_is_not_flagged() {
        // "love" appears twice in pane 0, once in pane 1. The set model counts
        // per-pane presence, not occurrences, so it stays calm in both.
        let a = [(1i64, "love love peace")];
        let b = [(1i64, "love peace")];
        let diffs = compute(&[input("en", &a), input("en", &b)]);
        assert!(keys(&diffs, 0, 1).is_empty());
        assert!(keys(&diffs, 1, 1).is_empty());
    }

    #[test]
    fn diacritics_distinguish_words_but_case_still_folds() {
        // Spanish: "Dios"/"Dios" agree; "creó"/"creo" differ by the accent and
        // are treated as distinct words (we fold case, not diacritics).
        let a = [(1i64, "Dios creó todo")];
        let b = [(1i64, "Dios creo todo")];
        let diffs = compute(&[input("es", &a), input("es", &b)]);
        assert_eq!(keys(&diffs, 0, 1), vec!["creó"]);
        assert_eq!(keys(&diffs, 1, 1), vec!["creo"]);
    }
}
