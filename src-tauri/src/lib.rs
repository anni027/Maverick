//! `maverick-backend` — the agent runtime for the Maverick chat-first automation app.
//!
//! This crate is the slice of `xai-grok` we reuse: a provider-agnostic
//! [`AgentLoop`] that composes the vendored, ACP-free core crates
//! (`xai-chat-state` for the conversation, `xai-grok-tools` for the tool
//! runtime, `xai-grok-sampling-types` for the wire format) with a pluggable
//! [`Provider`] (native LLM or an external agent like Kilo Code / OpenCode).

pub mod agent_event;
pub mod agent_loop;
pub mod commands;
pub mod config;
pub mod providers;
pub mod session_store;
pub mod tools;

pub use agent_event::{AgentEvent, AgentEventSink, PrintSink, TauriSink};
pub use agent_loop::AgentLoop;
pub use commands::AppState;
pub use config::{ConfigManager, ConfigSnapshot, McpServerConfig, MaverickConfig, UiConfig};
pub use providers::{
    Provider, ProviderCapabilities, ProviderConfig, ProviderInfo, ProviderInfoDto, ProviderKind,
    ProviderRegistry,
};
pub use session_store::{JsonlChatPersistence, SessionManager};
pub use tools::{add_mcp_server, build_chat_handle, build_tool_bridge};
pub use xai_grok_tools::bridge::ToolBridge;

/// Headless demo: build the loop with a real provider + the real tool bridge,
/// send one user message, and run the agent until it stops.
///
/// This proves the architecture end-to-end without a UI:
/// chat store -> build_request -> provider -> push assistant ->
/// execute tool calls via the real vendored tool runtime -> loop.
/// Requires a configured provider (e.g. xAI/OpenAI) — fails gracefully if none set.
pub async fn run_headless_demo() -> anyhow::Result<()> {
    let app_data = std::env::temp_dir().join("maverick-demo");
    let chat = crate::build_chat_handle("demo-session", Some(app_data))?;
    let tools = crate::build_tool_bridge().await?;
    // Use the default provider from config or error if none configured.
    let app_data_dir = std::env::temp_dir().join("maverick-app");
    let cfg = crate::config::MaverickConfig::load(&app_data_dir).unwrap_or_default();
    let (api_key, _base, _model) = (
        cfg.api_key("openai")
            .or_else(|| cfg.api_key("xai"))
            .or_else(|| cfg.api_key("anthropic")),
        String::new(),
        String::new(),
    );
    let Some(key) = api_key else {
        anyhow::bail!("No provider API key configured — set one in Settings (xAI/OpenAI/Anthropic) before running headless demo");
    };
    // Prefer OpenAI if available, else xAI, else Anthropic — resolved above.
    let provider_id = if cfg.api_key("openai").is_some() {
        "openai"
    } else if cfg.api_key("xai").is_some() {
        "xai"
    } else {
        "anthropic"
    };
    let provider = crate::providers::create_provider(provider_id, Some(key), None, None, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to create provider {provider_id}"))?;
    let agent = crate::AgentLoop::new(chat, tools, provider);
    let sink: std::sync::Arc<dyn crate::AgentEventSink> = std::sync::Arc::new(crate::PrintSink);
    agent
        .send_user_message(
            "Introduce yourself by running a shell command.",
            sink,
        )
        .await?;
    Ok(())
}