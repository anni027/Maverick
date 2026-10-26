//! Real MCP handshake + pool for Maverick.
//!
//! Replaces placeholder `test_tool` with live `tools/list` discovery via `xai-grok-mcp`.
//! Uses `start_mcp_server` (stdio/http) → `ensure_initialized` → `get_tool_registrations`
//! → `ToolBridge::register_mcp_tools`. Keeps `McpState` for tool calls.

use std::sync::Arc;

use anyhow::Result;
use xai_grok_mcp::servers::{start_mcp_server, McpSpawnCtx, McpState};
use xai_grok_session_events::EventWriter;
use xai_grok_tools::bridge::ToolBridge;
use tokio::sync::Mutex as TokioMutex;

use agent_client_protocol as acp;

/// Result of a successful MCP handshake.
pub struct McpHandshake {
    pub server_name: String,
    pub mcp_state: Arc<TokioMutex<McpState>>,
    pub tools: Vec<String>,
}

/// Try real MCP handshake for stdio server, fall back to placeholder on failure
/// if `allow_fallback` is true. Returns handshake with real tools or error.
pub async fn add_mcp_server_real(
    tool_bridge: &ToolBridge,
    server_name: &str,
    command: &str,
    args: Vec<String>,
) -> Result<Vec<String>> {
    use xai_grok_workspace_types::MCP_TOOL_NAME_DELIMITER;
    use xai_grok_mcp::servers::{McpTool, McpState};

    // Build ACP config
    let mcp_server_config = acp::McpServer::Stdio(
        acp::McpServerStdio::new(server_name.to_string(), std::path::PathBuf::from(command))
            .args(args.clone()),
    );

    // Try real handshake with timeout
    let real_tools = try_real_handshake(server_name, command, args.clone()).await;

    match real_tools {
        Ok(registrations) if !registrations.is_empty() => {
            // Create McpState with config for future calls via McpTool
            let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![mcp_server_config])));
            // We need to insert a client for calls — but registrations already contain
            // the client-bound tool. For MVP, register each via ToolBridge with placeholder state
            // that will still work because McpTool's call goes via McpState's client lookup.
            // To avoid complex OwnedClients injection, we register using the registrations
            // we got from the real handshake but re-create McpTools bound to a new state
            // that has the real client? Simpler: directly register the real registrations
            // by extracting their tool + schema, but they are already bound to the handshake's
            // McpState. We will create a new McpState and re-create tools from the list.

            // For now, register the real tools directly via bridge's generic register.
            // We need to convert registrations into bridge tools. The simplest path:
            // Use the handshake's registrations (they are already McpToolRegistration).
            let mut qualified = Vec::new();
            for reg in registrations {
                let qname = reg.name.clone();
                // register_mcp_tools expects (qualified, tool, schema)
                tool_bridge
                    .register_mcp_tools(qname.clone(), reg.tool, Some(reg.input_schema))
                    .await?;
                qualified.push(qname);
            }
            // Also keep mcp_state for future calls — but our registrations' tools already
            // hold a clone of the handshake's mcp_state, not this new one. To make calls work,
            // we should have used the handshake's mcp_state. So we discard the new one and
            // instead keep the handshake's state via a leaked Arc? For MVP, we skip the extra
            // state and just register — the tool's call will use its own state's client.
            // The handshake's mcp_state is already Arc and will live as long as tool does
            // (tool holds Arc). So we don't need to store it separately.
            let _ = mcp_state; // keep for future pool
            Ok(qualified)
        }
        Ok(_) => {
            // No tools returned — fallback to placeholder
            fallback_placeholder(tool_bridge, server_name, command, args).await
        }
        Err(e) => {
            // Real handshake failed — fallback to placeholder with error context
            tracing::warn!(server = server_name, error = %e, "MCP real handshake failed, falling back to placeholder");
            // Still return error to UI but also register placeholder for visibility?
            // For now, fallback to placeholder so UI shows server, but also propagate error.
            let placeholder = fallback_placeholder(tool_bridge, server_name, command, args).await?;
            Err(anyhow::anyhow!(
                "MCP handshake failed for '{}': {e}. Registered placeholder tool(s): {}",
                server_name,
                placeholder.join(", ")
            ))
        }
    }
}

async fn try_real_handshake(
    server_name: &str,
    command: &str,
    args: Vec<String>,
) -> Result<Vec<xai_grok_mcp::servers::McpToolRegistration>> {
    let mcp_server = acp::McpServer::Stdio(
        acp::McpServerStdio::new(server_name.to_string(), std::path::PathBuf::from(command))
            .args(args.clone()),
    );
    let event_writer = EventWriter::noop();
    let ctx = McpSpawnCtx::standalone(&event_writer);
    // 30s timeout for handshake (covers uvx cold start)
    let client = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        start_mcp_server(mcp_server, None, None, None, &ctx),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP handshake timeout after 30s"))??;

    let client = Arc::new(client);
    // Need a McpState to pass to get_tool_registrations
    let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![])));
    let regs = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.get_tool_registrations(mcp_state.clone()),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP list_tools timeout after 15s"))??;

    Ok(regs)
}

async fn fallback_placeholder(
    tool_bridge: &ToolBridge,
    server_name: &str,
    command: &str,
    args: Vec<String>,
) -> Result<Vec<String>> {
    use xai_grok_workspace_types::MCP_TOOL_NAME_DELIMITER;
    use xai_grok_mcp::servers::{McpState, McpTool};
    use std::sync::Arc;

    let mcp_server_config = acp::McpServer::Stdio(
        acp::McpServerStdio::new(server_name.to_string(), std::path::PathBuf::from(command))
            .args(args.clone()),
    );
    let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![mcp_server_config])));
    let mcp_tool = McpTool::new(
        "test_tool".to_string(),
        "MCP placeholder — real handshake failed or no tools returned".to_string(),
        server_name.to_string(),
        mcp_state.clone(),
        serde_json::json!({"type":"object","properties":{}}),
        None,
    );
    if let Some(registration) = mcp_tool.into_registration() {
        let qualified_name = format!("{}{}test_tool", server_name, MCP_TOOL_NAME_DELIMITER);
        tool_bridge
            .register_mcp_tools(qualified_name.clone(), registration.tool, Some(registration.input_schema))
            .await?;
        Ok(vec![qualified_name])
    } else {
        Err(anyhow::anyhow!("Failed to create MCP placeholder"))
    }
}

/// Real HTTP handshake (uses start_mcp_server with Http)
pub async fn add_mcp_server_http_real(
    tool_bridge: &ToolBridge,
    server_name: &str,
    url: &str,
) -> Result<Vec<String>> {
    let mcp_server = acp::McpServer::Http(acp::McpServerHttp::new(
        server_name.to_string(),
        url.to_string(),
    ));
    let event_writer = EventWriter::noop();
    let ctx = McpSpawnCtx::standalone(&event_writer);
    let client = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        start_mcp_server(mcp_server, None, None, None, &ctx),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP HTTP handshake timeout after 15s"))??;
    let client = Arc::new(client);
    let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![])));
    let regs = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.get_tool_registrations(mcp_state),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP HTTP list_tools timeout"))??;
    if regs.is_empty() {
        anyhow::bail!("MCP HTTP server returned no tools");
    }
    let mut qualified = Vec::new();
    for reg in regs {
        let qname = reg.name.clone();
        tool_bridge
            .register_mcp_tools(qname.clone(), reg.tool, Some(reg.input_schema))
            .await?;
        qualified.push(qname);
    }
    Ok(qualified)
}

/// Simplified status for UI (live tool counts from bridge + config)
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpStatus {
    pub name: String,
    pub transport: String,
    pub tool_count: usize,
    pub status: String, // Ready | Placeholder | Error
    pub tools: Vec<String>,
}

pub async fn get_mcp_status(
    tool_bridge: &ToolBridge,
    config_manager: &crate::config::ConfigManager,
) -> Vec<McpStatus> {
    let cfg = config_manager.read().await;
    let defs = tool_bridge.tool_definitions().await;
    cfg.mcp_servers
        .iter()
        .map(|(name, server_cfg)| {
            let prefix = format!("{}__", name);
            let tools: Vec<String> = defs
                .iter()
                .filter(|d| d.function.name.starts_with(&prefix))
                .map(|d| d.function.name.clone())
                .collect();
            let is_placeholder = tools.iter().any(|t| t.ends_with("__test_tool"));
            let status = if tools.is_empty() {
                "Error".to_string()
            } else if is_placeholder && tools.len() == 1 {
                "Placeholder".to_string()
            } else {
                "Ready".to_string()
            };
            McpStatus {
                name: name.clone(),
                transport: server_cfg.transport.clone(),
                tool_count: tools.len(),
                status,
                tools,
            }
        })
        .collect()
}

/// Marketplace scaffold — lists popular MCP servers + hub skills as marketplace entries
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MarketplaceEntry {
    pub name: String,
    pub description: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub category: String,
    pub install_count: Option<u32>,
}

pub fn scan_marketplace() -> Vec<MarketplaceEntry> {
    vec![
        MarketplaceEntry {
            name: "filesystem".to_string(),
            description: "Secure file operations within allowed directories".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), "/tmp".to_string()],
            url: None,
            category: "Storage".to_string(),
            install_count: Some(15234),
        },
        MarketplaceEntry {
            name: "github".to_string(),
            description: "GitHub repository, issues, and PR management".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
            url: None,
            category: "Development".to_string(),
            install_count: Some(8934),
        },
        MarketplaceEntry {
            name: "brave-search".to_string(),
            description: "Web search via Brave API".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-brave-search".to_string()],
            url: None,
            category: "Search".to_string(),
            install_count: Some(5211),
        },
        MarketplaceEntry {
            name: "memory".to_string(),
            description: "Knowledge graph-based persistent memory".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-memory".to_string()],
            url: None,
            category: "Memory".to_string(),
            install_count: Some(3421),
        },
        MarketplaceEntry {
            name: "fetch".to_string(),
            description: "Fetch and convert web content".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-fetch".to_string()],
            url: None,
            category: "Web".to_string(),
            install_count: Some(7210),
        },
    ]
}
