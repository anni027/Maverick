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
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, info: ProviderInfo) {
        self.providers.insert(info.id.clone(), info);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).map(|i| Arc::clone(&i.provider))
    }

    pub fn list(&self) -> Vec<&ProviderInfo> {
        self.providers.values().collect()
    }

    pub fn ids(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    pub fn config(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.get(id).map(|i| &i.config)
    }

    pub fn default_id(&self) -> Option<&str> {
        self.providers.keys().next().map(String::as_str)
    }

    pub fn remove(&mut self, id: &str) -> Option<ProviderInfo> {
        self.providers.remove(id)
    }
}