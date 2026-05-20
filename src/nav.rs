//! Navigation helpers: walk the book/chapter graph in canonical order.

use anyhow::{Result, anyhow};

use crate::db::{Book, Db};

#[derive(Debug, Clone)]
pub struct Position {
    pub book: String,
    pub chapter: i64,
    /// When `Some(v)`, callers that *jump* to this position should land the
    /// cursor on verse `v` (e.g. find-results, bookmarks, `:John 3:16`).
    /// `None` means "no preference" — the receiver lands on verse 1.
    pub verse: Option<i64>,
}

pub struct Navigator<'a> {
    books: &'a [Book],
}

impl<'a> Navigator<'a> {
    pub fn new(books: &'a [Book]) -> Self {
        Self { books }
    }

    fn book_index(&self, code: &str) -> Result<usize> {
        self.books
            .iter()
            .position(|b| b.code == code)
            .ok_or_else(|| anyhow!("unknown book {code}"))
    }

    pub fn prev_chapter(&self, db: &Db, pos: &Position) -> Result<Position> {
        if pos.chapter > 1 {
            return Ok(Position {
                book: pos.book.clone(),
                chapter: pos.chapter - 1,
                verse: None,
            });
        }
        let idx = self.book_index(&pos.book)?;
        if idx == 0 {
            return Ok(pos.clone());
        }
        let prev = &self.books[idx - 1];
        let last = db.chapter_count(&prev.code)?.max(1);
        Ok(Position {
            book: prev.code.clone(),
            chapter: last,
            verse: None,
        })
    }

    pub fn next_chapter(&self, db: &Db, pos: &Position) -> Result<Position> {
        let current_max = db.chapter_count(&pos.book)?.max(1);
        if pos.chapter < current_max {
            return Ok(Position {
                book: pos.book.clone(),
                chapter: pos.chapter + 1,
                verse: None,
            });
        }
        let idx = self.book_index(&pos.book)?;
        if idx + 1 >= self.books.len() {
            return Ok(pos.clone());
        }
        Ok(Position {
            book: self.books[idx + 1].code.clone(),
            chapter: 1,
            verse: None,
        })
    }

    pub fn prev_book(&self, pos: &Position) -> Result<Position> {
        let idx = self.book_index(&pos.book)?;
        if idx == 0 {
            return Ok(pos.clone());
        }
        Ok(Position {
            book: self.books[idx - 1].code.clone(),
            chapter: 1,
            verse: None,
        })
    }

    pub fn next_book(&self, pos: &Position) -> Result<Position> {
        let idx = self.book_index(&pos.book)?;
        if idx + 1 >= self.books.len() {
            return Ok(pos.clone());
        }
        Ok(Position {
            book: self.books[idx + 1].code.clone(),
            chapter: 1,
            verse: None,
        })
    }
}
