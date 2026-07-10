use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter};

use crate::services::client_discovery::discover_primary_client;
use crate::services::error::{AppError, AppResult};
use crate::services::lcu::{LcuClient, RiotClient};
use crate::services::models::{
    ChampSelectPlayer, ChampionSummaryItem, ClientAuth, ConnectionStatus, GameAssetBundle,
    GameflowPlayer, GameflowSession, LiveGameResponse, LivePlayer, LivePremadeMarker, LiveTeam,
    MatchDetailResponse, PlayerStatsResponse, PlayerSummary, RankedQueueEntry, RankedStatsResponse,
    RecentGame, SafeClientInfo, SummonerInfo, SummonerSearchCandidate,
};
use crate::services::stats::{
    load_match_detail as load_match_detail_service, load_player_stats,
    load_player_stats_with_progress, normalize_depth,
};

const EMPTY_PUUID: &str = "00000000-0000-0000-0000-000000000000";
const STATS_PROGRESS_EVENT: &str = "stats-load-progress";
const LIVE_QUERY_CONCURRENCY: usize = 4;
const LIVE_SCAN_BATCH_DEPTH: usize = 20;
const LIVE_MAX_SCAN_DEPTH: usize = 150;
const LIVE_MATCH_LIMIT: usize = 50;
const LIVE_MIN_VALID_GAME_DURATION_SECONDS: i64 = 8 * 60;
const PREMADE_HISTORY_THRESHOLD: usize = 3;
const PREMADE_GROUP_IDS: [&str; 10] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];
const UPDATE_MANIFEST_URL: &str = "https://gitee.com/Crescenre/lol-stats/raw/main/update.json";
const UPDATE_DOWNLOAD_PAGE_URL: &str = "https://crescendum.lanzout.com/b00rp145sh";
const ALL_SEARCH_SERVER_ID: &str = "ALL";
const ALL_SEARCH_SGP_SERVER_IDS: [&str; 8] = [
    "TENCENT_HN1",
    "TENCENT_HN10",
    "TENCENT_NJ100",
    "TENCENT_GZ100",
    "TENCENT_CQ100",
    "TENCENT_TJ100",
    "TENCENT_TJ101",
    "TENCENT_BGP2",
];

static CANCELLED_STATS_LOADS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsLoadProgress {
    request_id: String,
    loaded: usize,
    total: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    current_version: String,
    latest_version: String,
    has_update: bool,
    release_page_url: String,
    release_name: Option<String>,
    release_message: Option<String>,
    release_notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    version: String,
    download_url: String,
    title: Option<String>,
    message: Option<String>,
    notes: Option<Vec<String>>,
    changelog: Option<String>,
}

struct Clients {
    auth: ClientAuth,
    lcu: LcuClient,
    riot: Option<RiotClient>,
}

fn app_error(error: AppError) -> String {
    error.to_string()
}

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn send_alt_left_shortcut() -> Result<(), String> {
    send_alt_left_shortcut_impl()
}

#[cfg(target_os = "windows")]
fn send_alt_left_shortcut_impl() -> Result<(), String> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_LEFT, VK_MENU,
    };

    fn key_input(vk: u16, key_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let mut inputs = [
        key_input(VK_MENU, false),
        key_input(VK_LEFT, false),
        key_input(VK_LEFT, true),
        key_input(VK_MENU, true),
    ];

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_mut_ptr(),
            size_of::<INPUT>() as i32,
        )
    };

    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(format!("系统只发送了 {sent}/{} 个输入事件", inputs.len()))
    }
}

#[cfg(not(target_os = "windows"))]
fn send_alt_left_shortcut_impl() -> Result<(), String> {
    Err("当前平台不支持系统级 Alt + 左方向键".to_string())
}

fn clear_cancelled_request(request_id: &str) {
    if let Ok(mut requests) = CANCELLED_STATS_LOADS.lock() {
        requests.remove(request_id);
    }
}

fn is_cancelled_request(request_id: &str) -> bool {
    CANCELLED_STATS_LOADS
        .lock()
        .map(|requests| requests.contains(request_id))
        .unwrap_or(false)
}

fn emit_stats_progress(app: &AppHandle, request_id: &str, loaded: usize, total: usize) {
    let _ = app.emit(
        STATS_PROGRESS_EVENT,
        StatsLoadProgress {
            request_id: request_id.to_string(),
            loaded,
            total,
        },
    );
}

fn is_real_puuid(puuid: &str) -> bool {
    !puuid.trim().is_empty() && puuid != EMPTY_PUUID
}

fn is_probably_puuid(input: &str) -> bool {
    let parts = input.split('-').collect::<Vec<_>>();
    parts.len() == 5 && input.len() >= 32
}

fn is_ranked_stats_empty(stats: &RankedStatsResponse) -> bool {
    is_ranked_queue_empty(&stats.queue_map.ranked_solo_5x5)
        && is_ranked_queue_empty(&stats.queue_map.ranked_flex_sr)
}

fn is_ranked_queue_empty(entry: &RankedQueueEntry) -> bool {
    ranked_text_is_empty(&entry.tier)
        && ranked_text_is_empty(&entry.highest_tier)
        && ranked_text_is_empty(&entry.previous_season_highest_tier)
        && ranked_text_is_empty(&entry.previous_season_end_tier)
        && entry.league_points == 0
        && entry.wins == 0
        && entry.losses == 0
        && !entry.is_provisional
}

fn ranked_text_is_empty(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "" | "NONE" | "NA"
    )
}

fn normalize_version_tag(version: &str) -> String {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .to_string()
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let latest_parts = version_parts(latest);
    let current_parts = version_parts(current);

    for index in 0..latest_parts.len().max(current_parts.len()) {
        let latest_part = *latest_parts.get(index).unwrap_or(&0);
        let current_part = *current_parts.get(index).unwrap_or(&0);
        if latest_part != current_part {
            return latest_part > current_part;
        }
    }

    false
}

fn version_parts(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

fn normalize_release_notes(notes: Option<Vec<String>>, changelog: Option<String>) -> Vec<String> {
    if let Some(notes) = notes {
        let notes = notes
            .into_iter()
            .map(|note| note.trim().to_string())
            .filter(|note| !note.is_empty())
            .collect::<Vec<_>>();

        if !notes.is_empty() {
            return notes;
        }
    }

    changelog
        .unwrap_or_default()
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(['-', '*', '•'])
                .trim()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn create_clients() -> AppResult<Clients> {
    let auth = discover_primary_client()?;
    let lcu = LcuClient::new(&auth)?;
    let riot = RiotClient::new(&auth)?;
    Ok(Clients { auth, lcu, riot })
}

#[tauri::command]
pub async fn connection_status() -> Result<ConnectionStatus, String> {
    let clients = match create_clients() {
        Ok(clients) => clients,
        Err(error) => {
            return Ok(ConnectionStatus {
                connected: false,
                client: None,
                summoner: None,
                gameflow_phase: None,
                message: error.to_string(),
            })
        }
    };

    if let Err(error) = clients.lcu.ping().await {
        return Ok(ConnectionStatus {
            connected: false,
            client: Some(SafeClientInfo::from(&clients.auth)),
            summoner: None,
            gameflow_phase: None,
            message: error.to_string(),
        });
    }

    let summoner = clients.lcu.current_summoner().await.ok();
    let phase = clients.lcu.gameflow_phase().await.ok();

    Ok(ConnectionStatus {
        connected: true,
        client: Some(SafeClientInfo::from(&clients.auth)),
        summoner,
        gameflow_phase: phase,
        message: "已连接国服客户端".to_string(),
    })
}

#[tauri::command]
pub async fn check_app_update() -> Result<AppUpdateInfo, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| app_error(AppError::Http(error)))?;

    let fallback = AppUpdateInfo {
        current_version: current_version.clone(),
        latest_version: current_version.clone(),
        has_update: false,
        release_page_url: UPDATE_DOWNLOAD_PAGE_URL.to_string(),
        release_name: None,
        release_message: None,
        release_notes: Vec::new(),
    };

    let Ok(response) = http
        .get(UPDATE_MANIFEST_URL)
        .header("User-Agent", "lol-stats")
        .send()
        .await
    else {
        return Ok(fallback);
    };

    let Ok(response) = response.error_for_status() else {
        return Ok(fallback);
    };

    let Ok(manifest) = response.json::<UpdateManifest>().await else {
        return Ok(fallback);
    };

    let latest_version = normalize_version_tag(&manifest.version);
    if latest_version.is_empty() {
        return Ok(fallback);
    }

    let release_page_url = if manifest.download_url.trim().is_empty() {
        UPDATE_DOWNLOAD_PAGE_URL.to_string()
    } else {
        manifest.download_url
    };
    let release_notes = normalize_release_notes(manifest.notes, manifest.changelog);

    Ok(AppUpdateInfo {
        has_update: version_is_newer(&latest_version, &current_version),
        current_version,
        latest_version,
        release_page_url,
        release_name: manifest.title,
        release_message: manifest.message,
        release_notes,
    })
}

#[tauri::command]
pub async fn load_self_stats(
    depth: usize,
    force_refresh: Option<bool>,
    persist_cache: Option<bool>,
) -> Result<PlayerStatsResponse, String> {
    let clients = create_clients().map_err(app_error)?;
    let summoner = clients.lcu.current_summoner().await.map_err(app_error)?;
    load_player_stats_with_progress(
        &clients.lcu,
        Some(&clients.auth),
        None,
        summoner,
        normalize_depth(depth),
        force_refresh.unwrap_or(false),
        persist_cache.unwrap_or(true),
        |_, _| true,
    )
    .await
    .map_err(app_error)
}

#[tauri::command]
pub async fn load_self_stats_with_progress(
    app: AppHandle,
    depth: usize,
    request_id: String,
    force_refresh: Option<bool>,
    persist_cache: Option<bool>,
) -> Result<PlayerStatsResponse, String> {
    clear_cancelled_request(&request_id);
    let clients = create_clients().map_err(app_error)?;
    let summoner = clients.lcu.current_summoner().await.map_err(app_error)?;
    let depth = normalize_depth(depth);
    let progress_app = app.clone();
    let progress_request_id = request_id.clone();

    let result = load_player_stats_with_progress(
        &clients.lcu,
        Some(&clients.auth),
        None,
        summoner,
        depth,
        force_refresh.unwrap_or(false),
        persist_cache.unwrap_or(true),
        move |loaded, total| {
            emit_stats_progress(&progress_app, &progress_request_id, loaded, total);
            !is_cancelled_request(&progress_request_id)
        },
    )
    .await
    .map_err(app_error);

    clear_cancelled_request(&request_id);
    result
}

#[tauri::command]
pub async fn load_champions() -> Result<Vec<ChampionSummaryItem>, String> {
    let clients = create_clients().map_err(app_error)?;
    clients.lcu.champion_summary().await.map_err(app_error)
}

#[tauri::command]
pub async fn load_game_assets() -> Result<GameAssetBundle, String> {
    let clients = create_clients().map_err(app_error)?;
    clients.lcu.game_assets().await.map_err(app_error)
}

#[tauri::command]
pub async fn load_current_ranked_stats() -> Result<RankedStatsResponse, String> {
    let clients = create_clients().map_err(app_error)?;
    let summoner = clients.lcu.current_summoner().await.map_err(app_error)?;
    clients
        .lcu
        .ranked_stats(&summoner.puuid)
        .await
        .map_err(app_error)
}

#[tauri::command]
pub async fn load_gameflow_phase() -> Result<Option<String>, String> {
    let Ok(clients) = create_clients() else {
        return Ok(None);
    };

    Ok(clients.lcu.gameflow_phase().await.ok())
}

#[tauri::command]
pub async fn load_ranked_stats(
    puuid: String,
    sgp_server_id: Option<String>,
) -> Result<RankedStatsResponse, String> {
    let clients = create_clients().map_err(app_error)?;
    let normalized_sgp_server_id = sgp_server_id
        .as_deref()
        .map(crate::services::sgp::normalize_sgp_server_id);
    let default_sgp_server_id = crate::services::sgp::default_sgp_server_id(&clients.auth);
    let can_fallback_to_lcu = normalized_sgp_server_id
        .as_deref()
        .unwrap_or(default_sgp_server_id.as_str())
        .eq_ignore_ascii_case(&default_sgp_server_id);
    let sgp = crate::services::sgp::SgpClient::new(
        &clients.lcu,
        &clients.auth,
        normalized_sgp_server_id.as_deref(),
    )
    .await
    .map_err(app_error)?;
    let puuid = puuid.trim().to_string();

    match sgp.ranked_stats(&puuid).await {
        Ok(ranked) if can_fallback_to_lcu && is_ranked_stats_empty(&ranked) => {
            clients.lcu.ranked_stats(&puuid).await.map_err(app_error)
        }
        Ok(ranked) => Ok(ranked),
        Err(error) if can_fallback_to_lcu => clients
            .lcu
            .ranked_stats(&puuid)
            .await
            .map_err(|_| app_error(error)),
        Err(error) => Err(app_error(error)),
    }
}

#[tauri::command]
pub async fn load_lcu_asset(path: String) -> Result<String, String> {
    let clients = create_clients().map_err(app_error)?;
    clients.lcu.asset_data_url(&path).await.map_err(app_error)
}

#[tauri::command]
pub async fn load_lcu_assets(
    paths: Vec<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let clients = create_clients().map_err(app_error)?;
    clients.lcu.asset_data_urls(paths).await.map_err(app_error)
}

#[tauri::command]
pub async fn search_summoner_candidates(
    query: String,
    sgp_server_id: Option<String>,
) -> Result<Vec<SummonerSearchCandidate>, String> {
    let clients = create_clients().map_err(app_error)?;
    let query = normalize_query_text(&query);
    let query = query.as_str();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let riot = clients.riot.as_ref().ok_or_else(|| {
        app_error(AppError::RiotClientUnavailable(
            "Riot Client 未连接，无法搜索召唤师候选".to_string(),
        ))
    })?;
    let (game_name, tag_line) = query
        .split_once('#')
        .map(|(name, tag)| (name.trim(), Some(tag.trim())))
        .unwrap_or((query, None));
    let target_name = normalize_search_name(game_name);
    let target_tag = tag_line.map(normalize_search_name);
    let raw_sgp_server_id = sgp_server_id
        .as_deref()
        .map(crate::services::sgp::normalize_sgp_server_id);
    let search_all_servers = raw_sgp_server_id
        .as_deref()
        .is_some_and(|id| id.eq_ignore_ascii_case(ALL_SEARCH_SERVER_ID));
    let target_server_ids = if search_all_servers {
        ALL_SEARCH_SGP_SERVER_IDS
            .iter()
            .map(|server_id| (*server_id).to_string())
            .collect::<Vec<_>>()
    } else {
        vec![crate::services::sgp::resolve_sgp_server_id(
            &clients.auth,
            raw_sgp_server_id.as_deref(),
        )]
    };

    let mut aliases = riot
        .lookup_alias(game_name, tag_line)
        .await
        .map_err(app_error)?;
    aliases.retain(|alias| {
        normalize_search_name(&alias.alias.game_name) == target_name
            && target_tag
                .as_ref()
                .map(|tag| normalize_search_name(&alias.alias.tag_line) == *tag)
                .unwrap_or(true)
            && is_real_puuid(&alias.puuid)
    });

    let mut seen = HashSet::new();
    aliases.retain(|alias| seen.insert(normalize_puuid(&alias.puuid)));
    if aliases.is_empty() {
        return Ok(Vec::new());
    }

    let puuids = aliases
        .iter()
        .map(|alias| alias.puuid.clone())
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for server_id in target_server_ids {
        let sgp = match crate::services::sgp::SgpClient::new(
            &clients.lcu,
            &clients.auth,
            Some(&server_id),
        )
        .await
        {
            Ok(sgp) => sgp,
            Err(_) if search_all_servers => continue,
            Err(error) => return Err(app_error(error)),
        };

        let summoners = match sgp.summoners_by_puuids(&puuids).await {
            Ok(summoners) => summoners,
            Err(_) if search_all_servers => continue,
            Err(error) => return Err(app_error(error)),
        };
        let summoner_map = summoners
            .into_iter()
            .map(|summoner| (normalize_puuid(&summoner.puuid), summoner))
            .collect::<HashMap<_, _>>();

        for alias in &aliases {
            let Some(summoner) = summoner_map.get(&normalize_puuid(&alias.puuid)) else {
                continue;
            };

            candidates.push(SummonerSearchCandidate {
                puuid: alias.puuid.clone(),
                game_name: alias.alias.game_name.clone(),
                tag_line: alias.alias.tag_line.clone(),
                sgp_server_id: server_id.clone(),
                profile_icon_id: summoner.profile_icon_id,
                summoner_level: summoner.summoner_level,
                privacy: summoner.privacy.clone(),
            });
        }
    }

    Ok(candidates)
}

#[tauri::command]
pub async fn search_player(
    query: String,
    depth: usize,
    sgp_server_id: Option<String>,
    force_refresh: Option<bool>,
    persist_cache: Option<bool>,
) -> Result<PlayerStatsResponse, String> {
    let clients = create_clients().map_err(app_error)?;
    let summoner = resolve_summoner(&clients, query.trim())
        .await
        .map_err(app_error)?;

    load_player_stats_with_progress(
        &clients.lcu,
        Some(&clients.auth),
        sgp_server_id.as_deref(),
        summoner,
        normalize_depth(depth),
        force_refresh.unwrap_or(false),
        persist_cache.unwrap_or(true),
        |_, _| true,
    )
    .await
    .map_err(app_error)
}

#[tauri::command]
pub async fn search_player_with_progress(
    app: AppHandle,
    query: String,
    depth: usize,
    request_id: String,
    sgp_server_id: Option<String>,
    force_refresh: Option<bool>,
    persist_cache: Option<bool>,
) -> Result<PlayerStatsResponse, String> {
    clear_cancelled_request(&request_id);
    let clients = create_clients().map_err(app_error)?;
    let summoner = resolve_summoner(&clients, query.trim())
        .await
        .map_err(app_error)?;
    let depth = normalize_depth(depth);
    let progress_app = app.clone();
    let progress_request_id = request_id.clone();

    let result = load_player_stats_with_progress(
        &clients.lcu,
        Some(&clients.auth),
        sgp_server_id.as_deref(),
        summoner,
        depth,
        force_refresh.unwrap_or(false),
        persist_cache.unwrap_or(true),
        move |loaded, total| {
            emit_stats_progress(&progress_app, &progress_request_id, loaded, total);
            !is_cancelled_request(&progress_request_id)
        },
    )
    .await
    .map_err(app_error);

    clear_cancelled_request(&request_id);
    result
}

#[tauri::command]
pub fn cancel_stats_load(request_id: String) {
    if let Ok(mut requests) = CANCELLED_STATS_LOADS.lock() {
        requests.insert(request_id);
    }
}

#[tauri::command]
pub fn copy_png_to_clipboard(bytes: Vec<u8>) -> Result<(), String> {
    let image = image::load_from_memory(&bytes)
        .map_err(|error| format!("图片解码失败：{error}"))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    let data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: Cow::Owned(image.into_raw()),
    };

    arboard::Clipboard::new()
        .map_err(|error| format!("无法打开系统剪切板：{error}"))?
        .set_image(data)
        .map_err(|error| format!("无法写入系统剪切板：{error}"))
}

#[tauri::command]
pub async fn load_match_detail(
    game_id: u64,
    sgp_server_id: Option<String>,
) -> Result<MatchDetailResponse, String> {
    let clients = create_clients().map_err(app_error)?;
    load_match_detail_service(
        &clients.lcu,
        Some(&clients.auth),
        sgp_server_id.as_deref(),
        game_id,
    )
    .await
    .map_err(app_error)
}

#[tauri::command]
pub async fn load_live_game(depth: usize) -> Result<LiveGameResponse, String> {
    let clients = create_clients().map_err(app_error)?;
    let display_depth = normalize_depth(depth).min(LIVE_MATCH_LIMIT);
    let Some(session) = clients.lcu.gameflow_session().await.map_err(app_error)? else {
        return Err(app_error(AppError::LiveGameUnavailable));
    };

    let phase = session.phase.clone();
    let queue_id = Some(session.game_data.queue.id).filter(|id| *id != 0);
    let queue_type = Some(session.game_data.queue.type_.clone()).filter(|value| !value.is_empty());
    let game_mode =
        Some(session.game_data.queue.game_mode.clone()).filter(|value| !value.is_empty());
    let game_id = Some(session.game_data.game_id).filter(|id| *id != 0);
    let queue_filter = live_queue_filter(queue_id, queue_type.as_deref(), game_mode.as_deref());
    let current_puuid = clients
        .lcu
        .current_summoner()
        .await
        .ok()
        .map(|summoner| summoner.puuid);

    if phase == "ChampSelect" {
        let team_participant_map = gameflow_team_participant_map(&session);
        let Some(session) = clients
            .lcu
            .champ_select_session()
            .await
            .map_err(app_error)?
        else {
            return Err(app_error(AppError::LiveGameUnavailable));
        };

        let our_players = session
            .my_team
            .into_iter()
            .filter(|p| is_real_puuid(&p.puuid))
            .map(|player| champ_select_seed(player, &team_participant_map))
            .collect::<Vec<_>>();
        let their_players = session
            .their_team
            .into_iter()
            .filter(|p| is_real_puuid(&p.puuid))
            .map(|player| champ_select_seed(player, &team_participant_map))
            .collect::<Vec<_>>();

        let mut teams = if queue_type.as_deref() == Some("CHERRY") {
            vec![LiveTeam {
                name: "全部玩家".to_string(),
                players: load_live_players(
                    &clients.lcu,
                    &clients.auth,
                    our_players
                        .into_iter()
                        .chain(their_players.into_iter())
                        .collect(),
                    display_depth,
                    queue_filter,
                )
                .await,
            }]
        } else {
            vec![
                LiveTeam {
                    name: "我方".to_string(),
                    players: load_live_players(
                        &clients.lcu,
                        &clients.auth,
                        our_players,
                        display_depth,
                        queue_filter,
                    )
                    .await,
                },
                LiveTeam {
                    name: "敌方".to_string(),
                    players: load_live_players(
                        &clients.lcu,
                        &clients.auth,
                        their_players,
                        display_depth,
                        queue_filter,
                    )
                    .await,
                },
            ]
        };
        assign_premade_markers(&mut teams);

        return Ok(LiveGameResponse {
            phase,
            query_stage: "champ-select".to_string(),
            game_id,
            queue_id,
            queue_type,
            game_mode,
            teams,
        });
    }

    if matches!(
        phase.as_str(),
        "GameStart" | "InProgress" | "Reconnect" | "WaitingForStats" | "PreEndOfGame" | "EndOfGame"
    ) {
        let selection_map = champion_selection_map(&session);
        let real_players = session
            .game_data
            .player_champion_selections
            .iter()
            .filter(|selection| is_real_puuid(&selection.puuid))
            .map(|selection| selection.puuid.clone())
            .collect::<HashSet<_>>();
        let mut blue_players = gameflow_seeds(session.game_data.team_one, &selection_map);
        let mut red_players = gameflow_seeds(session.game_data.team_two, &selection_map);

        if queue_type.as_deref() == Some("CHERRY") && !real_players.is_empty() {
            blue_players.retain(|player| real_players.contains(&player.puuid));
            red_players.retain(|player| real_players.contains(&player.puuid));
        }

        let mut teams = if queue_type.as_deref() == Some("CHERRY") {
            vec![LiveTeam {
                name: "全部玩家".to_string(),
                players: load_live_players(
                    &clients.lcu,
                    &clients.auth,
                    blue_players
                        .into_iter()
                        .chain(red_players.into_iter())
                        .collect(),
                    display_depth,
                    queue_filter,
                )
                .await,
            }]
        } else {
            let current_on_red = current_puuid
                .as_deref()
                .map(|puuid| player_list_contains_puuid(&red_players, puuid))
                .unwrap_or(false);
            let current_on_blue = current_puuid
                .as_deref()
                .map(|puuid| player_list_contains_puuid(&blue_players, puuid))
                .unwrap_or(false);
            let (our_players, their_players, our_name, their_name) = if current_on_red {
                (red_players, blue_players, "我方", "敌方")
            } else if current_on_blue {
                (blue_players, red_players, "我方", "敌方")
            } else {
                (blue_players, red_players, "蓝方", "红方")
            };

            vec![
                LiveTeam {
                    name: our_name.to_string(),
                    players: load_live_players(
                        &clients.lcu,
                        &clients.auth,
                        our_players,
                        display_depth,
                        queue_filter,
                    )
                    .await,
                },
                LiveTeam {
                    name: their_name.to_string(),
                    players: load_live_players(
                        &clients.lcu,
                        &clients.auth,
                        their_players,
                        display_depth,
                        queue_filter,
                    )
                    .await,
                },
            ]
        };
        assign_premade_markers(&mut teams);

        return Ok(LiveGameResponse {
            phase,
            query_stage: "in-game".to_string(),
            game_id,
            queue_id,
            queue_type,
            game_mode,
            teams,
        });
    }

    Err(app_error(AppError::LiveGameUnavailable))
}

async fn resolve_summoner(clients: &Clients, query: &str) -> AppResult<SummonerInfo> {
    let query = normalize_query_text(query);
    let query = query.as_str();
    if query.is_empty() {
        return Err(AppError::PlayerNotFound("搜索内容为空".to_string()));
    }

    if is_probably_puuid(query) {
        return clients.lcu.summoner_by_puuid(query).await.or_else(|_| {
            Ok(SummonerInfo {
                display_name: query.to_string(),
                puuid: query.to_string(),
                ..Default::default()
            })
        });
    }

    if let Some((game_name, tag_line)) = query.split_once('#') {
        if let Some(riot) = &clients.riot {
            let aliases = riot
                .lookup_alias(game_name.trim(), Some(tag_line.trim()))
                .await?;

            if let Some(alias) = aliases.first() {
                return clients
                    .lcu
                    .summoner_by_puuid(&alias.puuid)
                    .await
                    .or_else(|_| Ok(alias_to_summoner(alias)));
            }
        }

        return clients.lcu.summoner_by_name(game_name.trim()).await;
    }

    if let Some(riot) = &clients.riot {
        let aliases = riot.lookup_alias(query, None).await?;
        if let Some(alias) = aliases.first() {
            return clients
                .lcu
                .summoner_by_puuid(&alias.puuid)
                .await
                .or_else(|_| Ok(alias_to_summoner(alias)));
        }
    }

    clients.lcu.summoner_by_name(query).await
}

fn alias_to_summoner(alias: &crate::services::models::RiotAlias) -> SummonerInfo {
    SummonerInfo {
        puuid: alias.puuid.clone(),
        game_name: alias.alias.game_name.clone(),
        tag_line: alias.alias.tag_line.clone(),
        ..Default::default()
    }
}

#[derive(Clone)]
struct LivePlayerSeed {
    puuid: String,
    champion_id: u32,
    champion_pick_intent: u32,
    position: String,
    selected_role: String,
    summoner_id: u64,
    team_participant_id: Option<u32>,
    fallback_name: String,
    is_placeholder: bool,
}

fn champ_select_seed(
    player: ChampSelectPlayer,
    team_participant_map: &HashMap<String, u32>,
) -> LivePlayerSeed {
    let champion_id = if player.champion_id != 0 {
        player.champion_id
    } else {
        player.champion_pick_intent
    };
    let team_participant_id = team_participant_map
        .get(&normalize_puuid(&player.puuid))
        .copied()
        .filter(|id| *id != 0);

    LivePlayerSeed {
        puuid: player.puuid,
        champion_id,
        champion_pick_intent: player.champion_pick_intent,
        position: player.assigned_position.to_ascii_uppercase(),
        selected_role: String::new(),
        summoner_id: player.summoner_id,
        team_participant_id,
        fallback_name: String::new(),
        is_placeholder: player.player_type.eq_ignore_ascii_case("BOT")
            || player.name_visibility_type.eq_ignore_ascii_case("HIDDEN"),
    }
}

fn gameflow_team_participant_map(session: &GameflowSession) -> HashMap<String, u32> {
    session
        .game_data
        .team_one
        .iter()
        .chain(session.game_data.team_two.iter())
        .filter(|player| is_real_puuid(&player.puuid) && player.team_participant_id != 0)
        .map(|player| (normalize_puuid(&player.puuid), player.team_participant_id))
        .collect()
}

fn champion_selection_map(session: &GameflowSession) -> HashMap<String, u32> {
    let mut selections = session
        .game_data
        .player_champion_selections
        .iter()
        .filter(|selection| is_real_puuid(&selection.puuid) && selection.champion_id != 0)
        .map(|selection| (selection.puuid.clone(), selection.champion_id))
        .collect::<HashMap<_, _>>();

    // teamOne/teamTwo 在国服游戏中阶段有时比 playerChampionSelections 更新得更快。
    for player in session
        .game_data
        .team_one
        .iter()
        .chain(session.game_data.team_two.iter())
    {
        if is_real_puuid(&player.puuid) && player.champion_id != 0 {
            selections.insert(player.puuid.clone(), player.champion_id);
        }
    }

    selections
}

fn gameflow_seeds(
    players: Vec<GameflowPlayer>,
    selections: &HashMap<String, u32>,
) -> Vec<LivePlayerSeed> {
    players
        .into_iter()
        .filter(|player| is_real_puuid(&player.puuid))
        .map(|player| {
            let champion_id = if player.champion_id != 0 {
                player.champion_id
            } else {
                selections.get(&player.puuid).copied().unwrap_or_default()
            };

            LivePlayerSeed {
                puuid: player.puuid,
                champion_id,
                champion_pick_intent: 0,
                position: player.selected_position,
                selected_role: player.selected_role,
                summoner_id: player.summoner_id,
                team_participant_id: Some(player.team_participant_id).filter(|id| *id != 0),
                fallback_name: if player.summoner_name.is_empty() {
                    player.summoner_internal_name
                } else {
                    player.summoner_name
                },
                is_placeholder: false,
            }
        })
        .collect()
}

fn player_list_contains_puuid(players: &[LivePlayerSeed], puuid: &str) -> bool {
    players
        .iter()
        .any(|player| player.puuid.eq_ignore_ascii_case(puuid))
}

#[derive(Clone)]
struct PremadeCandidate {
    puuids: Vec<String>,
    source: &'static str,
    together_times: usize,
}

fn assign_premade_markers(teams: &mut [LiveTeam]) {
    let mut marked = HashSet::<String>::new();
    let mut group_index = 0usize;

    for team in teams.iter_mut() {
        for group in explicit_premade_groups(team) {
            if assign_premade_group(team, group, &mut marked, &mut group_index) {
                continue;
            }
        }
    }

    for team in teams.iter_mut() {
        for group in inferred_premade_groups(team, &marked) {
            let _ = assign_premade_group(team, group, &mut marked, &mut group_index);
        }
    }
}

fn explicit_premade_groups(team: &LiveTeam) -> Vec<PremadeCandidate> {
    let mut groups = BTreeMap::<u32, Vec<String>>::new();

    for player in &team.players {
        let Some(team_participant_id) = player.team_participant_id else {
            continue;
        };

        if !is_real_puuid(&player.puuid) {
            continue;
        }

        groups
            .entry(team_participant_id)
            .or_default()
            .push(normalize_puuid(&player.puuid));
    }

    groups
        .into_values()
        .filter(|puuids| puuids.len() >= 2)
        .map(|mut puuids| {
            puuids.sort();
            puuids.dedup();
            PremadeCandidate {
                puuids,
                source: "客户端分组",
                together_times: 0,
            }
        })
        .collect()
}

fn inferred_premade_groups(team: &LiveTeam, marked: &HashSet<String>) -> Vec<PremadeCandidate> {
    let current_players = team
        .players
        .iter()
        .filter(|player| player.premade.is_none() && is_real_puuid(&player.puuid))
        .map(|player| normalize_puuid(&player.puuid))
        .collect::<BTreeSet<_>>();

    if current_players.len() < 2 || current_players.len() > 10 {
        return Vec::new();
    }

    let current_players = current_players
        .into_iter()
        .filter(|puuid| !marked.contains(puuid))
        .collect::<Vec<_>>();

    if current_players.len() < 2 {
        return Vec::new();
    }

    let current_set = current_players.iter().cloned().collect::<HashSet<_>>();
    let mut pair_games = HashMap::<(String, String), HashSet<u64>>::new();

    for player in &team.players {
        let Some(stats) = &player.stats else {
            continue;
        };

        for game in &stats.recent_games {
            let same_side = game
                .team_puuids
                .iter()
                .map(|puuid| normalize_puuid(puuid))
                .filter(|puuid| current_set.contains(puuid) && !marked.contains(puuid))
                .collect::<BTreeSet<_>>();

            if same_side.len() < 2 {
                continue;
            }

            let same_side = same_side.into_iter().collect::<Vec<_>>();
            for i in 0..same_side.len().saturating_sub(1) {
                for j in (i + 1)..same_side.len() {
                    pair_games
                        .entry((same_side[i].clone(), same_side[j].clone()))
                        .or_default()
                        .insert(game.game_id);
                }
            }
        }
    }

    let mut candidates = Vec::<PremadeCandidate>::new();
    let total_masks = 1usize << current_players.len();
    for mask in 0..total_masks {
        if mask.count_ones() < 2 {
            continue;
        }

        let puuids = current_players
            .iter()
            .enumerate()
            .filter_map(|(index, puuid)| ((mask & (1usize << index)) != 0).then_some(puuid.clone()))
            .collect::<Vec<_>>();

        let Some(times) = premade_subset_times(&puuids, &pair_games) else {
            continue;
        };

        if times >= PREMADE_HISTORY_THRESHOLD {
            candidates.push(PremadeCandidate {
                puuids,
                source: "历史同队",
                together_times: times,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.puuids
            .len()
            .cmp(&a.puuids.len())
            .then_with(|| b.together_times.cmp(&a.together_times))
            .then_with(|| a.puuids.cmp(&b.puuids))
    });

    let mut selected = Vec::<PremadeCandidate>::new();
    let mut used = HashSet::<String>::new();
    for candidate in candidates {
        if candidate.puuids.iter().any(|puuid| used.contains(puuid)) {
            continue;
        }

        for puuid in &candidate.puuids {
            used.insert(puuid.clone());
        }
        selected.push(candidate);
    }

    selected
}

fn premade_subset_times(
    puuids: &[String],
    pair_games: &HashMap<(String, String), HashSet<u64>>,
) -> Option<usize> {
    let mut min_times = usize::MAX;

    for i in 0..puuids.len().saturating_sub(1) {
        for j in (i + 1)..puuids.len() {
            let key = if puuids[i] <= puuids[j] {
                (puuids[i].clone(), puuids[j].clone())
            } else {
                (puuids[j].clone(), puuids[i].clone())
            };
            let times = pair_games.get(&key)?.len();
            min_times = min_times.min(times);
        }
    }

    Some(min_times)
}

fn assign_premade_group(
    team: &mut LiveTeam,
    group: PremadeCandidate,
    marked: &mut HashSet<String>,
    group_index: &mut usize,
) -> bool {
    if group.puuids.len() < 2 || group.puuids.iter().any(|puuid| marked.contains(puuid)) {
        return false;
    }

    let group_id = PREMADE_GROUP_IDS
        .get(*group_index)
        .copied()
        .unwrap_or("Z")
        .to_string();
    let label = format!("{}队 · {}黑", group_id, group.puuids.len());
    let member_set = group.puuids.iter().cloned().collect::<HashSet<_>>();
    let marker = LivePremadeMarker {
        group_id,
        label,
        source: group.source.to_string(),
        together_times: group.together_times,
        member_puuids: group.puuids.clone(),
    };
    let mut assigned = false;

    for player in &mut team.players {
        if member_set.contains(&normalize_puuid(&player.puuid)) {
            player.premade = Some(marker.clone());
            assigned = true;
        }
    }

    if assigned {
        for puuid in group.puuids {
            marked.insert(puuid);
        }
        *group_index += 1;
    }

    assigned
}

fn normalize_puuid(puuid: &str) -> String {
    puuid.to_ascii_lowercase()
}

fn normalize_search_name(value: &str) -> String {
    value.trim().to_lowercase()
}

fn normalize_query_text(value: &str) -> String {
    value
        .trim()
        .replace('＃', "#")
        .replace('﹟', "#")
        .replace('\u{FEFF}', "")
        .replace('\u{200B}', "")
}

/// 实时战绩使用当前对局模式筛选历史记录，最多从近 150 场里凑 100 场。
#[derive(Clone, Copy)]
enum LiveQueueFilter {
    HexAram,
    Aram,
    Ranked,
    MatchAndRanked,
    Any,
}

impl LiveQueueFilter {
    fn matches(self, game: &RecentGame) -> bool {
        match self {
            LiveQueueFilter::HexAram => is_hex_aram_game(game.queue_id, &game.game_mode),
            LiveQueueFilter::Aram => {
                !is_hex_aram_game(game.queue_id, &game.game_mode)
                    && (game.queue_id == 450 || game.game_mode.eq_ignore_ascii_case("ARAM"))
            }
            LiveQueueFilter::Ranked => matches!(game.queue_id, 420 | 440),
            LiveQueueFilter::MatchAndRanked => {
                matches!(game.queue_id, 400 | 420 | 430 | 440 | 490)
                    || (game.game_mode.eq_ignore_ascii_case("CLASSIC")
                        && !matches!(game.queue_id, 450 | 1700 | 1710 | 1711 | 1712 | 2400))
            }
            LiveQueueFilter::Any => true,
        }
    }
}

fn live_queue_filter(
    queue_id: Option<u32>,
    queue_type: Option<&str>,
    game_mode: Option<&str>,
) -> LiveQueueFilter {
    if is_hex_aram_game(queue_id.unwrap_or_default(), game_mode.unwrap_or_default()) {
        return LiveQueueFilter::HexAram;
    }

    if queue_id == Some(450)
        || game_mode
            .map(|mode| mode.eq_ignore_ascii_case("ARAM"))
            .unwrap_or(false)
    {
        return LiveQueueFilter::Aram;
    }

    if matches!(queue_id, Some(420 | 440))
        || queue_type
            .map(|value| value.to_ascii_uppercase().contains("RANKED"))
            .unwrap_or(false)
    {
        return LiveQueueFilter::Ranked;
    }

    if matches!(queue_id, Some(400 | 430 | 490))
        || game_mode
            .map(|mode| mode.eq_ignore_ascii_case("CLASSIC"))
            .unwrap_or(false)
    {
        return LiveQueueFilter::MatchAndRanked;
    }

    LiveQueueFilter::Any
}

fn is_hex_aram_game(queue_id: u32, game_mode: &str) -> bool {
    queue_id == 2400
        || matches!(
            game_mode.to_ascii_uppercase().as_str(),
            "STRAWBERRY" | "KIWI"
        )
}

fn filter_live_stats(
    mut stats: PlayerStatsResponse,
    queue_filter: LiveQueueFilter,
    display_depth: usize,
) -> PlayerStatsResponse {
    let recent_games = stats
        .recent_games
        .iter()
        .filter(|game| {
            game.game_duration >= LIVE_MIN_VALID_GAME_DURATION_SECONDS && queue_filter.matches(game)
        })
        .take(display_depth)
        .cloned()
        .collect::<Vec<_>>();

    stats.depth_requested = display_depth;
    stats.depth_loaded = recent_games.len();
    stats.summary = summarize_live_games(&recent_games);
    stats.champion_stats.clear();
    stats.recent_games = recent_games;
    stats
}

fn summarize_live_games(games: &[RecentGame]) -> PlayerSummary {
    let mut wins = 0usize;
    let mut kills = 0u64;
    let mut deaths = 0u64;
    let mut assists = 0u64;
    let mut champion_counts = HashMap::<u32, usize>::new();

    for game in games {
        wins += usize::from(game.win);
        kills += game.kills as u64;
        deaths += game.deaths as u64;
        assists += game.assists as u64;
        *champion_counts.entry(game.champion_id).or_insert(0) += 1;
    }

    let total = games.len();
    let unique_champions = champion_counts.len();
    let most_played_champion_id = champion_counts
        .into_iter()
        .max_by_key(|(champion_id, games)| (*games, *champion_id))
        .map(|(champion_id, _)| champion_id);

    PlayerSummary {
        games: total,
        wins,
        losses: total.saturating_sub(wins),
        win_rate: live_ratio(wins as u64, total as u64),
        average_kda: live_kda(kills, deaths, assists),
        average_kills: live_average(kills, total),
        average_deaths: live_average(deaths, total),
        average_assists: live_average(assists, total),
        unique_champions,
        most_played_champion_id,
    }
}

fn live_kda(kills: u64, deaths: u64, assists: u64) -> f64 {
    live_round2((kills + assists) as f64 / deaths.max(1) as f64)
}

fn live_average(total: u64, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        live_round2(total as f64 / count as f64)
    }
}

fn live_ratio(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        live_round2(part as f64 / total as f64)
    }
}

fn live_round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

async fn load_live_players(
    lcu: &LcuClient,
    auth: &ClientAuth,
    players: Vec<LivePlayerSeed>,
    display_depth: usize,
    queue_filter: LiveQueueFilter,
) -> Vec<LivePlayer> {
    let auth = auth.clone();

    stream::iter(players.into_iter().map(|player| {
        let lcu = lcu.clone();
        let auth = auth.clone();
        async move { load_live_player(&lcu, &auth, player, display_depth, queue_filter).await }
    }))
    .buffered(LIVE_QUERY_CONCURRENCY)
    .collect()
    .await
}

async fn load_live_player(
    lcu: &LcuClient,
    auth: &ClientAuth,
    seed: LivePlayerSeed,
    display_depth: usize,
    queue_filter: LiveQueueFilter,
) -> LivePlayer {
    let fallback_summoner = fallback_live_summoner(&seed);
    let summoner_result = lcu.summoner_by_puuid(&seed.puuid).await;
    let identity_error = summoner_result.as_ref().err().map(ToString::to_string);
    let summoner = summoner_result.unwrap_or(fallback_summoner);

    match load_live_player_stats(lcu, auth, summoner.clone(), display_depth, queue_filter).await {
        Ok(stats) => LivePlayer {
            puuid: seed.puuid,
            champion_id: seed.champion_id,
            champion_pick_intent: seed.champion_pick_intent,
            position: seed.position,
            selected_role: seed.selected_role,
            summoner_id: seed.summoner_id,
            team_participant_id: seed.team_participant_id,
            is_placeholder: seed.is_placeholder,
            premade: None,
            summoner: Some(summoner),
            stats: Some(stats),
            error: None,
        },
        Err(error) => LivePlayer {
            puuid: seed.puuid,
            champion_id: seed.champion_id,
            champion_pick_intent: seed.champion_pick_intent,
            position: seed.position,
            selected_role: seed.selected_role,
            summoner_id: seed.summoner_id,
            team_participant_id: seed.team_participant_id,
            is_placeholder: seed.is_placeholder,
            premade: None,
            summoner: Some(summoner),
            stats: None,
            error: Some(match identity_error {
                Some(identity_error) => format!("召唤师信息：{identity_error}；战绩：{error}"),
                None => error.to_string(),
            }),
        },
    }
}

async fn load_live_player_stats(
    lcu: &LcuClient,
    auth: &ClientAuth,
    summoner: SummonerInfo,
    display_depth: usize,
    queue_filter: LiveQueueFilter,
) -> AppResult<PlayerStatsResponse> {
    let mut scan_depth = LIVE_SCAN_BATCH_DEPTH.min(LIVE_MAX_SCAN_DEPTH);
    let mut last_filtered = None;

    loop {
        match load_player_stats(lcu, Some(auth), None, summoner.clone(), scan_depth).await {
            Ok(stats) => {
                let loaded_before_filter = stats.depth_loaded;
                let filtered = filter_live_stats(stats, queue_filter, display_depth);
                let enough_games = filtered.recent_games.len() >= display_depth;
                let reached_limit = scan_depth >= LIVE_MAX_SCAN_DEPTH;
                let no_more_games = loaded_before_filter < scan_depth;

                if enough_games || reached_limit || no_more_games {
                    return Ok(filtered);
                }

                last_filtered = Some(filtered);
                scan_depth = (scan_depth + LIVE_SCAN_BATCH_DEPTH).min(LIVE_MAX_SCAN_DEPTH);
            }
            Err(error) => {
                if let Some(filtered) = last_filtered {
                    return Ok(filtered);
                }

                return Err(error);
            }
        }
    }
}

fn fallback_live_summoner(seed: &LivePlayerSeed) -> SummonerInfo {
    let display_name = if seed.fallback_name.trim().is_empty() {
        format!("玩家 {}", seed.puuid.chars().take(8).collect::<String>())
    } else {
        seed.fallback_name.trim().to_string()
    };

    SummonerInfo {
        display_name: display_name.clone(),
        internal_name: display_name,
        puuid: seed.puuid.clone(),
        summoner_id: seed.summoner_id,
        ..Default::default()
    }
}
