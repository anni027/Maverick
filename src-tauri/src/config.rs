//! Configuration system for Nexus.
//!
//! Handles loading/saving `config.toml` with API keys, provider settings,
//! MCP server configs, and user preferences.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::providers::ProviderConfig;

/// Top-level configuration file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NexusConfig {
    /// API keys per provider (never logged).
    #[serde(default)]
    pub api_keys: HashMap<String, String>,

    /// Default provider to use for new sessions.
    #[serde(default)]
    pub default_provider: Option<String>,

    /// Per-provider base URL / model overrides for OpenAI/Anthropic-compatible APIs.
    #[serde(default)]
    pub provider_settings: HashMap<String, ProviderSettings>,

    /// Configured MCP servers.
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,

    /// UI preferences.
    #[serde(default)]
    pub ui: UiConfig,
}

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Transport type: "stdio" or "http"
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Command for stdio transport
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments for stdio transport
    #[serde(default)]
    pub args: Vec<String>,
    /// URL for http transport
    #[serde(default)]
    pub url: Option<String>,
    /// Whether this server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Per-provider overrides for OpenAI/Anthropic-compatible endpoints.
/// Allows any base URL + model, e.g. Ollama `http://localhost:11434/v1` + `llama3`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderSettings {
    /// Override base URL (e.g. `https://api.openai.com` or `http://localhost:11434/v1`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Override model name (e.g. `gpt-4o`, `claude-3-5-sonnet-20240620`, `llama3.2`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// For custom providers: which family to use (`openai` or `anthropic`). Defaults to `openai`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

fn default_transport() -> String {
    "stdio".to_string()
}
fn default_true() -> bool {
    true
}

/// UI preferences.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiConfig {
    /// Theme: "dark", "light", or "system"
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Show tool calls inline
    #[serde(default = "default_true")]
    pub show_tool_calls: bool,
    /// Auto-scroll to bottom on new messages
    #[serde(default = "default_true")]
    pub auto_scroll: bool,
    /// Compact mode (smaller message spacing)
    #[serde(default)]
    pub compact_mode: bool,
}

fn default_theme() -> String {
    "dark".to_string()
}

impl NexusConfig {
    /// Config file path: `<app_data>/config.toml`
    pub fn config_path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("config.toml")
    }

    /// Load config from disk, or create default.
    pub fn load(app_data_dir: &Path) -> Result<Self> {
        let path = Self::config_path(app_data_dir);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    /// Save config to disk atomically (write tmp + rename) to avoid
    /// torn writes if two `persist()` calls race.
    pub fn save(&self, app_data_dir: &Path) -> Result<()> {
        let path = Self::config_path(app_data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Get API key for a provider.
    pub fn api_key(&self, provider_id: &str) -> Option<String> {
        self.api_keys.get(provider_id).cloned()
    }

    /// Set API key for a provider.
    pub fn set_api_key(&mut self, provider_id: &str, key: String) {
        self.api_keys.insert(provider_id.to_string(), key);
    }

    /// Remove API key for a provider.
    pub fn remove_api_key(&mut self, provider_id: &str) {
        self.api_keys.remove(provider_id);
    }

    /// Get provider settings (base_url/model overrides).
    pub fn provider_settings(&self, provider_id: &str) -> Option<ProviderSettings> {
        self.provider_settings.get(provider_id).cloned()
    }

    /// Set provider settings (base_url / model / kind). Pass None to clear.
    pub fn set_provider_settings(&mut self, provider_id: &str, settings: ProviderSettings) {
        let is_empty = settings.base_url.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true)
            && settings.model.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true)
            && settings.kind.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true);
        if is_empty {
            self.provider_settings.remove(provider_id);
        } else {
            let mut s = settings;
            if s.base_url.as_ref().map(|x| x.trim().is_empty()).unwrap_or(false) {
                s.base_url = None;
            }
            if s.model.as_ref().map(|x| x.trim().is_empty()).unwrap_or(false) {
                s.model = None;
            }
            if s.kind.as_ref().map(|x| x.trim().is_empty()).unwrap_or(false) {
                s.kind = None;
            }
            self.provider_settings.insert(provider_id.to_string(), s);
        }
    }

    /// Effective base_url and model for a provider, with defaults for OpenAI/Anthropic-compatible.
    pub fn effective_provider_config(&self, provider_id: &str) -> (String, String, Option<String>) {
        let settings = self.provider_settings.get(provider_id);
        let api_key = self.api_key(provider_id);
        let (default_base, default_model) = match provider_id {
            "xai" => ("https://api.x.ai/v1".to_string(), "grok-4".to_string()),
            "openai" => ("https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
            "anthropic" => ("https://api.anthropic.com/v1".to_string(), "claude-3-5-sonnet-20240620".to_string()),
            _ => ("https://api.openai.com/v1".to_string(), "gpt-4o".to_string()),
        };
        let raw_base = settings
            .and_then(|s| s.base_url.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default_base);
        let base = crate::providers::normalize_base_url(provider_id, &raw_base);
        let model = settings
            .and_then(|s| s.model.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default_model);
        (base, model, api_key)
    }

    /// Get enabled MCP servers as ProviderConfig.
    pub fn mcp_provider_configs(&self) -> Vec<(String, ProviderConfig)> {
        self.mcp_servers
            .iter()
            .filter(|(_, c)| c.enabled)
            .filter_map(|(name, config)| {
                match config.transport.as_str() {
                    "stdio" => config.command.as_ref().map(|cmd| {
                        let config = ProviderConfig::Subprocess {
                            command: cmd.clone(),
                            args: config.args.clone(),
                            env: vec![],
                        };
                        (name.clone(), config)
                    }),
                    "http" => config.url.as_ref().map(|url| {
                        let config = ProviderConfig::Http {
                            base_url: url.clone(),
                            api_key: None,
                        };
                        (name.clone(), config)
                    }),
                    _ => None,
                }
            })
            .collect()
    }
}

/// Runtime configuration holder with hot-reload support.
pub struct ConfigManager {
    config: Arc<RwLock<NexusConfig>>,
    app_data_dir: PathBuf,
}

impl ConfigManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self> {
        let config = NexusConfig::load(&app_data_dir)?;
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            app_data_dir,
        })
    }

    /// Get a read guard on the config.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, NexusConfig> {
        self.config.read().await
    }

    /// Get a write guard on the config.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, NexusConfig> {
        self.config.write().await
    }

    /// Get API key for a provider.
    pub async fn api_key(&self, provider_id: &str) -> Option<String> {
        self.config.read().await.api_key(provider_id)
    }

    /// Set API key and persist.
    pub async fn set_api_key(&self, provider_id: &str, key: String) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.set_api_key(provider_id, key);
        }
        self.persist().await
    }

    /// Remove API key and persist.
    pub async fn remove_api_key(&self, provider_id: &str) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.remove_api_key(provider_id);
        }
        self.persist().await
    }

    /// Get provider settings.
    pub async fn provider_settings(&self, provider_id: &str) -> Option<ProviderSettings> {
        self.config.read().await.provider_settings(provider_id)
    }

    /// Set provider base_url / model / kind overrides and persist.
    pub async fn set_provider_settings(
        &self,
        provider_id: &str,
        base_url: Option<String>,
        model: Option<String>,
        kind: Option<String>,
    ) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.set_provider_settings(
                provider_id,
                ProviderSettings { base_url, model, kind },
            );
        }
        self.persist().await
    }

    /// Effective base_url/model + api_key for a provider.
    pub async fn effective_provider_config(&self, provider_id: &str) -> (String, String, Option<String>) {
        self.config.read().await.effective_provider_config(provider_id)
    }

    /// Get MCP provider configs.
    pub async fn mcp_provider_configs(&self) -> Vec<(String, ProviderConfig)> {
        self.config.read().await.mcp_provider_configs()
    }

    /// Add/update an MCP server.
    pub async fn add_mcp_server(&self, name: String, config: McpServerConfig) -> Result<()> {
        {
            let mut config_guard = self.config.write().await;
            config_guard.mcp_servers.insert(name, config);
        }
        self.persist().await
    }

    /// Remove an MCP server.
    pub async fn remove_mcp_server(&self, name: &str) -> Result<()> {
        {
            let mut config_guard = self.config.write().await;
            config_guard.mcp_servers.remove(name);
        }
        self.persist().await
    }

    /// Get default provider.
    pub async fn default_provider(&self) -> Option<String> {
        self.config.read().await.default_provider.clone()
    }

    /// Set default provider.
    pub async fn set_default_provider(&self, provider_id: Option<String>) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.default_provider = provider_id;
        }
        self.persist().await
    }

    /// Get UI config.
    pub async fn ui_config(&self) -> UiConfig {
        self.config.read().await.ui.clone()
    }

    /// Set UI config.
    pub async fn set_ui_config(&self, ui: UiConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.ui = ui;
        }
        self.persist().await
    }

    /// Get config for Tauri commands (serializable snapshot).
    pub async fn snapshot(&self) -> ConfigSnapshot {
        let config = self.config.read().await;
        ConfigSnapshot {
            api_keys: config.api_keys.keys().cloned().collect(),
            default_provider: config.default_provider.clone(),
            provider_settings: config.provider_settings.clone(),
            mcp_servers: config.mcp_servers.clone(),
            ui: config.ui.clone(),
        }
    }

    /// Persist current config to disk — snapshot under read lock so the
    /// filesystem write happens without holding the lock.
    async fn persist(&self) -> Result<()> {
        let snapshot = self.config.read().await.clone();
        snapshot.save(&self.app_data_dir)
    }
}

/// Serializable snapshot for Tauri commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub api_keys: Vec<String>,
    pub default_provider: Option<String>,
    pub provider_settings: HashMap<String, ProviderSettings>,
    pub mcp_servers: HashMap<String, McpServerConfig>,
    pub ui: UiConfig,
}