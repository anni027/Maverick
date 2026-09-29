//! Real MCP handshake + pool for Maverick.
//!
//! Live `tools/list` discovery via `xai-grok-mcp`: `start_mcp_server`
//! (stdio/http) → `ensure_initialized` → `get_tool_registrations` →
//! `ToolBridge::register_mcp_tools`. The `McpState` handed to the handshake owns
//! the spawned client, and every registered tool holds a clone of that state, so
//! later calls resolve. Handshake failures are returned to the caller rather than
//! papered over with a placeholder tool.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex as TokioMutex;
use xai_grok_mcp::servers::{McpSpawnCtx, McpState, start_mcp_server};
use xai_grok_session_events::EventWriter;
use xai_grok_tools::bridge::ToolBridge;

use agent_client_protocol as acp;

/// Result of a successful MCP handshake.
pub struct McpHandshake {
    pub server_name: String,
    pub mcp_state: Arc<TokioMutex<McpState>>,
    pub tools: Vec<String>,
}

/// Run a real handshake for a stdio server and register everything it exposes.
///
/// Returns the qualified tool names on success. A server that cannot be reached
/// or that exposes no tools returns an error instead of registering a stand-in
/// tool: the caller persists the config first, so the UI can still list and
/// remove the server, and `get_mcp_status` reports it as `Error`.
pub async fn add_mcp_server_real(
    tool_bridge: &ToolBridge,
    server_name: &str,
    command: &str,
    args: Vec<String>,
) -> Result<Vec<String>> {
    match try_real_handshake(server_name, command, args).await {
        Ok((registrations, _mcp_state)) if !registrations.is_empty() => {
            // Each registration owns a clone of `_mcp_state`, which in turn owns the
            // live client, so the server stays callable for as long as these tools do.
            let mut qualified = Vec::with_capacity(registrations.len());
            for reg in registrations {
                let qname = reg.name.clone();
                tool_bridge
                    .register_mcp_tools(qname.clone(), reg.tool, Some(reg.input_schema))
                    .await?;
                qualified.push(qname);
            }
            Ok(qualified)
        }
        Ok(_) => Err(anyhow::anyhow!(
            "MCP server '{server_name}' connected but exposed no tools"
        )),
        Err(e) => {
            tracing::warn!(server = server_name, error = %e, "MCP handshake failed");
            Err(anyhow::anyhow!("MCP handshake failed for '{server_name}': {e}"))
        }
    }
}

/// Spawn the server, list its tools, and bind them to a state that owns the client.
///
/// `get_tool_registrations` copies the state reference into every returned tool, and
/// a tool call resolves its target with `McpState::get_client`. Handing it an empty
/// state therefore makes every later call fail with "MCP server 'x' not found", so
/// the live client is installed *before* the tools are created.
async fn try_real_handshake(
    server_name: &str,
    command: &str,
    args: Vec<String>,
) -> Result<(
    Vec<xai_grok_mcp::servers::McpToolRegistration>,
    Arc<TokioMutex<McpState>>,
)> {
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
    let mut state = McpState::new(vec![acp::McpServer::Stdio(
        acp::McpServerStdio::new(server_name.to_string(), std::path::PathBuf::from(command))
            .args(args),
    )]);
    state
        .owned_clients
        .insert(server_name.to_string(), Arc::clone(&client));
    let mcp_state = Arc::new(TokioMutex::new(state));

    let regs = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.get_tool_registrations(mcp_state.clone()),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP list_tools timeout after 15s"))??;

    Ok((regs, mcp_state))
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
    // Same contract as the stdio path: the state handed to `get_tool_registrations`
    // is the state the tools will call through, so it must own the live client.
    let mut state = McpState::new(vec![acp::McpServer::Http(acp::McpServerHttp::new(
        server_name.to_string(),
        url.to_string(),
    ))]);
    state
        .owned_clients
        .insert(server_name.to_string(), Arc::clone(&client));
    let mcp_state = Arc::new(TokioMutex::new(state));
    let regs = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.get_tool_registrations(mcp_state.clone()),
    )
    .await
    .map_err(|_| anyhow::anyhow!("MCP HTTP list_tools timeout"))??;
    if regs.is_empty() {
        anyhow::bail!("MCP HTTP server '{server_name}' returned no tools");
    }
    let mut qualified = Vec::with_capacity(regs.len());
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
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                "/tmp".to_string(),
            ],
            url: None,
            category: "Storage".to_string(),
            install_count: Some(15234),
        },
        MarketplaceEntry {
            name: "github".to_string(),
            description: "GitHub repository, issues, and PR management".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-github".to_string(),
            ],
            url: None,
            category: "Development".to_string(),
            install_count: Some(8934),
        },
        MarketplaceEntry {
            name: "brave-search".to_string(),
            description: "Web search via Brave API".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-brave-search".to_string(),
            ],
            url: None,
            category: "Search".to_string(),
            install_count: Some(5211),
        },
        MarketplaceEntry {
            name: "memory".to_string(),
            description: "Knowledge graph-based persistent memory".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-memory".to_string(),
            ],
            url: None,
            category: "Memory".to_string(),
            install_count: Some(3421),
        },
        MarketplaceEntry {
            name: "fetch".to_string(),
            description: "Fetch and convert web content".to_string(),
            transport: "stdio".to_string(),
            command: Some("npx".to_string()),
            args: vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-fetch".to_string(),
            ],
            url: None,
            category: "Web".to_string(),
            install_count: Some(7210),
        },
    ]
}
