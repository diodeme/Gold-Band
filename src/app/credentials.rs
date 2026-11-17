//! Credential storage for server connections.
//!
//! Secrets (SSH password / key passphrase) never live in the connections JSON
//! config. They are stored in the OS credential store, keyed by the connection
//! stable id (service `gold-band-ssh`). The connections config only records the
//! fact that a credential is present (`credentialRef`), never the secret itself.
//!
//! The storage is abstracted behind [`CredentialStore`] so the app can resolve
//! the appropriate backend: the real OS keyring in production, and an in-memory
//! store in tests (the OS keychain is not hermetic and may be unavailable in a
//! headless CI / sandbox).

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

/// Service name used for every keyring entry so secrets are namespaced under a
/// single, identifiable group in the OS credential manager.
pub const CREDENTIAL_SERVICE_NAME: &str = "gold-band-ssh";

/// Abstract per-connection secret store. All operations are keyed solely by the
/// connection stable id so a connection can be removed / renamed without leaking
/// or duplicating its credential.
pub trait CredentialStore: Send + Sync {
    fn store_secret(&self, connection_id: &str, secret: &str) -> Result<()>;
    /// Returns `None` when no secret exists for `connection_id`.
    fn get_secret(&self, connection_id: &str) -> Result<Option<String>>;
    fn delete_secret(&self, connection_id: &str) -> Result<()>;
    /// Returns whether a secret exists for `connection_id`. Never returns the
    /// secret itself; used only to derive `credentialRef`.
    fn has_secret(&self, connection_id: &str) -> Result<bool>;
}

/// Production backend backed by the OS credential manager (macOS Keychain /
/// Windows Credential Manager / Linux Secret Service).
pub struct KeyringCredentialStore {
    service: &'static str,
}

impl KeyringCredentialStore {
    pub fn new() -> Self {
        Self {
            service: CREDENTIAL_SERVICE_NAME,
        }
    }
}

impl Default for KeyringCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn store_secret(&self, connection_id: &str, secret: &str) -> Result<()> {
        let entry = keyring::Entry::new(self.service, connection_id)
            .map_err(|error| anyhow!("failed to open keyring entry: {error}"))?;
        entry
            .set_password(secret)
            .with_context(|| format!("failed to store secret for `{connection_id}`"))
    }

    fn get_secret(&self, connection_id: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(self.service, connection_id)
            .map_err(|error| anyhow!("failed to open keyring entry: {error}"))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(anyhow!("failed to read keyring secret: {error}")),
        }
    }

    fn delete_secret(&self, connection_id: &str) -> Result<()> {
        let entry = keyring::Entry::new(self.service, connection_id)
            .map_err(|error| anyhow!("failed to open keyring entry: {error}"))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow!("failed to delete keyring secret: {error}")),
        }
    }

    fn has_secret(&self, connection_id: &str) -> Result<bool> {
        self.get_secret(connection_id)
            .map(|secret| secret.is_some())
    }
}

/// Configures the app-wide [`CredentialStore`]. Kept out of `App`'s core
/// runtime paths so production always defaults to the real OS keyring while
/// tests can swap in a deterministic in-memory store.
pub fn default_credential_store() -> Arc<dyn CredentialStore> {
    Arc::new(KeyringCredentialStore::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    pub struct InMemoryCredentialStore {
        entries: std::sync::Mutex<std::collections::HashMap<String, String>>,
    }

    impl InMemoryCredentialStore {
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl CredentialStore for InMemoryCredentialStore {
        fn store_secret(&self, connection_id: &str, secret: &str) -> Result<()> {
            self.entries
                .lock()
                .map_err(|_| anyhow!("in-memory credential store poisoned"))?
                .insert(connection_id.to_string(), secret.to_string());
            Ok(())
        }

        fn get_secret(&self, connection_id: &str) -> Result<Option<String>> {
            Ok(self
                .entries
                .lock()
                .map_err(|_| anyhow!("in-memory credential store poisoned"))?
                .get(connection_id)
                .cloned())
        }

        fn delete_secret(&self, connection_id: &str) -> Result<()> {
            self.entries
                .lock()
                .map_err(|_| anyhow!("in-memory credential store poisoned"))?
                .remove(connection_id);
            Ok(())
        }

        fn has_secret(&self, connection_id: &str) -> Result<bool> {
            Ok(self
                .entries
                .lock()
                .map_err(|_| anyhow!("in-memory credential store poisoned"))?
                .contains_key(connection_id))
        }
    }
}

/// Test helper: a fresh, isolated in-memory store. Only construct it under
/// `#[cfg(test)]` so the memory store never ships into the production binary.
#[cfg(test)]
pub fn test_in_memory_store() -> Arc<dyn CredentialStore> {
    Arc::new(self::tests::InMemoryCredentialStore::new())
}
