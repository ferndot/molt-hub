//! Optional index of `(scope, alias)` pairs for backends that cannot enumerate entries
//! (for example [`super::KeyringStore`]).

use std::sync::Arc;

use dashmap::DashMap;

use super::CredentialScope;

/// Tracks which credential aliases exist per scope.
///
/// Call [`Self::register`] after a successful [`super::CredentialStore::store`] and
/// [`Self::unregister`] after [`super::CredentialStore::delete`].
#[derive(Clone, Default)]
pub struct CredentialAliasIndex {
    scopes: Arc<DashMap<String, DashMap<String, ()>>>,
}

impl CredentialAliasIndex {
    pub fn new() -> Self {
        Self::default()
    }

    fn scope_key(scope: &CredentialScope) -> String {
        scope.to_string()
    }

    pub fn register(&self, scope: &CredentialScope, alias: &str) {
        let sk = Self::scope_key(scope);
        self.scopes
            .entry(sk)
            .or_default()
            .insert(alias.to_string(), ());
    }

    pub fn unregister(&self, scope: &CredentialScope, alias: &str) {
        let sk = Self::scope_key(scope);
        if let Some(inner) = self.scopes.get_mut(&sk) {
            inner.remove(alias);
        }
    }

    /// Sorted alias names recorded for this scope (may be empty).
    pub fn list(&self, scope: &CredentialScope) -> Vec<String> {
        let sk = Self::scope_key(scope);
        let mut out: Vec<String> = self
            .scopes
            .get(&sk)
            .map(|m| m.iter().map(|e| e.key().clone()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use molt_hub_core::model::ProjectId;

    #[test]
    fn register_list_roundtrip() {
        let idx = CredentialAliasIndex::new();
        let scope = CredentialScope::Global;
        idx.register(&scope, "a/token");
        idx.register(&scope, "b/token");
        let mut list = idx.list(&scope);
        list.sort();
        assert_eq!(list, vec!["a/token", "b/token"]);
    }

    #[test]
    fn scopes_stay_isolated() {
        let idx = CredentialAliasIndex::new();
        let g = CredentialScope::Global;
        let p = CredentialScope::Project(ProjectId::new());
        idx.register(&g, "x");
        idx.register(&p, "y");
        assert_eq!(idx.list(&g), vec!["x"]);
        assert_eq!(idx.list(&p), vec!["y"]);
    }

    #[test]
    fn unregister_removes_alias() {
        let idx = CredentialAliasIndex::new();
        let scope = CredentialScope::Global;
        idx.register(&scope, "only");
        idx.unregister(&scope, "only");
        assert!(idx.list(&scope).is_empty());
    }
}
