use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use super::{Note, NoteInput, Store, StoreError};

/// Notes in memory: for tests, and for running without a database.
#[derive(Default)]
pub struct MemoryStore {
    notes: Mutex<Vec<Note>>,
}

impl MemoryStore {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Vec<Note>>, StoreError> {
        self.notes
            .lock()
            .map_err(|_| StoreError::Database("poisoned".into()))
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Note>, StoreError> {
        let notes = self.lock()?;
        Ok(notes
            .iter()
            .rev()
            .skip(usize::try_from(offset).unwrap_or(0))
            .take(usize::try_from(limit).unwrap_or(0))
            .cloned()
            .collect())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Note>, StoreError> {
        Ok(self.lock()?.iter().find(|n| n.id == id).cloned())
    }

    async fn create(&self, input: NoteInput) -> Result<Note, StoreError> {
        let now = Utc::now();
        let note = Note {
            id: Uuid::new_v4(),
            title: input.title,
            body: input.body,
            created_at: now,
            updated_at: now,
        };
        self.lock()?.push(note.clone());
        Ok(note)
    }

    async fn replace(&self, id: Uuid, input: NoteInput) -> Result<Option<Note>, StoreError> {
        let mut notes = self.lock()?;
        Ok(notes.iter_mut().find(|n| n.id == id).map(|n| {
            n.title = input.title;
            n.body = input.body;
            n.updated_at = Utc::now();
            n.clone()
        }))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut notes = self.lock()?;
        let before = notes.len();
        notes.retain(|n| n.id != id);
        Ok(notes.len() != before)
    }

    async fn count(&self) -> Result<i64, StoreError> {
        Ok(i64::try_from(self.lock()?.len()).unwrap_or(i64::MAX))
    }

    async fn ping(&self) -> Result<(), StoreError> {
        Ok(())
    }
}
