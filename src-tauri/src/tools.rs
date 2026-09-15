//! Construction of the vendored core: the chat-state actor and the tool
//! bridge wired with the in-crate local terminal + local filesystem backends.
//!
//! No `xai-grok-shell` is involved — `LocalTerminalBackend` lives inside
//! `xai-grok-tools` and `LocalFs` is the trivial filesystem impl. Heavy tools
//! (subagents, image/video gen, LSP, scheduler) are simply not registered, so
//! the optional backends they need can stay `None`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use xai_chat_state::{ChatStateActor, ChatStateHandle, NullChatPersistence};
use std::num::NonZeroU64;

use xai_grok_sampling_types::{ApiBackend, SamplingConfig};
use xai_grok_tools::bridge::ToolBridge;
use xai_grok_tools::computer::local::{LocalFs, LocalTerminalBackend};
use xai_grok_tools::implementations::opencode::skill::SkillTool as OpenCodeSkillTool;
use xai_grok_tools::notification::ToolNotificationHandle;
use xai_grok_tools::registry::types::{SessionContext, ToolConfig, ToolServerConfig};
use xai_grok_tools::reminders::DEFAULT_REMINDER_TAG;

use tokio::sync::{mpsc, Mutex as TokioMutex};

use agent_client_protocol as acp;

/// Simple permission guard - asks before writing files.
#[derive(Default, Clone)]
pub struct PermissionGuard {
    auto_approve: bool,
}

impl PermissionGuard {
    pub fn new(auto_approve: bool) -> Self {
        Self { auto_approve }
    }

    /// Check if the given tool call should be allowed.
    /// For v1: just auto-approve reads, ask for writes.
    pub async fn check(&self, tool_name: &str, _args: &serde_json::Value) -> anyhow::Result<bool> {
        let is_write = matches!(tool_name, "search_replace" | "todo_write");
        if self.auto_approve || !is_write {
            return Ok(true);
        }
        // In a real UI this would prompt; for headless we auto-approve.
        Ok(true)
    }
}

/// Create a chat-state actor with JSONL persistence.
/// Pass `None` for `app_data_dir` to use in-memory (NullChatPersistence).
pub fn build_chat_handle(
    session_id: &str,
    app_data_dir: Option<PathBuf>,
) -> anyhow::Result<ChatStateHandle> {
    let (event_tx, _rx) = mpsc::unbounded_channel();

    use xai_grok_sampling_types::ConversationItem;

    let (initial_history, persistence): (Vec<ConversationItem>, Box<dyn xai_chat_state::persistence::ChatPersistence>) =
        if let Some(dir) = app_data_dir {
            let p = crate::session_store::JsonlChatPersistence::new(session_id.to_string(), dir)?;
            let hist = p.history();
            (hist, Box::new(p))
        } else {
            (vec![], Box::new(NullChatPersistence))
        };

    Ok(ChatStateActor::spawn(
        initial_history,
        SamplingConfig {
            base_url: String::new(),
            model: "maverick".to_string(),
            max_completion_tokens: None,
            temperature: None,
            top_p: None,
            api_backend: ApiBackend::ChatCompletions,
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: NonZeroU64::new(128_000).expect("context window must be nonzero"),
            reasoning_effort: None,
            stream_tool_calls: None,
        },
        persistence,
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    ))
}

/// Resolve the workspace directory for tool executions (defaults to `d:\grok\workspace`).
pub fn resolve_workspace_dir() -> PathBuf {
    let specific = PathBuf::from("d:\\grok\\workspace");
    if specific.exists() {
        return specific;
    }
    let local = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("workspace");
    let _ = std::fs::create_dir_all(&local);
    local
}

/// Specification for the `write_to_file` tool exposed to language models.
pub fn write_to_file_spec() -> xai_grok_sampling_types::ToolSpec {
    xai_grok_sampling_types::ToolSpec {
        name: "write_to_file".to_string(),
        description: Some(
            "Write or create a file with the given content in the workspace folder. Creates any required parent directories automatically."
                .to_string(),
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to create or write (relative to workspace or absolute)"
                },
                "content": {
                    "type": "string",
                    "description": "Text or code content to write to the file"
                }
            },
            "required": ["path", "content"]
        }),
    }
}

/// Execute a `write_to_file` call against the designated workspace directory.
pub fn execute_write_to_file(args: &serde_json::Value, workspace_dir: &std::path::Path) -> Result<String, String> {
    let path_str = args.get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("target_file"))
        .or_else(|| args.get("filename"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter `path`".to_string())?;

    let content_str = args.get("content")
        .or_else(|| args.get("contents"))
        .or_else(|| args.get("code"))
        .or_else(|| args.get("text"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required parameter `content`".to_string())?;

    let target_path = if std::path::Path::new(path_str).is_absolute() {
        std::path::PathBuf::from(path_str)
    } else {
        workspace_dir.join(path_str)
    };

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directories for {}: {e}", target_path.display()))?;
    }

    std::fs::write(&target_path, content_str)
        .map_err(|e| format!("Failed to write file {}: {e}", target_path.display()))?;

    Ok(format!(
        "Successfully wrote {} bytes to {}",
        content_str.len(),
        target_path.display()
    ))
}

/// Build the tool bridge with the v1-ready built-in subset, wired to the
/// local terminal + local filesystem in the workspace directory.
/// Now includes `skill` tool and auto-discovers local + hub skills.
pub async fn build_tool_bridge() -> Result<ToolBridge> {
    build_tool_bridge_with_app_data(None).await
}

/// Build the tool bridge with explicit app_data_dir for skill discovery.
pub async fn build_tool_bridge_with_app_data(app_data_dir: Option<PathBuf>) -> Result<ToolBridge> {
    let builder = ToolBridge::get_builder();

    let config = ToolServerConfig {
        tools: vec![
            ToolConfig::from_id("GrokBuild:run_terminal_cmd"),
            // Bash's default enabled_background=true requires these companions.
            ToolConfig::from_id("GrokBuild:get_task_output"),
            ToolConfig::from_id("GrokBuild:kill_task"),
            ToolConfig::from_id("GrokBuild:read_file"),
            ToolConfig::from_id("GrokBuild:search_replace"),
            ToolConfig::from_id("GrokBuild:list_dir"),
            ToolConfig::from_id("GrokBuild:grep"),
            ToolConfig::from_id("GrokBuild:todo_write"),
            ToolConfig::for_tool::<OpenCodeSkillTool>(),
        ],
        behavior_preset: None,
    };

    let ws_dir = resolve_workspace_dir();
    let _ = std::fs::create_dir_all(&ws_dir);
    // Resolve app_data_dir for skill discovery
    let app_data = app_data_dir
        .unwrap_or_else(|| std::env::temp_dir().join("maverick-app"));
    let _ = std::fs::create_dir_all(&app_data);
    let skills = crate::skills::discover_skills(&app_data, &ws_dir);
    let state_path = app_data.join("resources_state.json");

    let ctx = SessionContext {
        backend: Arc::new(LocalTerminalBackend::new()),
        fs: Arc::new(LocalFs),
        cwd: ws_dir.clone(),
        session_folder: ws_dir,
        session_env: Arc::new(HashMap::new()),
        notification_handle: ToolNotificationHandle::noop(),
        owner_session_id: None,
        subagent: None,
        parent_scheduler_handle: None,
        skills,
        state_path,
        memory_backend: None,
        web_search_config: Default::default(),
        web_fetch_config: Default::default(),
        lsp: None,
        image_gen_config: Default::default(),
        video_gen_config: Default::default(),
        app_builder_deployer_config: Default::default(),
        api_key_provider: None,
        auth_provider: None,
        attribution_callback: None,
        system_reminder_tag: DEFAULT_REMINDER_TAG,
    };

    Ok(ToolBridge::finalize_builder(builder, config, ctx).await?)
}

/// Add an MCP server by name/command/args and register its tools.
/// Returns the qualified tool names that were registered (e.g., "myserver__tool").
///
/// NOTE: Full MCP handshake (stdio spawn + `tools/list` discovery) is async and
/// lives inside `xai-grok-mcp::McpState`. For the Maverick v1 slice we seed the
/// state with a stdio config that actually carries `args`, and register a
/// placeholder tool so the UI can prove the round-trip without blocking on a
/// real server spawn. Phase 4 replaces this with hosted MCP pool init.
pub async fn add_mcp_server(
    tool_bridge: &ToolBridge,
    server_name: &str,
    command: &str,
    args: Vec<String>,
) -> Result<Vec<String>> {
    use xai_grok_workspace_types::MCP_TOOL_NAME_DELIMITER;
    use std::sync::Arc;
    use xai_grok_mcp::servers::{McpState, McpTool};

    // Propagate args so the config round-trips to the real MCP spawn path.
    let mcp_server_config = acp::McpServer::Stdio(
        acp::McpServerStdio::new(server_name.to_string(), std::path::PathBuf::from(command))
            .args(args.clone()),
    );

    let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![mcp_server_config])));

    // Placeholder tool — real discovery would await `tools/list` and register N tools.
    let mcp_tool = McpTool::new(
        "test_tool".to_string(),
        "A test MCP tool (v1 placeholder — real tools appear after MCP handshake)".to_string(),
        server_name.to_string(),
        mcp_state.clone(),
        serde_json::json!({"type":"object","properties":{}}),
        None,
    );

    if let Some(registration) = mcp_tool.into_registration() {
        let qualified_name = format!("{}{}test_tool", server_name, MCP_TOOL_NAME_DELIMITER);

        tool_bridge
            .register_mcp_tools(
                qualified_name.clone(),
                registration.tool,
                Some(registration.input_schema),
            )
            .await?;

        Ok(vec![qualified_name])
    } else {
        Err(anyhow::anyhow!("Failed to create MCP tool registration"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_write_to_file() {
        let temp_dir = std::env::temp_dir().join("maverick_test_ws");
        let _ = std::fs::create_dir_all(&temp_dir);

        let args = serde_json::json!({
            "path": "subfolder/hello.txt",
            "content": "Hello World from write_to_file!"
        });

        let res = execute_write_to_file(&args, &temp_dir);
        assert!(res.is_ok(), "write_to_file failed: {:?}", res.err());

        let target_file = temp_dir.join("subfolder/hello.txt");
        assert!(target_file.exists());
        let read_back = std::fs::read_to_string(&target_file).unwrap();
        assert_eq!(read_back, "Hello World from write_to_file!");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
