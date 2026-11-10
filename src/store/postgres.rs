use std::{str::FromStr, time::Duration};

use anyhow::{Context, Result};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions, PgRow},
};
use uuid::Uuid;

use super::{Note, NoteInput, Store, StoreError};

/// Migrations are part of the binary and run at start-up.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub struct PgStore {
    pool: PgPool,
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Database(e.to_string())
    }
}

fn note(row: &PgRow) -> Result<Note, sqlx::Error> {
    Ok(Note {
        id: row.try_get("id")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

impl PgStore {
    /// `uri` is the CloudNativePG `uri` file: `postgresql://user:password@host:5432/db`.
    /// The pool connects lazily: the database may still be starting.
    pub fn connect_lazy(uri: &SecretString, max_connections: u32) -> Result<Self> {
        let options = PgConnectOptions::from_str(uri.expose_secret())
            .context("the database URI is not valid")?
            .application_name("notes-api");
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy_with(options);
        Ok(Self { pool })
    }

    /// Applies the embedded migrations (idempotent).
    pub async fn migrate(&self) -> Result<()> {
        MIGRATOR
            .run(&self.pool)
            .await
            .context("running the migrations")
    }
}

#[async_trait]
impl Store for PgStore {
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Note>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, title, body, created_at, updated_at FROM notes \
             ORDER BY created_at DESC, id LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(note).collect::<Result<_, _>>()?)
    }

    async fn get(&self, id: Uuid) -> Result<Option<Note>, StoreError> {
        let row =
            sqlx::query("SELECT id, title, body, created_at, updated_at FROM notes WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.as_ref().map(note).transpose()?)
    }

    async fn create(&self, input: NoteInput) -> Result<Note, StoreError> {
        let row = sqlx::query(
            "INSERT INTO notes (title, body) VALUES ($1, $2) \
             RETURNING id, title, body, created_at, updated_at",
        )
        .bind(input.title)
        .bind(input.body)
        .fetch_one(&self.pool)
        .await?;
        Ok(note(&row)?)
    }

    async fn replace(&self, id: Uuid, input: NoteInput) -> Result<Option<Note>, StoreError> {
        let row = sqlx::query(
            "UPDATE notes SET title = $2, body = $3, updated_at = now() WHERE id = $1 \
             RETURNING id, title, body, created_at, updated_at",
        )
        .bind(id)
        .bind(input.title)
        .bind(input.body)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(note).transpose()?)
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let done = sqlx::query("DELETE FROM notes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    async fn count(&self) -> Result<i64, StoreError> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    async fn ping(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}
