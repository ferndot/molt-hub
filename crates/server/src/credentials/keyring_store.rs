//! System keychain credential store.
//!
//! Uses the `keyring` crate to delegate to the platform keychain:
//! - macOS Keychain on macOS
//! - Secret Service on Linux
//! - Windows Credential Manager on Windows
//!
//! Storage keys follow the format `molt-hub:{scope}:{alias}`.
//! The keyring service name is always `"molt-hub"`.
//!
//! # Note on `list`
//!
//! The `keyring` crate (v3) does not expose an enumerate-by-prefix API,
//! so `list` is not natively supported. This implementation returns an
//! `Err(CredentialError::Keychain)` with an explanatory message.
//! Callers that need listing should use [`MemoryStore`](super::MemoryStore)
//! or maintain their own alias index.

use keyring::Entry;

use super::{CredentialError, CredentialScope, CredentialStore};

const SERVICE: &str = "molt-hub";

/// Credential store backed by the OS keychain.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        Self
    }

    fn entry(alias: &str, scope: &CredentialScope) -> Result<Entry, CredentialError> {
        // The account name encodes both scope and alias: "{scope}:{alias}"
        let account = format!("{scope}:{alias}");
        Entry::new(SERVICE, &account).map_err(|e| CredentialError::Keychain(e.to_string()))
    }
}

impl CredentialStore for KeyringStore {
    fn store(
        &self,
        alias: &str,
        scope: &CredentialScope,
        value: &str,
    ) -> Result<(), CredentialError> {
        if alias.is_empty() {
            return Err(CredentialError::InvalidAlias(alias.into()));
        }
        let entry = Self::entry(alias, scope)?;
        entry
            .set_password(value)
            .map_err(|e| CredentialError::Keychain(e.to_string()))
    }

    fn retrieve(&self, alias: &str, scope: &CredentialScope) -> Result<String, CredentialError> {
        let entry = Self::entry(alias, scope)?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => CredentialError::NotFound {
                alias: alias.into(),
                scope: scope.to_string(),
            },
            other => CredentialError::Keychain(other.to_string()),
        })
    }

    fn delete(&self, alias: &str, scope: &CredentialScope) -> Result<(), CredentialError> {
        let entry = Self::entry(alias, scope)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            // Deleting a non-existent credential is idempotent.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Keychain(e.to_string())),
        }
    }

    fn list(&self, _scope: &CredentialScope) -> Result<Vec<String>, CredentialError> {
        Err(CredentialError::Keychain(
            "KeyringStore does not support listing credentials; \
             maintain an alias index externally or use MemoryStore in tests"
                .into(),
        ))
    }
}
