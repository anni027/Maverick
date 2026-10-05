//! Configuration system for Maverick.
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
pub struct MaverickConfig {
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
    /// Context management (Phase 3).
    #[serde(default)]
    pub context: ContextConfig,
    /// Turn/segment budgets + spend guardrails (§5.6-B/F).
    #[serde(default)]
    pub budget: BudgetConfig,
    /// Skills marketplace sources (GitHub repos used as skill catalogs).
    #[serde(default)]
    pub marketplace_sources: Vec<MarketplaceSource>,
    /// Saved model presets: provider + model + reasoning effort combos
    /// switchable from the composer menu and header badge. Legacy configs
    /// without the key deserialize to an empty list.
    #[serde(default)]
    pub model_presets: Vec<ModelPreset>,
    /// Unified cross-chat memory (global + workspace MEMORY.md). Legacy
    /// configs without the key deserialize to enabled-with-defaults.
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Clarifying-question policy (`ask_user` tool). Legacy configs without
    /// the key deserialize to enabled.
    #[serde(default)]
    pub interaction: InteractionConfig,
}

/// Context-management policy (Phase 3).
///
/// All knobs have safe defaults; the whole subsystem is a no-op unless
/// estimated context occupancy crosses `auto_compact_threshold_percent` of
/// the session context window. Mirrors the vendored `CompactionPolicy`
/// threshold semantics (percent of window) without pulling in the shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Master switch for segment-boundary checkpoints. Default true.
    #[serde(default = "default_true")]
    pub auto_compact_enabled: bool,
    /// Percentage of the context window that triggers a checkpoint.
    /// Default 85 (matches the vendored `CompactionPolicy` default).
    #[serde(default = "default_compact_threshold")]
    pub auto_compact_threshold_percent: u32,
    /// Newest conversation items always kept verbatim. Default 24.
    #[serde(default = "default_tail_keep")]
    pub tail_keep_items: usize,
    /// Per-tool output budgets in bytes (Phase 3 tuning): tool name →
    /// max bytes before head+tail truncation. Unlisted tools use the
    /// loop default (12 KB). Budgets only ever *shrink* below the default.
    #[serde(default)]
    pub tool_output_budgets: HashMap<String, usize>,
}

fn default_compact_threshold() -> u32 {
    85
}

fn default_tail_keep() -> usize {
    24
}

/// Turn/segment budgets + spend guardrails (§5.6-B/F).
///
/// `MAX_TURNS` (40) and `MAX_SEGMENTS` (3) used to be hardcoded in
/// `agent_loop.rs`; they now live here with the same defaults so real-run
/// measurements (§5.6-A) can tune them without a rebuild. Legacy
/// `config.toml` files without `[budget]` deserialize to these defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Tool-call turns allowed per segment before the forced wrap-up.
    /// Default 40. Clamped to ≥ 1 at read time.
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Auto-continued segments allowed per user message. Default 3.
    /// Clamped to ≥ 1 at read time.
    ///
    /// Safety limit only when `spend_cap_usd` is `None`: with a spend cap set,
    /// segments are unbounded and the run stops on natural finish, the
    /// identical-wrap-up guard, the cap, or cancel.
    #[serde(default = "default_max_segments")]
    pub max_segments: u32,
    /// When false, the runner stops after the first segment even on a
    /// budget hit (K=1 behaviour with graceful wrap-up). Default true.
    #[serde(default = "default_true")]
    pub auto_continue: bool,
    /// Optional per-task spend cap in USD. When the accumulated
    /// provider-reported cost exceeds it, further segments abort.
    /// Default None (unbounded). `None` also survives a TOML round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_cap_usd: Option<f64>,
    /// Assumed tokens per turn for the 80%-of-budget warn event
    /// (`max_segments × max_turns × avg_tokens_per_turn`). Default 2000.
    #[serde(default = "default_avg_tokens_per_turn")]
    pub avg_tokens_per_turn: u64,
}

fn default_max_turns() -> u32 {
    crate::agent_loop::MAX_TURNS
}

fn default_max_segments() -> u32 {
    crate::agent_loop::MAX_SEGMENTS
}

fn default_avg_tokens_per_turn() -> u64 {
    2000
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            max_segments: default_max_segments(),
            auto_continue: true,
            spend_cap_usd: None,
            avg_tokens_per_turn: default_avg_tokens_per_turn(),
        }
    }
}

/// Which memory files the loop reads and the editor shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// Global `MEMORY.md` + per-workspace file.
    #[default]
    Both,
    /// Global file only.
    Global,
    /// Workspace file only.
    Workspace,
}

impl MemoryScope {
    pub fn includes_global(self) -> bool {
        matches!(self, MemoryScope::Both | MemoryScope::Global)
    }

    pub fn includes_workspace(self) -> bool {
        matches!(self, MemoryScope::Both | MemoryScope::Workspace)
    }
}

/// Unified cross-chat memory policy.
///
/// Memories live in plain-Markdown `MEMORY.md` files (global + per
/// workspace); this config only controls whether they are read, written,
/// and how much context they may occupy. Legacy `config.toml` files without
/// `[memory]` deserialize to these defaults (enabled).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Master switch: injection, agent tools, and background extraction.
    /// Default true.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Post-run background extraction of durable facts via a cheap model.
    /// Default true.
    #[serde(default = "default_true")]
    pub auto_extract: bool,
    /// Which memory files the loop reads. Default both.
    #[serde(default)]
    pub scope: MemoryScope,
    /// Model slug used for background extraction (resolved against the
    /// active provider's credentials/base URL, model swapped in).
    /// Default "gpt-4o-mini".
    #[serde(default = "default_extract_model")]
    pub extract_model: String,
    /// Max injected memory chars per request. Default 8000.
    #[serde(default = "default_memory_max_chars")]
    pub max_chars: usize,
}

fn default_extract_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_memory_max_chars() -> usize {
    8000
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_extract: true,
            scope: MemoryScope::Both,
            extract_model: default_extract_model(),
            max_chars: default_memory_max_chars(),
        }
    }
}

/// Clarifying-question policy for the `ask_user` tool.
///
/// Legacy `config.toml` files without `[interaction]` deserialize to enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionConfig {
    /// Master switch: when false the `ask_user` tool is unoffered and any
    /// call is refused, so the model proceeds with its best judgment.
    /// Default true.
    #[serde(default = "default_true")]
    pub ask_user_enabled: bool,
}

impl Default for InteractionConfig {
    fn default() -> Self {
        Self {
            ask_user_enabled: true,
        }
    }
}

impl BudgetConfig {
    /// Effective per-segment turn budget (≥ 1).
    pub fn effective_max_turns(&self) -> u32 {
        self.max_turns.max(1)
    }

    /// Effective segment cap (≥ 1).
    pub fn effective_max_segments(&self) -> u32 {
        self.max_segments.max(1)
    }

    /// Turn at which the budget warning fires (≈ 3 turns before wrap-up).
    /// Saturating so tiny budgets (1–3) still warn on turn 1 instead of
    /// underflowing.
    pub fn warning_at(&self) -> u32 {
        self.effective_max_turns().saturating_sub(3).max(1)
    }

    /// Forced no-tools wrap-up turn.
    pub fn wrap_up_at(&self) -> u32 {
        self.effective_max_turns() + 1
    }

    /// Last-resort bail (model emits tool calls despite none offered).
    pub fn hard_ceiling(&self) -> u32 {
        self.effective_max_turns() + 2
    }

    /// Token budget for the 80% spend warning. Saturating: a zero average
    /// disables the warning (budget 0 ⇒ never reach 80% of nothing… instead
    /// treat as unbounded by returning `u64::MAX`).
    pub fn budget_tokens(&self) -> u64 {
        if self.avg_tokens_per_turn == 0 {
            return u64::MAX;
        }
        (self.effective_max_segments() as u64)
            .saturating_mul(self.effective_max_turns() as u64)
            .saturating_mul(self.avg_tokens_per_turn)
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            auto_compact_enabled: true,
            auto_compact_threshold_percent: default_compact_threshold(),
            tail_keep_items: default_tail_keep(),
            tool_output_budgets: HashMap::new(),
        }
    }
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

/// A saved model preset: a provider + model (+ optional reasoning effort)
/// combination the user can jump to from the composer menu or header badge.
/// Applying a preset changes the *global* default (provider settings +
/// default provider), matching how the rest of the app switches models.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPreset {
    /// Stable id (list key, deletion target). Frontend-generated.
    pub id: String,
    /// Display name chosen by the user.
    pub name: String,
    /// Provider the preset points at (e.g. `kilo`).
    pub provider_id: String,
    /// Model key for that provider (e.g. `swift-code` or namespaced).
    pub model: String,
    /// Reasoning effort label; `None` = provider/model default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

fn default_transport() -> String {
    "stdio".to_string()
}
fn default_true() -> bool {
    true
}

/// A GitHub repo used as a skills marketplace catalog source.
///
/// Skills are discovered as `SKILL.md` files under `skills_path`
/// (e.g. `skills/pdf/SKILL.md` in `anthropics/skills`), enumerated via
/// the public GitHub Trees API and downloaded via raw.githubusercontent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSource {
    /// Stable id, also used for the hub cache + catalog cache filenames.
    pub id: String,
    pub display_name: String,
    pub owner: String,
    pub repo: String,
    /// Branch to read (default `main`).
    #[serde(default = "default_marketplace_branch")]
    pub branch: String,
    /// Repo-relative directory scanned for skills (default `skills`).
    /// Empty string means the repo root.
    #[serde(default = "default_marketplace_skills_path")]
    pub skills_path: String,
}

fn default_marketplace_branch() -> String {
    "main".to_string()
}
fn default_marketplace_skills_path() -> String {
    "skills".to_string()
}

impl MarketplaceSource {
    /// Built-in default source. Returned by `ConfigManager` whenever the
    /// user has no stored sources, so a fresh install browses something
    /// without writing to `config.toml` first.
    pub fn anthropic_official() -> Self {
        Self {
            id: "anthropic-skills".to_string(),
            display_name: "Anthropic Official Skills".to_string(),
            owner: "anthropics".to_string(),
            repo: "skills".to_string(),
            branch: "main".to_string(),
            skills_path: "skills".to_string(),
        }
    }
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

impl MaverickConfig {
    /// Config file path: `<app_data>/config.toml`
    pub fn config_path(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("config.toml")
    }

    /// Load config from disk, or create default.
    ///
    /// A corrupt file is moved aside rather than refusing to start the app:
    /// losing a few settings beats a hard startup failure. The original is
    /// kept as `config.toml.corrupt` for manual recovery. Pure I/O failures
    /// (permissions, disk) still surface, since silently ignoring them would
    /// write defaults over a real config on the next persist.
    pub fn load(app_data_dir: &Path) -> Result<Self> {
        let path = Self::config_path(app_data_dir);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        match toml::from_str::<Self>(&content) {
            Ok(config) => Ok(config),
            Err(e) => {
                let backup = path.with_extension("toml.corrupt");
                let _ = std::fs::rename(&path, &backup);
                tracing::error!(
                    error = %e,
                    backup = %backup.display(),
                    "config.toml failed to parse; moved aside and starting from defaults"
                );
                Ok(Self::default())
            }
        }
    }

    /// Save config to disk atomically (write tmp + rename) to avoid
    /// torn writes if two `persist()` calls race.
    pub fn save(&self, app_data_dir: &Path) -> Result<()> {
        use std::io::Write;
        let path = Self::config_path(app_data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        // Sync before the rename: a crash between the two would otherwise
        // leave an empty/partial config.toml in place of a valid one.
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
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
        let is_empty = settings
            .base_url
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
            && settings
                .model
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            && settings
                .kind
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
        if is_empty {
            self.provider_settings.remove(provider_id);
        } else {
            let mut s = settings;
            if s.base_url
                .as_ref()
                .map(|x| x.trim().is_empty())
                .unwrap_or(false)
            {
                s.base_url = None;
            }
            if s.model
                .as_ref()
                .map(|x| x.trim().is_empty())
                .unwrap_or(false)
            {
                s.model = None;
            }
            if s.kind
                .as_ref()
                .map(|x| x.trim().is_empty())
                .unwrap_or(false)
            {
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
            "openai" => (
                "https://api.openai.com/v1".to_string(),
                "gpt-4o".to_string(),
            ),
            "anthropic" => (
                "https://api.anthropic.com/v1".to_string(),
                "claude-3-5-sonnet-20240620".to_string(),
            ),
            _ => (
                "https://api.openai.com/v1".to_string(),
                "gpt-4o".to_string(),
            ),
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
            .filter_map(|(name, config)| match config.transport.as_str() {
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
            })
            .collect()
    }
}

/// Runtime configuration holder with hot-reload support.
pub struct ConfigManager {
    config: Arc<RwLock<MaverickConfig>>,
    app_data_dir: PathBuf,
    /// Serializes disk writes. Two commands persisting at once would share the
    /// same `config.toml.tmp` and the loser's rename can fail (or land a
    /// half-written file) once the winner already moved it into place.
    persist_lock: tokio::sync::Mutex<()>,
}

impl ConfigManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self> {
        let mut config = MaverickConfig::load(&app_data_dir)?;
        if config.marketplace_sources.is_empty() {
            // Fresh installs (and pre-marketplace configs) browse the
            // built-in source. In-memory only — persisted on the next
            // config mutation, so startup never rewrites the user's file.
            config
                .marketplace_sources
                .push(MarketplaceSource::anthropic_official());
        }
        if config.default_provider.as_deref() == Some("xai") {
            // The xAI provider was removed: remap to auto-resolve
            // (first available). In-memory only, same precedent as above.
            config.default_provider = None;
        }
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            app_data_dir,
            persist_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Get a read guard on the config.
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, MaverickConfig> {
        self.config.read().await
    }

    /// Get a write guard on the config.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, MaverickConfig> {
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
                ProviderSettings {
                    base_url,
                    model,
                    kind,
                },
            );
        }
        self.persist().await
    }

    /// Effective base_url/model + api_key for a provider.
    pub async fn effective_provider_config(
        &self,
        provider_id: &str,
    ) -> (String, String, Option<String>) {
        self.config
            .read()
            .await
            .effective_provider_config(provider_id)
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

    /// Stored marketplace sources (the built-in default is seeded in
    /// `ConfigManager::new`, so this is never empty in practice).
    pub async fn marketplace_sources(&self) -> Vec<MarketplaceSource> {
        self.config.read().await.marketplace_sources.clone()
    }

    /// Add (or replace by id) a marketplace source and persist.
    pub async fn add_marketplace_source(&self, source: MarketplaceSource) -> Result<()> {
        {
            let mut config_guard = self.config.write().await;
            if let Some(existing) = config_guard
                .marketplace_sources
                .iter_mut()
                .find(|s| s.id == source.id)
            {
                *existing = source;
            } else {
                config_guard.marketplace_sources.push(source);
            }
        }
        self.persist().await
    }

    /// Remove a marketplace source by id and persist.
    pub async fn remove_marketplace_source(&self, id: &str) -> Result<()> {
        {
            let mut config_guard = self.config.write().await;
            config_guard.marketplace_sources.retain(|s| s.id != id);
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

    /// Get saved model presets.
    /// Get saved model presets (entries for the removed xAI provider are
    /// filtered out; they vanish from the file on the next preset save).
    pub async fn model_presets(&self) -> Vec<ModelPreset> {
        self.config
            .read()
            .await
            .model_presets
            .iter()
            .filter(|p| p.provider_id != "xai")
            .cloned()
            .collect()
    }

    /// Replace the model preset list and persist.
    pub async fn set_model_presets(&self, presets: Vec<ModelPreset>) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.model_presets = presets;
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

    /// Get context-management config.
    pub async fn context_config(&self) -> ContextConfig {
        self.config.read().await.context.clone()
    }

    /// Set context-management config and persist.
    pub async fn set_context_config(&self, context: ContextConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.context = context;
        }
        self.persist().await
    }

    /// Get turn/segment budget config.
    pub async fn budget_config(&self) -> BudgetConfig {
        self.config.read().await.budget.clone()
    }

    /// Set turn/segment budget config and persist.
    pub async fn set_budget_config(&self, budget: BudgetConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.budget = budget;
        }
        self.persist().await
    }

    /// Get unified-memory config.
    pub async fn memory_config(&self) -> MemoryConfig {
        self.config.read().await.memory.clone()
    }

    /// Set unified-memory config and persist.
    pub async fn set_memory_config(&self, memory: MemoryConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.memory = memory;
        }
        self.persist().await
    }

    /// Get clarifying-question config.
    pub async fn interaction_config(&self) -> InteractionConfig {
        self.config.read().await.interaction.clone()
    }

    /// Set clarifying-question config and persist.
    pub async fn set_interaction_config(&self, interaction: InteractionConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config.interaction = interaction;
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
            context: config.context.clone(),
            budget: config.budget.clone(),
            memory: config.memory.clone(),
            interaction: config.interaction.clone(),
        }
    }

    /// Persist current config to disk — snapshot under read lock so the
    /// filesystem write happens without holding the lock.
    async fn persist(&self) -> Result<()> {
        let _write = self.persist_lock.lock().await;
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
    pub context: ContextConfig,
    #[serde(default)]
    pub budget: BudgetConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub interaction: InteractionConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_without_presets_deserializes() {
        let toml = r#"
default_provider = "openai"

[ui]
theme = "dark"
"#;
        let config: MaverickConfig = toml::from_str(toml).expect("legacy config parses");
        assert_eq!(config.default_provider.as_deref(), Some("openai"));
        assert!(config.model_presets.is_empty());
    }

    #[test]
    fn presets_round_trip_through_toml() {
        let mut config = MaverickConfig::default();
        config.model_presets = vec![
            ModelPreset {
                id: "p1".into(),
                name: "Fast coder".into(),
                provider_id: "kilo".into(),
                model: "swift-code".into(),
                effort: Some("high".into()),
            },
            ModelPreset {
                id: "p2".into(),
                name: "Default effort".into(),
                provider_id: "openai".into(),
                model: "gpt-5".into(),
                effort: None,
            },
        ];
        let encoded = toml::to_string_pretty(&config).expect("serializes");
        let decoded: MaverickConfig = toml::from_str(&encoded).expect("parses back");
        assert_eq!(decoded.model_presets, config.model_presets);
    }

    #[test]
    fn preset_effort_none_is_omitted_from_toml() {
        let mut config = MaverickConfig::default();
        config.model_presets = vec![ModelPreset {
            id: "p2".into(),
            name: "Plain".into(),
            provider_id: "openai".into(),
            model: "gpt-5".into(),
            effort: None,
        }];
        let encoded = toml::to_string_pretty(&config).expect("serializes");
        assert!(!encoded.contains("effort"));
        let decoded: MaverickConfig = toml::from_str(&encoded).expect("parses back");
        assert_eq!(decoded.model_presets[0].effort, None);
    }

    #[test]
    fn preset_effort_defaults_to_none() {
        let toml = r#"
id = "p3"
name = "No effort key"
provider_id = "kilo"
model = "swift-code"
"#;
        let preset: ModelPreset = toml::from_str(toml).expect("parses without effort");
        assert_eq!(preset.effort, None);
    }

    #[test]
    fn legacy_config_without_memory_deserializes_enabled() {
        let toml = r#"
default_provider = "openai"

[ui]
theme = "dark"
"#;
        let config: MaverickConfig = toml::from_str(toml).expect("legacy config parses");
        assert!(config.memory.enabled);
        assert!(config.memory.auto_extract);
        assert_eq!(config.memory.scope, MemoryScope::Both);
        assert_eq!(config.memory.extract_model, "gpt-4o-mini");
        assert_eq!(config.memory.max_chars, 8000);
    }

    #[test]
    fn memory_round_trip_through_toml() {
        let mut config = MaverickConfig::default();
        config.memory.enabled = false;
        config.memory.scope = MemoryScope::Workspace;
        config.memory.extract_model = "llama3".to_string();
        let encoded = toml::to_string_pretty(&config).expect("serializes");
        let decoded: MaverickConfig = toml::from_str(&encoded).expect("parses back");
        assert!(!decoded.memory.enabled);
        assert_eq!(decoded.memory.scope, MemoryScope::Workspace);
        assert_eq!(decoded.memory.extract_model, "llama3");
        assert_eq!(decoded.memory.max_chars, 8000);
    }

    #[test]
    fn legacy_config_without_interaction_deserializes_enabled() {
        let toml = r#"
default_provider = "openai"

[ui]
theme = "dark"
"#;
        let config: MaverickConfig = toml::from_str(toml).expect("legacy config parses");
        assert!(config.interaction.ask_user_enabled);
    }

    #[test]
    fn interaction_round_trip_through_toml() {
        let mut config = MaverickConfig::default();
        config.interaction.ask_user_enabled = false;
        let encoded = toml::to_string_pretty(&config).expect("serializes");
        let decoded: MaverickConfig = toml::from_str(&encoded).expect("parses back");
        assert!(!decoded.interaction.ask_user_enabled);
    }

    /// The removed xAI provider migrates gracefully: stored "xai" defaults
    /// remap to auto-resolve, dead xai presets are filtered from reads.
    #[test]
    fn xai_removal_migrates_defaults_and_presets() {
        let dir = std::env::temp_dir().join(format!("maverick-xai-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "default_provider = \"xai\"\n\n\
             [[model_presets]]\n\
             id = \"dead\"\n\
             name = \"Dead\"\n\
             provider_id = \"xai\"\n\
             model = \"old\"\n\n\
             [[model_presets]]\n\
             id = \"live\"\n\
             name = \"Live\"\n\
             provider_id = \"openai\"\n\
             model = \"gpt-4o\"\n",
        )
        .unwrap();
        let manager = ConfigManager::new(dir.clone()).expect("manager loads");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            assert_eq!(manager.default_provider().await, None);
            let presets = manager.model_presets().await;
            assert_eq!(presets.len(), 1);
            assert_eq!(presets[0].id, "live");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
