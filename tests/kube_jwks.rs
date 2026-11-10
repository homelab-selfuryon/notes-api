//! `KubeJwks` against a stand-in for the kube-apiserver.

mod common;

use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use common::{AUDIENCE, CALLER, ISSUER, TestKey};
use notes_api::{
    auth::{JwksSource, KubeJwks, Verifier, VerifierOptions},
    config::Config,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;

#[derive(Clone)]
struct Api {
    jwks: Value,
    token: &'static str,
}

fn authorised(api: &Api, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        == Some(&format!("Bearer {}", api.token))
}

async fn discovery(State(api): State<Api>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&api, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(
        json!({ "issuer": ISSUER, "jwks_uri": "https://elsewhere.invalid/jwks" }),
    ))
}

async fn jwks(State(api): State<Api>, headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    if !authorised(&api, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(api.jwks))
}

async fn serve(api: Api) -> Result<String> {
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/openid/v1/jwks", get(jwks))
        .with_state(api);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(url)
}

fn config(url: &str, token_file: &std::path::Path) -> Result<Config> {
    let map: HashMap<String, String> = [
        ("AUTH_AUDIENCE", AUDIENCE),
        ("AUTH_ALLOWED_SUBJECTS", CALLER),
        ("AUTH_KUBE_API_URL", url),
        ("AUTH_KUBE_TOKEN_FILE", &token_file.display().to_string()),
        (
            "AUTH_KUBE_CA_FILE",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ca.pem"),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    Config::from_lookup(|k| map.get(k).cloned())
}

#[tokio::test]
async fn fetches_issuer_and_keys_with_the_pods_token() -> Result<()> {
    let key = TestKey::new("k1")?;
    let url = serve(Api {
        jwks: serde_json::to_value(common::jwks(&[&key]))?,
        token: "pod-token",
    })
    .await?;
    let dir = tempfile::tempdir()?;
    let token_file = dir.path().join("token");
    std::fs::write(&token_file, "pod-token\n")?;

    let cfg = config(&url, &token_file)?;
    let auth = cfg.auth.clone().ok_or_else(|| anyhow::anyhow!("auth"))?;
    let source = KubeJwks::new(&auth)?;
    let fetched = source.fetch().await?;
    assert_eq!(fetched.issuer.as_deref(), Some(ISSUER));
    assert_eq!(fetched.jwks.keys.len(), 1);

    // End to end: verify a token with keys from the stand-in apiserver.
    let verifier = Verifier::new(
        VerifierOptions::new(auth.audience, auth.allowed_subjects),
        Arc::new(source),
    );
    assert_eq!(verifier.verify(&key.token(json!({}))?).await?, CALLER);
    Ok(())
}

#[tokio::test]
async fn a_rotated_token_file_is_picked_up_and_a_wrong_token_fails() -> Result<()> {
    let key = TestKey::new("k1")?;
    let url = serve(Api {
        jwks: serde_json::to_value(common::jwks(&[&key]))?,
        token: "second",
    })
    .await?;
    let dir = tempfile::tempdir()?;
    let token_file = dir.path().join("token");
    std::fs::write(&token_file, "first")?;

    let cfg = config(&url, &token_file)?;
    let auth = cfg.auth.ok_or_else(|| anyhow::anyhow!("auth"))?;
    let source = KubeJwks::new(&auth)?;
    assert!(
        source.fetch().await.is_err(),
        "the apiserver refuses the old token"
    );

    std::fs::write(&token_file, "second")?; // the kubelet rotated it
    assert!(source.fetch().await.is_ok());
    Ok(())
}
