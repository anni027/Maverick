//! Tauri commands — the bridge between the React UI and the Rust backend.
//!
//! IPC CONTRACT (load-bearing): the frontend speaks snake_case argument keys
//! (`session_id`, `provider_id`, …). Tauri's `#[command]` macro converts
//! parameter names to lowerCamelCase on the wire **by default**, so every
//! command below MUST carry `rename_all = "snake_case"` — without it the UI's
//! `session_id` arrives as a missing `sessionId` and the invoke is rejected
//! before any handler runs. Do not add a bare `#[command]` here.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{AppHandle, State, command};

use crate::{
    agent_event::TauriSink,
    agent_loop::AgentLoop,
    config::{ConfigManager, EmailSettingsPatch, EmailStatus},
    email,
    providers::{ProviderInfoDto, ProviderRegistry},
    session_store::SessionManager,
    tools::{PermissionGuard, build_chat_handle},
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
            for key in ["openai", "anthropic"] {
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
                if ["openai", "anthropic"].contains(&id.as_str()) {
                    continue;
                }
                let api_key = cfg.api_key(id);
                let base = settings.base_url.clone();
                let model = settings.model.clone();
                let kind = settings.kind.clone();
                // Only register if at least base_url or model is set
                if base.is_some() || model.is_some() || api_key.is_some() {
                    if let Some(info) =
                        crate::providers::provider_info_for(id, api_key, base, model, kind)
                    {
                        provider_registry.register(info);
                    }
                }
            }
        }

        // Consulted (and audited) at every tool dispatch, but no allow/deny lists
    // are configurable yet, so nothing is actually blocked. The log path is
    // fixed once here so the audit never re-resolves the workspace dir
    // (env read + create_dir_all) on the hot path.
    let permission_guard =
        PermissionGuard::new(true).with_log_path(app_data_dir.join("tool-decisions.log"));
        let tool_bridge = Arc::new(
            crate::tools::build_tool_bridge_with_app_data(Some(app_data_dir.clone())).await?,
        );

        // Auto-restore MCP servers from config in background (real handshake, non-blocking)
        {
            let bridge_clone = tool_bridge.clone();
            let cfg_clone = config_manager.clone();
            tokio::spawn(async move {
                let snapshot = cfg_clone.read().await.mcp_servers.clone();
                for (name, srv) in snapshot {
                    if !srv.enabled {
                        continue;
                    }
                    match srv.transport.as_str() {
                        "stdio" => {
                            if let Some(cmd) = srv.command {
                                let tb = bridge_clone.clone();
                                let n = name.clone();
                                let a = srv.args.clone();
                                let label = name.clone();
                                let joined =
                                    crate::mcp::on_mcp_thread(move || {
                                        Box::pin(async move {
                                            crate::mcp::add_mcp_server_real(&tb, &n, &cmd, a).await
                                        })
                                    })
                                    .await;
                                log_restore_result(&label, joined);
                            }
                        }
                        "http" => {
                            if let Some(url) = srv.url {
                                let tb = bridge_clone.clone();
                                let n = name.clone();
                                let label = name.clone();
                                let joined =
                                    crate::mcp::on_mcp_thread(move || {
                                        Box::pin(async move {
                                            crate::mcp::add_mcp_server_http_real(&tb, &n, &url)
                                                .await
                                        })
                                    })
                                    .await;
                                log_restore_result(&label, joined);
                            }
                        }
                        _ => {}
                    }
                }
            });
        }

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

/// Hot-swap the provider for every active session that uses `provider_id`.
///
/// Saving a new model / base URL / API key in Settings only re-registered the
/// provider in the registry; the live `AgentLoop`s kept holding the old
/// provider object, so the next turn still sampled with the old model. This
/// re-resolves the provider from the registry and replaces it on every
/// matching session, keeping conversation history intact.
async fn hot_swap_provider(state: &AppState, provider_id: &str) {
    let provider = {
        let reg = state.provider_registry.read().await;
        match reg.get(provider_id) {
            Some(p) => p,
            None => return,
        }
    };

    let loops = state.agent_loops.read().await;
    for agent in loops.values() {
        if agent.provider().await.id() == provider_id {
            agent.set_provider(provider.clone()).await;
            agent.sync_context_window().await;
        }
    }
}

/// Record the outcome of a background MCP restore.
///
/// The restore is fire-and-forget on purpose — `AppState::new` must not block
/// on a server that can take 30s to handshake — but fire-and-forget should
/// still mean *recorded*. Without this, a handshake failure shows up in the UI
/// as `Error` status with nothing in the logs explaining why.
fn log_restore_result(
    name: &str,
    joined: Result<Result<Vec<String>, anyhow::Error>, String>,
) {
    match joined {
        Ok(Ok(tools)) => tracing::info!(server = name, tools = tools.len(), "MCP server restored"),
        Ok(Err(e)) => tracing::warn!(server = name, error = %e, "MCP restore handshake failed"),
        Err(e) => tracing::warn!(server = name, error = %e, "MCP restore task failed"),
    }
}

/// Point every session that still uses `provider_id` somewhere else after that
/// id was unregistered from the registry.
///
/// `hot_swap_provider` deliberately returns early when the id is gone, which is
/// right for a *settings* change but wrong for key removal: the live loops keep
/// holding a `Provider` built with the revoked key, so the next turn would keep
/// sending it. Sessions are re-resolved with the same fallback `init_session`
/// uses; when no provider is left to fall back to, the loop is dropped so the
/// credential cannot be used again. History is flushed per message, and the
/// next `init_session` rebuilds the handle from disk.
async fn reconcile_loops_after_unregister(state: &AppState, provider_id: &str) {
    let affected: Vec<String> = {
        let loops = state.agent_loops.read().await;
        let mut sessions = Vec::new();
        for (session, agent) in loops.iter() {
            if agent.provider().await.id() == provider_id {
                sessions.push(session.clone());
            }
        }
        sessions
    };
    if affected.is_empty() {
        return;
    }

    let replacement = {
        let reg = state.provider_registry.read().await;
        reg.resolve(provider_id)
    };

    match replacement {
        Ok(provider) => {
            for session in affected {
                let agent = {
                    let loops = state.agent_loops.read().await;
                    loops.get(&session).cloned()
                };
                if let Some(agent) = agent {
                    agent.set_provider(provider.clone()).await;
                    agent.sync_context_window().await;
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                provider = provider_id,
                error = %e,
                "no provider left to fall back to; dropping affected session loops"
            );
            let mut loops = state.agent_loops.write().await;
            for session in affected {
                loops.remove(&session);
            }
        }
    }
}

/// Workspace dir override for session-scoped commands: the session mapping
/// (or stored default) when set, else the global resolver. Never trusts raw
/// paths — only the validated mapping store.
pub(crate) fn session_workspace_dir(
    state: &AppState,
    session_id: Option<&str>,
) -> Option<std::path::PathBuf> {
    let store = crate::workspaces::WorkspaceStore::new(&state.app_data_dir);
    match session_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => Some(
            store
                .effective_session_dir(id)
                .unwrap_or_else(|| store.global_dir()),
        ),
        None => None,
    }
}

/// Global workspace dir honoring the stored default (env → stored → `./workspace`).
pub(crate) fn global_workspace_dir(state: &AppState) -> std::path::PathBuf {
    crate::workspaces::WorkspaceStore::new(&state.app_data_dir).global_dir()
}

/// Why a session has no live loop, for the errors users actually see.
async fn missing_loop_error(state: &AppState, session_id: &str) -> String {
    if state.provider_registry.read().await.list().is_empty() {
        "No provider configured - add an API key in Settings (OpenAI/Anthropic)".to_string()
    } else {
        format!("session not initialized: {session_id}")
    }
}

/// Initialize the agent loop for a session.
/// `provider_id` is optional — if omitted uses the configured default.
#[command(rename_all = "snake_case")]
pub async fn init_session(
    session_id: String,
    provider_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Resolve provider: explicit → default → first available
    let wanted = if let Some(pid) = provider_id.clone() {
        pid
    } else {
        state
            .config_manager
            .default_provider()
            .await
            .unwrap_or_default()
    };
    let provider: Arc<dyn crate::providers::Provider> = {
        let reg = state.provider_registry.read().await;
        reg.resolve(&wanted)?
    };

    // If agent loop already exists for this session, hot-swap the provider without destroying state
    let existing = {
        let loops = state.agent_loops.read().await;
        loops.get(&session_id).cloned()
    };
    if let Some(agent) = existing {
        agent.set_provider(provider).await;
        agent.set_provider_id(&wanted);
        agent.sync_context_window().await;
        return Ok(());
    }

    let chat = build_chat_handle(&session_id, Some(state.app_data_dir.clone()))
        .map_err(|e| e.to_string())?;
    // Warm the session-manager cache so `get_session_messages` peeks hit
    // instead of opening a new handle per read.
    let _ = state.session_manager.get_or_create(&session_id);
    // Align the session window with the active provider (§5.6-E) so
    // compaction thresholds track the real model, not the 128k default.
    crate::tools::sync_chat_context_window(&chat, provider.context_window()).await;
    let tools = (*state.tool_bridge).clone();
    let config_manager: Arc<ConfigManager> = Arc::clone(&state.config_manager);
    let agent = Arc::new(
        AgentLoop::new(chat, tools, provider)
            .with_config_manager(config_manager)
            .with_memory_store(crate::memory::MemoryStore::new(
                state.app_data_dir.clone(),
            ))
            .with_workspace_store(crate::workspaces::WorkspaceStore::new(
                &state.app_data_dir,
            ))
            .with_permission_guard(state.permission_guard.clone()),
    );
    agent.set_provider_id(&wanted);
    // Session workspace mapping (falls back to the global default).
    let ws_store = crate::workspaces::WorkspaceStore::new(&state.app_data_dir);
    agent
        .set_session_workspace(ws_store.effective_session_dir(&session_id))
        .await;
    state.agent_loops.write().await.insert(session_id, agent);
    Ok(())
}

/// Align a session's system head with the generative-UI (OpenUI) component
/// language. `instructions` is the `library.prompt()` block generated by the
/// frontend (it owns the component definitions, so prompt and renderer can
/// never drift apart); `None` strips the block back to the base addendum
/// (feature toggled off). Rebuilt from the known const every time, so the
/// call is idempotent and needs no read-modify-write.
#[command(rename_all = "snake_case")]
pub async fn set_generative_ui_prompt(
    session_id: String,
    instructions: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    crate::session_store::validate_session_id(&session_id).map_err(|e| e.to_string())?;
    let agent = {
        let loops = state.agent_loops.read().await;
        loops.get(&session_id).cloned()
    };
    let Some(agent) = agent else {
        return Err(missing_loop_error(&state, &session_id).await);
    };
    let mut head = crate::prompts::AUTOMATION_SYSTEM_ADDENDUM.to_string();
    if let Some(extra) = instructions.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        head.push_str("\n\n");
        head.push_str(extra);
    }
    agent
        .chat_handle()
        .replace_system_head(&head)
        .await
        .ok_or_else(|| format!("session actor unavailable: {session_id}"))
}

/// Gmail connection state for Settings (secrets never cross the boundary).
#[command(rename_all = "snake_case")]
pub async fn get_email_config(state: State<'_, AppState>) -> Result<EmailStatus, String> {
    Ok(state.config_manager.email_status().await)
}

/// Update Gmail settings. `None` fields keep their stored values, so the UI
/// can flip the switch without re-sending (or ever seeing) secrets.
#[command(rename_all = "snake_case")]
pub async fn set_email_config(
    patch: EmailSettingsPatch,
    state: State<'_, AppState>,
) -> Result<EmailStatus, String> {
    let mut email = state.config_manager.email_config().await;
    if let Some(enabled) = patch.enabled {
        email.enabled = enabled;
    }
    if let Some(client_id) = patch.client_id {
        email.client_id = client_id.trim().to_string();
    }
    if let Some(client_secret) = patch.client_secret {
        email.client_secret = client_secret.trim().to_string();
    }
    state
        .config_manager
        .set_email_config(email)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.config_manager.email_status().await)
}

/// Run the Gmail OAuth connect flow: opens the browser, waits for consent
/// (up to 3 minutes), exchanges the code, and persists the tokens. Returns
/// the connected account address.
#[command(rename_all = "snake_case")]
pub async fn email_connect(state: State<'_, AppState>) -> Result<String, String> {
    let cfg = state.config_manager.email_config().await;
    if cfg.client_id.trim().is_empty() || cfg.client_secret.trim().is_empty() {
        return Err("paste your Google OAuth client id and secret first (see the setup link in Settings → Email)".to_string());
    }
    let (refresh_token, access_token, expires_at, account) =
        email::run_connect_flow(&cfg.client_id, &cfg.client_secret).await?;
    let mut updated = cfg;
    updated.refresh_token = refresh_token;
    updated.access_token = access_token;
    updated.expires_at = expires_at;
    updated.account_email = account.clone();
    state
        .config_manager
        .set_email_config(updated)
        .await
        .map_err(|e| format!("connected, but could not persist the tokens: {e}"))?;
    Ok(account)
}

/// Disconnect Gmail: drops tokens + account, keeps the client credentials so
/// reconnecting is one click.
#[command(rename_all = "snake_case")]
pub async fn email_disconnect(state: State<'_, AppState>) -> Result<EmailStatus, String> {
    let mut email = state.config_manager.email_config().await;
    email.refresh_token.clear();
    email.access_token.clear();
    email.expires_at = 0;
    email.account_email.clear();
    state
        .config_manager
        .set_email_config(email)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.config_manager.email_status().await)
}

fn check_email_recipient(to: &str) -> Result<String, String> {
    let to = to.trim().to_string();
    if to.is_empty() || !to.contains('@') {
        return Err("a valid recipient address is required".to_string());
    }
    Ok(to)
}

/// Programmatic send (Settings test button, draft-send actions). Returns the
/// Gmail message id. The agent's `send_email` tool shares this path.
pub async fn email_send_inner(
    manager: &ConfigManager,
    to: &str,
    subject: &str,
    body_text: &str,
    body_html: Option<&str>,
    cc: &[String],
    bcc: &[String],
) -> Result<String, String> {
    let to = check_email_recipient(to)?;
    if body_text.trim().is_empty() && body_html.map(str::trim).unwrap_or_default().is_empty() {
        return Err("an email needs a text or HTML body".to_string());
    }
    let (access_token, account) = email::valid_access_token(manager).await?;
    let mime = email::build_mime(&account, &to, cc, bcc, subject, body_text, body_html);
    email::send_raw(&access_token, &email::base64url(mime.as_bytes())).await
}

#[command(rename_all = "snake_case")]
pub async fn email_send(
    to: String,
    subject: String,
    body_text: String,
    body_html: Option<String>,
    cc: Option<Vec<String>>,
    bcc: Option<Vec<String>>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    email_send_inner(
        &state.config_manager,
        &to,
        &subject,
        &body_text,
        body_html.as_deref(),
        &cc.unwrap_or_default(),
        &bcc.unwrap_or_default(),
    )
    .await
}

/// Send a user message to the agent. `effort` is an optional reasoning-effort
/// tier picked in the composer (`null` = keep the provider default).
#[command(rename_all = "snake_case")]
pub async fn send_message(
    app: AppHandle,
    session_id: String,
    text: String,
    effort: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let agent = {
        let loops = state.agent_loops.read().await;
        loops.get(&session_id).cloned()
    };
    if let Some(agent) = agent {
        // Catalog-gate the picked effort: a model that rejects
        // `reasoning_effort` (or lacks the tier) must not receive one even
        // when the provider kind accepts the field.
        let effective = match effort
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
        {
            Some(raw) => {
                let provider = agent.provider().await;
                let profile =
                    reasoning_profile(&state, provider.id(), Some(provider.model())).await;
                let wanted = normalize_effort(raw);
                if profile.supported
                    && (profile.efforts.is_empty()
                        || profile.efforts.iter().any(|tier| *tier == wanted))
                {
                    Some(raw.to_string())
                } else {
                    None
                }
            }
            None => None,
        };
        // Applied before the send so the ordered command queue sees the new
        // sampling config first; unsupported providers clear instead.
        // Degrade to no-effort on a bad tier: a stale preset must never drop
        // the user's message.
        if agent.set_reasoning_effort(effective.as_deref()).await.is_err() {
            agent.set_reasoning_effort(None).await.ok();
        }
        let sink = TauriSink::new(app, session_id.clone());
        agent
            .send_user_message_auto(&text, Arc::new(sink))
            .await
            .map_err(|e| e.to_string())?;
        // Post-run memory extraction: fire-and-forget, never blocks the
        // reply. Throttled inside the loop (cooldown + fresh-content gate).
        {
            let agent = Arc::clone(&agent);
            tokio::spawn(async move {
                agent.maybe_extract_memories().await;
            });
        }
        Ok(())
    } else {
        Err(missing_loop_error(&state, &session_id).await)
    }
}

/// Cancel the in-flight run for a session (§5.6-C). Idempotent: with no
/// active run it is a no-op. The loop emits `Cancelled` and `send_message`
/// resolves with a "run cancelled by user" error.
#[command(rename_all = "snake_case")]
pub async fn cancel_message(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let agent = {
        let loops = state.agent_loops.read().await;
        loops.get(&session_id).cloned()
    };
    if let Some(agent) = agent {
        agent.cancel_current_run();
        Ok(())
    } else {
        Err(format!("session not initialized: {session_id}"))
    }
}

/// Get the list of sessions.
#[command(rename_all = "snake_case")]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state.session_manager.list_sessions())
}

#[command(rename_all = "snake_case")]
pub async fn delete_session(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(agent) = state.agent_loops.write().await.remove(&session_id) {
        agent.cancel_current_run();
    }
    state
        .session_manager
        .delete_session(&session_id)
        .map_err(|e| e.to_string())?;
    crate::workspaces::WorkspaceStore::new(&state.app_data_dir)
        .remove_session(&session_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[command(rename_all = "snake_case")]
pub async fn rename_session(
    old_id: String,
    new_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let new_id = new_id.trim().to_string();
    if new_id.is_empty() {
        return Err("Session name cannot be empty".to_string());
    }
    // The chat handle (and its open history file) is bound to `old_id`'s
    // directory. Drop the loop first: a live handle can block the directory
    // rename on Windows and would otherwise keep writing to the old path.
    let removed = state.agent_loops.write().await.remove(&old_id);
    let (old_provider, old_provider_id) = match &removed {
        Some(agent) => (Some(agent.provider().await), agent.provider_id_label()),
        None => (None, String::new()),
    };
    drop(removed);
    // Let the chat-state actor observe the closed command channel and release
    // its file handle before the directory moves. Retry on Windows sharing
    // violations instead of failing on the first attempt.
    let mut rename_err = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(25 * attempt as u64)).await;
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        match state.session_manager.rename_session(&old_id, &new_id) {
            Ok(()) => {
                rename_err.clear();
                break;
            }
            Err(e) => rename_err = e.to_string(),
        }
    }
    if !rename_err.is_empty() {
        return Err(rename_err);
    }

    if let Some(provider) = old_provider {
        // Rebuild over the renamed directory — history is re-read from disk,
        // so the conversation carries over while writes land in the new path.
        // Mirror `init_session` exactly: a partial rebuild used to drop the
        // memory store, workspace store, provider id, and session workspace.
        let chat = build_chat_handle(&new_id, Some(state.app_data_dir.clone()))
            .map_err(|e| e.to_string())?;
        let _ = state.session_manager.get_or_create(&new_id);
        crate::tools::sync_chat_context_window(&chat, provider.context_window()).await;
        let tools = (*state.tool_bridge).clone();
        let config_manager: Arc<ConfigManager> = Arc::clone(&state.config_manager);
        let agent = Arc::new(
            AgentLoop::new(chat, tools, provider)
                .with_config_manager(config_manager)
                .with_memory_store(crate::memory::MemoryStore::new(
                    state.app_data_dir.clone(),
                ))
                .with_workspace_store(crate::workspaces::WorkspaceStore::new(
                    &state.app_data_dir,
                ))
                .with_permission_guard(state.permission_guard.clone()),
        );
        if !old_provider_id.is_empty() {
            agent.set_provider_id(&old_provider_id);
        }
        let ws_store = crate::workspaces::WorkspaceStore::new(&state.app_data_dir);
        agent
            .set_session_workspace(ws_store.effective_session_dir(&new_id))
            .await;
        state.agent_loops.write().await.insert(new_id, agent);
    }
    Ok(())
}

/// Get the message history for a session.
#[command(rename_all = "snake_case")]
pub async fn get_session_messages(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    use xai_grok_sampling_types::{ContentPart, ConversationItem};

    crate::session_store::validate_session_id(&session_id).map_err(|e| e.to_string())?;
    // A read must never create the session directory: probing an unknown id
    // used to leave a phantom empty session behind in the sidebar.
    if !state
        .app_data_dir
        .join("sessions")
        .join(&session_id)
        .exists()
    {
        return Ok(Vec::new());
    }

    // Prefer the handle a live session already owns: constructing a second
    // one opens another append handle on the same history file.
    let persistence = match state.session_manager.peek(&session_id) {
        Some(p) => p,
        None => state
            .session_manager
            .get_or_create(&session_id)
            .map_err(|e| e.to_string())?,
    };

    let items = persistence.history_with_timestamps();
    let mut messages = Vec::new();

    let mut tool_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for (i, (ts, item)) in items.into_iter().enumerate() {
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
                    "timestamp": ts,
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
                for tc in a.tool_calls.iter() {
                    tool_names.insert(tc.id.to_string(), tc.name.clone());
                }
                let mut val = serde_json::json!({
                    "id": format!("msg-{i}"),
                    "role": "assistant",
                    "content": a.content.as_ref(),
                    "timestamp": ts,
                });
                if !tool_calls.is_empty() {
                    val["toolCalls"] = serde_json::Value::Array(tool_calls);
                }
                messages.push(val);
            }
            ConversationItem::ToolResult(tr) => {
                // Label the fold with the tool's name (what a live run shows),
                // not the opaque call id.
                let label = tool_names
                    .get(&tr.tool_call_id)
                    .cloned()
                    .unwrap_or_else(|| tr.tool_call_id.to_string());
                messages.push(serde_json::json!({
                    "id": format!("tool-{i}"),
                    "role": "tool",
                    "content": label,
                    "toolResult": {
                        "toolCallId": tr.tool_call_id.to_string(),
                        "content": tr.content,
                    },
                    "timestamp": ts,
                }));
            }
            _ => {}
        }
    }

    Ok(messages)
}

/// Get the list of available providers (serializable DTO).
#[command(rename_all = "snake_case")]
pub async fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderInfoDto>, String> {
    let reg = state.provider_registry.read().await;
    Ok(reg
        .list()
        .iter()
        .map(|p| ProviderInfoDto::from(*p))
        .collect())
}

/// Add an MCP server — tries real handshake, falls back to placeholder.
#[command(rename_all = "snake_case")]
pub async fn add_mcp(
    name: String,
    command: String,
    args: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    // Register in config first (so it persists even if handshake fails)
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

    // The handshake future is `!Send` (contains `*mut`), so it runs on the
    // dedicated persistent MCP thread — the transport's service loop is
    // spawned there and must outlive registration.
    let tool_bridge = state.tool_bridge.clone();
    let name_c = name.clone();
    let command_c = command.clone();
    let args_c = args.clone();
    crate::mcp::on_mcp_thread(move || {
        Box::pin(async move {
            crate::mcp::add_mcp_server_real(&tool_bridge, &name_c, &command_c, args_c).await
        })
    })
    .await?
    .map_err(|e| e.to_string())
}

/// Remove an MCP server.
#[command(rename_all = "snake_case")]
pub async fn remove_mcp(name: String, state: State<'_, AppState>) -> Result<(), String> {
    // Remove from tool bridge
    state
        .tool_bridge
        .unregister_tools_by_prefix(&format!("{}__", name));
    // Remove from config
    state
        .config_manager
        .remove_mcp_server(&name)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// List available tools.
#[command(rename_all = "snake_case")]
pub async fn list_tools(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let defs = state.tool_bridge.tool_definitions().await;
    Ok(defs.iter().map(|d| d.function.name.clone()).collect())
}

/// Get current config snapshot.
#[command(rename_all = "snake_case")]
pub async fn get_config(
    state: State<'_, AppState>,
) -> Result<crate::config::ConfigSnapshot, String> {
    Ok(state.config_manager.snapshot().await)
}

/// Set API key for a provider — persists and hot-registers the provider.
#[command(rename_all = "snake_case")]
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
    if let Some(info) =
        crate::providers::provider_info_for(&provider_id, Some(api_key), base_url, model, kind)
    {
        let mut reg = state.provider_registry.write().await;
        reg.register(info);
    }
    hot_swap_provider(&state, &provider_id).await;
    Ok(())
}

/// Remove API key for a provider — persists and unregisters it.
#[command(rename_all = "snake_case")]
pub async fn remove_api_key(provider_id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .config_manager
        .remove_api_key(&provider_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut reg = state.provider_registry.write().await;
    reg.remove(&provider_id);
    drop(reg);
    reconcile_loops_after_unregister(&state, &provider_id).await;
    Ok(())
}

/// Set base URL / model / kind for a provider (works for any OpenAI/Anthropic-compatible endpoint).
/// Examples: `openai` with `base_url: http://localhost:11434/v1` + `model: llama3` for Ollama,
/// `anthropic` with custom base, or custom id `my-ollama` with `kind: openai`/`anthropic`.
/// Calling with all `None`/empty clears the override and restores defaults.
#[command(rename_all = "snake_case")]
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
    // Re-register the provider when there is something to register it with
    // (an API key or an override). Without either, a non-builtin id must be
    // *removed* — otherwise saving settings for a provider the user just
    // cleared leaves a ghost entry in the picker that always fails at request
    // time. (The old condition registered every non-builtin id, which also
    // made the removal branch below unreachable.)
    let api_key = state.config_manager.api_key(&provider_id).await;
    let has_override = base_url.is_some() || model.is_some() || kind.is_some();
    let is_builtin = ["openai", "anthropic"].contains(&provider_id.as_str());
    let unregistered = if api_key.is_some() || has_override {
        if let Some(info) = crate::providers::provider_info_for(
            &provider_id,
            api_key,
            base_url.clone(),
            model.clone(),
            kind.clone(),
        ) {
            let mut reg = state.provider_registry.write().await;
            reg.register(info);
        }
        false
    } else if !is_builtin {
        // Custom provider cleared completely — remove it
        let mut reg = state.provider_registry.write().await;
        reg.remove(&provider_id);
        true
    } else {
        false
    };
    if unregistered {
        // Same stale-object problem as key removal: live loops still point at
        // the backend this override used to build, and `hot_swap_provider`
        // returns early once the id is gone.
        reconcile_loops_after_unregister(&state, &provider_id).await;
    } else {
        hot_swap_provider(&state, &provider_id).await;
    }
    Ok(())
}

/// Get base URL / model overrides for a provider.
#[command(rename_all = "snake_case")]
pub async fn get_provider_settings(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Option<crate::config::ProviderSettings>, String> {
    Ok(state.config_manager.provider_settings(&provider_id).await)
}

/// Set default provider.
#[command(rename_all = "snake_case")]
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
#[command(rename_all = "snake_case")]
pub async fn get_default_provider(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(state.config_manager.default_provider().await)
}

/// Trim/validate a replace-all preset list from the UI.
/// Add/edit/delete all funnel through `save_model_presets`.
fn validate_presets(
    presets: Vec<crate::config::ModelPreset>,
) -> Result<Vec<crate::config::ModelPreset>, String> {
    const MAX_PRESETS: usize = 50;
    if presets.len() > MAX_PRESETS {
        return Err(format!("Too many presets (max {MAX_PRESETS})"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(presets.len());
    for mut p in presets {
        p.id = p.id.trim().to_string();
        p.name = p.name.trim().to_string();
        p.provider_id = p.provider_id.trim().to_string();
        p.model = p.model.trim().to_string();
        if p.id.is_empty() {
            return Err("Preset id cannot be empty".to_string());
        }
        if p.name.is_empty() {
            return Err(format!("Preset id '{}' needs a name", p.id));
        }
        if p.provider_id.is_empty() {
            return Err(format!("Preset '{}' needs a provider", p.name));
        }
        if p.model.is_empty() {
            return Err(format!("Preset '{}' needs a model", p.name));
        }
        if !seen.insert(p.id.clone()) {
            return Err(format!("Duplicate preset id '{}'", p.id));
        }
        if let Some(effort) = p.effort.as_mut() {
            *effort = effort.trim().to_string();
            if effort.is_empty() {
                p.effort = None;
            }
        }
        out.push(p);
    }
    Ok(out)
}

/// List saved model presets.
#[command(rename_all = "snake_case")]
pub async fn list_model_presets(
    state: State<'_, AppState>,
) -> Result<Vec<crate::config::ModelPreset>, String> {
    Ok(state.config_manager.model_presets().await)
}

/// Replace the saved model preset list (add, edit, delete all go through this).
#[command(rename_all = "snake_case")]
pub async fn save_model_presets(
    presets: Vec<crate::config::ModelPreset>,
    state: State<'_, AppState>,
) -> Result<Vec<crate::config::ModelPreset>, String> {
    let presets = validate_presets(presets)?;
    state
        .config_manager
        .set_model_presets(presets.clone())
        .await
        .map_err(|e| e.to_string())?;
    Ok(presets)
}

/// Add an MCP server with full config — persists + real handshake.
#[command(rename_all = "snake_case")]
pub async fn add_mcp_server_full(
    name: String,
    transport: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mcp_config = crate::config::McpServerConfig {
        transport: transport.clone(),
        command: command.clone(),
        args: args.clone(),
        url: url.clone(),
        enabled: true,
    };
    state
        .config_manager
        .add_mcp_server(name.clone(), mcp_config)
        .await
        .map_err(|e| e.to_string())?;
    register_mcp_handshake(&state, name, transport, command, args, url).await
}

/// Run the real MCP `tools/list` handshake for a server already written to
/// config, on the dedicated persistent MCP thread (`!Send`). Shared by the
/// generic add command and the Computer panel so both get identical behavior.
async fn register_mcp_handshake(
    state: &AppState,
    name: String,
    transport: String,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
) -> Result<(), String> {
    let tool_bridge = state.tool_bridge.clone();
    crate::mcp::on_mcp_thread(move || {
        Box::pin(async move {
            match transport.as_str() {
                "stdio" => {
                    let cmd = command
                        .ok_or_else(|| anyhow::anyhow!("stdio MCP server '{name}' has no command"))?;
                    crate::mcp::add_mcp_server_real(&tool_bridge, &name, &cmd, args).await?;
                }
                "http" => {
                    let u = url.ok_or_else(|| anyhow::anyhow!("http MCP server '{name}' has no url"))?;
                    crate::mcp::add_mcp_server_http_real(&tool_bridge, &name, &u).await?;
                }
                other => anyhow::bail!("unsupported MCP transport '{other}'"),
            }
            Ok::<(), anyhow::Error>(())
        })
    })
    .await?
    .map_err(|e| e.to_string())
}

/* ------------------------------------------------------------------ */
/* Computer: the agent's browser (Playwright MCP, headed so you watch) */
/* ------------------------------------------------------------------ */

/// Config name of the computer's browser server. Deliberately a plain MCP
/// entry so the generic MCP tab, status list and remove path all keep working
/// without special-casing.
const COMPUTER_SERVER_NAME: &str = "browser";

/// Default flags for the browser MCP server: a real installed Chrome (not
/// bundled Chromium — same fingerprint class as a normal browser), vision so
/// screenshots come back to the model, and a persistent profile so logins
/// survive stop/start. `--headless` is opt-in; headed is the default, which
/// means the user literally watches the agent work.
fn computer_profile_dir(app_data_dir: &std::path::Path) -> std::path::PathBuf {
    app_data_dir.join("browser-profile")
}

fn computer_args(app_data_dir: &std::path::Path, headless: bool, persistent: bool) -> Vec<String> {
    let mut args = vec![
        "-y".to_string(),
        "@playwright/mcp@latest".to_string(),
        "--browser".to_string(),
        "chrome".to_string(),
        "--caps".to_string(),
        "vision".to_string(),
    ];
    if persistent {
        args.push("--user-data-dir".to_string());
        args.push(computer_profile_dir(app_data_dir).to_string_lossy().into_owned());
    }
    if headless {
        args.push("--headless".to_string());
    }
    args
}

/// Read the persisted options back out of the stored args, so the panel
/// survives an app restart without a second config section.
fn computer_options_from_args(args: &[String]) -> (bool, bool) {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    (has("--headless"), has("--user-data-dir"))
}

/// Best-effort check for an installed Chrome/Edge. Playwright resolves the
/// channel itself, so a miss here is a warning, never a hard failure.
fn installed_browser_present() -> bool {
    [
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
        "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}

/// Panel state: enabled?, options, profile dir, and the live MCP status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComputerStatus {
    pub enabled: bool,
    pub headless: bool,
    pub persistent: bool,
    pub profile_dir: String,
    pub tool_count: usize,
    /// `Ready` | `Placeholder` | `Error` | `Disabled`.
    pub status: String,
    pub tools: Vec<String>,
    /// No installed browser on a known path — surfaced as a warning.
    pub browser_found: bool,
}

#[command(rename_all = "snake_case")]
pub async fn computer_status(state: State<'_, AppState>) -> Result<ComputerStatus, String> {
    let cfg = state.config_manager.read().await;
    let server = cfg.mcp_servers.get(COMPUTER_SERVER_NAME).cloned();
    let enabled = server.as_ref().is_some_and(|s| s.enabled);
    let (headless, persistent) = server
        .as_ref()
        .map(|s| computer_options_from_args(&s.args))
        .unwrap_or((false, true));
    drop(cfg);

    // One status fetch — derives both the tool count and the tool list.
    let live = if enabled {
        crate::mcp::get_mcp_status(&state.tool_bridge, &state.config_manager)
            .await
            .into_iter()
            .find(|s| s.name == COMPUTER_SERVER_NAME)
    } else {
        None
    };
    let tool_count = live.as_ref().map(|s| s.tool_count).unwrap_or(0);

    // Same status vocabulary the MCP tab uses, plus an explicit Disabled state.
    let status = if !enabled {
        "Disabled".to_string()
    } else if tool_count == 0 {
        "Error".to_string()
    } else {
        "Ready".to_string()
    };
    let tools = live.map(|s| s.tools).unwrap_or_default();

    Ok(ComputerStatus {
        enabled,
        headless,
        persistent,
        profile_dir: computer_profile_dir(&state.app_data_dir)
            .to_string_lossy()
            .into_owned(),
        tool_count,
        status,
        tools,
        browser_found: installed_browser_present(),
    })
}

/// Connect (or reconnect) the computer's browser: write config, then run the
/// real handshake so the tools reach the model on this very turn.
#[command(rename_all = "snake_case")]
pub async fn computer_enable(
    headless: bool,
    persistent: bool,
    state: State<'_, AppState>,
) -> Result<ComputerStatus, String> {
    if !installed_browser_present() {
        // Don't block — Playwright may still find the channel — but say so
        // instead of letting the model hit an opaque launch error later.
        tracing::warn!("no Chrome/Edge on a known path; Playwright channel 'chrome' may fail to launch");
    }
    let args = computer_args(&state.app_data_dir, headless, persistent);
    let already = state
        .config_manager
        .read()
        .await
        .mcp_servers
        .contains_key(COMPUTER_SERVER_NAME);
    if already {
        // Reconnect: unregister first so the old process's tools don't linger
        // alongside the new ones under the same prefix.
        state
            .tool_bridge
            .unregister_tools_by_prefix(&format!("{}__", COMPUTER_SERVER_NAME));
    }
    state
        .config_manager
        .add_mcp_server(
            COMPUTER_SERVER_NAME.to_string(),
            crate::config::McpServerConfig {
                transport: "stdio".to_string(),
                command: Some("npx".to_string()),
                args: args.clone(),
                url: None,
                enabled: true,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    register_mcp_handshake(
        &state,
        COMPUTER_SERVER_NAME.to_string(),
        "stdio".to_string(),
        Some("npx".to_string()),
        args,
        None,
    )
    .await?;
    computer_status(state).await
}

/// Disconnect the browser: unregister its tools and drop the config entry.
/// The profile directory is left on disk so logins survive a reconnect.
#[command(rename_all = "snake_case")]
pub async fn computer_disable(state: State<'_, AppState>) -> Result<ComputerStatus, String> {
    state
        .tool_bridge
        .unregister_tools_by_prefix(&format!("{}__", COMPUTER_SERVER_NAME));
    state
        .config_manager
        .remove_mcp_server(COMPUTER_SERVER_NAME)
        .await
        .map_err(|e| e.to_string())?;
    computer_status(state).await
}

#[command(rename_all = "snake_case")]
pub async fn list_mcp_status(
    state: State<'_, AppState>,
) -> Result<Vec<crate::mcp::McpStatus>, String> {
    Ok(crate::mcp::get_mcp_status(&state.tool_bridge, &state.config_manager).await)
}

#[command(rename_all = "snake_case")]
pub async fn scan_marketplace() -> Result<Vec<crate::mcp::MarketplaceEntry>, String> {
    Ok(crate::mcp::scan_marketplace())
}

/// Get UI config.
#[command(rename_all = "snake_case")]
pub async fn get_ui_config(state: State<'_, AppState>) -> Result<crate::config::UiConfig, String> {
    Ok(state.config_manager.ui_config().await)
}

/// Set UI config.
#[command(rename_all = "snake_case")]
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

/// Set context-management policy.
#[command(rename_all = "snake_case")]
pub async fn set_context_config(
    context: crate::config::ContextConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_context_config(context)
        .await
        .map_err(|e| e.to_string())
}

/// Get turn/segment budgets + spend guardrails (§5.6-B/F).
#[command(rename_all = "snake_case")]
pub async fn get_budget_config(
    state: State<'_, AppState>,
) -> Result<crate::config::BudgetConfig, String> {
    Ok(state.config_manager.budget_config().await)
}

/// Set turn/segment budgets + spend guardrails.
#[command(rename_all = "snake_case")]
pub async fn set_budget_config(
    budget: crate::config::BudgetConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_budget_config(budget)
        .await
        .map_err(|e| e.to_string())
}

/// Set unified cross-chat memory config.
#[command(rename_all = "snake_case")]
pub async fn set_memory_config(
    memory: crate::config::MemoryConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_memory_config(memory)
        .await
        .map_err(|e| e.to_string())
}

/// Set clarifying-question policy.
#[command(rename_all = "snake_case")]
pub async fn set_interaction_config(
    interaction: crate::config::InteractionConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .config_manager
        .set_interaction_config(interaction)
        .await
        .map_err(|e| e.to_string())
}

/// Read one memory file verbatim for the Settings editor.
/// `scope`: "global" (default) or "workspace". `session_id` selects whose
/// workspace file (absent = global default workspace).
#[command(rename_all = "snake_case")]
pub async fn get_memory_text(
    scope: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let store = crate::memory::MemoryStore::new(state.app_data_dir.clone());
    let ws = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));
    let path = match scope.trim().to_lowercase().as_str() {
        "workspace" => store.workspace_path(&ws),
        _ => store.global_path(),
    };
    Ok(std::fs::read_to_string(path).unwrap_or_default())
}

/// Overwrite one memory file verbatim (Settings editor).
/// `scope`: "global" (default) or "workspace".
#[command(rename_all = "snake_case")]
pub async fn save_memory_text(
    scope: String,
    text: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if text.len() > 200_000 {
        return Err("memory text too large (max 200000 chars)".to_string());
    }
    let store = crate::memory::MemoryStore::new(state.app_data_dir.clone());
    let ws = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));
    let target = match scope.trim().to_lowercase().as_str() {
        "workspace" => crate::memory::MemoryFileScope::Workspace,
        _ => crate::memory::MemoryFileScope::Global,
    };
    store
        .overwrite(target, Some(&ws), &text)
        .map_err(|e| e.to_string())
}

/// Delete memory file(s). `scope`: "global", "workspace", or "both".
#[command(rename_all = "snake_case")]
pub async fn clear_memory(
    scope: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let store = crate::memory::MemoryStore::new(state.app_data_dir.clone());
    let ws = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));
    // Explicit match (not `resolve_forget_target`): an unknown scope string
    // must error, never wipe everything by falling through to "both".
    let (both, only_workspace) = match scope.trim().to_lowercase().as_str() {
        "global" => (false, false),
        "workspace" => (false, true),
        "both" => (true, false),
        other => return Err(format!("unknown memory scope: {other}")),
    };
    store
        .clear(both, only_workspace, Some(&ws))
        .map_err(|e| e.to_string())
}

/// Memory stats for the composer indicator + Settings header.
#[command(rename_all = "snake_case")]
pub async fn get_memory_stats(
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<crate::memory::MemoryStats, String> {
    let store = crate::memory::MemoryStore::new(state.app_data_dir.clone());
    let ws = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));
    Ok(store.stats(Some(&ws)))
}

/// Per-session memory kill-switch for the composer's Memory toggle.
/// Loop-local (resets when the session is re-initialized); the global
/// default stays in `MemoryConfig.enabled`.
#[command(rename_all = "snake_case")]
pub async fn set_session_memory_enabled(
    session_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let loops = state.agent_loops.read().await;
    match loops.get(&session_id) {
        Some(agent) => {
            agent.set_memory_enabled(enabled);
            Ok(())
        }
        None => Err(missing_loop_error(&state, &session_id).await),
    }
}

/// Workspace info for the session header chip.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionWorkspaceInfo {
    pub path: String,
    pub is_default: bool,
}

/// Effective workspace for a session + whether it is the default.
/// `path` is always concrete (mapping → env → stored default → `./workspace`).
#[command(rename_all = "snake_case")]
pub async fn get_session_workspace(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<SessionWorkspaceInfo, String> {
    let store = crate::workspaces::WorkspaceStore::new(&state.app_data_dir);
    if let Some(dir) = store.session_workspace(&session_id) {
        return Ok(SessionWorkspaceInfo {
            path: dir,
            is_default: false,
        });
    }
    Ok(SessionWorkspaceInfo {
        path: store.global_dir().to_string_lossy().replace('\\', "/"),
        is_default: true,
    })
}

/// Set (or clear with null) a session's workspace. Validates the folder
/// exists; a live loop picks it up for the next dispatch (immediate).
/// Returns the canonical path.
#[command(rename_all = "snake_case")]
pub async fn set_session_workspace(
    session_id: String,
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let store = crate::workspaces::WorkspaceStore::new(&state.app_data_dir);
    let canon = store.set_session_workspace(&session_id, path.as_deref())?;
    if let Some(agent) = state.agent_loops.read().await.get(&session_id) {
        agent
            .set_session_workspace(canon.clone().map(std::path::PathBuf::from))
            .await;
    }
    Ok(canon)
}

/// Stored default workspace (canonical path or null).
#[command(rename_all = "snake_case")]
pub async fn get_default_workspace(
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    Ok(crate::workspaces::WorkspaceStore::new(&state.app_data_dir).default_workspace())
}

/// Set (or clear with null) the default workspace.
#[command(rename_all = "snake_case")]
pub async fn set_default_workspace(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    crate::workspaces::WorkspaceStore::new(&state.app_data_dir)
        .set_default_workspace(path.as_deref())
}

/// Recent workspaces (folders that still exist, most-recent first).
#[command(rename_all = "snake_case")]
pub async fn list_recent_workspaces(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    Ok(crate::workspaces::WorkspaceStore::new(&state.app_data_dir).recent())
}

/// Answer a pending `ask_user` question batch. `answers` aligns with the
/// batch's questions by index (empty string = that one skipped).
/// Unknown/gone ids error — e.g. the run was cancelled or already answered.
#[command(rename_all = "snake_case")]
pub async fn answer_question(
    session_id: String,
    question_id: String,
    answers: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let answers: Vec<String> = answers
        .into_iter()
        .take(8)
        .map(|a| a.chars().take(4000).collect())
        .collect();
    let loops = state.agent_loops.read().await;
    match loops.get(&session_id) {
        Some(agent) => {
            if agent.answer_pending_question(&question_id, answers).await {
                Ok(())
            } else {
                Err("question is no longer pending (answered, skipped, or run ended)".to_string())
            }
        }
        None => Err(missing_loop_error(&state, &session_id).await),
    }
}

/// Pending `ask_user` question batches for a session (re-sync after
/// switching back to a session whose run is waiting on the user).
#[command(rename_all = "snake_case")]
pub async fn get_pending_questions(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::agent_event::PendingBatch>, String> {
    let loops = state.agent_loops.read().await;
    match loops.get(&session_id) {
        Some(agent) => Ok(agent.pending_questions().await),
        None => Err(missing_loop_error(&state, &session_id).await),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KiloModel {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
    pub is_free: Option<bool>,
    pub pricing: Option<serde_json::Value>,
    /// Raw `supported_parameters` list from Kilo — lets the picker badge
    /// models that accept `reasoning_effort`.
    pub supported_parameters: Option<Vec<String>>,
    /// Distinct `opencode.variants.*.reasoning.effort` tiers this model
    /// exposes, ascending (`none`…`max`). Empty when the catalog lists no
    /// reasoning variants.
    pub efforts: Option<Vec<String>>,
}

const KILO_DEFAULT_BASE_URL: &str = "https://api.kilo.ai/api/gateway";

/// Ascending wire tiers a model can accept, used to order catalog efforts.
const CANONICAL_EFFORTS: [&str; 7] = [
    "none", "minimal", "low", "medium", "high", "xhigh", "max",
];

/// Kind-wide fallback when no catalog entry applies — the tiers the composer
/// offered before per-model sync (and the ones every native kind accepts).
const PROVIDER_DEFAULT_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

fn kilo_models_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/models") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/models")
    }
}

fn kilo_error_detail(body: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(body).ok();
    let message = value
        .as_ref()
        .and_then(|v| v.pointer("/error/message"))
        .and_then(serde_json::Value::as_str);
    let provider = value
        .as_ref()
        .and_then(|v| v.pointer("/error/metadata/provider_name"))
        .and_then(serde_json::Value::as_str);
    let remedy = value
        .as_ref()
        .and_then(|v| v.pointer("/error/metadata/remedy_hint"))
        .and_then(serde_json::Value::as_str);

    let mut parts = Vec::new();
    if let Some(message) = message {
        parts.push(message.to_string());
    }
    if let Some(provider) = provider {
        parts.push(format!("upstream provider: {provider}"));
    }
    if let Some(remedy) = remedy {
        parts.push(remedy.to_string());
    }
    if parts.is_empty() {
        parts.push("upstream provider returned an error".to_string());
    }
    parts.join("; ").chars().take(320).collect()
}

fn parse_kilo_models(value: &serde_json::Value) -> Result<Vec<KiloModel>, String> {
    let array = value
        .get("data")
        .and_then(|v| v.as_array())
        .or_else(|| value.get("models").and_then(|v| v.as_array()))
        .or_else(|| value.as_array())
        .ok_or("Kilo model response did not contain a model array")?;

    Ok(array
        .iter()
        .filter_map(|model| {
            let id = model
                .get("id")
                .or_else(|| model.get("model"))
                .or_else(|| model.get("slug"))
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.trim().is_empty())?
                .trim()
                .to_string();
            let name = model
                .get("name")
                .or_else(|| model.get("display_name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let context_length = [
                "context_length",
                "contextLength",
                "max_context_length",
                "maxContextLength",
                "context_window",
            ]
            .into_iter()
            .find_map(|key| model.get(key).and_then(serde_json::Value::as_u64));
            let is_free = ["isFree", "is_free", "free"]
                .into_iter()
                .find_map(|key| model.get(key).and_then(serde_json::Value::as_bool));
            let supported_parameters = model
                .get("supported_parameters")
                .and_then(serde_json::Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .filter(|v| !v.is_empty());
            // Effort tiers live under `opencode.variants.<name>.reasoning.effort`
            // (Kilo's per-model reasoning variants), ordered canonically and
            // deduplicated.
            let mut efforts: Vec<String> = model
                .pointer("/opencode/variants")
                .and_then(serde_json::Value::as_object)
                .map(|variants| {
                    let mut tiers: Vec<String> = variants
                        .values()
                        .filter_map(|v| v.pointer("/reasoning/effort").and_then(serde_json::Value::as_str))
                        .map(|s| s.to_lowercase())
                        .collect();
                    tiers.sort_by_key(|tier| {
                        CANONICAL_EFFORTS
                            .iter()
                            .position(|c| c == tier)
                            .unwrap_or(CANONICAL_EFFORTS.len())
                    });
                    tiers.dedup();
                    tiers
                })
                .unwrap_or_default();
            efforts.shrink_to_fit();
            let efforts = (!efforts.is_empty()).then_some(efforts);
            Some(KiloModel {
                id,
                name,
                context_length,
                is_free,
                pricing: model.get("pricing").cloned(),
                supported_parameters,
                efforts,
            })
        })
        .collect())
}

async fn send_kilo_models_request(
    client: &reqwest::Client,
    endpoint: &str,
    key: Option<&str>,
) -> Result<reqwest::Response, String> {
    let mut request = client.get(endpoint);
    if let Some(key) = key {
        let key = key.trim();
        let value = if key.starts_with("Bearer ") {
            key.to_string()
        } else {
            format!("Bearer {key}")
        };
        let header = reqwest::header::HeaderValue::from_str(&value)
            .map_err(|_| "Kilo API key contains invalid header characters".to_string())?;
        request = request.header(reqwest::header::AUTHORIZATION, header);
    }
    request.send().await.map_err(|e| e.to_string())
}

#[command(rename_all = "snake_case")]
pub async fn list_kilo_models(
    provider_id: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<KiloModel>, String> {
    let provider_id = provider_id
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| "kilo".to_string());
    let configured_base = state
        .config_manager
        .provider_settings(&provider_id)
        .await
        .and_then(|settings| settings.base_url);
    let base_url = base_url
        .filter(|url| !url.trim().is_empty())
        .or(configured_base)
        .unwrap_or_else(|| KILO_DEFAULT_BASE_URL.to_string());
    let key = match api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => Some(key),
        None => state.config_manager.api_key(&provider_id).await,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    fetch_kilo_catalog(&client, &base_url, key.as_deref()).await
}

/// One uncached Kilo `/models` fetch: request, auth-fallback retry, parse.
async fn fetch_kilo_catalog(
    client: &reqwest::Client,
    base_url: &str,
    key: Option<&str>,
) -> Result<Vec<KiloModel>, String> {
    let endpoint = kilo_models_url(base_url);
    let mut response = send_kilo_models_request(client, &endpoint, key).await?;
    if key.is_some()
        && matches!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED
                | reqwest::StatusCode::FORBIDDEN
                | reqwest::StatusCode::TOO_MANY_REQUESTS
        )
    {
        response = send_kilo_models_request(client, &endpoint, None).await?;
    }
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "Kilo Gateway returned HTTP {}: {}",
            status,
            kilo_error_detail(&body)
        ));
    }
    let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    parse_kilo_models(&value)
}

/// Per-model reasoning capability for the composer: whether the model takes a
/// `reasoning_effort` request field, and which tiers it accepts. The slider in
/// the chat bar is filtered to `efforts`; the send path re-checks it so a
/// stale pick can never reach a model that rejects it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReasoningProfile {
    pub model: String,
    /// Model accepts `reasoning_effort` at all.
    pub supported: bool,
    /// Wire tiers (`none`…`max`) in ascending order; empty when unsupported.
    pub efforts: Vec<String>,
    /// `kilo-catalog` (per-model, from the gateway catalog),
    /// `provider-kind` (kind-wide default), or `none` (field unavailable).
    pub source: String,
}

/// Alias-normalize a picked effort to its wire form (`Extra` → `xhigh`).
fn normalize_effort(raw: &str) -> String {
    let tier = raw.trim().to_lowercase();
    if tier == "extra" { "xhigh".to_string() } else { tier }
}

/// Profile used whenever no catalog entry applies (non-Kilo base URL, catalog
/// unreachable, or a routing slug the catalog cannot name).
fn kind_default_profile(model: String) -> ReasoningProfile {
    ReasoningProfile {
        model,
        supported: true,
        efforts: PROVIDER_DEFAULT_EFFORTS.iter().map(|s| (*s).to_string()).collect(),
        source: "provider-kind".to_string(),
    }
}

/// Match a configured model slug against catalog ids: exact, bare-slug, then
/// either side namespacing the other (`grok-4` ↔ `x-ai/grok-4`).
fn find_kilo_model<'a>(catalog: &'a [KiloModel], model: &str) -> Option<&'a KiloModel> {
    let needle = model.trim();
    if needle.is_empty() {
        return None;
    }
    let bare = needle.rsplit('/').next().unwrap_or(needle);
    catalog
        .iter()
        .find(|entry| entry.id == needle)
        .or_else(|| catalog.iter().find(|entry| entry.id.rsplit('/').next() == Some(bare)))
        .or_else(|| catalog.iter().find(|entry| entry.id.ends_with(&format!("/{needle}"))))
}

/// Build a profile from a matched catalog entry. A model only counts as
/// supported when `supported_parameters` lists `reasoning_effort` — variants
/// behind the separate `reasoning` parameter are not what we send.
fn profile_from_entry(model: &str, entry: &KiloModel) -> ReasoningProfile {
    let supported = entry
        .supported_parameters
        .as_deref()
        .map(|params| params.iter().any(|p| p.eq_ignore_ascii_case("reasoning_effort")))
        .unwrap_or(false);
    if !supported {
        return ReasoningProfile {
            model: model.to_string(),
            supported: false,
            efforts: Vec::new(),
            source: "kilo-catalog".to_string(),
        };
    }
    // Catalog listed no variants for a reasoning_effort model → offer the
    // kind-wide tiers rather than an empty slider.
    let mut efforts = entry.efforts.clone().unwrap_or_default();
    if efforts.is_empty() {
        efforts = PROVIDER_DEFAULT_EFFORTS.iter().map(|s| (*s).to_string()).collect();
    }
    ReasoningProfile {
        model: model.to_string(),
        supported: true,
        efforts,
        source: "kilo-catalog".to_string(),
    }
}

/// Kilo catalog cache keyed by base URL: `(last attempt, models-or-None)`.
/// Fresh successes serve for 10 minutes; a failed fetch backs off for a minute
/// (keeping any previous catalog as a stale fallback) so the send path never
/// stalls on a gateway hiccup.
type KiloCatalogCache = HashMap<String, (std::time::Instant, Option<Vec<KiloModel>>)>;
static KILO_CATALOG: std::sync::LazyLock<std::sync::Mutex<KiloCatalogCache>> =
    std::sync::LazyLock::new(std::sync::Mutex::default);
const KILO_CATALOG_TTL: std::time::Duration = std::time::Duration::from_secs(600);
const KILO_CATALOG_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

async fn kilo_catalog(
    base_url: &str,
    key: Option<&str>,
) -> Result<Vec<KiloModel>, String> {
    let cache_key = base_url.trim_end_matches('/').to_string();
    {
        let cache = KILO_CATALOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((at, models)) = cache.get(&cache_key) {
            let age = at.elapsed();
            if age < KILO_CATALOG_TTL {
                if let Some(models) = models {
                    return Ok(models.clone());
                }
            }
            if age < KILO_CATALOG_BACKOFF {
                return models
                    .clone()
                    .ok_or_else(|| "Kilo model catalog unavailable".to_string());
            }
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let fetched = fetch_kilo_catalog(&client, base_url, key).await;
    let mut cache = KILO_CATALOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = cache
        .entry(cache_key)
        .or_insert_with(|| (std::time::Instant::now(), None));
    match fetched {
        Ok(models) => {
            *entry = (std::time::Instant::now(), Some(models.clone()));
            Ok(models)
        }
        Err(err) => {
            // Keep any previous catalog as stale fallback; the refreshed
            // timestamp is what arms the backoff window.
            let stale = entry.1.clone();
            entry.0 = std::time::Instant::now();
            stale.ok_or(err)
        }
    }
}

/// Resolve the reasoning-effort profile for a provider's active model.
/// Registered providers answer from their real capability flag; Kilo gateways
/// refine that with the live catalog; every other kind reports its default.
pub async fn reasoning_profile(
    state: &AppState,
    provider_id: &str,
    model: Option<&str>,
) -> ReasoningProfile {
    // Registered provider wins: real capability flag, effective model, base URL.
    // (Guard stays inside the block — never await across the lock.)
    let registered = {
        let reg = state.provider_registry.read().await;
        reg.list()
            .into_iter()
            .find(|info| info.id == provider_id)
            .map(|info| {
                let base_url = match &info.config {
                    crate::providers::ProviderConfig::Http { base_url, .. }
                        if !base_url.trim().is_empty() =>
                    {
                        Some(base_url.clone())
                    }
                    _ => None,
                };
                (
                    info.provider.capabilities().supports_reasoning_effort,
                    info.provider.model().to_string(),
                    base_url,
                )
            })
    };
    let settings = state.config_manager.provider_settings(provider_id).await;
    let model = model
        .filter(|m| !m.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            registered
                .as_ref()
                .and_then(|(_, m, _)| (!m.trim().is_empty()).then(|| m.clone()))
        })
        .or_else(|| settings.as_ref().and_then(|s| s.model.clone()))
        .unwrap_or_else(|| crate::providers::default_provider_config(provider_id).1);

    let supports = match &registered {
        Some((supports, _, _)) => *supports,
        // Unregistered: trust the stored kind (subprocess/MCP runtimes have no
        // request field to fill).
        None => !matches!(
            settings.as_ref().and_then(|s| s.kind.as_deref()),
            Some("subprocess" | "mcp")
        ),
    };
    if !supports {
        return ReasoningProfile {
            model,
            supported: false,
            efforts: Vec::new(),
            source: "none".to_string(),
        };
    }

    let base_url = registered
        .as_ref()
        .and_then(|(_, _, base)| base.clone())
        .or_else(|| settings.as_ref().and_then(|s| s.base_url.clone()))
        .unwrap_or_else(|| {
            if provider_id == "kilo" {
                KILO_DEFAULT_BASE_URL.to_string()
            } else {
                crate::providers::default_provider_config(provider_id).0
            }
        });
    if !base_url.to_lowercase().contains("kilo") {
        return kind_default_profile(model);
    }
    let key = state.config_manager.api_key(provider_id).await;
    match kilo_catalog(&base_url, key.as_deref()).await {
        Ok(catalog) => match find_kilo_model(&catalog, &model) {
            Some(entry) => profile_from_entry(&model, entry),
            None => kind_default_profile(model),
        },
        // Catalog unreachable — degrade to the kind default, never block a send.
        Err(_) => kind_default_profile(model),
    }
}

/// Reasoning profile for the composer: reads the provider's model (or an
/// explicit `model` override) and reports the effort tiers it accepts.
#[command(rename_all = "snake_case")]
pub async fn get_model_reasoning(
    provider_id: Option<String>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Result<ReasoningProfile, String> {
    let id = provider_id
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| "kilo".to_string());
    Ok(reasoning_profile(&state, &id, model.as_deref()).await)
}

/// List discovered skills (local + hub)
#[command(rename_all = "snake_case")]
pub async fn list_skills(
    state: State<'_, AppState>,
) -> Result<Vec<crate::skills::SkillDto>, String> {
    let ws = global_workspace_dir(&state);
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    Ok(skills.iter().map(crate::skills::SkillDto::from).collect())
}

/// Install a skill from raw SKILL.md content
#[command(rename_all = "snake_case")]
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
#[command(rename_all = "snake_case")]
pub async fn remove_skill(name: String, state: State<'_, AppState>) -> Result<(), String> {
    crate::skills::remove_skill(&state.app_data_dir, &name).map_err(|e| e.to_string())?;
    refresh_skills_internal(&state).await?;
    Ok(())
}

/// Fetch a skill from hub (agentskills.io or raw URL)
#[command(rename_all = "snake_case")]
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

async fn refresh_skills_internal(state: &State<'_, AppState>) -> Result<(), String> {
    let ws = global_workspace_dir(&state);
    let skills = crate::skills::discover_skills(&state.app_data_dir, &ws);
    // Update ToolBridge baseline
    let infos: Vec<xai_grok_tools::implementations::skills::types::SkillInfo> = skills;
    state.tool_bridge.update_skill_baseline(infos).await;
    // Also update AppState's tool_bridge available skills for next sessions
    // Apply pending to refresh AvailableSkills
    let _ = state.tool_bridge.apply_pending_skill_update().await;
    Ok(())
}

// ── Skills marketplace (GitHub-backed catalog) ────────────────────────────

/// List configured marketplace sources (built-in default seeded at startup).
#[command(rename_all = "snake_case")]
pub async fn list_marketplace_sources(
    state: State<'_, AppState>,
) -> Result<Vec<crate::config::MarketplaceSource>, String> {
    Ok(state.config_manager.marketplace_sources().await)
}

/// Add (or replace by id) a marketplace source. Fields are validated
/// before anything is stored or used in a request URL.
#[command(rename_all = "snake_case")]
pub async fn add_marketplace_source(
    id: String,
    display_name: String,
    owner: String,
    repo: String,
    branch: Option<String>,
    skills_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<crate::config::MarketplaceSource>, String> {
    let source = crate::skills::sanitize_marketplace_source(&crate::config::MarketplaceSource {
        id,
        display_name,
        owner,
        repo,
        branch: branch.unwrap_or_else(|| "main".to_string()),
        skills_path: skills_path.unwrap_or_else(|| "skills".to_string()),
    })
    .map_err(|e| e.to_string())?;
    state
        .config_manager
        .add_marketplace_source(source)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.config_manager.marketplace_sources().await)
}

/// Remove a marketplace source by id.
#[command(rename_all = "snake_case")]
pub async fn remove_marketplace_source(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::config::MarketplaceSource>, String> {
    state
        .config_manager
        .remove_marketplace_source(&id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state.config_manager.marketplace_sources().await)
}

fn find_marketplace_source(
    sources: &[crate::config::MarketplaceSource],
    source_id: &str,
) -> Result<crate::config::MarketplaceSource, String> {
    sources
        .iter()
        .find(|s| s.id == source_id)
        .cloned()
        .ok_or_else(|| format!("Marketplace source '{source_id}' not found"))
}

/// Normalize a path for `installed` matching across Windows/Unix separators.
fn normalized_skill_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// Mark catalog entries already present in the hub cache as installed.
/// Install dirs look like `.../hub/skills/marketplace/<source>/<flat>/SKILL.md`
/// where `flat` is the repo-relative dir with `/` flattened to `-`
/// (`skills/pdf` → `pdf`, `plugins/foo/skills` → `foo-skills`), so an
/// entry matches when the cached flat segment equals (or ends with
/// `-<leaf of its dir>`).
fn mark_marketplace_installed(
    skills: Vec<crate::skills::MarketplaceSkill>,
    installed_paths: &[String],
    source_id: &str,
) -> Vec<crate::skills::MarketplaceSkill> {
    let marker = format!("hub/skills/marketplace/{}/", source_id.to_lowercase());
    skills
        .into_iter()
        .map(|mut s| {
            let leaf = s
                .dir
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_lowercase();
            s.installed = installed_paths.iter().any(|p| {
                let n = normalized_skill_path(p);
                match n.find(&marker) {
                    Some(i) => match n[i + marker.len()..].split('/').next() {
                        Some(flat) => {
                            flat == leaf || flat.ends_with(&format!("-{leaf}"))
                        }
                        None => false,
                    },
                    None => false,
                }
            });
            s
        })
        .collect()
}

/// Browse a source catalog. Serves the disk cache unless `refresh` is set,
/// which re-enumerates the repo (costs GitHub API requests).
#[command(rename_all = "snake_case")]
pub async fn list_marketplace_skills(
    source_id: String,
    refresh: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<crate::skills::MarketplaceSkill>, String> {
    let sources = state.config_manager.marketplace_sources().await;
    let source = find_marketplace_source(&sources, &source_id)?;
    let mut skills = if refresh.unwrap_or(false) {
        crate::skills::fetch_marketplace_catalog(&state.app_data_dir, &source)
            .await
            .map_err(|e| e.to_string())?
    } else {
        let cached = crate::skills::cached_marketplace_catalog(&state.app_data_dir, &source.id);
        if cached.is_empty() {
            // First browse fetches automatically so the panel is useful
            // immediately; later browses serve cache until Refresh.
            crate::skills::fetch_marketplace_catalog(&state.app_data_dir, &source)
                .await
                .map_err(|e| e.to_string())?
        } else {
            cached
        }
    };
    let ws = global_workspace_dir(&state);
    let installed: Vec<String> = crate::skills::discover_skills(&state.app_data_dir, &ws)
        .iter()
        .map(|s| s.path.clone())
        .collect();
    skills = mark_marketplace_installed(skills, &installed, &source.id);
    Ok(skills)
}

/// Search a source catalog by name/description (case-insensitive, cache).
#[command(rename_all = "snake_case")]
pub async fn search_marketplace_skills(
    source_id: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<crate::skills::MarketplaceSkill>, String> {
    let sources = state.config_manager.marketplace_sources().await;
    let source = find_marketplace_source(&sources, &source_id)?;
    let cached = crate::skills::cached_marketplace_catalog(&state.app_data_dir, &source.id);
    let skills = if cached.is_empty() {
        crate::skills::fetch_marketplace_catalog(&state.app_data_dir, &source)
            .await
            .map_err(|e| e.to_string())?
    } else {
        cached
    };
    let q = query.to_lowercase();
    let filtered: Vec<_> = skills
        .into_iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q)
        })
        .collect();
    let ws = global_workspace_dir(&state);
    let installed: Vec<String> = crate::skills::discover_skills(&state.app_data_dir, &ws)
        .iter()
        .map(|s| s.path.clone())
        .collect();
    Ok(mark_marketplace_installed(filtered, &installed, &source.id))
}

/// Install a marketplace skill by repo-relative dir, then refresh the bridge.
#[command(rename_all = "snake_case")]
pub async fn install_marketplace_skill(
    source_id: String,
    dir: String,
    state: State<'_, AppState>,
) -> Result<crate::skills::SkillDto, String> {
    let sources = state.config_manager.marketplace_sources().await;
    let source = find_marketplace_source(&sources, &source_id)?;
    let dto =
        crate::skills::install_marketplace_skill(&state.app_data_dir, &source, &dir)
            .await
            .map_err(|e| e.to_string())?;
    refresh_skills_internal(&state).await?;
    Ok(dto)
}

// ── Composer `+` menu: workspace file browser ─────────────────────────────────

/// One row of the composer workspace browser (path relative to the
/// workspace root, forward slashes).
#[derive(serde::Serialize, Clone)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

/// A workspace text file read for attachment (24 KB cap, binary rejected).
#[derive(serde::Serialize, Clone)]
pub struct WorkspaceFileRead {
    pub path: String,
    pub content: String,
    pub size: u64,
    pub truncated: bool,
}

const WS_LIST_MAX_DEPTH: usize = 3;
const WS_LIST_MAX_ENTRIES: usize = 500;
const WS_READ_MAX_BYTES: usize = 24 * 1024;
/// Artifact-viewer reads use a far larger cap than attachments: a generated
/// page with inline CSS/JS is routinely 30-80 KB, and cutting one off
/// mid-tag leaves the preview rendering nothing at all.
const PREVIEW_MAX_BYTES: usize = 4 * 1024 * 1024;
/// Non-hidden noise dirs; hidden dirs (`.git`, `.venv`, …) are skipped too.
const WS_SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "__pycache__"];

/// Resolve a workspace-relative path to a canonical absolute path.
/// Denies by default: absolute paths, `..`, and anything (symlinks
/// included) that canonicalizes outside the workspace root. Empty = root.
pub(crate) fn ws_resolve(root: &std::path::Path, rel: &str) -> Result<std::path::PathBuf, String> {
    let root_c = root.canonicalize().map_err(|e| format!("workspace unavailable: {e}"))?;
    let rel = rel.trim().replace('\\', "/");
    let candidate = if rel.is_empty() { root_c.clone() } else { root_c.join(&rel) };
    let canon = candidate
        .canonicalize()
        .map_err(|_| format!("not found: {}", rel.trim_matches('/')))?;
    if !canon.starts_with(&root_c) {
        return Err("path escapes the workspace".to_string());
    }
    Ok(canon)
}

/// List files/dirs for the composer browser: depth ≤ 3, ≤ 500 entries,
/// noise dirs skipped, dirs sorted before files (then name, case-insensitive).
/// Unreadable subdirectories are skipped; an unreadable root is an error.
fn ws_list(root: &std::path::Path, rel: &str) -> Result<Vec<WorkspaceEntry>, String> {
    let start = ws_resolve(root, rel)?;
    if !start.is_dir() {
        return Err("not a directory".to_string());
    }
    let root_c = root.canonicalize().map_err(|e| format!("workspace unavailable: {e}"))?;
    let base = rel.trim().replace('\\', "/").trim_matches('/').to_string();
    let mut out: Vec<WorkspaceEntry> = Vec::new();
    let mut stack: Vec<(std::path::PathBuf, String, usize)> = vec![(start, base, 0)];
    'walk: while let Some((dir, prefix, depth)) = stack.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                if depth == 0 {
                    return Err(format!("cannot list workspace: {e}"));
                }
                continue;
            }
        };
        let mut children = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // metadata() follows symlinks so link-to-dir browses like a dir.
            let md = e.metadata().ok();
            let is_dir = md.as_ref().is_some_and(|m| m.is_dir());
            if is_dir
                && (name.starts_with('.') || WS_SKIP_DIRS.contains(&name.as_str()))
            {
                continue;
            }
            let size = md.as_ref().filter(|m| !m.is_dir()).map(|m| m.len());
            children.push((e.path(), name, is_dir, size));
        }
        children.sort_by(|a, b| {
            b.2.cmp(&a.2).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        for (p, name, is_dir, size) in children {
            if out.len() >= WS_LIST_MAX_ENTRIES {
                break 'walk;
            }
            let child_rel =
                if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            out.push(WorkspaceEntry { path: child_rel.clone(), name, is_dir, size });
            if is_dir && depth + 1 < WS_LIST_MAX_DEPTH {
                // Jail each descent: a symlink pointing outside is not followed.
                match p.canonicalize() {
                    Ok(c) if c.starts_with(&root_c) => stack.push((c, child_rel, depth + 1)),
                    _ => {}
                }
            }
        }
    }
    Ok(out)
}

/// Read a workspace text file for attachment. Caps at 24 KB, rejects
/// binary content (NUL sniff), and never escapes the workspace root.
fn ws_read(root: &std::path::Path, rel: &str) -> Result<WorkspaceFileRead, String> {
    ws_read_capped(root, rel, WS_READ_MAX_BYTES)
}

/// [`ws_read`] with an explicit byte cap. Attachments stay at 24 KB so a huge
/// file can't bloat the conversation; the artifact viewer passes
/// [`PREVIEW_MAX_BYTES`] because a preview truncated mid-tag renders as
/// nothing, which is worse than reading a little more of the file.
fn ws_read_capped(
    root: &std::path::Path,
    rel: &str,
    max_bytes: usize,
) -> Result<WorkspaceFileRead, String> {
    if rel.trim().is_empty() {
        return Err("path required".to_string());
    }
    let p = ws_resolve(root, rel)?;
    if p.is_dir() {
        return Err("not a file".to_string());
    }
    let size = std::fs::metadata(&p).map_err(|e| e.to_string())?.len();
    use std::io::Read;
    let file = std::fs::File::open(&p).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity(max_bytes + 1);
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    let truncated = buf.len() > max_bytes;
    if truncated {
        buf.truncate(max_bytes);
    }
    if buf.iter().take(8192).any(|&b| b == 0) {
        return Err("binary file — attach text files only".to_string());
    }
    let content = match std::str::from_utf8(&buf) {
        Ok(s) => s.to_string(),
        Err(e) => {
            if e.error_len().is_none() {
                // Cut mid-character at the 24 KB boundary — drop the tail.
                buf.truncate(e.valid_up_to());
            }
            String::from_utf8_lossy(&buf).into_owned()
        }
    };
    Ok(WorkspaceFileRead { path: rel.trim().replace('\\', "/"), content, size, truncated })
}

/// List workspace files/dirs for the composer `+` menu. `path` is
/// workspace-relative; empty lists the workspace root. `session_id` selects
/// whose workspace (absent = global default).
#[command(rename_all = "snake_case")]
pub async fn list_workspace_files(
    path: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceEntry>, String> {
    let root = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));
    ws_list(&root, &path)
}

/// Read a workspace text file for attaching (24 KB cap, binary rejected).
#[command(rename_all = "snake_case")]
pub async fn read_workspace_file(
    path: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceFileRead, String> {
    let root = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));
    ws_read(&root, &path)
}

/// Extensions the artifact viewer may preview.
const PREVIEWABLE_EXTENSIONS: [&str; 4] = ["html", "htm", "svg", "md"];

/// Read a file for the artifact viewer. Accepts workspace-relative paths
/// like `read_workspace_file`, plus the absolute paths tools echo back. The
/// file must exist and have a previewable extension; binary is refused.
#[command(rename_all = "snake_case")]
pub async fn preview_workspace_file(
    path: String,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<WorkspaceFileRead, String> {
    let roots = preview_roots(&state, session_id.as_deref());
    let (serving_root, rel) = preview_target(&roots, &path)?;
    let ext = std::path::Path::new(&rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !PREVIEWABLE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("not previewable (only .html, .svg, .md): {rel}"));
    }
    ws_read_capped(&serving_root, &rel, PREVIEW_MAX_BYTES)
}

/// Roots an artifact path may be resolved against, in order: the session
/// workspace the UI shows, then the directory the tools actually write into.
/// The two can differ — a tool resolves a relative path against its own
/// workspace, which is not always the folder the UI stored — so a bare
/// `index.html` has to be looked for in both.
fn preview_roots(state: &AppState, session_id: Option<&str>) -> Vec<std::path::PathBuf> {
    let session = session_workspace_dir(state, session_id)
        .unwrap_or_else(|| global_workspace_dir(state));
    let mut roots = vec![session];
    let tool_root = crate::tools::resolve_workspace_dir();
    let known = roots.iter().any(|r| match (r.canonicalize(), tool_root.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    });
    if !known {
        roots.push(tool_root);
    }
    roots
}

/// Resolve an artifact to `(serving root, path relative to that root)`.
///
/// The workspace that contains the file is the serving root, so its sibling
/// CSS/JS resolve. A file the agent wrote outside every known workspace (an
/// absolute path, or a one-off cwd) is served from its own directory instead
/// of 404ing against a root that never held it.
///
/// Relative input may not climb out with `..` or a symlink; an absolute path
/// is the one a tool itself reported, so it is taken as-is.
fn preview_target(
    roots: &[std::path::PathBuf],
    path: &str,
) -> Result<(std::path::PathBuf, String), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("path required".to_string());
    }
    let raw = std::path::PathBuf::from(trimmed);
    // Rooted but prefix-less (`/etc/hostname`): absolute on Unix, and on
    // Windows it would join onto the workspace drive — reject up front.
    if trimmed.starts_with('/') && !raw.is_absolute() {
        return Err("absolute paths outside the workspace are rejected".to_string());
    }
    if !raw.is_absolute()
        && raw
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("path escapes the workspace".to_string());
    }

    if raw.is_absolute() {
        let canon = raw
            .canonicalize()
            .map_err(|_| format!("not found: {}", trimmed.trim_matches('/')))?;
        if !canon.is_file() {
            return Err("not a file".to_string());
        }
        for root in roots {
            let Ok(root_c) = root.canonicalize() else { continue };
            let Ok(rel) = canon.strip_prefix(&root_c) else { continue };
            return Ok((root_c, rel.to_string_lossy().replace('\\', "/")));
        }
        return Ok(file_dir(&canon));
    }

    // Relative: the first root that actually holds the file wins.
    for root in roots {
        let Ok(root_c) = root.canonicalize() else { continue };
        let Ok(canon) = root_c.join(&raw).canonicalize() else { continue };
        if !canon.is_file() {
            continue;
        }
        // Relative input that canonicalized outside the root (a symlink out)
        // is still an escape, not a stray agent write.
        let Ok(rel) = canon.strip_prefix(&root_c) else {
            return Err("path escapes the workspace".to_string());
        };
        return Ok((root_c, rel.to_string_lossy().replace('\\', "/")));
    }
    Err(format!("not found: {}", trimmed.trim_matches('/')))
}

/// A file served from its own directory — the only way its siblings resolve
/// when it lives outside every known workspace.
fn file_dir(canon: &std::path::Path) -> (std::path::PathBuf, String) {
    let parent = canon
        .parent()
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| canon.to_path_buf());
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    (parent, name)
}

/// Base URL of the loopback preview server for one artifact, starting it on
/// first use.
///
/// `path` decides the root: see [`preview_target`], the same resolver
/// `preview_workspace_file` uses, so the URL always points at the file that
/// was actually read. With no `path`, the session workspace root is returned.
///
/// Returns the full URL for `path`.
#[command(rename_all = "snake_case")]
pub async fn preview_server_start(
    path: Option<String>,
    session_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let session_root = session_workspace_dir(&state, session_id.as_deref())
        .unwrap_or_else(|| global_workspace_dir(&state));

    let Some(p) = path.filter(|p| !p.trim().is_empty()) else {
        return crate::preview_server::ensure_url(session_root).await;
    };

    let (root, rel) = preview_target(&preview_roots(&state, session_id.as_deref()), &p)?;
    let base = crate::preview_server::ensure_url(root).await?;
    Ok(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        url_path_encode(&rel)
    ))
}

/// Percent-encode one URL path: unreserved characters and `/` pass through,
/// everything else (spaces, `#`, `?`, non-ASCII) is escaped byte-wise. Enough
/// for a loopback URL handed straight to an iframe.
fn url_path_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod workspace_browser_tests {
    use super::*;

    fn tmp_root(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("maverick-ws-browser-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn resolve_denies_escapes_and_accepts_root() {
        let root = tmp_root("esc");
        std::fs::create_dir_all(root.join("inner")).unwrap();
        assert!(ws_resolve(&root, "").is_ok());
        assert!(ws_resolve(&root, "inner").is_ok());
        assert!(ws_resolve(&root, "inner/../inner").is_ok());
        assert!(ws_resolve(&root, "../escape").is_err());
        assert!(ws_resolve(&root, "inner/../../escape").is_err());
        assert!(ws_resolve(&root, "/etc").is_err());
        assert!(ws_resolve(&root, "missing-dir").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_depth_skip_cap_and_order() {
        let root = tmp_root("list");
        std::fs::write(root.join("notes.txt"), "hi").unwrap();
        std::fs::create_dir_all(root.join("dir1")).unwrap();
        std::fs::write(root.join("dir1/file.txt"), "x").unwrap();
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("a/b/c/deep.txt"), "deep").unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "j").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "g").unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join(".hidden/secret.txt"), "s").unwrap();

        let out = ws_list(&root, "").unwrap();
        let paths: Vec<&str> = out.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"notes.txt"));
        assert!(paths.contains(&"dir1") && paths.contains(&"dir1/file.txt"));
        assert!(paths.contains(&"a") && paths.contains(&"a/b") && paths.contains(&"a/b/c"));
        assert!(!paths.contains(&"a/b/c/deep.txt"), "level 4 must not be listed: {paths:?}");
        assert!(!paths.iter().any(|p| p.starts_with("node_modules")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.starts_with(".git")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.starts_with(".hidden")), "{paths:?}");
        // Root level: dirs before files, alphabetical.
        assert_eq!(out[0].path, "a");
        assert_eq!(out[1].path, "dir1");
        assert_eq!(out[2].path, "notes.txt");
        assert!(out[0].is_dir && !out[2].is_dir);
        assert_eq!(out[2].size, Some(2));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_caps_entries_at_max() {
        let root = tmp_root("cap");
        for i in 0..600 {
            std::fs::write(root.join(format!("f{i}.txt")), "x").unwrap();
        }
        let out = ws_list(&root, "").unwrap();
        assert_eq!(out.len(), WS_LIST_MAX_ENTRIES);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_text_binary_truncation_and_jail() {
        let root = tmp_root("read");
        std::fs::write(root.join("hello.txt"), "hi there").unwrap();
        std::fs::create_dir_all(root.join("adir")).unwrap();
        std::fs::write(root.join("bin.dat"), b"ab\x00cd").unwrap();
        std::fs::write(root.join("big.txt"), "a".repeat(WS_READ_MAX_BYTES + 500)).unwrap();

        let r = ws_read(&root, "hello.txt").unwrap();
        assert_eq!(r.content, "hi there");
        assert!(!r.truncated);
        assert_eq!(r.size, 8);

        let big = ws_read(&root, "big.txt").unwrap();
        assert!(big.truncated);
        assert_eq!(big.content.len(), WS_READ_MAX_BYTES);
        assert_eq!(big.size, (WS_READ_MAX_BYTES + 500) as u64);

        assert!(ws_read(&root, "bin.dat").is_err());
        assert!(ws_read(&root, "adir").is_err());
        assert!(ws_read(&root, "").is_err());
        assert!(ws_read(&root, "../outside.txt").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn preview_target_finds_files_in_either_root_and_serves_strays_locally() {
        let ui = tmp_root("pt-ui");
        let tool = tmp_root("pt-tool");
        let stray = std::env::temp_dir().join("maverick-ws-browser-pt-stray");
        let _ = std::fs::remove_dir_all(&stray);
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(ui.join("ui.html"), "<h1>ui</h1>").unwrap();
        std::fs::write(tool.join("index.html"), "<h1>hi</h1>").unwrap();
        std::fs::create_dir_all(tool.join("site")).unwrap();
        std::fs::write(tool.join("site").join("index.html"), "<p>s</p>").unwrap();
        std::fs::write(stray.join("standalone.html"), "<p>stray</p>").unwrap();

        let roots = vec![ui.clone(), tool.clone()];
        let ui_c = ui.canonicalize().unwrap();
        let tool_c = tool.canonicalize().unwrap();

        // The real case: the agent wrote into the tool workspace while the UI
        // stores a different one, so a bare `index.html` misses root #1.
        let (serving, rel) = preview_target(&roots, "index.html").unwrap();
        assert_eq!(serving, tool_c);
        assert_eq!(rel, "index.html");
        assert_eq!(
            preview_target(&roots, "site/index.html").unwrap().1,
            "site/index.html"
        );

        // Root order decides when both hold the name.
        std::fs::write(ui.join("index.html"), "<h1>ui</h1>").unwrap();
        assert_eq!(preview_target(&roots, "index.html").unwrap().0, ui_c);

        // Absolute paths root at whichever workspace contains them.
        let abs_ui = ui.join("ui.html");
        assert_eq!(
            preview_target(&roots, &abs_ui.to_string_lossy()).unwrap().0,
            ui_c
        );
        // Outside every root: its own directory serves it, so siblings resolve.
        let (serving, rel) = preview_target(&roots, &stray.join("standalone.html").to_string_lossy())
            .unwrap();
        assert_eq!(serving, stray.canonicalize().unwrap());
        assert_eq!(rel, "standalone.html");

        // Escapes and misses stay refused.
        assert!(preview_target(&roots, "../escape.html").is_err());
        assert!(preview_target(&roots, "missing.html").is_err());
        assert!(preview_target(&roots, "").is_err());

        let _ = std::fs::remove_dir_all(&ui);
        let _ = std::fs::remove_dir_all(&tool);
        let _ = std::fs::remove_dir_all(&stray);
    }
}

#[cfg(test)]
mod reasoning_profile_tests {
    use super::*;

    fn entry(json: &str) -> KiloModel {
        let model: serde_json::Value = serde_json::from_str(json).expect("entry json");
        parse_kilo_models(&serde_json::json!({ "data": [model] }))
            .expect("catalog parses")
            .pop()
            .expect("entry parsed")
    }

    #[test]
    fn variants_are_ordered_canonically_and_deduped() {
        let model = entry(
            r#"{"id":"x/y","supported_parameters":["reasoning_effort"],
                "opencode":{"variants":{
                  "high":{"reasoning":{"effort":"high"}},
                  "xhigh":{"reasoning":{"effort":"xhigh"}},
                  "low":{"reasoning":{"effort":"low"}},
                  "again-high":{"reasoning":{"effort":"high"}},
                  "none":{"reasoning":{"effort":"none"}}}}}"#,
        );
        assert_eq!(
            model.efforts,
            Some(vec![
                "none".to_string(),
                "low".to_string(),
                "high".to_string(),
                "xhigh".to_string()
            ])
        );
        let profile = profile_from_entry("x/y", &model);
        assert!(profile.supported);
        assert_eq!(profile.source, "kilo-catalog");
        assert_eq!(profile.efforts, vec!["none", "low", "high", "xhigh"]);
    }

    #[test]
    fn variants_behind_reasoning_param_are_unsupported() {
        let model = entry(
            r#"{"id":"a/b","supported_parameters":["reasoning","include_reasoning"],
                "opencode":{"variants":{
                  "instant":{"reasoning":{"enabled":false,"effort":"none"}},
                  "thinking":{"reasoning":{"enabled":true,"effort":"high"}}}}}"#,
        );
        let profile = profile_from_entry("a/b", &model);
        assert!(!profile.supported);
        assert!(profile.efforts.is_empty());
        assert_eq!(profile.source, "kilo-catalog");
    }

    #[test]
    fn supported_model_without_variants_falls_back_to_kind_tiers() {
        let model = entry(r#"{"id":"c/d","supported_parameters":["reasoning_effort"]}"#);
        let profile = profile_from_entry("c/d", &model);
        assert!(profile.supported);
        assert_eq!(profile.source, "kilo-catalog");
        assert_eq!(
            profile.efforts,
            PROVIDER_DEFAULT_EFFORTS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn catalog_matching_handles_bare_and_namespaced_slugs() {
        let catalog = vec![
            entry(r#"{"id":"x-ai/grok-4"}"#),
            entry(r#"{"id":"kilo-auto/free"}"#),
        ];
        let id_of = |m: &str| find_kilo_model(&catalog, m).map(|e| e.id.clone());
        assert_eq!(id_of("x-ai/grok-4").as_deref(), Some("x-ai/grok-4"));
        assert_eq!(id_of("grok-4").as_deref(), Some("x-ai/grok-4"));
        assert_eq!(id_of("kilo-auto/free").as_deref(), Some("kilo-auto/free"));
        assert!(id_of("missing/model").is_none());
        assert!(id_of("  ").is_none());
    }

    #[test]
    fn normalize_effort_aliases_extra_and_lowercases() {
        assert_eq!(normalize_effort(" Extra "), "xhigh");
        assert_eq!(normalize_effort("HIGH"), "high");
        assert_eq!(normalize_effort("Max"), "max");
    }

    fn preset(
        id: &str,
        name: &str,
        provider: &str,
        model: &str,
        effort: Option<&str>,
    ) -> crate::config::ModelPreset {
        crate::config::ModelPreset {
            id: id.to_string(),
            name: name.to_string(),
            provider_id: provider.to_string(),
            model: model.to_string(),
            effort: effort.map(str::to_string),
        }
    }

    #[test]
    fn validate_presets_trims_fields_and_keeps_order() {
        let out = validate_presets(vec![
            preset(" p1 ", " Fast coder ", " kilo ", " grok-code-fast ", Some("high")),
            preset("p2", "Plain", "openai", "gpt-5", None),
        ])
        .expect("valid list");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "p1");
        assert_eq!(out[0].name, "Fast coder");
        assert_eq!(out[0].provider_id, "kilo");
        assert_eq!(out[0].model, "grok-code-fast");
        assert_eq!(out[0].effort.as_deref(), Some("high"));
        assert_eq!(out[1].effort, None);
    }

    #[test]
    fn validate_presets_rejects_blank_effort_by_clearing_it() {
        let out = validate_presets(vec![preset("p1", "N", "kilo", "m", Some("  "))])
            .expect("blank effort is not an error");
        assert_eq!(out[0].effort, None);
    }

    #[test]
    fn validate_presets_rejects_duplicates_and_missing_fields() {
        assert!(validate_presets(vec![
            preset("p1", "A", "kilo", "m", None),
            preset("p1", "B", "kilo", "m", None),
        ])
        .is_err());
        assert!(validate_presets(vec![preset("p1", "  ", "kilo", "m", None)]).is_err());
        assert!(validate_presets(vec![preset("", "A", "kilo", "m", None)]).is_err());
        assert!(validate_presets(vec![preset("p1", "A", "kilo", "  ", None)]).is_err());
    }

    #[test]
    fn validate_presets_caps_list_size() {
        let many: Vec<_> = (0..51)
            .map(|i| preset(&format!("p{i}"), "N", "kilo", "m", None))
            .collect();
        assert!(validate_presets(many).is_err());
    }
}
