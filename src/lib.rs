//! notes-api: a REST API for notes on Postgres.
//!
//! - [`config`]: non-secret settings from the environment
//! - [`secrets`]: secrets from files, wrapped in `secrecy`
//! - [`auth`]: verification of projected ServiceAccount tokens (`/v1/notes/count`)
//! - [`store`]: the notes: Postgres, and an in-memory store for tests
//! - [`routes`]: the HTTP routes; `/healthz` and `/readyz` for the kubelet

pub mod auth;
pub mod config;
pub mod routes;
pub mod secrets;
pub mod state;
pub mod store;

pub use state::AppState;
