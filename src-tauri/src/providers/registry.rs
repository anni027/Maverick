//! Provider registry: maps provider ids to [`Provider`]s.
//!
//! Phase 2 seeds the registry with the native HTTP providers and lets the
//! UI register external agent providers (Kilo Code / OpenCode subprocesses,
//! MCP servers) at runtime.

use std::collections::HashMap;
use std::sync::Arc;

use super::provider::{Provider, ProviderConfig, ProviderInfo};

#[derive(Default)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderInfo>,
    /// Insertion order. `HashMap` iteration order changes between runs, which
    /// made the provider list — and the "first provider" fallback for new
    /// sessions — jump around arbitrarily.
    order: Vec<String>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, info: ProviderInfo) {
        let id = info.id.clone();
        if !self.providers.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.providers.insert(id, info);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).map(|i| Arc::clone(&i.provider))
    }

    pub fn list(&self) -> Vec<&ProviderInfo> {
        self.order
            .iter()
            .filter_map(|id| self.providers.get(id))
            .collect()
    }

    pub fn ids(&self) -> Vec<&str> {
        self.order.iter().map(String::as_str).collect()
    }

    pub fn config(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.get(id).map(|i| &i.config)
    }

    pub fn default_id(&self) -> Option<&str> {
        self.order.first().map(String::as_str)
    }

    pub fn remove(&mut self, id: &str) -> Option<ProviderInfo> {
        self.order.retain(|existing| existing != id);
        self.providers.remove(id)
    }

    /// Resolve the provider a session should run on: an explicit id when it is
    /// registered, otherwise the first provider in insertion order.
    ///
    /// This is the single copy of the fallback rule — `init_session` and the
    /// post-`remove_api_key` reconciliation both call it, so a session that
    /// loses its provider falls back exactly the same way a new one resolves.
    pub fn resolve(&self, wanted: &str) -> Result<Arc<dyn Provider>, String> {
        if !wanted.is_empty() {
            if let Some(p) = self.get(wanted) {
                return Ok(p);
            }
        }
        self.list()
            .first()
            .map(|info| Arc::clone(&info.provider))
            .ok_or_else(|| {
                "No provider configured - add an API key in Settings (xAI/OpenAI/Anthropic)"
                    .to_string()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::provider::{ProviderCapabilities, ProviderKind};
    use xai_grok_sampling_types::{ConversationRequest, ConversationResponse};

    struct StubProvider(&'static str);

    #[async_trait::async_trait]
    impl Provider for StubProvider {
        fn id(&self) -> &str {
            self.0
        }
        fn name(&self) -> &str {
            self.0
        }
        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenAi
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::default()
        }
        async fn complete(
            &self,
            _request: ConversationRequest,
        ) -> anyhow::Result<ConversationResponse> {
            anyhow::bail!("stub provider")
        }
    }

    fn info(id: &'static str) -> ProviderInfo {
        ProviderInfo {
            id: id.to_string(),
            name: id.to_string(),
            kind: ProviderKind::OpenAi,
            provider: Arc::new(StubProvider(id)),
            config: ProviderConfig::Http {
                base_url: "https://example.test".to_string(),
                api_key: None,
            },
        }
    }

    #[test]
    fn resolve_prefers_the_explicit_id() {
        let mut reg = ProviderRegistry::new();
        reg.register(info("xai"));
        reg.register(info("openai"));
        assert_eq!(reg.resolve("openai").unwrap().id(), "openai");
    }

    #[test]
    fn resolve_falls_back_to_the_first_registered() {
        let mut reg = ProviderRegistry::new();
        reg.register(info("xai"));
        reg.register(info("openai"));
        // Unknown explicit id and empty explicit id both mean "first registered".
        assert_eq!(reg.resolve("gone").unwrap().id(), "xai");
        assert_eq!(reg.resolve("").unwrap().id(), "xai");
    }

    #[test]
    fn resolve_errors_when_nothing_is_registered() {
        let reg = ProviderRegistry::new();
        // `.err()` rather than `unwrap_err()`: `Arc<dyn Provider>` is not `Debug`.
        let err = reg
            .resolve("xai")
            .err()
            .expect("resolve must fail with an empty registry");
        assert!(err.contains("No provider configured"), "got: {err}");
    }

    #[test]
    fn resolve_tracks_the_registry_after_remove() {
        let mut reg = ProviderRegistry::new();
        reg.register(info("xai"));
        reg.register(info("openai"));
        reg.remove("xai");
        assert_eq!(reg.resolve("xai").unwrap().id(), "openai");
        reg.remove("openai");
        assert!(reg.resolve("xai").is_err());
        assert!(reg.ids().is_empty());
    }
}
