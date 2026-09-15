// Maverick main entry point - Tauri app
fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let app_data_dir = std::env::temp_dir().join("maverick-app");
    // AppState::new is async (builds ToolBridge); block on Tauri's runtime.
    let app_state = tauri::async_runtime::block_on(maverick_backend::AppState::new(app_data_dir))?;

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            maverick_backend::commands::init_session,
            maverick_backend::commands::send_message,
            maverick_backend::commands::list_sessions,
            maverick_backend::commands::delete_session,
            maverick_backend::commands::rename_session,
            maverick_backend::commands::get_session_messages,
            maverick_backend::commands::list_providers,
            maverick_backend::commands::add_mcp,
            maverick_backend::commands::remove_mcp,
            maverick_backend::commands::list_tools,
            maverick_backend::commands::get_config,
            maverick_backend::commands::set_api_key,
            maverick_backend::commands::remove_api_key,
            maverick_backend::commands::set_provider_settings,
            maverick_backend::commands::get_provider_settings,
            maverick_backend::commands::list_provider_settings,
            maverick_backend::commands::set_default_provider,
            maverick_backend::commands::get_default_provider,
            maverick_backend::commands::add_mcp_server_full,
            maverick_backend::commands::get_ui_config,
            maverick_backend::commands::set_ui_config,
            maverick_backend::commands::list_kilo_models,
        ])
        .run(tauri::generate_context!())
        .map_err(anyhow::Error::from)
}