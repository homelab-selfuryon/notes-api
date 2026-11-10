use std::sync::atomic::Ordering;

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Caller,
    store::{Note, NoteInput, StoreError},
};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;
/// Notes are small; refuse anything larger before parsing it.
const MAX_BODY_BYTES: usize = 64 * 1024;

pub fn router(state: AppState) -> Router {
    let timeout = state.config.request_timeout;
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/notes", get(list).post(create))
        // A fixed path wins over `{id}`.
        .route("/v1/notes/count", get(count))
        .route("/v1/notes/{id}", get(read).put(replace).delete(remove))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            timeout,
        ))
        .with_state(state)
}

/// What a handler can fail with.
enum ApiError {
    BadRequest(String),
    NotFound,
    Store(StoreError),
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(e: JsonRejection) -> Self {
        Self::BadRequest(e.body_text())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            Self::NotFound => (StatusCode::NOT_FOUND, "no such note".to_owned()),
            Self::Store(e) => {
                // The client learns nothing about the database.
                tracing::error!(error = %e, "store error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_owned(),
                )
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

async fn healthz() -> &'static str {
    "ok"
}

/// Ready: migrations done, not shutting down, database answers.
async fn readyz(State(state): State<AppState>) -> (StatusCode, &'static str) {
    if !state.ready.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "not ready");
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), state.store.ping()).await {
        Ok(Ok(())) => (StatusCode::OK, "ready"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "database unavailable"),
    }
}

#[derive(Deserialize)]
struct Page {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list(
    State(state): State<AppState>,
    Query(page): Query<Page>,
) -> Result<Json<Vec<Note>>, ApiError> {
    let limit = page.limit.unwrap_or(DEFAULT_LIMIT);
    let offset = page.offset.unwrap_or(0);
    if !(1..=MAX_LIMIT).contains(&limit) || offset < 0 {
        return Err(ApiError::BadRequest(format!(
            "limit must be 1..={MAX_LIMIT} and offset 0 or more"
        )));
    }
    Ok(Json(state.store.list(limit, offset).await?))
}

async fn create(
    State(state): State<AppState>,
    input: Result<Json<NoteInput>, JsonRejection>,
) -> Result<(StatusCode, Json<Note>), ApiError> {
    let Json(input) = input?;
    let input = input.validate().map_err(ApiError::BadRequest)?;
    Ok((StatusCode::CREATED, Json(state.store.create(input).await?)))
}

async fn read(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Note>, ApiError> {
    state
        .store
        .get(id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn replace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    input: Result<Json<NoteInput>, JsonRejection>,
) -> Result<Json<Note>, ApiError> {
    let Json(input) = input?;
    let input = input.validate().map_err(ApiError::BadRequest)?;
    state
        .store
        .replace(id, input)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if state.store.delete(id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Only for callers presenting a token of an allowlisted ServiceAccount
/// (notes-stats), verified against the cluster's keys.
async fn count(
    State(state): State<AppState>,
    Caller(caller): Caller,
) -> Result<Json<Value>, ApiError> {
    let count = state.store.count().await?;
    tracing::info!(%caller, count, "count requested");
    Ok(Json(json!({ "count": count })))
}
