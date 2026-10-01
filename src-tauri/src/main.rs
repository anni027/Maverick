// Maverick main entry point - Tauri app
fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            use tauri::Manager;
            // Tauri's per-app data directory (e.g. `%APPDATA%\com.maverick.app`
            // on Windows), not the OS temp dir: temp gets swept by the system
            // and would take every session transcript and setting with it.
            // AppState::new is async (builds ToolBridge).
            let app_data_dir = app.path().app_data_dir()?;
            let app_state =
                tauri::async_runtime::block_on(maverick_backend::AppState::new(app_data_dir))?;
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            maverick_backend::commands::init_session,
            maverick_backend::commands::send_message,
            maverick_backend::commands::cancel_message,
            maverick_backend::commands::get_usage,
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
            maverick_backend::commands::get_context_config,
            maverick_backend::commands::set_context_config,
            maverick_backend::commands::get_budget_config,
            maverick_backend::commands::set_budget_config,
            maverick_backend::commands::list_kilo_models,
            maverick_backend::commands::get_model_reasoning,
            maverick_backend::commands::list_skills,
            maverick_backend::commands::install_skill,
            maverick_backend::commands::remove_skill,
            maverick_backend::commands::fetch_hub_skill,
            maverick_backend::commands::list_hub_skills,
            maverick_backend::commands::refresh_skills,
            maverick_backend::commands::search_skills,
            maverick_backend::commands::get_skill_content,
            maverick_backend::commands::list_marketplace_sources,
            maverick_backend::commands::add_marketplace_source,
            maverick_backend::commands::remove_marketplace_source,
            maverick_backend::commands::list_marketplace_skills,
            maverick_backend::commands::search_marketplace_skills,
            maverick_backend::commands::install_marketplace_skill,
            maverick_backend::commands::list_mcp_status,
            maverick_backend::commands::scan_marketplace,
            maverick_backend::commands::list_workspace_files,
            maverick_backend::commands::read_workspace_file,
        ])
        .run(tauri::generate_context!())
        .map_err(anyhow::Error::from)
}
