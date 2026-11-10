//! Non-secret configuration, from environment variables. The platform sets
//! `PORT`, `APP_ENV`, `APP_VERSION` and, for apps with callers,
//! `AUTH_AUDIENCE` and `AUTH_ALLOWED_SUBJECTS`; anything else comes from
//! `env:` in `deploy/deploy.cue`.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};

/// Default URL of the kube-apiserver inside the cluster.
pub const DEFAULT_KUBE_API_URL: &str = "https://kubernetes.default.svc";
/// Token for the kube-apiserver, mounted when `network.kubeAPI` is true.
pub const DEFAULT_KUBE_TOKEN_FILE: &str = "/var/run/secrets/kube-api/token";
/// The cluster CA, mounted next to it.
pub const DEFAULT_KUBE_CA_FILE: &str = "/var/run/secrets/kube-api/ca.crt";

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub app_env: String,
    pub app_version: String,
    /// How long to keep serving after SIGTERM with `/readyz` failing, so that
    /// the endpoint is removed from the Service before connections are refused.
    pub shutdown_delay: Duration,
    /// Upper bound of a request.
    pub request_timeout: Duration,
    /// File with the database connection string: the CloudNativePG `uri` key.
    pub db_uri_file: PathBuf,
    pub db_max_connections: u32,
    /// `None` when the app has no authenticated endpoints.
    pub auth: Option<AuthConfig>,
}

/// How tokens presented by other apps are verified.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// The `aud` a token must carry: this app's name.
    pub audience: String,
    /// `system:serviceaccount:<namespace>:<name>` of the callers. Anyone else
    /// is refused; an empty list refuses everybody.
    pub allowed_subjects: Vec<String>,
    /// Base URL of the kube-apiserver; `/openid/v1/jwks` and
    /// `/.well-known/openid-configuration` are read from it.
    pub kube_api_url: String,
    /// The pod's own token, sent to the kube-apiserver.
    pub kube_token_file: PathBuf,
    /// CA that signed the kube-apiserver certificate; the only CA this app trusts.
    pub kube_ca_file: PathBuf,
    /// Override of the token issuer; by default it is discovered.
    pub issuer: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Reads settings through `get`; tests pass a map.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let port = match get("PORT") {
            Some(v) => v.parse().with_context(|| format!("PORT={v:?}"))?,
            None => 8080,
        };
        let secs = |key: &str, default: u64| -> Result<Duration> {
            match get(key) {
                Some(v) => Ok(Duration::from_secs(
                    v.parse().with_context(|| format!("{key}={v:?}"))?,
                )),
                None => Ok(Duration::from_secs(default)),
            }
        };

        let auth = match get("AUTH_AUDIENCE").filter(|a| !a.is_empty()) {
            None => None,
            Some(audience) => {
                let allowed_subjects: Vec<String> = get("AUTH_ALLOWED_SUBJECTS")
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                if allowed_subjects.is_empty() {
                    bail!("AUTH_AUDIENCE is set but AUTH_ALLOWED_SUBJECTS is empty");
                }
                Some(AuthConfig {
                    audience,
                    allowed_subjects,
                    kube_api_url: get("AUTH_KUBE_API_URL")
                        .unwrap_or_else(|| DEFAULT_KUBE_API_URL.to_owned()),
                    kube_token_file: get("AUTH_KUBE_TOKEN_FILE")
                        .unwrap_or_else(|| DEFAULT_KUBE_TOKEN_FILE.to_owned())
                        .into(),
                    kube_ca_file: get("AUTH_KUBE_CA_FILE")
                        .unwrap_or_else(|| DEFAULT_KUBE_CA_FILE.to_owned())
                        .into(),
                    issuer: get("AUTH_ISSUER").filter(|i| !i.is_empty()),
                })
            }
        };

        Ok(Self {
            port,
            app_env: get("APP_ENV").unwrap_or_else(|| "local".to_owned()),
            app_version: get("APP_VERSION").unwrap_or_else(|| "dev".to_owned()),
            shutdown_delay: secs("SHUTDOWN_DELAY_SECS", 2)?,
            request_timeout: secs("REQUEST_TIMEOUT_SECS", 30)?,
            db_uri_file: get("DB_URI_FILE")
                .unwrap_or_else(|| format!("{}/uri", crate::secrets::DB_SECRETS_DIR))
                .into(),
            db_max_connections: match get("DB_MAX_CONNECTIONS") {
                Some(v) => v
                    .parse()
                    .with_context(|| format!("DB_MAX_CONNECTIONS={v:?}"))?,
                None => 5,
            },
            auth,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn defaults() -> Result<()> {
        let c = cfg(&[])?;
        assert_eq!(c.port, 8080);
        assert_eq!(c.app_env, "local");
        assert!(c.auth.is_none());
        assert_eq!(c.db_uri_file, PathBuf::from("/var/run/secrets/db/uri"));
        assert_eq!(c.db_max_connections, 5);
        Ok(())
    }

    #[test]
    fn bad_port() {
        assert!(cfg(&[("PORT", "http")]).is_err());
    }

    #[test]
    fn auth_from_platform_variables() -> Result<()> {
        let c = cfg(&[
            ("AUTH_AUDIENCE", "notes-api"),
            (
                "AUTH_ALLOWED_SUBJECTS",
                "system:serviceaccount:notes-prod:notes-stats, system:serviceaccount:notes-prod:other",
            ),
        ])?;
        let auth = c.auth.context("auth configured")?;
        assert_eq!(auth.audience, "notes-api");
        assert_eq!(auth.allowed_subjects.len(), 2);
        assert_eq!(auth.kube_api_url, DEFAULT_KUBE_API_URL);
        assert!(auth.issuer.is_none());
        Ok(())
    }

    #[test]
    fn audience_without_subjects_is_refused() {
        assert!(cfg(&[("AUTH_AUDIENCE", "notes-api")]).is_err());
    }
}
