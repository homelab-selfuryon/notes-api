//! The token verifier, with keys generated for the test.

mod common;

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use common::{AUDIENCE, CALLER, StaticJwks, TestKey, now};
use notes_api::auth::{AuthError, Verifier, VerifierOptions};
use serde_json::json;

fn verifier(source: &StaticJwks) -> Verifier {
    let mut options = VerifierOptions::new(AUDIENCE.to_owned(), [CALLER.to_owned()]);
    options.min_refresh_interval = Duration::ZERO;
    Verifier::new(options, Arc::new(source.clone()))
}

#[tokio::test]
async fn accepts_an_allowed_caller() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    assert_eq!(v.verify(&key.token(json!({}))?).await?, CALLER);
    Ok(())
}

#[tokio::test]
async fn refuses_a_subject_that_is_not_allowed() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    let token = key.token(json!({"sub": "system:serviceaccount:team-staging:intruder"}))?;
    assert!(matches!(
        v.verify(&token).await,
        Err(AuthError::Forbidden(_))
    ));
    Ok(())
}

#[tokio::test]
async fn refuses_the_wrong_audience() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    let token = key.token(json!({"aud": ["https://kubernetes.default.svc"]}))?;
    assert!(matches!(
        v.verify(&token).await,
        Err(AuthError::InvalidToken(_))
    ));
    Ok(())
}

#[tokio::test]
async fn refuses_the_wrong_issuer() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    let token = key.token(json!({"iss": "https://evil.example.test"}))?;
    assert!(matches!(
        v.verify(&token).await,
        Err(AuthError::InvalidToken(_))
    ));
    Ok(())
}

#[tokio::test]
async fn refuses_an_expired_token() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    let token = key.token(json!({"exp": now() - 3600}))?;
    assert!(matches!(
        v.verify(&token).await,
        Err(AuthError::InvalidToken(_))
    ));
    Ok(())
}

#[tokio::test]
async fn refuses_a_token_without_expiry() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    let token = key.token(json!({"exp": null}))?;
    assert!(matches!(
        v.verify(&token).await,
        Err(AuthError::InvalidToken(_))
    ));
    Ok(())
}

#[tokio::test]
async fn refuses_a_token_signed_by_another_key() -> Result<()> {
    let trusted = TestKey::new("k1")?;
    let attacker = TestKey::new("k1")?; // same key id, different key
    let v = verifier(&StaticJwks::new(&[&trusted]));
    let token = attacker.token(json!({}))?;
    assert!(matches!(
        v.verify(&token).await,
        Err(AuthError::InvalidToken(_))
    ));
    Ok(())
}

#[tokio::test]
async fn refuses_garbage_and_unsigned_tokens() -> Result<()> {
    let key = TestKey::new("k1")?;
    let v = verifier(&StaticJwks::new(&[&key]));
    assert!(v.verify("not a jwt").await.is_err());
    // alg "none"
    // Built at run time: a literal JWT in the source trips secret scanners.
    let b64 =
        |s: &str| base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, s);
    let none = format!(
        "{}.{}.",
        b64(r#"{"alg":"none","kid":"k1"}"#),
        b64(r#"{"sub":"x"}"#)
    );
    assert!(v.verify(&none).await.is_err());
    Ok(())
}

#[tokio::test]
async fn keys_are_cached_and_refetched_on_rotation() -> Result<()> {
    let old = TestKey::new("old")?;
    let new = TestKey::new("new")?;
    let source = StaticJwks::new(&[&old]);
    let v = verifier(&source);

    v.verify(&old.token(json!({}))?).await?;
    v.verify(&old.token(json!({}))?).await?;
    assert_eq!(
        source.fetch_count(),
        1,
        "second call is served from the cache"
    );

    // The cluster rotates its signing key.
    source.replace(&[&new]);
    v.verify(&new.token(json!({}))?).await?;
    assert_eq!(
        source.fetch_count(),
        2,
        "an unknown key id triggers a fetch"
    );
    Ok(())
}

#[tokio::test]
async fn unknown_key_ids_are_rate_limited() -> Result<()> {
    let key = TestKey::new("k1")?;
    let other = TestKey::new("other")?;
    let source = StaticJwks::new(&[&key]);
    let mut options = VerifierOptions::new(AUDIENCE.to_owned(), [CALLER.to_owned()]);
    options.min_refresh_interval = Duration::from_secs(60);
    let v = Verifier::new(options, Arc::new(source.clone()));

    v.verify(&key.token(json!({}))?).await?;
    for _ in 0..5 {
        assert!(v.verify(&other.token(json!({}))?).await.is_err());
    }
    assert_eq!(
        source.fetch_count(),
        1,
        "bad tokens do not reach the apiserver"
    );
    Ok(())
}
