use std::sync::{Arc, atomic::AtomicBool};

use crate::{auth::Verifier, config::Config, store::Store};

/// Shared by all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    /// `/readyz` answers 200 while this is true (and the store answers).
    pub ready: Arc<AtomicBool>,
    /// Present when `AUTH_AUDIENCE` is configured.
    pub verifier: Option<Arc<Verifier>>,
    pub store: Arc<dyn Store>,
}

impl AppState {
    /// Not ready until the caller sets `ready` (after the migrations ran).
    pub fn new(config: Config, verifier: Option<Verifier>, store: Arc<dyn Store>) -> Self {
        Self {
            config: Arc::new(config),
            ready: Arc::new(AtomicBool::new(false)),
            verifier: verifier.map(Arc::new),
            store,
        }
    }
}
