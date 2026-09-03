mod commands;
mod services;

use services::local_api::{start_local_api_server, LocalApiState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(LocalApiState::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            services::runtime_log::start_session();
            services::global_baselines::start_daily_update_check();
            let local_api_state = app.state::<LocalApiState>().inner().clone();
            if let Err(error) = start_local_api_server(app.handle().clone(), local_api_state) {
                eprintln!("[local-api] {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::accept_ready_check,
            commands::cancel_stats_load,
            commands::check_app_update,
            commands::connection_status,
            commands::copy_png_to_clipboard,
            commands::dismiss_end_of_game,
            commands::get_chat_status,
            commands::get_game_settings_locked,
            commands::load_champions,
            commands::load_current_ranked_stats,
            commands::load_gameflow_phase,
            commands::load_game_assets,
            commands::load_global_baselines,
            commands::set_hex_winrate_recognition_enabled,
            commands::set_hex_winrate_capture_mode,
            commands::set_quick_bench_swap_enabled,
            commands::load_ranked_stats,
            commands::load_lcu_asset,
            commands::load_lcu_assets,
            commands::load_self_stats,
            commands::load_self_stats_with_progress,
            commands::search_summoner_candidates,
            commands::search_player,
            commands::search_player_with_progress,
            commands::load_match_detail,
            commands::load_player_elo_estimate,
            commands::start_player_elo_estimate_task,
            commands::get_player_elo_estimate_task,
            commands::clear_player_elo_cache,
            commands::complete_local_api_request,
            commands::set_local_api_frontend_ready,
            commands::load_live_game,
            commands::load_friends,
            commands::set_chat_availability,
            commands::set_chat_status_message,
            commands::set_game_settings_locked,
            commands::send_alt_left_shortcut
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
