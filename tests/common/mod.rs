//! Test helpers: a signing key, a JWKS that contains it, and signed tokens.
#![allow(dead_code)]

use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use aws_lc_rs::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, jwk::JwkSet};
use notes_api::auth::{Fetched, JwksSource};
use serde_json::{Value, json};

pub const ISSUER: &str = "https://kubernetes.example.test";
pub const AUDIENCE: &str = "my-app";
pub const CALLER: &str = "system:serviceaccount:team-staging:caller";

/// An ES256 key pair generated for the test.
pub struct TestKey {
    pub kid: String,
    encoding: EncodingKey,
    public: Vec<u8>,
}

impl TestKey {
    pub fn new(kid: &str) -> Result<Self> {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self {
            kid: kid.to_owned(),
            encoding: EncodingKey::from_ec_der(pkcs8.as_ref()),
            public: pair.public_key().as_ref().to_vec(),
        })
    }

    /// The public half as a JWK (uncompressed point: 0x04 || x || y).
    pub fn jwk(&self) -> Value {
        json!({
            "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig",
            "kid": self.kid,
            "x": URL_SAFE_NO_PAD.encode(&self.public[1..33]),
            "y": URL_SAFE_NO_PAD.encode(&self.public[33..65]),
        })
    }

    pub fn sign(&self, claims: &Value) -> Result<String> {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.kid.clone());
        Ok(jsonwebtoken::encode(&header, claims, &self.encoding)?)
    }

    /// Claims of a valid token for [`CALLER`], with `overrides` applied.
    pub fn token(&self, overrides: Value) -> Result<String> {
        let mut claims = json!({
            "iss": ISSUER, "sub": CALLER, "aud": [AUDIENCE],
            "iat": now(), "exp": now() + 600,
        });
        if let (Some(c), Some(o)) = (claims.as_object_mut(), overrides.as_object()) {
            for (k, v) in o {
                if v.is_null() {
                    c.remove(k);
                } else {
                    c.insert(k.clone(), v.clone());
                }
            }
        }
        self.sign(&claims)
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub fn jwks(keys: &[&TestKey]) -> JwkSet {
    serde_json::from_value(json!({ "keys": keys.iter().map(|k| k.jwk()).collect::<Vec<_>>() }))
        .unwrap_or(JwkSet { keys: vec![] })
}

/// A JWKS source whose keys can be replaced, counting fetches.
#[derive(Clone)]
pub struct StaticJwks {
    pub keys: Arc<Mutex<JwkSet>>,
    pub fetches: Arc<Mutex<u32>>,
    pub issuer: Option<String>,
}

impl StaticJwks {
    pub fn new(keys: &[&TestKey]) -> Self {
        Self {
            keys: Arc::new(Mutex::new(jwks(keys))),
            fetches: Arc::new(Mutex::new(0)),
            issuer: Some(ISSUER.to_owned()),
        }
    }

    pub fn replace(&self, keys: &[&TestKey]) {
        if let Ok(mut k) = self.keys.lock() {
            *k = jwks(keys);
        }
    }

    pub fn fetch_count(&self) -> u32 {
        self.fetches.lock().map(|f| *f).unwrap_or_default()
    }
}

impl JwksSource for StaticJwks {
    fn fetch(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Fetched>> + Send + '_>> {
        Box::pin(async move {
            if let Ok(mut f) = self.fetches.lock() {
                *f += 1;
            }
            let jwks = self
                .keys
                .lock()
                .map_err(|_| anyhow::anyhow!("poisoned"))?
                .clone();
            Ok(Fetched {
                jwks,
                issuer: self.issuer.clone(),
            })
        })
    }
}
