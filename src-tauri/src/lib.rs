mod commands;
mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::cancel_stats_load,
            commands::connection_status,
            commands::copy_png_to_clipboard,
            commands::load_champions,
            commands::load_current_ranked_stats,
            commands::load_game_assets,
            commands::load_ranked_stats,
            commands::load_lcu_asset,
            commands::load_lcu_assets,
            commands::load_self_stats,
            commands::load_self_stats_with_progress,
            commands::search_player,
            commands::search_player_with_progress,
            commands::load_match_detail,
            commands::load_live_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
