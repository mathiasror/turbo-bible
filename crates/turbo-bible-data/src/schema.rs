//! SQL schemas for the data pipeline's output files.
//!
//! `TRANSLATION_SCHEMA_SQL` is per-translation (one file = one
//! translation, so there's no `translation` column on any content table —
//! the filename is the identity). `XREF_SCHEMA_SQL` is the shared
//! `xrefs.db` schema.
//!
//! Order of statements matters for FTS5: the virtual table must be
//! declared before the triggers that maintain it.

/// Schema version stamped into the per-translation `meta` table.
/// Bump deliberately when changing [`TRANSLATION_SCHEMA_SQL`].
pub const SCHEMA_VERSION: i64 = 1;

pub const TRANSLATION_SCHEMA_SQL: &str = "
CREATE TABLE meta (
  code           TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  language       TEXT NOT NULL,
  license        TEXT NOT NULL,
  attribution    TEXT NOT NULL,
  source_commit  TEXT NOT NULL,
  built_at       INTEGER NOT NULL,
  verse_count    INTEGER NOT NULL,
  schema_version INTEGER NOT NULL
);

CREATE TABLE book (
  code      TEXT PRIMARY KEY,
  testament TEXT NOT NULL CHECK (testament IN ('OT','NT')),
  ord       INTEGER NOT NULL UNIQUE
);

CREATE TABLE book_label (
  book         TEXT PRIMARY KEY REFERENCES book(code),
  name         TEXT NOT NULL,
  abbreviation TEXT NOT NULL,
  full_name    TEXT
);

CREATE TABLE verse (
  book    TEXT NOT NULL REFERENCES book(code),
  chapter INTEGER NOT NULL,
  verse   INTEGER NOT NULL,
  osis_id TEXT NOT NULL,
  text    TEXT NOT NULL,
  PRIMARY KEY (book, chapter, verse)
);
CREATE INDEX verse_osis_idx ON verse(osis_id);

CREATE TABLE heading (
  book          TEXT NOT NULL REFERENCES book(code),
  chapter       INTEGER NOT NULL,
  before_verse  INTEGER NOT NULL,
  style         TEXT NOT NULL,
  text          TEXT NOT NULL
);
CREATE INDEX heading_loc_idx ON heading(book, chapter, before_verse);

CREATE TABLE footnote (
  id          TEXT NOT NULL,
  verse_osis  TEXT NOT NULL,
  kind        TEXT NOT NULL CHECK (kind IN ('f','x')),
  body        TEXT NOT NULL,
  PRIMARY KEY (id)
);
CREATE INDEX footnote_verse_idx ON footnote(verse_osis);

CREATE VIRTUAL TABLE verse_fts USING fts5(
  text,
  content='verse',
  content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 1',
  prefix='2 3'
);

-- Keep FTS in sync with `verse` for live writes. Mirrors the trigger
-- shape in crates/turbo-bible-tui/src/db.rs:42.
CREATE TRIGGER verse_ai AFTER INSERT ON verse BEGIN
  INSERT INTO verse_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER verse_ad AFTER DELETE ON verse BEGIN
  INSERT INTO verse_fts(verse_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
END;
CREATE TRIGGER verse_au AFTER UPDATE ON verse BEGIN
  INSERT INTO verse_fts(verse_fts, rowid, text) VALUES ('delete', old.rowid, old.text);
  INSERT INTO verse_fts(rowid, text) VALUES (new.rowid, new.text);
END;
";

pub const XREF_SCHEMA_SQL: &str = "
CREATE TABLE book (
  code      TEXT PRIMARY KEY,
  testament TEXT NOT NULL CHECK (testament IN ('OT','NT')),
  ord       INTEGER NOT NULL UNIQUE
);

CREATE TABLE xref (
  from_book       TEXT NOT NULL REFERENCES book(code),
  from_chapter    INTEGER NOT NULL,
  from_verse      INTEGER NOT NULL,
  to_book         TEXT NOT NULL REFERENCES book(code),
  to_chapter      INTEGER NOT NULL,
  to_verse_start  INTEGER NOT NULL,
  to_verse_end    INTEGER NOT NULL,
  votes           INTEGER NOT NULL,
  PRIMARY KEY (from_book, from_chapter, from_verse,
               to_book, to_chapter, to_verse_start, to_verse_end)
);
CREATE INDEX xref_from_idx
  ON xref(from_book, from_chapter, from_verse, votes DESC);
";
