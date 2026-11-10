use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

use super::jwks::{Fetched, JwksSource};

/// Why a request was refused.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// No token, or one that does not verify: 401.
    #[error("invalid token: {0}")]
    InvalidToken(String),
    /// A valid token of a subject that is not allowed: 403.
    #[error("subject {0:?} is not allowed")]
    Forbidden(String),
    /// The signing keys cannot be fetched: 503.
    #[error("signing keys unavailable")]
    KeysUnavailable,
}

#[derive(Debug, Clone)]
pub struct VerifierOptions {
    /// Required `aud`.
    pub audience: String,
    /// Allowed `sub` values.
    pub allowed_subjects: HashSet<String>,
    /// The keys are fetched again after this long.
    pub cache_ttl: Duration,
    /// At most one extra fetch per this interval when a token names an
    /// unknown key id (rotation), so a flood of bad tokens cannot hammer the
    /// kube-apiserver.
    pub min_refresh_interval: Duration,
    /// Tolerated clock skew.
    pub leeway: u64,
}

impl VerifierOptions {
    pub fn new(audience: String, allowed_subjects: impl IntoIterator<Item = String>) -> Self {
        Self {
            audience,
            allowed_subjects: allowed_subjects.into_iter().collect(),
            cache_ttl: Duration::from_secs(300),
            min_refresh_interval: Duration::from_secs(30),
            leeway: 30,
        }
    }
}

struct Cached {
    jwks: JwkSet,
    issuer: Option<String>,
    fetched_at: Instant,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
}

/// Verifies tokens; keys are cached.
pub struct Verifier {
    options: VerifierOptions,
    source: Arc<dyn JwksSource>,
    cache: RwLock<Option<Cached>>,
    /// One fetch at a time.
    refresh: Mutex<()>,
}

/// Algorithms the kube-apiserver signs with. Nothing else is accepted, in
/// particular not `none` or the HMAC family.
const ALGORITHMS: [Algorithm; 2] = [Algorithm::RS256, Algorithm::ES256];

impl Verifier {
    pub fn new(options: VerifierOptions, source: Arc<dyn JwksSource>) -> Self {
        Self {
            options,
            source,
            cache: RwLock::new(None),
            refresh: Mutex::new(()),
        }
    }

    /// Returns the subject of a valid token of an allowed caller.
    pub async fn verify(&self, token: &str) -> Result<String, AuthError> {
        let header = decode_header(token).map_err(|e| AuthError::InvalidToken(e.to_string()))?;
        if !ALGORITHMS.contains(&header.alg) {
            return Err(AuthError::InvalidToken(format!(
                "algorithm {:?} not accepted",
                header.alg
            )));
        }
        let kid = header
            .kid
            .ok_or_else(|| AuthError::InvalidToken("no key id".into()))?;

        let (key, issuer) = self.key(&kid).await?;

        // Only the algorithm of the token's own header, which was checked against
        // ALGORITHMS above and must match the key's type.
        let mut validation = Validation::new(header.alg);
        validation.leeway = self.options.leeway;
        validation.validate_nbf = true;
        validation.set_audience(&[&self.options.audience]);
        validation.set_required_spec_claims(&["exp", "aud", "sub", "iss"]);
        match &issuer {
            Some(iss) => validation.set_issuer(&[iss]),
            None => return Err(AuthError::KeysUnavailable),
        }

        let data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))?;
        if self.options.allowed_subjects.contains(&data.claims.sub) {
            Ok(data.claims.sub)
        } else {
            Err(AuthError::Forbidden(data.claims.sub))
        }
    }

    /// The key `kid` and the issuer; fetches when the cache is empty, stale,
    /// or (rate limited) does not know the key.
    async fn key(&self, kid: &str) -> Result<(DecodingKey, Option<String>), AuthError> {
        if let Some(found) = self.lookup(kid, false).await? {
            return Ok(found);
        }
        self.refresh(kid).await?;
        self.lookup(kid, true)
            .await?
            .ok_or_else(|| AuthError::InvalidToken("unknown signing key".into()))
    }

    async fn lookup(
        &self,
        kid: &str,
        ignore_age: bool,
    ) -> Result<Option<(DecodingKey, Option<String>)>, AuthError> {
        let cache = self.cache.read().await;
        let Some(c) = cache.as_ref() else {
            return Ok(None);
        };
        if !ignore_age && c.fetched_at.elapsed() > self.options.cache_ttl {
            return Ok(None);
        }
        let Some(jwk) = c.jwks.find(kid) else {
            return Ok(None);
        };
        let key = DecodingKey::from_jwk(jwk)
            .map_err(|e| AuthError::InvalidToken(format!("unusable signing key: {e}")))?;
        Ok(Some((key, c.issuer.clone())))
    }

    async fn refresh(&self, kid: &str) -> Result<(), AuthError> {
        let _guard = self.refresh.lock().await;
        // Someone else may have refreshed while we waited.
        {
            let cache = self.cache.read().await;
            if let Some(c) = cache.as_ref() {
                let age = c.fetched_at.elapsed();
                let fresh = age <= self.options.cache_ttl;
                if (fresh && c.jwks.find(kid).is_some()) || age < self.options.min_refresh_interval
                {
                    return Ok(());
                }
            }
        }
        match self.source.fetch().await {
            Ok(Fetched { jwks, issuer }) => {
                *self.cache.write().await = Some(Cached {
                    jwks,
                    issuer,
                    fetched_at: Instant::now(),
                });
                Ok(())
            }
            Err(error) => {
                tracing::error!(
                    error = format!("{error:#}"),
                    "fetching the signing keys failed"
                );
                // Keep serving from an old cache rather than failing closed on
                // a transient error; unknown keys still fail.
                if self.cache.read().await.is_some() {
                    Ok(())
                } else {
                    Err(AuthError::KeysUnavailable)
                }
            }
        }
    }
}
