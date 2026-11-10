//! Where notes live. [`PgStore`] is the real one; [`MemoryStore`] backs the
//! tests that do not need a database.

mod memory;
mod postgres;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use memory::MemoryStore;
pub use postgres::PgStore;

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BODY_CHARS: usize = 10_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Note {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What a client sends to create or replace a note.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteInput {
    pub title: String,
    #[serde(default)]
    pub body: String,
}

impl NoteInput {
    /// Trims the title and checks the limits the database also enforces.
    pub fn validate(self) -> Result<Self, String> {
        let title = self.title.trim().to_owned();
        if title.is_empty() {
            return Err("title must not be empty".into());
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(format!("title is longer than {MAX_TITLE_CHARS} characters"));
        }
        if self.body.chars().count() > MAX_BODY_CHARS {
            return Err(format!("body is longer than {MAX_BODY_CHARS} characters"));
        }
        Ok(Self {
            title,
            body: self.body,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(String),
}

#[async_trait]
pub trait Store: Send + Sync {
    /// Newest first.
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Note>, StoreError>;
    async fn get(&self, id: Uuid) -> Result<Option<Note>, StoreError>;
    async fn create(&self, input: NoteInput) -> Result<Note, StoreError>;
    /// `None` when there is no such note.
    async fn replace(&self, id: Uuid, input: NoteInput) -> Result<Option<Note>, StoreError>;
    /// `false` when there was no such note.
    async fn delete(&self, id: Uuid) -> Result<bool, StoreError>;
    async fn count(&self) -> Result<i64, StoreError>;
    /// Is the database reachable?
    async fn ping(&self) -> Result<(), StoreError>;
}
