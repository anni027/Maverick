//! `nexus-backend` — the agent runtime for the Nexus chat-first automation app.
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
pub use config::{ConfigManager, ConfigSnapshot, McpServerConfig, NexusConfig, UiConfig};
pub use providers::{
    MockProvider, Provider, ProviderCapabilities, ProviderConfig, ProviderInfo, ProviderInfoDto,
    ProviderKind, ProviderRegistry,
};
pub use session_store::{JsonlChatPersistence, SessionManager};
pub use tools::{add_mcp_server, build_chat_handle, build_tool_bridge};
pub use xai_grok_tools::bridge::ToolBridge;

/// Headless demo: build the loop with a mock provider + the real tool bridge,
/// send one user message, and run the agent until it stops.
///
/// This proves the architecture end-to-end without a UI:
/// chat store -> build_request -> provider -> push assistant ->
/// execute tool calls via the real vendored tool runtime -> loop.
pub async fn run_headless_demo() -> anyhow::Result<()> {
    let app_data = std::env::temp_dir().join("nexus-demo");
    let chat = crate::build_chat_handle("demo-session", Some(app_data))?;
    let tools = crate::build_tool_bridge().await?;
    let provider: std::sync::Arc<dyn crate::Provider> =
        std::sync::Arc::new(crate::MockProvider::new());
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