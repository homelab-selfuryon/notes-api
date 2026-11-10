//! Secrets are files: OpenBao values mounted by the Secrets Store CSI driver
//! under `/var/run/secrets/app/<key>` and the database credentials under
//! `/var/run/secrets/db/`. They are read once at start-up and wrapped in
//! [`SecretString`], whose `Debug` output is redacted and which is zeroed on
//! drop. Never log the value, never put it in an environment variable.

use std::path::Path;

use anyhow::{Context, Result};
use secrecy::SecretString;

/// Where the platform mounts the app's OpenBao secrets.
pub const APP_SECRETS_DIR: &str = "/var/run/secrets/app";
/// Where the platform mounts the database credentials.
pub const DB_SECRETS_DIR: &str = "/var/run/secrets/db";

/// Reads `<dir>/<name>`; one trailing newline is removed.
pub fn read(dir: &Path, name: &str) -> Result<SecretString> {
    read_file(&dir.join(name))
}

/// Reads the file at `path`; one trailing newline is removed.
pub fn read_file(path: &Path) -> Result<SecretString> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading secret file {}", path.display()))?;
    Ok(SecretString::from(
        raw.strip_suffix('\n').unwrap_or(&raw).to_owned(),
    ))
}

/// Like [`read`], but a missing file is `None`.
pub fn read_optional(dir: &Path, name: &str) -> Result<Option<SecretString>> {
    if dir.join(name).exists() {
        read(dir, name).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn reads_and_trims_one_newline() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("key"), "s3cret\n")?;
        let secret = read(dir.path(), "key")?;
        assert_eq!(secret.expose_secret(), "s3cret");
        Ok(())
    }

    #[test]
    fn debug_output_is_redacted() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("key"), "s3cret")?;
        let secret = read(dir.path(), "key")?;
        assert!(!format!("{secret:?}").contains("s3cret"));
        Ok(())
    }

    #[test]
    fn missing_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        assert!(read(dir.path(), "nope").is_err());
        assert!(read_optional(dir.path(), "nope")?.is_none());
        Ok(())
    }
}
