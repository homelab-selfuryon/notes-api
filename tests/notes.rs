//! The HTTP surface over the in-memory store: no database needed.

mod common;

use std::{collections::HashMap, sync::Arc, sync::atomic::Ordering};

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use common::{AUDIENCE, CALLER, StaticJwks, TestKey};
use http_body_util::BodyExt;
use notes_api::{
    AppState,
    auth::{Verifier, VerifierOptions},
    config::Config,
    routes,
    store::MemoryStore,
};
use serde_json::{Value, json};
use tower::ServiceExt;

struct Harness {
    app: Router,
    state: AppState,
    key: TestKey,
}

fn harness() -> Result<Harness> {
    let key = TestKey::new("k1")?;
    let map: HashMap<String, String> = HashMap::new();
    let config = Config::from_lookup(|k| map.get(k).cloned())?;
    let verifier = Verifier::new(
        VerifierOptions::new(AUDIENCE.to_owned(), [CALLER.to_owned()]),
        Arc::new(StaticJwks::new(&[&key])),
    );
    let state = AppState::new(config, Some(verifier), Arc::new(MemoryStore::default()));
    state.ready.store(true, Ordering::Relaxed);
    Ok(Harness {
        app: routes::router(state.clone()),
        state,
        key,
    })
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> Result<(StatusCode, Value)> {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = bearer {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let body = match body {
        Some(b) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&b)?)
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body)?).await?;
    let status = res.status();
    let bytes = res.into_body().collect().await?.to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok((status, value))
}

#[tokio::test]
async fn crud() -> Result<()> {
    let h = harness()?;

    let (status, created) = call(
        &h.app,
        Method::POST,
        "/v1/notes",
        None,
        Some(json!({"title": "  first  ", "body": "hello"})),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["title"], "first", "the title is trimmed");
    let id = created["id"].as_str().unwrap_or_default().to_owned();

    let (status, read) = call(&h.app, Method::GET, &format!("/v1/notes/{id}"), None, None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["body"], "hello");

    let (status, replaced) = call(
        &h.app,
        Method::PUT,
        &format!("/v1/notes/{id}"),
        None,
        Some(json!({"title": "second"})),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replaced["title"], "second");
    assert_eq!(replaced["body"], "", "a missing body is empty");

    let (status, list) = call(&h.app, Method::GET, "/v1/notes", None, None).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().map(Vec::len), Some(1));

    let (status, _) = call(
        &h.app,
        Method::DELETE,
        &format!("/v1/notes/{id}"),
        None,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&h.app, Method::GET, &format!("/v1/notes/{id}"), None, None).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &h.app,
        Method::DELETE,
        &format!("/v1/notes/{id}"),
        None,
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn validation() -> Result<()> {
    let h = harness()?;
    for bad in [
        json!({"title": "   "}),
        json!({"title": "x".repeat(201)}),
        json!({"title": "ok", "body": "y".repeat(10_001)}),
        json!({"title": "ok", "extra": 1}),
        json!({"body": "no title"}),
    ] {
        let (status, _) = call(&h.app, Method::POST, "/v1/notes", None, Some(bad.clone())).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (status, _) = call(&h.app, Method::GET, "/v1/notes/not-a-uuid", None, None).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&h.app, Method::GET, "/v1/notes?limit=0", None, None).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&h.app, Method::GET, "/v1/notes?limit=201", None, None).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn pagination_is_newest_first() -> Result<()> {
    let h = harness()?;
    for title in ["a", "b", "c"] {
        call(
            &h.app,
            Method::POST,
            "/v1/notes",
            None,
            Some(json!({"title": title})),
        )
        .await?;
    }
    let (_, page) = call(&h.app, Method::GET, "/v1/notes?limit=2", None, None).await?;
    let titles: Vec<&str> = page
        .as_array()
        .map(|a| a.iter().filter_map(|n| n["title"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(titles, ["c", "b"]);
    let (_, page) = call(
        &h.app,
        Method::GET,
        "/v1/notes?limit=2&offset=2",
        None,
        None,
    )
    .await?;
    assert_eq!(page.as_array().map(Vec::len), Some(1));
    Ok(())
}

#[tokio::test]
async fn count_requires_an_allowed_caller() -> Result<()> {
    let h = harness()?;
    call(
        &h.app,
        Method::POST,
        "/v1/notes",
        None,
        Some(json!({"title": "a"})),
    )
    .await?;

    let (status, _) = call(&h.app, Method::GET, "/v1/notes/count", None, None).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no token");

    let wrong_audience = h.key.token(json!({"aud": ["notes-stats"]}))?;
    let (status, _) = call(
        &h.app,
        Method::GET,
        "/v1/notes/count",
        Some(&wrong_audience),
        None,
    )
    .await?;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "token for another audience"
    );

    let other = h
        .key
        .token(json!({"sub": "system:serviceaccount:notes-staging:other"}))?;
    let (status, _) = call(&h.app, Method::GET, "/v1/notes/count", Some(&other), None).await?;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "subject not on the allowlist"
    );

    let (status, body) = call(
        &h.app,
        Method::GET,
        "/v1/notes/count",
        Some(&h.key.token(json!({}))?),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["count"], 1);
    Ok(())
}

#[tokio::test]
async fn count_is_not_shadowed_by_the_id_route() -> Result<()> {
    // Without a token it must be the auth failure, not a 400 "invalid uuid".
    let h = harness()?;
    let (status, _) = call(&h.app, Method::GET, "/v1/notes/count", None, None).await?;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    Ok(())
}

#[tokio::test]
async fn health_and_readiness() -> Result<()> {
    let h = harness()?;
    assert_eq!(
        call(&h.app, Method::GET, "/healthz", None, None).await?.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&h.app, Method::GET, "/readyz", None, None).await?.0,
        StatusCode::OK
    );
    h.state.ready.store(false, Ordering::Relaxed);
    assert_eq!(
        call(&h.app, Method::GET, "/readyz", None, None).await?.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&h.app, Method::GET, "/healthz", None, None).await?.0,
        StatusCode::OK
    );
    Ok(())
}
