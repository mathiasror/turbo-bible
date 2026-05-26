//! FTS5 search over `verse_fts`. Returns BM25-ranked hits with byte ranges
//! suitable for highlighting in `ratatui::text::Span`.
//!
//! We use the FTS5 `highlight(...)` auxiliary function — `offsets(...)` is an
//! FTS3/FTS4 function and isn't available on FTS5 tables — and parse the
//! ASCII-control delimiters back into ranges over the original text.

use anyhow::Result;
use rusqlite::params;

use crate::db::Db;

const MATCH_START: char = '\u{0001}';
const MATCH_END: char = '\u{0002}';

/// Hit cap for `n`/`N` repeat-search in the reading view. Common-word
/// queries (e.g. `the`) match tens of thousands of verses; we sort the
/// hits canonically and walk them, so the cap exists purely to bound
/// memory + sort cost in the corner case. Keep this comfortably larger
/// than the 50-row cap the Find dialog shows interactively — the repeat
/// path runs against the whole corpus, not a screenful.
pub const REPEAT_LIMIT: usize = 1000;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub book: String,
    pub chapter: i64,
    pub verse: i64,
    pub text: String,
    /// Byte ranges within `text` that matched the query. Sorted.
    pub hits: Vec<(usize, usize)>,
}

/// Build an FTS5 MATCH expression from free-text user input. Quotes each
/// whitespace-separated token and ANDs them; immune to operator characters in
/// the input.
pub fn build_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// # Errors
/// Fails when the FTS5 MATCH or the `verse` join queries error.
/// An empty `input` returns `Ok(vec![])` rather than an error.
///
/// The active translation is implicit: queries run on `db.conn()` — the active
/// translation's own `Connection`.
pub fn search(db: &Db, input: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let query = build_query(input);
    if query.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = db.conn().prepare_cached(
        "SELECT v.book, v.chapter, v.verse,
                highlight(verse_fts, 0, char(1), char(2)) AS hilit
         FROM verse_fts
         JOIN verse v ON v.rowid = verse_fts.rowid
         WHERE verse_fts MATCH ?1
         ORDER BY bm25(verse_fts) LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(
            params![query, i64::try_from(limit).unwrap_or(i64::MAX)],
            |r| {
                let book: String = r.get(0)?;
                let chapter: i64 = r.get(1)?;
                let verse: i64 = r.get(2)?;
                let hilit: String = r.get(3)?;
                let (text, hits) = parse_highlighted(&hilit);
                Ok(SearchHit {
                    book,
                    chapter,
                    verse,
                    text,
                    hits,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Strip the `\x01`/`\x02` delimiters that `highlight()` injects and emit the
/// byte ranges (over the cleaned text) where each match sits.
///
/// The returned ranges are sorted by start offset and adjacent/overlapping
/// ranges are merged. Callers can iterate the slice without re-sorting. The
/// merge step relies on FTS5's `highlight()` emitting matches in textual
/// order — which it does — so the input scan only needs to look at the
/// previous range, not the entire `merged` vector.
pub fn parse_highlighted(s: &str) -> (String, Vec<(usize, usize)>) {
    let mut text = String::with_capacity(s.len());
    let mut hits: Vec<(usize, usize)> = Vec::new();
    let mut match_start: Option<usize> = None;
    for ch in s.chars() {
        match ch {
            MATCH_START => match_start = Some(text.len()),
            MATCH_END => {
                if let Some(start) = match_start.take() {
                    hits.push((start, text.len()));
                }
            }
            _ => text.push(ch),
        }
    }
    // Merge adjacent ranges (FTS5 can emit consecutive runs for adjacent terms).
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(hits.len());
    for r in hits {
        if let Some(last) = merged.last_mut()
            && r.0 <= last.1
        {
            last.1 = last.1.max(r.1);
            continue;
        }
        merged.push(r);
    }
    (text, merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_quotes_tokens() {
        assert_eq!(build_query("Jesaja"), "\"Jesaja\"");
        assert_eq!(
            build_query("profeten jesaja"),
            "\"profeten\" AND \"jesaja\""
        );
    }

    #[test]
    fn escapes_inner_quotes() {
        assert_eq!(build_query(r#"a"b"#), r#""a""b""#);
    }

    #[test]
    fn parses_simple_highlight() {
        let (text, hits) = parse_highlighted("\u{1}Jesaja\u{2} sa: dette er sant");
        assert_eq!(text, "Jesaja sa: dette er sant");
        assert_eq!(hits, vec![(0, 6)]);
    }

    #[test]
    fn parses_two_separated_matches() {
        let (text, hits) = parse_highlighted("Hos \u{1}profeten\u{2} \u{1}Jesaja\u{2}");
        assert_eq!(text, "Hos profeten Jesaja");
        // "profeten" = bytes 4..12, "Jesaja" = bytes 13..19; the space in
        // between keeps them as two distinct ranges.
        assert_eq!(hits, vec![(4, 12), (13, 19)]);
    }

    #[test]
    fn parses_unicode_bytes_correctly() {
        // æøå are 2 bytes each; offsets must point to byte boundaries.
        let (text, hits) = parse_highlighted("\u{1}sær\u{2} bok");
        assert_eq!(text, "sær bok");
        assert_eq!(hits, vec![(0, 4)]); // 's'(1) + 'æ'(2) + 'r'(1) = 4 bytes
    }
}
