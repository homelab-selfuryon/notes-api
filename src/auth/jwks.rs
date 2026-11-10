//! Where the signing keys come from: the kube-apiserver's OIDC discovery
//! endpoints. Every authenticated ServiceAccount may read them
//! (`system:service-account-issuer-discovery`), so the pod needs its own token
//! (the `kube-api` volume of `network.kubeAPI: true`) and the cluster CA.

use std::{future::Future, path::PathBuf, pin::Pin, time::Duration};

use anyhow::{Context, Result};
use jsonwebtoken::jwk::JwkSet;
use serde::Deserialize;

use crate::config::AuthConfig;

/// The keys of an issuer and, when known, its name.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub jwks: JwkSet,
    pub issuer: Option<String>,
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A source of signing keys; the kube-apiserver in the cluster, a fixed set in tests.
pub trait JwksSource: Send + Sync {
    fn fetch(&self) -> BoxFuture<'_, Result<Fetched>>;
}

/// Reads the discovery document and the JWKS from the kube-apiserver.
pub struct KubeJwks {
    client: reqwest::Client,
    base_url: String,
    token_file: PathBuf,
    /// Skip discovery when the issuer is configured.
    issuer: Option<String>,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
}

impl KubeJwks {
    pub fn new(cfg: &AuthConfig) -> Result<Self> {
        let ca = &cfg.kube_ca_file;
        let pem =
            std::fs::read(ca).with_context(|| format!("reading cluster CA {}", ca.display()))?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(3))
            // Trust the cluster CA and nothing else: this client talks to the
            // kube-apiserver only.
            .tls_certs_only([
                reqwest::Certificate::from_pem(&pem).context("parsing the cluster CA")?
            ])
            .build()
            .context("building the HTTP client")?;
        Ok(Self {
            client,
            base_url: cfg.kube_api_url.trim_end_matches('/').to_owned(),
            token_file: cfg.kube_token_file.clone(),
            issuer: cfg.issuer.clone(),
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        // The kubelet rotates the token: read it on every request.
        let token = tokio::fs::read_to_string(&self.token_file)
            .await
            .with_context(|| format!("reading {}", self.token_file.display()))?;
        let url = format!("{}{}", self.base_url, path);
        self.client
            .get(&url)
            .bearer_auth(token.trim())
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("GET {url}"))?
            .json()
            .await
            .with_context(|| format!("decoding the response of {url}"))
    }
}

impl JwksSource for KubeJwks {
    fn fetch(&self) -> BoxFuture<'_, Result<Fetched>> {
        Box::pin(async move {
            let issuer = match &self.issuer {
                Some(i) => Some(i.clone()),
                None => Some(
                    self.get::<Discovery>("/.well-known/openid-configuration")
                        .await?
                        .issuer,
                ),
            };
            let jwks = self.get::<JwkSet>("/openid/v1/jwks").await?;
            Ok(Fetched { jwks, issuer })
        })
    }
}
