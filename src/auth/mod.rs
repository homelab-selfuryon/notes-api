//! Service-to-service authentication without shared keys.
//!
//! A caller presents its projected ServiceAccount token (audience: this
//! app's name) as `Authorization: Bearer <jwt>`. The token is verified
//! against the signing keys of the cluster's kube-apiserver (`jwks`), and its
//! subject (`system:serviceaccount:<namespace>:<name>`) must be on the
//! allowlist. Everything else is refused: deny by default.

mod extract;
pub mod jwks;
mod verifier;

pub use extract::Caller;
pub use jwks::{Fetched, JwksSource, KubeJwks};
pub use verifier::{AuthError, Verifier, VerifierOptions};
