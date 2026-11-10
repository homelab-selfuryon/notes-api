//! Tests against a real Postgres. They are `#[ignore]`d: they need a database.
//!
//! ```sh
//! # a throwaway Postgres (`devenv test` starts one; see the README)
//! nix shell nixpkgs#postgresql -c bash -c '
//!   d=$(mktemp -d); initdb -D "$d" -A trust -U app >/dev/null
//!   pg_ctl -D "$d" -o "-p 54329 -k $d" -l "$d/log" start
//!   createdb -h localhost -p 54329 -U app app
//!   DATABASE_URL=postgresql://app@localhost:54329/app cargo test --test db -- --ignored
//!   pg_ctl -D "$d" stop'
//! ```

use anyhow::{Context, Result};
use notes_api::store::{NoteInput, PgStore, Store};
use secrecy::SecretString;

async fn store() -> Result<PgStore> {
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL is not set")?;
    let store = PgStore::connect_lazy(&SecretString::from(url), 2)?;
    store.migrate().await?;
    store.migrate().await?; // idempotent
    Ok(store)
}

fn input(title: &str) -> NoteInput {
    NoteInput {
        title: title.to_owned(),
        body: "body".to_owned(),
    }
}

#[tokio::test]
#[ignore = "needs Postgres: set DATABASE_URL, see the top of this file"]
async fn crud_round_trip() -> Result<()> {
    let store = store().await?;
    store.ping().await?;

    let created = store.create(input("db test")).await?;
    assert_eq!(
        store.get(created.id).await?.map(|n| n.title),
        Some("db test".into())
    );
    assert!(store.count().await? >= 1);
    assert!(store.list(10, 0).await?.iter().any(|n| n.id == created.id));

    let replaced = store.replace(created.id, input("changed")).await?;
    let replaced = replaced.context("note exists")?;
    assert_eq!(replaced.title, "changed");
    assert!(replaced.updated_at >= created.updated_at);

    assert!(store.delete(created.id).await?);
    assert!(!store.delete(created.id).await?);
    assert!(store.get(created.id).await?.is_none());
    assert!(store.replace(created.id, input("x")).await?.is_none());
    Ok(())
}

#[tokio::test]
#[ignore = "needs Postgres: set DATABASE_URL, see the top of this file"]
async fn the_database_enforces_the_limits_too() -> Result<()> {
    let store = store().await?;
    assert!(store.create(input("")).await.is_err(), "empty title");
    assert!(
        store.create(input(&"x".repeat(201))).await.is_err(),
        "long title"
    );
    Ok(())
}
