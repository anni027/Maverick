// Nexus main entry point - Tauri app
fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let app_data_dir = std::env::temp_dir().join("nexus-app");
    // AppState::new is async (builds ToolBridge); block on Tauri's runtime.
    let app_state = tauri::async_runtime::block_on(nexus_backend::AppState::new(app_data_dir))?;

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            nexus_backend::commands::init_session,
            nexus_backend::commands::send_message,
            nexus_backend::commands::list_sessions,
            nexus_backend::commands::get_session_messages,
            nexus_backend::commands::list_providers,
            nexus_backend::commands::add_mcp,
            nexus_backend::commands::remove_mcp,
            nexus_backend::commands::list_tools,
            nexus_backend::commands::get_config,
            nexus_backend::commands::set_api_key,
            nexus_backend::commands::remove_api_key,
            nexus_backend::commands::set_provider_settings,
            nexus_backend::commands::get_provider_settings,
            nexus_backend::commands::list_provider_settings,
            nexus_backend::commands::set_default_provider,
            nexus_backend::commands::get_default_provider,
            nexus_backend::commands::add_mcp_server_full,
            nexus_backend::commands::get_ui_config,
            nexus_backend::commands::set_ui_config,
            nexus_backend::commands::list_kilo_models,
        ])
        .run(tauri::generate_context!())
        .map_err(anyhow::Error::from)
}