//! In-memory credential store for testing.
//!
//! Uses a `DashMap` keyed by storage key strings so it can be shared
//! across threads without wrapping in a Mutex.

use dashmap::DashMap;

use super::{storage_key, CredentialError, CredentialScope, CredentialStore};

/// An in-memory credential store.
///
/// All operations are synchronous and backed by a concurrent [`DashMap`].
/// Intended for unit tests; not suitable for production use.
#[derive(Debug, Default)]
pub struct MemoryStore {
    entries: DashMap<String, String>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for MemoryStore {
    fn store(
        &self,
        alias: &str,
        scope: &CredentialScope,
        value: &str,
    ) -> Result<(), CredentialError> {
        if alias.is_empty() {
            return Err(CredentialError::InvalidAlias(alias.into()));
        }
        self.entries.insert(storage_key(alias, scope), value.to_string());
        Ok(())
    }

    fn retrieve(&self, alias: &str, scope: &CredentialScope) -> Result<String, CredentialError> {
        let key = storage_key(alias, scope);
        self.entries
            .get(&key)
            .map(|v| v.clone())
            .ok_or_else(|| CredentialError::NotFound {
                alias: alias.into(),
                scope: scope.to_string(),
            })
    }

    fn delete(&self, alias: &str, scope: &CredentialScope) -> Result<(), CredentialError> {
        let key = storage_key(alias, scope);
        self.entries.remove(&key);
        Ok(())
    }

    fn list(&self, scope: &CredentialScope) -> Result<Vec<String>, CredentialError> {
        let prefix = format!("molt-hub:{scope}:");
        let mut aliases: Vec<String> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let k = entry.key();
                k.strip_prefix(&prefix).map(|alias| alias.to_string())
            })
            .collect();
        aliases.sort();
        Ok(aliases)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn global() -> CredentialScope {
        CredentialScope::Global
    }

    fn pipeline(name: &str) -> CredentialScope {
        CredentialScope::Pipeline(name.into())
    }

    #[test]
    fn store_and_retrieve_roundtrip() {
        let store = MemoryStore::new();
        store.store("TOKEN", &global(), "secret123").unwrap();
        let val = store.retrieve("TOKEN", &global()).unwrap();
        assert_eq!(val, "secret123");
    }

    #[test]
    fn retrieve_missing_returns_not_found() {
        let store = MemoryStore::new();
        let err = store.retrieve("MISSING", &global()).unwrap_err();
        assert!(matches!(err, CredentialError::NotFound { .. }));
    }

    #[test]
    fn overwrite_existing_credential() {
        let store = MemoryStore::new();
        store.store("TOKEN", &global(), "v1").unwrap();
        store.store("TOKEN", &global(), "v2").unwrap();
        assert_eq!(store.retrieve("TOKEN", &global()).unwrap(), "v2");
    }

    #[test]
    fn delete_removes_credential() {
        let store = MemoryStore::new();
        store.store("TOKEN", &global(), "secret").unwrap();
        store.delete("TOKEN", &global()).unwrap();
        assert!(matches!(
            store.retrieve("TOKEN", &global()),
            Err(CredentialError::NotFound { .. })
        ));
    }

    #[test]
    fn delete_nonexistent_is_ok() {
        let store = MemoryStore::new();
        assert!(store.delete("NOPE", &global()).is_ok());
    }

    #[test]
    fn list_returns_aliases_for_scope() {
        let store = MemoryStore::new();
        let scope = pipeline("ci");
        store.store("ALPHA", &scope, "a").unwrap();
        store.store("BETA", &scope, "b").unwrap();
        // Different scope — must not appear.
        store.store("GAMMA", &global(), "g").unwrap();

        let mut aliases = store.list(&scope).unwrap();
        aliases.sort();
        assert_eq!(aliases, vec!["ALPHA", "BETA"]);
    }

    #[test]
    fn list_empty_scope_returns_empty() {
        let store = MemoryStore::new();
        let aliases = store.list(&global()).unwrap();
        assert!(aliases.is_empty());
    }

    #[test]
    fn scope_isolation() {
        let store = MemoryStore::new();
        let g = global();
        let p = pipeline("ci");
        store.store("TOKEN", &g, "global_val").unwrap();
        store.store("TOKEN", &p, "pipeline_val").unwrap();

        assert_eq!(store.retrieve("TOKEN", &g).unwrap(), "global_val");
        assert_eq!(store.retrieve("TOKEN", &p).unwrap(), "pipeline_val");
    }

    #[test]
    fn empty_alias_returns_error() {
        let store = MemoryStore::new();
        assert!(matches!(
            store.store("", &global(), "val"),
            Err(CredentialError::InvalidAlias(_))
        ));
    }
}
