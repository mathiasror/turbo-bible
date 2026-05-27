# Importing your own translation

`turbo-bible` ships eleven curated translations, but you can add your
own from a JSON file with the `import` subcommand. It builds a
schema-correct SQLite database and installs it into the translations
directory, ready to read on the next launch — no scrollmapper checkout
and no data pipeline required.

```sh
turbo-bible import myversion.json \
  --code xx-myver --name "My Version" --language xx
```

After it runs, the translation is available via `--translation xx-myver`
and appears in the in-app picker (`t` / `F5`).

## CLI

```
turbo-bible import <FILE> --code <CODE> --name <NAME> --language <LANG>
                         [--license <SPDX>] [--attribution <TEXT>]
                         [--force] [--translations-dir <DIR>]
```

| Flag                  | Required | Default              | Purpose |
| --------------------- | :------: | -------------------- | ------- |
| `<FILE>`              | ✓        | —                    | Path to the input JSON (format below). |
| `--code`              | ✓        | —                    | Translation code; becomes both `meta.code` and the on-disk `<code>.db` filename. Lowercase letters, digits and hyphens only (e.g. `en-web`, `grc-na28`); must not be `xrefs` or a built-in code (e.g. `en-kjv`). |
| `--name`              | ✓        | —                    | Human-readable name shown in the picker. |
| `--language`          | ✓        | —                    | Language tag (e.g. `en`, `nb`, `la`, `grc`). |
| `--license`           |          | `LicenseRef-Unknown` | SPDX license expression for the text. |
| `--attribution`       |          | `""`                 | Attribution line (required by some licenses, e.g. CC-BY). |
| `--force`             |          | off                  | Overwrite an existing `<code>.db` instead of erroring. |
| `--translations-dir`  |          | XDG data dir         | Install somewhere other than `~/.local/share/turbo-bible/translations/`. |

The command writes atomically (a sibling temp file is renamed into
place), so an interrupted import never leaves a half-written database.

## Input JSON format

A single object with a `books` array. Each book has a `book` identifier,
optional label overrides, and `chapters`; each chapter has a number and a
`verses` array.

```json
{
  "books": [
    {
      "book": "JHN",
      "name": "John",
      "abbreviation": "Jn",
      "chapters": [
        {
          "chapter": 3,
          "verses": [
            { "verse": 16, "text": "For God so loved the world…" }
          ]
        }
      ]
    }
  ]
}
```

### Fields

| Path                        | Type     | Required | Notes |
| --------------------------- | -------- | :------: | ----- |
| `books[]`                   | array    | ✓        | At least one verse total, or the import is rejected. |
| `books[].book`              | string   | ✓        | OSIS code (e.g. `JHN`) **or** English book name (e.g. `John`). Case-insensitive. See the table below. |
| `books[].name`              | string   |          | Display name for this book. Defaults to the English name. |
| `books[].abbreviation`      | string   |          | Short label (alias: `abbr`). Defaults to the English abbreviation. |
| `books[].chapters[]`        | array    | ✓        | — |
| `books[].chapters[].chapter`| integer  | ✓        | Chapter number. |
| `…chapters[].verses[]`      | array    | ✓        | — |
| `…verses[].verse`           | integer  | ✓        | Verse number. Unique within a `(book, chapter)`. |
| `…verses[].text`            | string   | ✓        | Verse text. Leading/trailing whitespace is trimmed. |

### Rules and behavior

- **Book identity** resolves against the OSIS code first, then the
  English name (both case-insensitive). An identifier that matches
  neither is a hard error — nothing is silently dropped.
- **Partial Bibles are fine.** Only the books you list are inserted, so a
  single Gospel or a Psalter works; navigation and the book picker then
  list just those books.
- **`name`/`abbreviation`** set the displayed book labels. Omit them to
  use the built-in English defaults.
- **Duplicate verses** (same `book`/`chapter`/`verse`) and a book listed
  twice are rejected with a clear error.
- Unknown top-level/extra fields are ignored, so you can keep your own
  metadata in the file.

## Output SQLite schema

`import` produces exactly the schema the reader expects — the same one
the offline pipeline emits (`crates/turbo-bible-data/src/schema.rs`,
`schema_version = 1`). One file is one translation; there is no
`translation` column because the filename is the identity. Cross-references
live in a **separate, shared `xrefs.db`** that `import` does not touch.

```sql
CREATE TABLE meta (
  code           TEXT PRIMARY KEY,   -- equals --code (and the filename stem)
  name           TEXT NOT NULL,      -- --name
  language       TEXT NOT NULL,      -- --language
  license        TEXT NOT NULL,      -- --license
  attribution    TEXT NOT NULL,      -- --attribution
  source_commit  TEXT NOT NULL,      -- "user-import" for imported files
  built_at       INTEGER NOT NULL,   -- unix timestamp of the import
  verse_count    INTEGER NOT NULL,   -- number of verses inserted
  schema_version INTEGER NOT NULL    -- 1
);

CREATE TABLE book (
  code      TEXT PRIMARY KEY,        -- OSIS code, e.g. "JHN"
  testament TEXT NOT NULL CHECK (testament IN ('OT','NT')),
  ord       INTEGER NOT NULL UNIQUE  -- canonical position 1–66
);

CREATE TABLE book_label (
  book         TEXT PRIMARY KEY REFERENCES book(code),
  name         TEXT NOT NULL,        -- displayed book name
  abbreviation TEXT NOT NULL,        -- short label
  full_name    TEXT                  -- set equal to name on import
);

CREATE TABLE verse (
  book    TEXT NOT NULL REFERENCES book(code),
  chapter INTEGER NOT NULL,
  verse   INTEGER NOT NULL,
  osis_id TEXT NOT NULL,             -- "<OSIS>.<chapter>.<verse>", e.g. "JHN.3.16"
  text    TEXT NOT NULL,
  PRIMARY KEY (book, chapter, verse)
);
CREATE INDEX verse_osis_idx ON verse(osis_id);

-- Created for schema compatibility; import leaves these EMPTY.
CREATE TABLE heading (
  book TEXT NOT NULL REFERENCES book(code),
  chapter INTEGER NOT NULL, before_verse INTEGER NOT NULL,
  style TEXT NOT NULL, text TEXT NOT NULL
);
CREATE INDEX heading_loc_idx ON heading(book, chapter, before_verse);

CREATE TABLE footnote (
  id TEXT NOT NULL PRIMARY KEY, verse_osis TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('f','x')), body TEXT NOT NULL
);
CREATE INDEX footnote_verse_idx ON footnote(verse_osis);

-- Full-text search. The AFTER INSERT trigger indexes verses as they are
-- inserted, so search works immediately — no first-launch rebuild.
CREATE VIRTUAL TABLE verse_fts USING fts5(
  text, content='verse', content_rowid='rowid',
  tokenize='unicode61 remove_diacritics 1', prefix='2 3'
);
-- (+ verse_ai / verse_ad / verse_au triggers keeping verse_fts in sync)
```

Building the database directly (without this command) is supported too:
emit the schema above, place `<code>.db` in the translations directory,
and launch with `--translation <code>`. The only hard runtime requirement
is that `meta.code` equals the filename stem.

## OSIS book codes

Use any of these as `book` (or the English name shown). Imports are
limited to the 66-book Protestant canon.

### Old Testament

| Code | Book | Code | Book | Code | Book |
| --- | --- | --- | --- | --- | --- |
| `GEN` | Genesis | `2CH` | 2 Chronicles | `DAN` | Daniel |
| `EXO` | Exodus | `EZR` | Ezra | `HOS` | Hosea |
| `LEV` | Leviticus | `NEH` | Nehemiah | `JOL` | Joel |
| `NUM` | Numbers | `EST` | Esther | `AMO` | Amos |
| `DEU` | Deuteronomy | `JOB` | Job | `OBA` | Obadiah |
| `JOS` | Joshua | `PSA` | Psalms | `JON` | Jonah |
| `JDG` | Judges | `PRO` | Proverbs | `MIC` | Micah |
| `RUT` | Ruth | `ECC` | Ecclesiastes | `NAM` | Nahum |
| `1SA` | 1 Samuel | `SNG` | Song of Solomon | `HAB` | Habakkuk |
| `2SA` | 2 Samuel | `ISA` | Isaiah | `ZEP` | Zephaniah |
| `1KI` | 1 Kings | `JER` | Jeremiah | `HAG` | Haggai |
| `2KI` | 2 Kings | `LAM` | Lamentations | `ZEC` | Zechariah |
| `1CH` | 1 Chronicles | `EZK` | Ezekiel | `MAL` | Malachi |

### New Testament

| Code | Book | Code | Book | Code | Book |
| --- | --- | --- | --- | --- | --- |
| `MAT` | Matthew | `EPH` | Ephesians | `HEB` | Hebrews |
| `MRK` | Mark | `PHP` | Philippians | `JAS` | James |
| `LUK` | Luke | `COL` | Colossians | `1PE` | 1 Peter |
| `JHN` | John | `1TH` | 1 Thessalonians | `2PE` | 2 Peter |
| `ACT` | Acts | `2TH` | 2 Thessalonians | `1JN` | 1 John |
| `ROM` | Romans | `1TI` | 1 Timothy | `2JN` | 2 John |
| `1CO` | 1 Corinthians | `2TI` | 2 Timothy | `3JN` | 3 John |
| `2CO` | 2 Corinthians | `TIT` | Titus | `JUD` | Jude |
| `GAL` | Galatians | `PHM` | Philemon | `REV` | Revelation |

## Caveats

- **No cross-references, headings, or footnotes** for imported text. The
  `heading`/`footnote` tables exist but are empty, and cross-references
  come from the shared `xrefs.db`, which is keyed by canonical reference
  and applies regardless of translation.
- The reader opens every `<code>.db` it finds in the translations
  directory, so an imported translation is selectable immediately —
  including from the `t` / `F5` picker. Selecting it persists it as the
  default for the next launch.
- Only the 66-book Protestant canon is supported; deuterocanonical books
  have no OSIS code here and are rejected.
