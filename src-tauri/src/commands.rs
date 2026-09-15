//! Tauri commands — the bridge between the React UI and the Rust backend.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{command, AppHandle, State};

use crate::{
    agent_event::TauriSink,
    agent_loop::AgentLoop,
    providers::{ProviderInfoDto, ProviderRegistry},
    session_store::SessionManager,
    tools::{add_mcp_server, build_chat_handle, PermissionGuard},
};
use xai_grok_tools::bridge::ToolBridge;

// Application state shared across commands.
pub struct AppState {
    pub session_manager: SessionManager,
    pub provider_registry: Arc<tokio::sync::RwLock<ProviderRegistry>>,
    pub permission_guard: PermissionGuard,
    /// Per-session agent loops. `ToolBridge` is `Clone` (Arc-backed) so each
    /// session gets a cheap clone plus its own `ChatStateHandle`.
    /// Stored as `Arc` so the read lock can be released before awaiting.
    pub agent_loops: Arc<tokio::sync::RwLock<HashMap<String, Arc<AgentLoop>>>>,
    pub tool_bridge: Arc<ToolBridge>,
    pub config_manager: Arc<crate::config::ConfigManager>,
    pub app_data_dir: std::path::PathBuf,
}

impl AppState {
    pub async fn new(app_data_dir: std::path::PathBuf) -> anyhow::Result<Self> {
        let session_manager = SessionManager::new(app_data_dir.clone())?;
        let config_manager = Arc::new(crate::config::ConfigManager::new(app_data_dir.clone())?);
        let mut provider_registry = ProviderRegistry::new();

        // Register native providers for which we have API keys, with any base_url/model overrides
        {
            let cfg = config_manager.read().await;
            for key in ["xai", "openai", "anthropic"] {
                if let Some(api_key) = cfg.api_key(key) {
                    let settings = cfg.provider_settings(key);
                    if let Some(info) = crate::providers::provider_info_for(
                        key,
                        Some(api_key),
                        settings.as_ref().and_then(|s| s.base_url.clone()),
                        settings.as_ref().and_then(|s| s.model.clone()),
                        None,
                    ) {
                        provider_registry.register(info);
                    }
                }
            }
            // Also register any custom providers that have settings but no api_keys entry yet
            // (e.g. Ollama local with no key). They are stored in provider_settings.
            for (id, settings) in &cfg.provider_settings {
                if ["xai", "openai", "anthropic"].contains(&id.as_str()) {
                    continue;
                }
                let api_key = cfg.api_key(id);
                let base = settings.base_url.clone();
                let model = settings.model.clone();
                let kind = settings.kind.clone();
                // Only register if at least base_url or model is set
                if base.is_some() || model.is_some() || api_key.is_some() {
                    if let Some(info) = crate::providers::provider_info_for(id, api_key, base, model, kind) {
                        provider_registry.register(info);
                    }
                }
            }
        }

        let permission_guard = PermissionGuard::new(true); // auto-approve for now
        let tool_bridge = Arc::new(
            crate::tools::build_tool_bridge_with_app_data(Some(app_data_dir.clone())).await?,
        );

        Ok(Self {
            session_manager,
            provider_registry: Arc::new(tokio::sync::RwLock::new(provider_registry)),
            permission_guard,
            agent_loops: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            tool_bridge,
            config_manager,
            app_data_dir,
        })
    }
}

/// Initialize the agent loop for a session.
/// `provider_id` is optional — if omitted uses the configured default.
#[command]
pub async fn init_session(
    session_id: String,
    provider_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Resolve provider: explicit → default → first available
    let wanted = if let Some(pid) = provider_id {
        pid
    } else {
        state.config_manager.default_provider().await.unwrap_or_default()
    };
    let provider: Arc<dyn crate::providers::Provider> = {
        let reg = state.provider_registry.read().await;
        if !wanted.is_empty() {
            if let Some(p) = reg.get(&wanted) {
                p
            } else {
                // Fallback to first registered provider or error if none
                let list = reg.list();
                if let Some(first) = list.first() {
                    Arc::clone(&first.provider)
                } else {
                    return Err(
                        "No provider configured — add an API key in Settings (xAI/OpenAI/Anthropic)".to_string(),
                    );
                }
            }
        } else {
            let list = reg.list();
            if let Some(first) = list.first() {
                Arc::clone(&first.provider)
            } else {
                return Err(
                    "No provider configured — add an API key in Settings (xAI/OpenAI/Anthropic)".to_string(),
                );
            }
        }
    };

    // If agent loop already exists for this session, hot-swap the provider without destroying state
    let existing = {
        let loops = state.agent_loops.read().await;
        loops.get(&session_id).cloned()
    };
    if let Some(agent) = existing {
        agent.set_provider(provider).await;
        return Ok(());
    }

    let chat = build_chat_handle(&session_id, Some(state.app_data_dir.clone()))
        .map_err(|e| e.to_string())?;
    let tools = (*state.tool_bridge).clone();
    let agent = Arc::new(AgentLoop::new(chat, tools, provider));
    state.agent_loops.write().await.insert(session_id, agent);
    Ok(())
}

/// Send a user message to the agent.
#[command]
pub async fn send_message(
    app: AppHandle,
    session_id: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let agent = {
        let loops = state.agent_loops.read().await;
        loops.get(&session_id).cloned()
    };
    if let Some(agent) = agent {
        let sink = TauriSink::new(app, session_id.clone());
        agent
            .send_user_message(&text, Arc::new(sink))
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("session not initialized: {session_id}"))
    }
}

/// Get the list of sessions.
#[command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.session_manager.list_sessions())
}

#[command]
pub async fn delete_session(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.agent_loops.write().await.remove(&session_id);
    state
        .session_manager
        .delete_session(&session_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[command]
pub async fn rename_session(
    old_id: String,
    new_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let new_id = new_id.trim().to_string();
    if new_id.is_empty() {
        return Err("Session name cannot be empty".to_string());
    }
    state
        .session_manager
        .rename_session(&old_id, &new_id)
        .map_err(|e| e.to_string())?;
    let mut loops = state.agent_loops.write().await;
    if let Some(agent) = loops.remove(&old_id) {
        loops.insert(new_id, agent);
    }
    Ok(())
}

/// Get the message history for a session.
#[command]
pub async fn get_session_messages(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    use xai_grok_sampling_types::{ContentPart, ConversationItem};

    let persistence = crate::session_store::JsonlChatPersistence::new(
        session_id,
        state.app_data_dir.clone(),
    )
    .map_err(|e| e.to_string())?;

    let items = persistence.history();
    let mut messages = Vec::new();

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    for (i, item) in items.into_iter().enumerate() {
        match item {
            ConversationItem::User(u) => {
                let text = u
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_ref()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                messages.push(serde_json::json!({
                    "id": format!("msg-{i}"),
                    "role": "user",
                    "content": text,
                    "timestamp": now_ts,
                }));
            }
            ConversationItem::Assistant(a) => {
                let tool_calls: Vec<_> = a
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        serde_json::json!({
                            "id": tc.id.to_string(),
                            "name": tc.name,
                            "arguments": tc.arguments.to_string(),
                        })
                    })
                    .collect();
                let mut val = serde_json::json!({
                    "id": format!("msg-{i}"),
                    "role": "assistant",
                    "content": a.content.as_ref(),
                    "timestamp": now_ts,
                });
                if !tool_calls.is_empty() {
                    val["toolCalls"] = serde_json::Value::Array(tool_calls);
                }
                messages.push(val);
            }
            ConversationItem::ToolResult(tr) => {
                messages.push(serde_json::json!({
                    "id": format!("tool-{i}"),
                    "role": "tool",
                    "content": tr.tool_call_id.to_string(),
                    "toolResult": {
                        "toolCallId": tr.tool_call_id.to_string(),
                        "content": tr.content,
                    },
                    "timestamp": now_ts,
                }));
            }
            _ => {}
        }
    }

    Ok(messages)
}

/// Get the list of available providers (serializable DTO).
#[command]
pub async fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderInfoDto>, String> {
    let reg = state.provider_registry.read().await;
    Ok(reg
        .list()
        .iter()
        .map(|p| ProviderInfoDto::from(*p))
        .collect())
}

/// Add an MCP server.
#[command]
pub async fn add_mcp(
    name: String,
    command: String,
    args: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    // Register in config
    let mcp_config = crate::config::McpServerConfig {
        transport: "stdio".to_string(),
        command: Some(command.clone()),
        args: args.clone(),
        url: None,
        enabled: true,
    };
    state
        .config_manager
        .add_mcp_server(name.clone(), mcp_config)
        .await
        .map_err(|e| e.to_string())?;

    // Register tools
    add_mcp_server(&state.tool_bridge, &name, &command, args)
        .await
        .map_err(|e| e.to_string())
}

/// Remove an MCP server.
#[command]
pub async fn remove_mcp(name: String, state: State<'_, AppState>) -> Result<(), String> {
    // Remove from tool bridge
    state.tool_bridge.unregister_tools_by_prefix(&format!("{}__", name));
    // Remove from config
    state
        .config_manager
        .remove_mcp_server(&name)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// List available tools.
#[command]
pub async fn list_tools(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let defs = state.tool_bridge.tool_definitions().await;
    Ok(defs.iter().map(|d| d.function.name.clone()).collect())
}

/// Get current config snapshot.
#[command]
pub async fn get_config(
    state: State<'_, AppState>,
) -> Result<crate::config::ConfigSnapshot, String> {
    Ok(state.config_manager.snapshot().await)
}

/// Set API key for a provider — persists and hot-registers the provider.
#[command]
pub async fn set_api_key(
    provider_id: String,
    api_key: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_api_key(&provider_id, api_key.clone())
        .await
        .map_err(|e| e.to_string())?;
    // Hot-register / replace provider so `list_providers` immediately shows it, using any existing base_url/model/kind overrides
    let (base_url, model, kind) = {
        let cfg = state.config_manager.read().await;
        let s = cfg.provider_settings(&provider_id);
        (
            s.as_ref().and_then(|x| x.base_url.clone()),
            s.as_ref().and_then(|x| x.model.clone()),
            s.as_ref().and_then(|x| x.kind.clone()),
        )
    };
    if let Some(info) = crate::providers::provider_info_for(&provider_id, Some(api_key), base_url, model, kind) {
        let mut reg = state.provider_registry.write().await;
        reg.register(info);
    }
    Ok(())
}

/// Remove API key for a provider — persists and unregisters it.
#[command]
pub async fn remove_api_key(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .remove_api_key(&provider_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut reg = state.provider_registry.write().await;
    reg.remove(&provider_id);
    Ok(())
}

/// Set base URL / model / kind for a provider (works for any OpenAI/Anthropic-compatible endpoint).
/// Examples: `openai` with `base_url: http://localhost:11434/v1` + `model: llama3` for Ollama,
/// `anthropic` with custom base, or custom id `my-ollama` with `kind: openai`/`anthropic`.
/// Calling with all `None`/empty clears the override and restores defaults.
#[command]
pub async fn set_provider_settings(
    provider_id: String,
    base_url: Option<String>,
    model: Option<String>,
    kind: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Normalize empty strings to None
    let base_url = base_url.filter(|s| !s.trim().is_empty());
    let model = model.filter(|s| !s.trim().is_empty());
    let kind = kind.filter(|s| !s.trim().is_empty());
    state
        .config_manager
        .set_provider_settings(&provider_id, base_url.clone(), model.clone(), kind.clone())
        .await
        .map_err(|e| e.to_string())?;
    // Re-register provider if we have anything to register (api_key or override)
    let api_key = state.config_manager.api_key(&provider_id).await;
    let should_register = api_key.is_some()
        || base_url.is_some()
        || model.is_some()
        || kind.is_some()
        || !["xai", "openai", "anthropic"].contains(&provider_id.as_str());
    if should_register {
        if let Some(info) =
            crate::providers::provider_info_for(&provider_id, api_key, base_url.clone(), model.clone(), kind.clone())
        {
            let mut reg = state.provider_registry.write().await;
            reg.register(info);
        }
    } else if !["xai", "openai", "anthropic"].contains(&provider_id.as_str()) {
        // Custom provider cleared completely — remove it
        let mut reg = state.provider_registry.write().await;
        reg.remove(&provider_id);
    }
    Ok(())
}

/// Get base URL / model overrides for a provider.
#[command]
pub async fn get_provider_settings(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Option<crate::config::ProviderSettings>, String> {
    Ok(state.config_manager.provider_settings(&provider_id).await)
}

/// List all provider settings (for UI bulk load).
#[command]
pub async fn list_provider_settings(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, crate::config::ProviderSettings>, String> {
    Ok(state.config_manager.read().await.provider_settings.clone())
}

/// Set default provider.
#[command]
pub async fn set_default_provider(
    provider_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_default_provider(provider_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get default provider.
#[command]
pub async fn get_default_provider(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.config_manager.default_provider().await)
}

/// Add an MCP server with full config.
#[command]
pub async fn add_mcp_server_full(
    name: String,
    transport: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mcp_config = crate::config::McpServerConfig {
        transport,
        command,
        args,
        url,
        enabled: true,
    };
    state
        .config_manager
        .add_mcp_server(name, mcp_config)
        .await
        .map_err(|e| e.to_string())
}

/// Get UI config.
#[command]
pub async fn get_ui_config(
    state: State<'_, AppState>,
) -> Result<crate::config::UiConfig, String> {
    Ok(state.config_manager.ui_config().await)
}

/// Set UI config.
#[command]
pub async fn set_ui_config(
    ui: crate::config::UiConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_ui_config(ui)
        .await
        .map_err(|e| e.to_string())
}

/// List available Kilo Gateway models (proxied via backend to avoid CORS).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KiloModel {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
    pub is_free: Option<bool>,
    pub pricing: Option<serde_json::Value>,
}

#[command]
pub async fn list_kilo_models() -> Result<Vec<KiloModel>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://api.kilo.ai/api/gateway/models")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("gateway returned HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let arr = v.get("data").and_then(|d| d.as_array()).ok_or("missing data array")?;
    let mut out = Vec::new();
    for m in arr {
        let id = m.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if id.is_empty() { continue; }
        let name = m.get("name").and_then(|x| x.as_str()).unwrap_or(&id).to_string();
        let context_length = m.get("context_length").and_then(|x| x.as_u64());
        let is_free = m.get("isFree").and_then(|x| x.as_bool());
        let pricing = m.get("pricing").cloned();
        out.push(KiloModel { id, name, context_length, is_free, pricing });
    }
    Ok(out)
}

/// List discovered skills (local + hub)
#[command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<crate::skills::SkillDto>, String> {
    let ws = crate::tools::resolve_workspace_dir();
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    Ok(skills.iter().map(crate::skills::SkillDto::from).collect())
}

/// Install a skill from raw SKILL.md content
#[command]
pub async fn install_skill(
    name: String,
    content: String,
    scope: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let path = crate::skills::install_skill(&state.app_data_dir, &name, &content, scope)
        .map_err(|e| e.to_string())?;
    refresh_skills_internal(&state).await?;
    Ok(path.to_string_lossy().to_string())
}

/// Remove a skill by name
#[command]
pub async fn remove_skill(name: String, state: State<'_, AppState>) -> Result<(), String> {
    crate::skills::remove_skill(&state.app_data_dir, &name).map_err(|e| e.to_string())?;
    refresh_skills_internal(&state).await?;
    Ok(())
}

/// Fetch a skill from hub (agentskills.io or raw URL)
#[command]
pub async fn fetch_hub_skill(
    hub_url: String,
    owner: String,
    name: String,
    version: Option<String>,
    state: State<'_, AppState>,
) -> Result<crate::skills::SkillDto, String> {
    let dto = crate::skills::fetch_hub_skill(&state.app_data_dir, &hub_url, &owner, &name, version)
        .await
        .map_err(|e| e.to_string())?;
    refresh_skills_internal(&state).await?;
    Ok(dto)
}

#[command]
pub async fn list_hub_skills(state: State<'_, AppState>) -> Result<crate::skills::HubIndex, String> {
    Ok(crate::skills::list_hub_index(&state.app_data_dir))
}

#[command]
pub async fn refresh_skills(state: State<'_, AppState>) -> Result<Vec<crate::skills::SkillDto>, String> {
    refresh_skills_internal(&state).await?;
    let ws = crate::tools::resolve_workspace_dir();
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    Ok(skills.iter().map(crate::skills::SkillDto::from).collect())
}

async fn refresh_skills_internal(state: &State<'_, AppState>) -> Result<(), String> {
    let ws = crate::tools::resolve_workspace_dir();
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    // Update ToolBridge baseline
    let infos: Vec<xai_grok_tools::implementations::skills::types::SkillInfo> = skills;
    state.tool_bridge.update_skill_baseline(infos).await;
    // Also update AppState's tool_bridge available skills for next sessions
    // Apply pending to refresh AvailableSkills
    let _ = state.tool_bridge.apply_pending_skill_update().await;
    Ok(())
}

/// Search skills by query (name/description filter, case-insensitive)
#[command]
pub async fn search_skills(query: String, state: State<'_, AppState>) -> Result<Vec<crate::skills::SkillDto>, String> {
    let ws = crate::tools::resolve_workspace_dir();
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    let q = query.to_lowercase();
    let filtered: Vec<_> = skills
        .iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&q)
                || s.description.to_lowercase().contains(&q)
                || s.when_to_use.as_deref().unwrap_or("").to_lowercase().contains(&q)
        })
        .map(crate::skills::SkillDto::from)
        .collect();
    Ok(filtered)
}

/// Get raw SKILL.md content by skill name
#[command]
pub async fn get_skill_content(name: String, state: State<'_, AppState>) -> Result<String, String> {
    let ws = crate::tools::resolve_workspace_dir();
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    let skill = skills
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Skill '{name}' not found"))?;
    std::fs::read_to_string(&skill.path).map_err(|e| e.to_string())
}
