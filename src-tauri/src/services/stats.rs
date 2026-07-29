use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::stream::{self, StreamExt};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use super::error::{AppError, AppResult};
use super::lcu::LcuClient;
use super::models::{
    ChampionStat, ClientAuth, Game, IdentityPlayer, MatchDetailPlayer, MatchDetailResponse,
    MatchDetailTeam, Participant, PlayerEloEstimateResponse, PlayerEloGameSample,
    PlayerEloParticipant, PlayerStatsResponse, PlayerSummary, RatingCompositionEntry,
    RecentGame, RecentGameParticipantSummary, SummonerInfo,
};
use super::sgp::{resolve_sgp_server_id, SgpClient};

/// LCU match-history 单页最多稳定返回 50 条。
///
/// 如果请求 0-199，国服客户端也只回 50 条；分页值写太大会让调用方误判
/// “已经没有更多数据”，导致 500 局统计实际只加载第一页。
const MAX_PAGE_SIZE: usize = 50;
const SGP_PAGE_SIZE: usize = 50;
const MAX_DEPTH: usize = 1000;
const MIN_VALID_GAME_DURATION_SECONDS: i64 = 8 * 60;
const STATS_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const DATABASE_CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const CACHE_SCHEMA_VERSION: i64 = 14;
const DATABASE_FILE_NAME: &str = "lol-stats.sqlite3";
const HISTORY_PREMADE_THRESHOLD: usize = 3;
const PLAYER_ELO_SAMPLE_TARGET: usize = 10;
const PLAYER_ELO_BATCH_DEPTH: usize = 100;
const PLAYER_ELO_MAX_DEPTH: usize = 1000;
const PLAYER_ELO_PARTICIPANT_CONCURRENCY: usize = 4;
const PLAYER_ELO_OWNER_HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const PLAYER_ELO_PARTICIPANT_HTTP_TIMEOUT: Duration = Duration::from_secs(4);
const PLAYER_ELO_PARTICIPANT_FETCH_BUDGET: Duration = Duration::from_secs(14);
const PLAYER_ELO_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const EXTERNAL_ELO_SUCCESS_CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const EXTERNAL_ELO_FAILURE_CACHE_TTL: Duration = Duration::from_secs(20);
const EXTERNAL_ELO_DB_SUCCESS_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const EXTERNAL_ELO_DB_FAILURE_CACHE_TTL: Duration = Duration::from_secs(20);
const EXTERNAL_ELO_OWNER_RETRY_COUNT: usize = 3;
const EXTERNAL_ELO_PARTICIPANT_RETRY_COUNT: usize = 1;
const EXTERNAL_ELO_RETRY_DELAY: Duration = Duration::from_millis(1100);
const EXTERNAL_ELO_REFERER: &str = "https://a.lzyumi.top/";
const EXTERNAL_ELO_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

static STATS_CACHE: LazyLock<Mutex<HashMap<StatsCacheKey, StatsCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static EXTERNAL_ELO_CACHE: LazyLock<Mutex<HashMap<String, ExternalEloCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct StatsCacheKey {
    sgp_server_id: String,
    puuid: String,
    depth: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PlayerEloCacheKey {
    sgp_server_id: String,
    puuid: String,
}

#[derive(Clone)]
struct StatsCacheEntry {
    stored_at: Instant,
    response: PlayerStatsResponse,
}

#[derive(Clone)]
struct ExternalEloCacheEntry {
    stored_at: Instant,
    elo: Option<f64>,
    ttl: Duration,
}

struct CachedGameStoreResult {
    hit_cached_player_game: bool,
    available: usize,
}

#[derive(Default)]
struct ChampionAccumulator {
    champion_id: u32,
    games: usize,
    wins: usize,
    kills: u32,
    deaths: u32,
    assists: u32,
    damage_to_champions: u64,
    cs: u64,
    gold_earned: u64,
    team_damage_to_champions: u64,
    team_gold_earned: u64,
    damage_self_mitigated: u64,
    total_damage_taken: u64,
    team_damage_self_mitigated: u64,
    team_total_damage_taken: u64,
    total_heal: u64,
    team_total_heal: u64,
    last_played_at: i64,
}

#[derive(Default)]
struct TeamTotals {
    damage_to_champions: u32,
    gold_earned: u32,
    damage_self_mitigated: u32,
    total_damage_taken: u32,
    total_heal: u32,
    enemy_champion_immobilizations: u32,
    immobilize_and_kill_with_ally: u32,
    kills: u32,
    deaths: u32,
}

#[derive(Default)]
struct GameAccolades {
    team_damage_leader: bool,
    game_damage_leader: bool,
    team_mitigation_leader: bool,
    team_healing_leader: bool,
    team_damage_conversion_leader: bool,
    team_gold_leader: bool,
}

#[derive(Clone)]
struct EloArea {
    id: u32,
    name: &'static str,
}

const LZYUMI_SUPPORTED_AREAS: &[(u32, &str)] = &[
    (1, "艾欧尼亚"),
    (14, "黑色玫瑰"),
    (31, "峡谷之巅"),
    (30, "男爵领域"),
    (3, "祖安"),
    (4, "诺克萨斯"),
    (2, "比尔吉沃特"),
    (6, "德玛西亚"),
    (9, "弗雷尔卓德"),
    (5, "班德尔城"),
    (7, "皮尔特沃夫"),
    (8, "战争学院"),
    (10, "巨神峰"),
    (11, "雷瑟守备"),
    (12, "无畏先锋"),
    (13, "裁决之地"),
    (15, "暗影岛"),
    (16, "恕瑞玛"),
    (17, "钢铁烈阳"),
    (18, "水晶之痕"),
    (19, "均衡教派"),
    (20, "扭曲丛林"),
    (21, "教育网专区"),
    (22, "影流"),
    (23, "守望之海"),
    (24, "征服之海"),
    (25, "卡拉曼达"),
    (27, "皮城警备"),
    (26, "巨龙之巢"),
];

#[derive(Clone)]
struct EloParticipantSeed {
    puuid: String,
    summoner_name: String,
    game_name: String,
    tag_line: String,
    champion_id: u32,
    game_ids: HashSet<u64>,
}

#[derive(Clone, Copy)]
struct ExternalEloFetchOptions {
    http_timeout: Duration,
    retry_count: usize,
    retry_delay: Duration,
    allow_powershell_fallback: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LzyumiInfoResponse {
    #[serde(default)]
    battle_info: Option<LzyumiBattleInfo>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LzyumiBattleInfo {
    #[serde(default)]
    open_id: String,
    #[serde(default)]
    name_info_new: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LzyumiEloResponse {
    #[serde(default)]
    data: Option<LzyumiEloData>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LzyumiEloData {
    #[serde(default)]
    data_rank_elo_num: String,
}

/// 拉取并聚合某个玩家的历史战绩。
///
/// 这里刻意不拉 `/games/{gameId}` 详情接口。LCU 的 match-history summary 对
/// “英雄使用分布 / 单英雄胜率 / KDA / 伤害 / 补刀”已经够用，实战页也能更快出结果。
pub async fn load_player_stats(
    lcu: &LcuClient,
    auth: Option<&ClientAuth>,
    sgp_server_id: Option<&str>,
    summoner: SummonerInfo,
    depth: usize,
) -> AppResult<PlayerStatsResponse> {
    load_player_stats_with_progress(
        lcu,
        auth,
        sgp_server_id,
        summoner,
        depth,
        false,
        true,
        |_, _| true,
    )
    .await
}

pub async fn load_match_detail(
    lcu: &LcuClient,
    auth: Option<&ClientAuth>,
    sgp_server_id: Option<&str>,
    game_id: u64,
) -> AppResult<MatchDetailResponse> {
    if let Some(auth) = auth {
        if let Ok(sgp) = SgpClient::new(lcu, auth, sgp_server_id).await {
            if let Ok(game) = sgp.game_summary(game_id).await {
                return Ok(match_detail_from_game(game));
            }
        }
    }

    let game = lcu.match_history_game(game_id).await?;
    Ok(match_detail_from_game(game))
}

pub async fn estimate_player_elo(
    lcu: &LcuClient,
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    summoner: SummonerInfo,
) -> AppResult<PlayerEloEstimateResponse> {
    let cache_key = player_elo_cache_key(auth, sgp_server_id, &summoner);
    if let Some(cached) = read_db_cached_player_elo(&cache_key, false) {
        return Ok(cached);
    }

    let response = match estimate_player_elo_uncached(lcu, auth, sgp_server_id, summoner).await {
        Ok(response) => response,
        Err(error) => {
            if let Some(stale) = read_db_cached_player_elo(&cache_key, true) {
                return Ok(stale);
            }
            return Err(error);
        }
    };
    write_db_cached_player_elo(&cache_key, &response);
    Ok(response)
}

pub fn load_cached_player_elo(
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    summoner: &SummonerInfo,
) -> Option<PlayerEloEstimateResponse> {
    let cache_key = player_elo_cache_key(auth, sgp_server_id, summoner);
    read_db_cached_player_elo(&cache_key, false)
}

pub fn clear_player_elo_cache(
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    summoner: &SummonerInfo,
) -> AppResult<bool> {
    let cache_key = player_elo_cache_key(auth, sgp_server_id, summoner);
    Ok(delete_db_cached_player_elo(&cache_key).unwrap_or(false))
}

async fn estimate_player_elo_uncached(
    lcu: &LcuClient,
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    summoner: SummonerInfo,
) -> AppResult<PlayerEloEstimateResponse> {
    let resolved_sgp_server_id = resolve_sgp_server_id(auth, sgp_server_id);

    let owner_stats =
        load_stats_until_solo_sample(lcu, auth, Some(resolved_sgp_server_id.as_str()), &summoner)
            .await?;
    let solo_games = owner_stats
        .recent_games
        .iter()
        .filter(|game| is_solo_game(game))
        .take(PLAYER_ELO_SAMPLE_TARGET)
        .cloned()
        .collect::<Vec<_>>();

    if solo_games.is_empty() {
        return Err(AppError::InvalidInput(
            "当前角色没有可用的单人开局样本".to_string(),
        ));
    }

    let mut game_players = HashMap::<u64, Vec<String>>::new();
    let mut participants = HashMap::<String, EloParticipantSeed>::new();
    let normalized_owner_puuid = normalize_puuid(&summoner.puuid);
    let mut owner_identity = EloParticipantSeed {
        puuid: summoner.puuid.clone(),
        summoner_name: summoner.display_name.clone(),
        game_name: summoner.game_name.clone(),
        tag_line: summoner.tag_line.clone(),
        champion_id: 0,
        game_ids: HashSet::new(),
    };

    for game in &solo_games {
        let detail = load_match_detail(
            lcu,
            Some(auth),
            Some(resolved_sgp_server_id.as_str()),
            game.game_id,
        )
        .await?;
        let detail_players = detail
            .teams
            .iter()
            .flat_map(|team| team.players.iter())
            .cloned()
            .collect::<Vec<_>>();

        for player in detail_players.iter().cloned() {
            let key = elo_participant_key(&player);
            if key.is_empty() {
                continue;
            }

            if !normalized_owner_puuid.is_empty()
                && is_real_puuid(&player.puuid)
                && normalize_puuid(&player.puuid) == normalized_owner_puuid
            {
                if !player.summoner_name.trim().is_empty() {
                    owner_identity.summoner_name = player.summoner_name.clone();
                }
                if !player.game_name.trim().is_empty() {
                    owner_identity.game_name = player.game_name.clone();
                }
                if !player.tag_line.trim().is_empty() {
                    owner_identity.tag_line = player.tag_line.clone();
                }
                owner_identity.champion_id = player.record.champion_id;
                owner_identity.game_ids.insert(game.game_id);
            }

            game_players
                .entry(game.game_id)
                .or_default()
                .push(key.clone());

            let is_solo_in_detail = is_solo_detail_player(&player, &detail_players);
            participants
                .entry(key)
                .and_modify(|entry| {
                    if is_solo_in_detail {
                        entry.game_ids.insert(game.game_id);
                    }
                })
                .or_insert_with(|| EloParticipantSeed {
                    puuid: player.puuid.clone(),
                    summoner_name: player.summoner_name.clone(),
                    game_name: player.game_name.clone(),
                    tag_line: player.tag_line.clone(),
                    champion_id: player.record.champion_id,
                    game_ids: if is_solo_in_detail {
                        HashSet::from([game.game_id])
                    } else {
                        HashSet::new()
                    },
                });
        }
    }

    let area = resolve_lzyumi_area_for_summoner(
        &resolved_sgp_server_id,
        owner_identity.game_name.as_str(),
        owner_identity.tag_line.as_str(),
        Some(owner_identity.summoner_name.as_str()),
    )
    .await?;
    let owner_elo = fetch_external_aram_elo(
        &area,
        owner_identity.game_name.as_str(),
        owner_identity.tag_line.as_str(),
        Some(owner_identity.summoner_name.as_str()),
    )
    .await?;
    let owner_display_name = display_name_from_parts(
        owner_identity.summoner_name.as_str(),
        owner_identity.game_name.as_str(),
        owner_identity.tag_line.as_str(),
    );

    let mut solo_game_sets = HashMap::<String, HashSet<u64>>::new();
    let mut elo_values = HashMap::<String, f64>::new();
    let mut participant_entries = participants
        .iter()
        .filter(|(_, participant)| !participant.game_ids.is_empty())
        .map(|(key, participant)| (key.clone(), participant.clone()))
        .collect::<Vec<_>>();
    participant_entries.sort_by(|left, right| {
        right
            .1
            .game_ids
            .len()
            .cmp(&left.1.game_ids.len())
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut participant_results = stream::iter(participant_entries.into_iter().map(
        |(key, participant)| {
            let area = area.clone();
            let options = participant_external_elo_fetch_options();

            async move {
                let elo = match fetch_external_aram_elo_with_options(
                    &area,
                    participant.game_name.as_str(),
                    participant.tag_line.as_str(),
                    Some(participant.summoner_name.as_str()),
                    options,
                )
                .await
                {
                    Ok(Some(elo)) => elo,
                    _ => return None,
                };

                Some((key, participant.game_ids, elo))
            }
        },
    ))
    .buffer_unordered(PLAYER_ELO_PARTICIPANT_CONCURRENCY);

    let participant_deadline = Instant::now() + PLAYER_ELO_PARTICIPANT_FETCH_BUDGET;
    loop {
        let remaining = participant_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        let next_result = match tokio::time::timeout(remaining, participant_results.next()).await {
            Ok(value) => value,
            Err(_) => break,
        };
        let Some(result) = next_result else {
            break;
        };
        let Some((key, solo_game_ids, elo)) = result else {
            continue;
        };
        solo_game_sets.insert(key.clone(), solo_game_ids);
        elo_values.insert(key, elo);
    }

    let mut game_samples = Vec::new();
    for game in &solo_games {
        let mut solo_players = game_players
            .get(&game.game_id)
            .into_iter()
            .flatten()
            .filter_map(|key| {
                let game_ids = solo_game_sets.get(key)?;
                if !game_ids.contains(&game.game_id) {
                    return None;
                }

                let participant = participants.get(key)?;
                let elo = *elo_values.get(key)?;

                Some(PlayerEloParticipant {
                    puuid: participant.puuid.clone(),
                    summoner_name: display_name_from_parts(
                        participant.summoner_name.as_str(),
                        participant.game_name.as_str(),
                        participant.tag_line.as_str(),
                    ),
                    game_name: participant.game_name.clone(),
                    tag_line: participant.tag_line.clone(),
                    champion_id: participant.champion_id,
                    elo: round2(elo),
                })
            })
            .collect::<Vec<_>>();

        if let Some(owner_elo) = owner_elo {
            let owner_exists = solo_players.iter().any(|player| {
                is_real_puuid(&owner_stats.summoner.puuid)
                    && is_real_puuid(&player.puuid)
                    && normalize_puuid(&player.puuid) == normalize_puuid(&owner_stats.summoner.puuid)
            }) || solo_players.iter().any(|player| {
                normalized_riot_id(player.game_name.as_str(), player.tag_line.as_str())
                    == normalized_riot_id(
                        owner_stats.summoner.game_name.as_str(),
                        owner_stats.summoner.tag_line.as_str(),
                    )
            });

            if !owner_exists {
                solo_players.push(PlayerEloParticipant {
                    puuid: owner_identity.puuid.clone(),
                    summoner_name: owner_display_name.clone(),
                    game_name: owner_identity.game_name.clone(),
                    tag_line: owner_identity.tag_line.clone(),
                    champion_id: if owner_identity.champion_id != 0 {
                        owner_identity.champion_id
                    } else {
                        game.champion_id
                    },
                    elo: round2(owner_elo),
                });
            }
        }

        solo_players.sort_by(|left, right| right.elo.total_cmp(&left.elo));

        if let Some(max_elo) = solo_players.first().map(|player| player.elo) {
            game_samples.push(PlayerEloGameSample {
                game_id: game.game_id,
                game_creation: game.game_creation,
                max_elo: max_elo,
                solo_players,
            });
        }
    }

    if game_samples.is_empty() {
        if owner_elo.is_none() {
            return Err(AppError::SgpUnavailable(format!(
                "在 ELO 查询网站上用用户名 {} 和区服 {} 无法查到 ELO 分数",
                owner_display_name, area.name
            )));
        }

        return Err(AppError::SgpUnavailable(format!(
            "外部 ELO 服务暂时没有返回可用样本（单人对局 {} 局，命中玩家 ELO {} 人）",
            solo_games.len(),
            elo_values.len()
        )));
    }

    let raw_game_max_elos = game_samples
        .iter()
        .map(|sample| sample.max_elo)
        .collect::<Vec<_>>();
    let trimmed_game_max_elos = trim_extreme_elos(&raw_game_max_elos);
    let scoring_values = if trimmed_game_max_elos.is_empty() {
        raw_game_max_elos.clone()
    } else {
        trimmed_game_max_elos.clone()
    };

    let average_elo = round2(mean(&scoring_values));
    let median_elo = round2(median(&scoring_values));
    let final_elo = round2((average_elo + median_elo) / 2.0);

    Ok(PlayerEloEstimateResponse {
        sample_target: PLAYER_ELO_SAMPLE_TARGET,
        sample_games: game_samples.len(),
        trimmed_games: scoring_values.len(),
        average_elo,
        median_elo,
        final_elo,
        raw_game_max_elos: raw_game_max_elos.iter().copied().map(round2).collect(),
        trimmed_game_max_elos: scoring_values.iter().copied().map(round2).collect(),
        game_samples,
    })
}

pub async fn load_player_stats_with_progress<F>(
    lcu: &LcuClient,
    auth: Option<&ClientAuth>,
    sgp_server_id: Option<&str>,
    summoner: SummonerInfo,
    depth: usize,
    force_refresh: bool,
    persist_cache: bool,
    mut progress: F,
) -> AppResult<PlayerStatsResponse>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    let depth = depth.clamp(1, MAX_DEPTH);

    if !progress(0, depth) {
        return Err(super::error::AppError::Cancelled);
    }

    let cache_key = stats_cache_key(auth, sgp_server_id, &summoner, depth);
    if !persist_cache {
        let games =
            load_games_direct(lcu, auth, sgp_server_id, &cache_key, depth, &mut progress).await?;
        return Ok(analyze_games(summoner, depth, games));
    }

    if !force_refresh {
        if let Some(cached) = get_cached_stats(&cache_key) {
            if !progress(depth, depth) {
                return Err(super::error::AppError::Cancelled);
            }
            return Ok(cached);
        }

        if let Some(cached) = read_db_cached_stats(&cache_key, false) {
            store_cached_stats(cache_key.clone(), &cached);
            if !progress(depth, depth) {
                return Err(super::error::AppError::Cancelled);
            }
            return Ok(cached);
        }
    }

    if let Some(available) = count_db_cached_games(&cache_key) {
        let loaded = available.min(depth);
        if loaded > 0 && !progress(loaded, depth) {
            return Err(super::error::AppError::Cancelled);
        }
    }

    let loaded =
        load_games_incremental(lcu, auth, sgp_server_id, &cache_key, depth, &mut progress).await;

    let games = match loaded {
        Ok(games) => games,
        Err(error) => {
            if let Some(games) =
                read_db_cached_games(&cache_key, depth).filter(|games| !games.is_empty())
            {
                let response = analyze_games(summoner, depth, games);
                write_db_cached_stats(&cache_key, &response);
                store_cached_stats(cache_key.clone(), &response);
                return Ok(response);
            }

            if let Some(cached) = read_db_cached_stats(&cache_key, true) {
                store_cached_stats(cache_key.clone(), &cached);
                if !progress(depth, depth) {
                    return Err(super::error::AppError::Cancelled);
                }
                return Ok(cached);
            }

            return Err(error);
        }
    };

    let response = analyze_games(summoner, depth, games);
    write_db_cached_stats(&cache_key, &response);
    store_cached_stats(cache_key, &response);
    Ok(response)
}

fn stats_cache_key(
    auth: Option<&ClientAuth>,
    sgp_server_id: Option<&str>,
    summoner: &SummonerInfo,
    depth: usize,
) -> StatsCacheKey {
    let sgp_server_id = auth
        .map(|auth| resolve_sgp_server_id(auth, sgp_server_id))
        .unwrap_or_else(|| {
            sgp_server_id
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.trim().to_ascii_uppercase())
                .unwrap_or_else(|| "LCU_CURRENT".to_string())
        });

    StatsCacheKey {
        sgp_server_id,
        puuid: summoner.puuid.to_ascii_lowercase(),
        depth,
    }
}

fn player_elo_cache_key(
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    summoner: &SummonerInfo,
) -> PlayerEloCacheKey {
    PlayerEloCacheKey {
        sgp_server_id: resolve_sgp_server_id(auth, sgp_server_id),
        puuid: summoner.puuid.to_ascii_lowercase(),
    }
}

fn get_cached_stats(key: &StatsCacheKey) -> Option<PlayerStatsResponse> {
    let mut cache = STATS_CACHE.lock().ok()?;
    let Some(entry) = cache.get(key) else {
        return None;
    };

    if entry.stored_at.elapsed() <= STATS_CACHE_TTL {
        return Some(entry.response.clone());
    }

    cache.remove(key);
    None
}

fn store_cached_stats(key: StatsCacheKey, response: &PlayerStatsResponse) {
    if let Ok(mut cache) = STATS_CACHE.lock() {
        cache.insert(
            key,
            StatsCacheEntry {
                stored_at: Instant::now(),
                response: response.clone(),
            },
        );
    }
}

/// 从 SQLite 读取统计缓存。
///
/// 当前版本先把聚合后的 `PlayerStatsResponse` 整包保存下来，减少重复拉取 500 局的压力。
/// 后续做增量更新时，可以在同一个数据库里继续拆 `matches` / `participants` 明细表。
fn read_db_cached_stats(key: &StatsCacheKey, allow_stale: bool) -> Option<PlayerStatsResponse> {
    let conn = open_cache_db()?;
    let (stored_at_ms, response_json): (i64, String) = conn
        .query_row(
            r#"
            SELECT stored_at_ms, response_json
            FROM stats_cache
            WHERE sgp_server_id = ?1
              AND puuid = ?2
              AND depth = ?3
              AND schema_version = ?4
            "#,
            params![
                &key.sgp_server_id,
                &key.puuid,
                key.depth as i64,
                CACHE_SCHEMA_VERSION
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .ok()??;

    if !allow_stale && cache_age(stored_at_ms)? > DATABASE_CACHE_TTL {
        return None;
    }

    let response = serde_json::from_str::<PlayerStatsResponse>(&response_json).ok()?;
    if response.recent_games.iter().any(|game| {
        game.game_player_count == 0
            || game.team_player_count == 0
            || game.team_id == 0
            || game.team_composition.is_empty()
            || game.game_composition.is_empty()
    }) {
        return None;
    }

    Some(response)
}

/// 写入 SQLite 统计缓存，写入失败不会影响正常查战绩流程。
fn write_db_cached_stats(key: &StatsCacheKey, response: &PlayerStatsResponse) {
    let Some(conn) = open_cache_db() else {
        return;
    };

    let Ok(response_json) = serde_json::to_string(response) else {
        return;
    };

    let _ = conn.execute(
        r#"
        INSERT INTO stats_cache (
            sgp_server_id,
            puuid,
            depth,
            schema_version,
            stored_at_ms,
            response_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(sgp_server_id, puuid, depth) DO UPDATE SET
            schema_version = excluded.schema_version,
            stored_at_ms = excluded.stored_at_ms,
            response_json = excluded.response_json
        "#,
        params![
            &key.sgp_server_id,
            &key.puuid,
            key.depth as i64,
            CACHE_SCHEMA_VERSION,
            unix_time_ms(),
            response_json
        ],
    );
}

fn read_db_cached_player_elo(
    key: &PlayerEloCacheKey,
    allow_stale: bool,
) -> Option<PlayerEloEstimateResponse> {
    let conn = open_cache_db()?;
    let (stored_at_ms, response_json): (i64, String) = conn
        .query_row(
            r#"
            SELECT stored_at_ms, response_json
            FROM player_elo_cache
            WHERE sgp_server_id = ?1
              AND puuid = ?2
            "#,
            params![&key.sgp_server_id, &key.puuid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .ok()??;

    if !allow_stale && cache_age(stored_at_ms)? > PLAYER_ELO_CACHE_TTL {
        return None;
    }

    serde_json::from_str::<PlayerEloEstimateResponse>(&response_json).ok()
}

fn write_db_cached_player_elo(key: &PlayerEloCacheKey, response: &PlayerEloEstimateResponse) {
    let Some(conn) = open_cache_db() else {
        return;
    };

    let Ok(response_json) = serde_json::to_string(response) else {
        return;
    };

    let _ = conn.execute(
        r#"
        INSERT INTO player_elo_cache (
            sgp_server_id,
            puuid,
            stored_at_ms,
            response_json
        )
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(sgp_server_id, puuid) DO UPDATE SET
            stored_at_ms = excluded.stored_at_ms,
            response_json = excluded.response_json
        "#,
        params![&key.sgp_server_id, &key.puuid, unix_time_ms(), response_json],
    );
}

fn delete_db_cached_player_elo(key: &PlayerEloCacheKey) -> Option<bool> {
    let conn = open_cache_db()?;
    let affected = conn
        .execute(
            r#"
            DELETE FROM player_elo_cache
            WHERE sgp_server_id = ?1
              AND puuid = ?2
            "#,
            params![&key.sgp_server_id, &key.puuid],
        )
        .ok()?;
    Some(affected > 0)
}

fn read_db_cached_external_elo(cache_key: &str, allow_stale: bool) -> Option<Option<f64>> {
    let conn = open_cache_db()?;
    let (stored_at_ms, ttl_ms, elo): (i64, i64, Option<f64>) = conn
        .query_row(
            r#"
            SELECT stored_at_ms, ttl_ms, elo
            FROM external_elo_cache
            WHERE cache_key = ?1
            "#,
            params![cache_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .ok()??;

    let ttl = Duration::from_millis(ttl_ms.try_into().ok()?);
    if !allow_stale && cache_age(stored_at_ms)? > ttl {
        return None;
    }

    Some(elo)
}

fn write_db_cached_external_elo(cache_key: &str, elo: Option<f64>) {
    let Some(conn) = open_cache_db() else {
        return;
    };

    let ttl = if elo.is_some() {
        EXTERNAL_ELO_DB_SUCCESS_CACHE_TTL
    } else {
        EXTERNAL_ELO_DB_FAILURE_CACHE_TTL
    };
    let ttl_ms = i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX);

    let _ = conn.execute(
        r#"
        INSERT INTO external_elo_cache (
            cache_key,
            stored_at_ms,
            ttl_ms,
            elo
        )
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(cache_key) DO UPDATE SET
            stored_at_ms = excluded.stored_at_ms,
            ttl_ms = excluded.ttl_ms,
            elo = excluded.elo
        "#,
        params![cache_key, unix_time_ms(), ttl_ms, elo],
    );
}

fn open_cache_db() -> Option<Connection> {
    let path = cache_db_path()?;
    let parent = path.parent()?;
    fs::create_dir_all(parent).ok()?;

    let conn = Connection::open(path).ok()?;
    initialize_cache_schema(&conn).ok()?;
    Some(conn)
}

fn initialize_cache_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS stats_cache (
            sgp_server_id TEXT NOT NULL,
            puuid TEXT NOT NULL,
            depth INTEGER NOT NULL,
            schema_version INTEGER NOT NULL,
            stored_at_ms INTEGER NOT NULL,
            response_json TEXT NOT NULL,
            PRIMARY KEY (sgp_server_id, puuid, depth)
        );

        CREATE TABLE IF NOT EXISTS match_cache (
            sgp_server_id TEXT NOT NULL,
            game_id INTEGER NOT NULL,
            game_creation INTEGER NOT NULL,
            game_duration INTEGER NOT NULL,
            queue_id INTEGER NOT NULL,
            stored_at_ms INTEGER NOT NULL,
            game_json TEXT NOT NULL,
            PRIMARY KEY (sgp_server_id, game_id)
        );

        CREATE TABLE IF NOT EXISTS player_match_index (
            sgp_server_id TEXT NOT NULL,
            puuid TEXT NOT NULL,
            game_id INTEGER NOT NULL,
            game_creation INTEGER NOT NULL,
            queue_id INTEGER NOT NULL,
            stored_at_ms INTEGER NOT NULL,
            PRIMARY KEY (sgp_server_id, puuid, game_id)
        );

        CREATE INDEX IF NOT EXISTS idx_player_match_recent
            ON player_match_index (sgp_server_id, puuid, game_creation DESC);

        CREATE TABLE IF NOT EXISTS player_elo_cache (
            sgp_server_id TEXT NOT NULL,
            puuid TEXT NOT NULL,
            stored_at_ms INTEGER NOT NULL,
            response_json TEXT NOT NULL,
            PRIMARY KEY (sgp_server_id, puuid)
        );

        CREATE TABLE IF NOT EXISTS external_elo_cache (
            cache_key TEXT NOT NULL PRIMARY KEY,
            stored_at_ms INTEGER NOT NULL,
            ttl_ms INTEGER NOT NULL,
            elo REAL
        );

        CREATE TABLE IF NOT EXISTS cache_meta (
            key TEXT PRIMARY KEY,
            value INTEGER NOT NULL
        );
        "#,
    )?;

    let stored_version = conn
        .query_row(
            "SELECT value FROM cache_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    if stored_version != Some(CACHE_SCHEMA_VERSION) {
        conn.execute_batch(
            r#"
            DELETE FROM stats_cache;
            DELETE FROM match_cache;
            DELETE FROM player_match_index;
            DELETE FROM player_elo_cache;
            DELETE FROM external_elo_cache;
            "#,
        )?;
        conn.execute(
            r#"
            INSERT INTO cache_meta (key, value)
            VALUES ('schema_version', ?1)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
            params![CACHE_SCHEMA_VERSION],
        )?;
    }

    Ok(())
}

fn cache_db_path() -> Option<PathBuf> {
    let mut base = if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let mut path = PathBuf::from(local_app_data);
        path.push("lol-stats");
        path
    } else {
        let mut path = std::env::current_dir().ok()?;
        path.push(".lol-stats");
        path
    };

    base.push(DATABASE_FILE_NAME);
    Some(base)
}

fn cache_age(stored_at_ms: i64) -> Option<Duration> {
    let now = unix_time_ms();
    let elapsed = now.checked_sub(stored_at_ms)?;
    Some(Duration::from_millis(elapsed.try_into().ok()?))
}

fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

async fn load_games_direct<F>(
    lcu: &LcuClient,
    auth: Option<&ClientAuth>,
    sgp_server_id: Option<&str>,
    key: &StatsCacheKey,
    depth: usize,
    progress: &mut F,
) -> AppResult<Vec<Game>>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    if let Some(auth) = auth {
        return load_games_direct_from_sgp(lcu, auth, sgp_server_id, key, depth, progress).await;
    }

    load_games_direct_from_lcu(lcu, key, depth, progress).await
}

async fn load_games_direct_from_lcu<F>(
    lcu: &LcuClient,
    key: &StatsCacheKey,
    depth: usize,
    progress: &mut F,
) -> AppResult<Vec<Game>>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    let mut games = Vec::new();
    let mut seen_game_ids = HashSet::new();
    let mut start = 0usize;

    while games.len() < depth {
        let remaining = depth - games.len();
        let page_size = remaining.min(MAX_PAGE_SIZE);
        let end = start + page_size - 1;
        let page = lcu.match_history(&key.puuid, start, end).await?;

        if page.games.games.is_empty() {
            break;
        }

        let received = page.games.games.len();
        let before = games.len();
        games.extend(unique_games(page.games.games, &mut seen_game_ids));

        if !progress(games.len().min(depth), depth) {
            return Err(super::error::AppError::Cancelled);
        }

        if games.len() == before || received < page_size {
            break;
        }

        start += page_size;
    }

    Ok(games)
}

async fn load_games_direct_from_sgp<F>(
    lcu: &LcuClient,
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    key: &StatsCacheKey,
    depth: usize,
    progress: &mut F,
) -> AppResult<Vec<Game>>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    let sgp = SgpClient::new(lcu, auth, sgp_server_id).await?;
    let mut games = Vec::new();
    let mut seen_game_ids = HashSet::new();
    let mut start = 0usize;

    while games.len() < depth {
        let remaining = depth - games.len();
        let page_size = remaining.min(SGP_PAGE_SIZE);
        let page = sgp.match_history(&key.puuid, start, page_size).await?;

        if page.is_empty() {
            break;
        }

        let received = page.len();
        let before = games.len();
        games.extend(unique_games(page, &mut seen_game_ids));

        if !progress(games.len().min(depth), depth) {
            return Err(super::error::AppError::Cancelled);
        }

        if games.len() == before || received < page_size {
            break;
        }

        start += page_size;
    }

    Ok(games)
}

fn count_db_cached_games(key: &StatsCacheKey) -> Option<usize> {
    let conn = open_cache_db()?;
    let count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM player_match_index
            WHERE sgp_server_id = ?1
              AND puuid = ?2
            "#,
            params![&key.sgp_server_id, &key.puuid],
            |row| row.get(0),
        )
        .ok()?;

    count.try_into().ok()
}

fn read_db_cached_games(key: &StatsCacheKey, depth: usize) -> Option<Vec<Game>> {
    let conn = open_cache_db()?;
    let mut stmt = conn
        .prepare(
            r#"
            SELECT m.game_json
            FROM player_match_index i
            JOIN match_cache m
              ON m.sgp_server_id = i.sgp_server_id
             AND m.game_id = i.game_id
            WHERE i.sgp_server_id = ?1
              AND i.puuid = ?2
            ORDER BY i.game_creation DESC
            LIMIT ?3
            "#,
        )
        .ok()?;

    let rows = stmt
        .query_map(
            params![&key.sgp_server_id, &key.puuid, depth as i64],
            |row| row.get::<_, String>(0),
        )
        .ok()?;

    Some(
        rows.filter_map(|row| row.ok())
            .filter_map(|json| serde_json::from_str::<Game>(&json).ok())
            .collect(),
    )
}

fn store_db_cached_games(key: &StatsCacheKey, games: &[Game]) -> CachedGameStoreResult {
    let Some(conn) = open_cache_db() else {
        return CachedGameStoreResult {
            hit_cached_player_game: false,
            available: 0,
        };
    };

    let mut hit_cached_player_game = false;
    let stored_at_ms = unix_time_ms();

    for game in games {
        if game.game_id == 0 {
            continue;
        }

        if db_player_match_exists(&conn, key, game.game_id).unwrap_or(false) {
            hit_cached_player_game = true;
        }

        let Ok(game_json) = serde_json::to_string(game) else {
            continue;
        };

        let _ = conn.execute(
            r#"
            INSERT INTO match_cache (
                sgp_server_id,
                game_id,
                game_creation,
                game_duration,
                queue_id,
                stored_at_ms,
                game_json
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(sgp_server_id, game_id) DO UPDATE SET
                game_creation = excluded.game_creation,
                game_duration = excluded.game_duration,
                queue_id = excluded.queue_id,
                stored_at_ms = excluded.stored_at_ms,
                game_json = excluded.game_json
            "#,
            params![
                &key.sgp_server_id,
                game.game_id as i64,
                game.game_creation,
                game.game_duration,
                game.queue_id as i64,
                stored_at_ms,
                game_json
            ],
        );

        let _ = conn.execute(
            r#"
            INSERT INTO player_match_index (
                sgp_server_id,
                puuid,
                game_id,
                game_creation,
                queue_id,
                stored_at_ms
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(sgp_server_id, puuid, game_id) DO UPDATE SET
                game_creation = excluded.game_creation,
                queue_id = excluded.queue_id,
                stored_at_ms = excluded.stored_at_ms
            "#,
            params![
                &key.sgp_server_id,
                &key.puuid,
                game.game_id as i64,
                game.game_creation,
                game.queue_id as i64,
                stored_at_ms
            ],
        );
    }

    CachedGameStoreResult {
        hit_cached_player_game,
        available: count_db_cached_games_with_conn(&conn, key).unwrap_or(0),
    }
}

fn db_player_match_exists(
    conn: &Connection,
    key: &StatsCacheKey,
    game_id: u64,
) -> rusqlite::Result<bool> {
    conn.query_row(
        r#"
        SELECT 1
        FROM player_match_index
        WHERE sgp_server_id = ?1
          AND puuid = ?2
          AND game_id = ?3
        LIMIT 1
        "#,
        params![&key.sgp_server_id, &key.puuid, game_id as i64],
        |_| Ok(()),
    )
    .optional()
    .map(|value| value.is_some())
}

fn count_db_cached_games_with_conn(
    conn: &Connection,
    key: &StatsCacheKey,
) -> rusqlite::Result<usize> {
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM player_match_index
        WHERE sgp_server_id = ?1
          AND puuid = ?2
        "#,
        params![&key.sgp_server_id, &key.puuid],
        |row| row.get(0),
    )?;

    Ok(count.try_into().unwrap_or(0))
}

async fn load_games_incremental<F>(
    lcu: &LcuClient,
    auth: Option<&ClientAuth>,
    sgp_server_id: Option<&str>,
    key: &StatsCacheKey,
    depth: usize,
    progress: &mut F,
) -> AppResult<Vec<Game>>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    if let Some(auth) = auth {
        load_games_incremental_from_sgp(lcu, auth, sgp_server_id, key, depth, progress).await?;
    } else {
        load_games_incremental_from_lcu(lcu, key, depth, progress).await?;
    }

    let games = read_db_cached_games(key, depth).unwrap_or_default();
    if games.is_empty() {
        return Err(super::error::AppError::SgpUnavailable(
            "未能读取到有效的本地增量战绩".to_string(),
        ));
    }

    if !progress(games.len().min(depth), depth) {
        return Err(super::error::AppError::Cancelled);
    }

    Ok(games)
}

async fn load_games_incremental_from_lcu<F>(
    lcu: &LcuClient,
    key: &StatsCacheKey,
    depth: usize,
    progress: &mut F,
) -> AppResult<Vec<Game>>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    let mut seen_game_ids = HashSet::new();
    let mut start = 0usize;
    let mut fetched_this_refresh = 0usize;

    loop {
        let available = count_db_cached_games(key).unwrap_or(0);
        if start > 0 && fetched_this_refresh >= depth {
            break;
        }

        let remaining = depth.saturating_sub(available).max(MAX_PAGE_SIZE);
        let page_size = remaining.min(MAX_PAGE_SIZE);
        let end = start + page_size - 1;
        let page = lcu.match_history(&key.puuid, start, end).await?;

        if page.games.games.is_empty() {
            break;
        }

        let received = page.games.games.len();
        let page_games = unique_games(page.games.games, &mut seen_game_ids);
        if page_games.is_empty() {
            break;
        }

        let result = store_db_cached_games(key, &page_games);
        fetched_this_refresh += page_games.len();
        if !progress(result.available.min(depth), depth) {
            return Err(super::error::AppError::Cancelled);
        }

        // 国服 LCU 在部分版本会忽略分页参数，后续页重复返回第一页。
        if result.hit_cached_player_game && result.available >= depth {
            break;
        }

        if received < page_size {
            break;
        }

        start += page_size;
    }

    Ok(read_db_cached_games(key, depth).unwrap_or_default())
}

async fn load_games_incremental_from_sgp<F>(
    lcu: &LcuClient,
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    key: &StatsCacheKey,
    depth: usize,
    progress: &mut F,
) -> AppResult<Vec<Game>>
where
    F: FnMut(usize, usize) -> bool + Send,
{
    let sgp = SgpClient::new(lcu, auth, sgp_server_id).await?;
    let mut seen_game_ids = HashSet::new();
    let mut start = 0usize;
    let mut fetched_this_refresh = 0usize;

    loop {
        let available = count_db_cached_games(key).unwrap_or(0);
        if start > 0 && fetched_this_refresh >= depth {
            break;
        }

        let remaining = depth.saturating_sub(available).max(SGP_PAGE_SIZE);
        let page_size = remaining.min(SGP_PAGE_SIZE);
        let page = sgp.match_history(&key.puuid, start, page_size).await?;

        if page.is_empty() {
            break;
        }

        let received = page.len();
        let page_games = unique_games(page, &mut seen_game_ids);
        if page_games.is_empty() {
            break;
        }

        let result = store_db_cached_games(key, &page_games);
        fetched_this_refresh += page_games.len();
        if !progress(result.available.min(depth), depth) {
            return Err(super::error::AppError::Cancelled);
        }

        if result.hit_cached_player_game && result.available >= depth {
            break;
        }

        if received < page_size {
            break;
        }

        start += page_size;
    }

    Ok(read_db_cached_games(key, depth).unwrap_or_default())
}

fn unique_games(games: Vec<Game>, seen_game_ids: &mut HashSet<u64>) -> Vec<Game> {
    games
        .into_iter()
        .filter(|game| game.game_id == 0 || seen_game_ids.insert(game.game_id))
        .collect()
}

fn analyze_games(summoner: SummonerInfo, depth: usize, games: Vec<Game>) -> PlayerStatsResponse {
    let mut recent_games = Vec::new();
    let mut champion_map: HashMap<u32, ChampionAccumulator> = HashMap::new();

    let mut wins = 0usize;
    let mut kills = 0u64;
    let mut deaths = 0u64;
    let mut assists = 0u64;

    for game in games {
        if is_pve_queue(game.queue_id) {
            continue;
        }

        if game.game_duration < MIN_VALID_GAME_DURATION_SECONDS {
            continue;
        }

        let Some(participant) = participant_for_puuid(&game, &summoner.puuid) else {
            continue;
        };

        let cs = participant.stats.total_minions_killed + participant.stats.neutral_minions_killed;
        let team_totals = team_totals_for_participant(&game, participant);

        if participant.stats.win {
            wins += 1;
        }
        kills += participant.stats.kills as u64;
        deaths += participant.stats.deaths as u64;
        assists += participant.stats.assists as u64;

        let entry = champion_map
            .entry(participant.champion_id)
            .or_insert_with(|| ChampionAccumulator {
                champion_id: participant.champion_id,
                ..Default::default()
            });
        entry.games += 1;
        entry.wins += usize::from(participant.stats.win);
        entry.kills += participant.stats.kills;
        entry.deaths += participant.stats.deaths;
        entry.assists += participant.stats.assists;
        entry.damage_to_champions += participant.stats.total_damage_dealt_to_champions as u64;
        entry.cs += cs as u64;
        entry.gold_earned += participant.stats.gold_earned as u64;
        entry.team_damage_to_champions += team_totals.damage_to_champions as u64;
        entry.team_gold_earned += team_totals.gold_earned as u64;
        entry.damage_self_mitigated += participant.stats.damage_self_mitigated as u64;
        entry.total_damage_taken += participant.stats.total_damage_taken as u64;
        entry.team_damage_self_mitigated += team_totals.damage_self_mitigated as u64;
        entry.team_total_damage_taken += team_totals.total_damage_taken as u64;
        entry.total_heal += participant.stats.total_heal as u64;
        entry.team_total_heal += team_totals.total_heal as u64;
        entry.last_played_at = entry.last_played_at.max(game.game_creation);

        recent_games.push(participant_to_recent_game(&game, participant));
    }

    infer_recent_game_premades(&summoner.puuid, &mut recent_games);

    let total = recent_games.len();
    let losses = total.saturating_sub(wins);
    let most_played_champion_id = champion_map
        .values()
        .max_by_key(|c| (c.games, c.last_played_at))
        .map(|c| c.champion_id);

    let mut champion_stats = champion_map
        .into_values()
        .map(|c| champion_accumulator_to_stat(c, total))
        .collect::<Vec<_>>();

    champion_stats.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then_with(|| b.last_played_at.cmp(&a.last_played_at))
            .then_with(|| a.champion_id.cmp(&b.champion_id))
    });

    PlayerStatsResponse {
        summoner,
        depth_requested: depth,
        depth_loaded: total,
        summary: PlayerSummary {
            games: total,
            wins,
            losses,
            win_rate: ratio(wins, total),
            average_kda: calc_kda(kills as u32, deaths as u32, assists as u32),
            average_kills: average(kills, total),
            average_deaths: average(deaths, total),
            average_assists: average(assists, total),
            unique_champions: champion_stats.len(),
            most_played_champion_id,
        },
        champion_stats,
        recent_games,
    }
}

fn match_detail_from_game(game: Game) -> MatchDetailResponse {
    let mut grouped_players: BTreeMap<u32, Vec<MatchDetailPlayer>> = BTreeMap::new();

    for participant in &game.participants {
        let identity = identity_for_participant(&game, participant.participant_id);
        let record = participant_to_recent_game(&game, participant);

        grouped_players
            .entry(participant.team_id)
            .or_default()
            .push(MatchDetailPlayer {
                participant_id: participant.participant_id,
                puuid: identity.puuid,
                game_name: identity.game_name,
                tag_line: identity.tag_line,
                summoner_name: identity.summoner_name,
                team_id: participant.team_id,
                player_subteam_id: participant.stats.player_subteam_id,
                record,
            });
    }

    let teams = grouped_players
        .into_iter()
        .map(|(team_id, mut players)| {
            players.sort_by_key(|player| player.participant_id);
            let win = players
                .first()
                .map(|player| player.record.win)
                .unwrap_or(false);

            MatchDetailTeam {
                team_id,
                name: team_name(team_id),
                win,
                tower_kills: tower_kills_for_team(&game, team_id),
                players,
            }
        })
        .collect::<Vec<_>>();

    MatchDetailResponse {
        game_id: game.game_id,
        queue_id: game.queue_id,
        game_mode: game.game_mode,
        game_creation: game.game_creation,
        game_duration: game.game_duration,
        teams,
    }
}

fn participant_to_recent_game(game: &Game, participant: &Participant) -> RecentGame {
    let cs = participant.stats.total_minions_killed + participant.stats.neutral_minions_killed;
    let team_totals = team_totals_for_participant(game, participant);
    let game_totals = game_totals(game);
    let team_player_count = game
        .participants
        .iter()
        .filter(|candidate| same_stat_team(game, participant, candidate))
        .count() as u32;
    let accolades = game_accolades_for_participant(game, participant, &team_totals);
    let kda = calc_kda(
        participant.stats.kills,
        participant.stats.deaths,
        participant.stats.assists,
    );

    RecentGame {
        game_id: game.game_id,
        champion_id: participant.champion_id,
        queue_id: game.queue_id,
        game_mode: game.game_mode.clone(),
        win: participant.stats.win,
        team_id: participant.team_id,
        spell1_id: participant.spell1_id,
        spell2_id: participant.spell2_id,
        item_ids: nonzero_values([
            participant.stats.item0,
            participant.stats.item1,
            participant.stats.item2,
            participant.stats.item3,
            participant.stats.item4,
            participant.stats.item5,
            participant.stats.item6,
        ]),
        perk_ids: nonzero_values([
            participant.stats.perk0,
            participant.stats.perk1,
            participant.stats.perk2,
            participant.stats.perk3,
            participant.stats.perk4,
            participant.stats.perk5,
        ]),
        augment_ids: nonzero_values([
            participant.stats.player_augment1,
            participant.stats.player_augment2,
            participant.stats.player_augment3,
            participant.stats.player_augment4,
            participant.stats.player_augment5,
            participant.stats.player_augment6,
        ]),
        team_damage_leader: accolades.team_damage_leader,
        game_damage_leader: accolades.game_damage_leader,
        team_mitigation_leader: accolades.team_mitigation_leader,
        team_healing_leader: accolades.team_healing_leader,
        team_damage_conversion_leader: accolades.team_damage_conversion_leader,
        team_gold_leader: accolades.team_gold_leader,
        kills: participant.stats.kills,
        deaths: participant.stats.deaths,
        assists: participant.stats.assists,
        team_kills: team_totals.kills,
        team_deaths: team_totals.deaths,
        game_kills: game_totals.kills,
        game_deaths: game_totals.deaths,
        team_tower_kills: tower_kills_for_team(game, participant.team_id),
        game_tower_kills: game_tower_kills(game),
        team_player_count,
        game_player_count: game.participants.len() as u32,
        team_puuids: team_puuids_for_participant(game, participant),
        team_composition: game
            .participants
            .iter()
            .filter(|candidate| same_stat_team(game, participant, candidate))
            .map(rating_composition_entry)
            .collect(),
        game_composition: game
            .participants
            .iter()
            .map(rating_composition_entry)
            .collect(),
        premade_teammate_puuids: Vec::new(),
        ally_players: related_players_for_participant(game, participant, true),
        enemy_players: related_players_for_participant(game, participant, false),
        premade_source: String::new(),
        premade_together_times: 0,
        kda,
        cs,
        gold_earned: participant.stats.gold_earned,
        damage_to_champions: participant.stats.total_damage_dealt_to_champions,
        team_damage_to_champions: team_totals.damage_to_champions,
        game_damage_to_champions: game_totals.damage_to_champions,
        damage_self_mitigated: participant.stats.damage_self_mitigated,
        total_damage_taken: participant.stats.total_damage_taken,
        team_damage_self_mitigated: team_totals.damage_self_mitigated,
        team_total_damage_taken: team_totals.total_damage_taken,
        total_heal: participant.stats.total_heal,
        team_total_heal: team_totals.total_heal,
        team_gold_earned: team_totals.gold_earned,
        game_gold_earned: game_totals.gold_earned,
        enemy_champion_immobilizations: participant.stats.enemy_champion_immobilizations,
        team_enemy_champion_immobilizations: team_totals.enemy_champion_immobilizations,
        immobilize_and_kill_with_ally: participant.stats.immobilize_and_kill_with_ally,
        team_immobilize_and_kill_with_ally: team_totals.immobilize_and_kill_with_ally,
        game_creation: game.game_creation,
        game_duration: game.game_duration,
    }
}

fn rating_composition_entry(participant: &Participant) -> RatingCompositionEntry {
    RatingCompositionEntry {
        champion_id: participant.champion_id,
        item_ids: nonzero_values([
            participant.stats.item0,
            participant.stats.item1,
            participant.stats.item2,
            participant.stats.item3,
            participant.stats.item4,
            participant.stats.item5,
            participant.stats.item6,
        ]),
    }
}

fn team_puuids_for_participant(game: &Game, participant: &Participant) -> Vec<String> {
    game.participants
        .iter()
        .filter(|candidate| same_stat_team(game, participant, candidate))
        .filter_map(|candidate| {
            let identity = identity_for_participant(game, candidate.participant_id);
            is_real_puuid(&identity.puuid).then_some(identity.puuid)
        })
        .collect()
}

fn related_players_for_participant(
    game: &Game,
    participant: &Participant,
    same_side: bool,
) -> Vec<RecentGameParticipantSummary> {
    game.participants
        .iter()
        .filter(|candidate| {
            let is_same_side = same_stat_team(game, participant, candidate);
            if same_side {
                is_same_side && candidate.participant_id != participant.participant_id
            } else {
                !is_same_side
            }
        })
        .map(|candidate| {
            let identity = identity_for_participant(game, candidate.participant_id);
            let display_name = participant_display_name(&identity);
            RecentGameParticipantSummary {
                puuid: identity.puuid,
                summoner_name: display_name,
                champion_id: candidate.champion_id,
            }
        })
        .collect()
}

fn participant_display_name(identity: &IdentityPlayer) -> String {
    if !identity.summoner_name.trim().is_empty() {
        return identity.summoner_name.trim().to_string();
    }

    let game_name = identity.game_name.trim();
    let tag_line = identity.tag_line.trim();

    if !game_name.is_empty() && !tag_line.is_empty() {
        return format!("{}#{}", game_name, tag_line);
    }

    if !game_name.is_empty() {
        return game_name.to_string();
    }

    if !tag_line.is_empty() {
        return tag_line.to_string();
    }

    String::new()
}

fn infer_recent_game_premades(owner_puuid: &str, recent_games: &mut [RecentGame]) {
    let owner = normalize_puuid(owner_puuid);
    if owner.is_empty() {
        return;
    }

    let mut pair_games = HashMap::<(String, String), HashSet<u64>>::new();

    for game in recent_games.iter() {
        let same_side = normalized_same_side_puuids(game, &owner);
        if same_side.len() < 2 {
            continue;
        }

        for i in 0..same_side.len().saturating_sub(1) {
            for j in (i + 1)..same_side.len() {
                pair_games
                    .entry((same_side[i].clone(), same_side[j].clone()))
                    .or_default()
                    .insert(game.game_id);
            }
        }
    }

    for game in recent_games.iter_mut() {
        let same_side = normalized_same_side_puuids(game, &owner);
        let Some((group, together_times)) =
            inferred_premade_group_for_game(&owner, &same_side, &pair_games)
        else {
            game.premade_teammate_puuids.clear();
            game.premade_source.clear();
            game.premade_together_times = 0;
            continue;
        };

        game.premade_teammate_puuids = group
            .into_iter()
            .filter(|puuid| puuid != &owner)
            .collect();
        game.premade_source = "历史同队".to_string();
        game.premade_together_times = together_times;
    }
}

fn normalized_same_side_puuids(game: &RecentGame, owner_puuid: &str) -> Vec<String> {
    let mut same_side = game
        .team_puuids
        .iter()
        .map(|puuid| normalize_puuid(puuid))
        .filter(|puuid| !puuid.is_empty())
        .collect::<Vec<_>>();

    if !owner_puuid.is_empty() {
        same_side.push(owner_puuid.to_string());
    }

    same_side.sort();
    same_side.dedup();
    same_side
}

fn inferred_premade_group_for_game(
    owner_puuid: &str,
    same_side: &[String],
    pair_games: &HashMap<(String, String), HashSet<u64>>,
) -> Option<(Vec<String>, usize)> {
    if same_side.len() < 2 || same_side.len() > 10 {
        return None;
    }

    if !same_side.iter().any(|puuid| puuid == owner_puuid) {
        return None;
    }

    let teammates = same_side
        .iter()
        .filter(|puuid| puuid.as_str() != owner_puuid)
        .cloned()
        .collect::<Vec<_>>();

    if teammates.is_empty() {
        return None;
    }

    let mut best_group = Vec::<String>::new();
    let mut best_times = 0usize;
    let total_masks = 1usize << teammates.len();

    for mask in 1..total_masks {
        let mut group = vec![owner_puuid.to_string()];
        for (index, teammate) in teammates.iter().enumerate() {
            if (mask & (1usize << index)) != 0 {
                group.push(teammate.clone());
            }
        }

        let Some(times) = premade_subset_times(&group, pair_games) else {
            continue;
        };

        if times < HISTORY_PREMADE_THRESHOLD {
            continue;
        }

        if group.len() > best_group.len()
            || (group.len() == best_group.len()
                && (times > best_times || (times == best_times && group < best_group)))
        {
            best_group = group;
            best_times = times;
        }
    }

    (!best_group.is_empty()).then_some((best_group, best_times))
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

async fn load_stats_until_solo_sample(
    lcu: &LcuClient,
    auth: &ClientAuth,
    sgp_server_id: Option<&str>,
    summoner: &SummonerInfo,
) -> AppResult<PlayerStatsResponse> {
    let mut depth = PLAYER_ELO_BATCH_DEPTH;

    loop {
        let stats = load_player_stats(
            lcu,
            Some(auth),
            sgp_server_id,
            summoner.clone(),
            depth,
        )
        .await?;

        let solo_games = stats
            .recent_games
            .iter()
            .filter(|game| is_solo_game(game))
            .take(PLAYER_ELO_SAMPLE_TARGET)
            .count();
        let no_more_games = stats.depth_loaded < depth;

        if solo_games >= PLAYER_ELO_SAMPLE_TARGET || no_more_games || depth >= PLAYER_ELO_MAX_DEPTH
        {
            return Ok(stats);
        }

        depth = (depth + PLAYER_ELO_BATCH_DEPTH).min(PLAYER_ELO_MAX_DEPTH);
    }
}

fn elo_participant_key(player: &MatchDetailPlayer) -> String {
    if is_real_puuid(&player.puuid) {
        return normalize_puuid(&player.puuid);
    }

    normalized_riot_id(player.game_name.as_str(), player.tag_line.as_str())
}

fn is_solo_game(game: &RecentGame) -> bool {
    (game.premade_teammate_puuids.len()) == 0
}

fn is_solo_detail_player(player: &MatchDetailPlayer, players: &[MatchDetailPlayer]) -> bool {
    if player.player_subteam_id == 0 {
        return true;
    }

    players
        .iter()
        .filter(|candidate| candidate.player_subteam_id == player.player_subteam_id)
        .count()
        <= 1
}

fn trim_extreme_elos(values: &[f64]) -> Vec<f64> {
    if values.len() <= 2 {
        return values.to_vec();
    }

    let mut trimmed = values.to_vec();
    let Some(max_index) = trimmed
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
    else {
        return values.to_vec();
    };
    trimmed.remove(max_index);

    let Some(min_index) = trimmed
        .iter()
        .enumerate()
        .min_by(|left, right| left.1.total_cmp(right.1))
        .map(|(index, _)| index)
    else {
        return values.to_vec();
    };
    trimmed.remove(min_index);
    trimmed
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.iter().sum::<f64>() / values.len() as f64
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let middle = sorted.len() / 2;

    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    }
}

fn lzyumi_area_for_sgp_server(sgp_server_id: &str) -> Option<EloArea> {
    let (id, name) = match sgp_server_id {
        "TENCENT_HN1" => (1, "艾欧尼亚"),
        "TENCENT_HN10" => (14, "黑色玫瑰"),
        "TENCENT_BGP2" => (31, "峡谷之巅"),
        _ => return None,
    };

    Some(EloArea { id, name })
}

fn is_lzyumi_supported_formal_server(sgp_server_id: &str) -> bool {
    matches!(
        sgp_server_id,
        "TENCENT_HN1"
            | "TENCENT_HN10"
            | "TENCENT_NJ100"
            | "TENCENT_GZ100"
            | "TENCENT_CQ100"
            | "TENCENT_TJ100"
            | "TENCENT_TJ101"
            | "TENCENT_BGP2"
    )
}

fn lzyumi_area_candidates_for_sgp_server(sgp_server_id: &str) -> Vec<EloArea> {
    if let Some(area) = lzyumi_area_for_sgp_server(sgp_server_id) {
        return vec![area];
    }

    if !is_lzyumi_supported_formal_server(sgp_server_id) {
        return Vec::new();
    }

    LZYUMI_SUPPORTED_AREAS
        .iter()
        .map(|(id, name)| EloArea {
            id: *id,
            name,
        })
        .collect()
}

fn display_server_name(sgp_server_id: &str) -> &str {
    match sgp_server_id {
        "TENCENT_HN1" => "艾欧尼亚",
        "TENCENT_HN10" => "黑色玫瑰",
        "TENCENT_NJ100" => "联盟一区",
        "TENCENT_GZ100" => "联盟二区",
        "TENCENT_CQ100" => "联盟三区",
        "TENCENT_TJ100" => "联盟四区",
        "TENCENT_TJ101" => "联盟五区",
        "TENCENT_BGP2" => "峡谷之巅",
        _ => sgp_server_id,
    }
}

async fn resolve_lzyumi_area_for_summoner(
    sgp_server_id: &str,
    game_name: &str,
    tag_line: &str,
    display_name: Option<&str>,
) -> AppResult<EloArea> {
    if let Some(area) = lzyumi_area_for_sgp_server(sgp_server_id) {
        return Ok(area);
    }

    let candidates = lzyumi_area_candidates_for_sgp_server(sgp_server_id);
    if candidates.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "当前大区 {sgp_server_id} 暂不支持外部 ELO 估算"
        )));
    }

    for area in candidates {
        let elo = fetch_external_aram_elo_with_options(
            &area,
            game_name,
            tag_line,
            display_name,
            owner_external_elo_fetch_options(),
        )
        .await?;

        if elo.is_some() {
            return Ok(area);
        }
    }

    let riot_id = normalized_riot_id_with_fallback(game_name, tag_line, display_name);
    Err(AppError::SgpUnavailable(format!(
        "在 ELO 查询网站上用用户名 {} 和区服 {} 无法查到 ELO 分数",
        riot_id,
        display_server_name(sgp_server_id)
    )))
}

async fn fetch_external_aram_elo(
    area: &EloArea,
    game_name: &str,
    tag_line: &str,
    display_name: Option<&str>,
) -> AppResult<Option<f64>> {
    fetch_external_aram_elo_with_options(
        area,
        game_name,
        tag_line,
        display_name,
        owner_external_elo_fetch_options(),
    )
    .await
}

fn owner_external_elo_fetch_options() -> ExternalEloFetchOptions {
    ExternalEloFetchOptions {
        http_timeout: PLAYER_ELO_OWNER_HTTP_TIMEOUT,
        retry_count: EXTERNAL_ELO_OWNER_RETRY_COUNT,
        retry_delay: EXTERNAL_ELO_RETRY_DELAY,
        allow_powershell_fallback: true,
    }
}

fn participant_external_elo_fetch_options() -> ExternalEloFetchOptions {
    ExternalEloFetchOptions {
        http_timeout: PLAYER_ELO_PARTICIPANT_HTTP_TIMEOUT,
        retry_count: EXTERNAL_ELO_PARTICIPANT_RETRY_COUNT,
        retry_delay: EXTERNAL_ELO_RETRY_DELAY,
        allow_powershell_fallback: true,
    }
}

async fn fetch_external_aram_elo_with_options(
    area: &EloArea,
    game_name: &str,
    tag_line: &str,
    display_name: Option<&str>,
    options: ExternalEloFetchOptions,
) -> AppResult<Option<f64>> {
    let normalized_id = normalized_riot_id_with_fallback(game_name, tag_line, display_name);
    if normalized_id.is_empty() {
        return Ok(None);
    }

    let cache_key = format!("{}:{}", area.id, normalized_id);
    if let Some(cached) = get_cached_external_elo(&cache_key) {
        return Ok(cached);
    }
    if let Some(cached) = read_db_cached_external_elo(&cache_key, false) {
        store_cached_external_elo(cache_key.clone(), cached);
        return Ok(cached);
    }

    let http = build_external_elo_http_client(options.http_timeout)?;
    let area_id = area.id.to_string();
    let mut parsed = None;

    for attempt in 0..options.retry_count {
        let (sign, sign_str) = lzyumi_sign_components();
        let request_nonce = external_elo_request_nonce();
        let info_params = vec![
            ("nickname", normalized_id.clone()),
            ("allCount", "10".to_string()),
            ("areaId", area_id.clone()),
            ("areaName", area.name.to_string()),
            ("seleMe", "1".to_string()),
            ("filter", "5".to_string()),
            ("openId", String::new()),
            ("lzyumiSign", sign.clone()),
            ("signStr", sign_str.clone()),
            ("_t", request_nonce.clone()),
        ];
        let info_text = http
            .get("https://a.lzyumi.top/lzyumi/lol/info")
            .query(&info_params)
            .send()
            .await?
            .text()
            .await?;
        let mut info = parse_external_json::<LzyumiInfoResponse>(&info_text, "外部 ELO info 响应")?;
        if info.battle_info.is_none() && options.allow_powershell_fallback {
            let fallback_text =
                fetch_external_info_text_via_powershell(area, normalized_id.as_str())?;
            info = parse_external_json::<LzyumiInfoResponse>(
                &fallback_text,
                "外部 ELO info 回退响应",
            )?;
        }

        let Some(battle_info) = info.battle_info else {
            if attempt + 1 < options.retry_count {
                tokio::time::sleep(options.retry_delay).await;
                continue;
            }
            break;
        };

        if battle_info.open_id.trim().is_empty() {
            if attempt + 1 < options.retry_count {
                tokio::time::sleep(options.retry_delay).await;
                continue;
            }
            break;
        }

        let returned_id = normalize_search_name(&battle_info.name_info_new);
        if !returned_id.is_empty() && returned_id != normalize_search_name(&normalized_id) {
            if attempt + 1 < options.retry_count {
                tokio::time::sleep(options.retry_delay).await;
                continue;
            }
            break;
        }

        let (sign, sign_str) = lzyumi_sign_components();
        let request_nonce = external_elo_request_nonce();
        let elo_params = vec![
            ("openId", battle_info.open_id.clone()),
            ("areaId", area_id.clone()),
            ("lzyumiSign", sign),
            ("signStr", sign_str),
            ("filter", "5".to_string()),
            ("_t", request_nonce),
        ];
        let elo_text = http
            .get("https://a.lzyumi.top/lzyumi/lol/info/getRankEloInfo")
            .query(&elo_params)
            .send()
            .await?
            .text()
            .await?
            ;
        let mut elo_response =
            parse_external_json::<LzyumiEloResponse>(&elo_text, "外部 ELO rank 响应")?;
        let mut elo_text_value = elo_response
            .data
            .as_ref()
            .map(|data| data.data_rank_elo_num.trim().to_string())
            .unwrap_or_default();
        if elo_text_value.is_empty() && options.allow_powershell_fallback {
            let fallback_text =
                fetch_external_rank_text_via_powershell(battle_info.open_id.as_str(), area.id, 5)?;
            elo_response = parse_external_json::<LzyumiEloResponse>(
                &fallback_text,
                "外部 ELO rank 回退响应",
            )?;
            elo_text_value = elo_response
                .data
                .as_ref()
                .map(|data| data.data_rank_elo_num.trim().to_string())
                .unwrap_or_default();
        }

        parsed = parse_lzyumi_elo(elo_text_value.as_str());
        if parsed.is_some() || attempt + 1 >= options.retry_count {
            break;
        }

        tokio::time::sleep(options.retry_delay).await;
    }

    if parsed.is_none() {
        if let Some(stale_elo) = read_db_cached_external_elo(&cache_key, true).flatten() {
            store_cached_external_elo(cache_key, Some(stale_elo));
            return Ok(Some(stale_elo));
        }
    }

    store_cached_external_elo(cache_key.clone(), parsed);
    write_db_cached_external_elo(&cache_key, parsed);
    Ok(parsed)
}

fn get_cached_external_elo(cache_key: &str) -> Option<Option<f64>> {
    let mut cache = EXTERNAL_ELO_CACHE.lock().ok()?;
    let Some(entry) = cache.get(cache_key) else {
        return None;
    };

    if entry.stored_at.elapsed() <= entry.ttl {
        return Some(entry.elo);
    }

    cache.remove(cache_key);
    None
}

fn store_cached_external_elo(cache_key: String, elo: Option<f64>) {
    if let Ok(mut cache) = EXTERNAL_ELO_CACHE.lock() {
        cache.insert(
            cache_key,
            ExternalEloCacheEntry {
                stored_at: Instant::now(),
                elo,
                ttl: if elo.is_some() {
                    EXTERNAL_ELO_SUCCESS_CACHE_TTL
                } else {
                    EXTERNAL_ELO_FAILURE_CACHE_TTL
                },
            },
        );
    }
}

fn build_external_elo_http_client(timeout: Duration) -> AppResult<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(EXTERNAL_ELO_USER_AGENT),
    );
    headers.insert(
        reqwest::header::REFERER,
        reqwest::header::HeaderValue::from_static(EXTERNAL_ELO_REFERER),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(
        reqwest::header::ACCEPT_LANGUAGE,
        reqwest::header::HeaderValue::from_static("zh-CN,zh;q=0.9"),
    );
    headers.insert(
        reqwest::header::CACHE_CONTROL,
        reqwest::header::HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        reqwest::header::PRAGMA,
        reqwest::header::HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("x-requested-with"),
        reqwest::header::HeaderValue::from_static("XMLHttpRequest"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-site"),
        reqwest::header::HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-mode"),
        reqwest::header::HeaderValue::from_static("cors"),
    );
    headers.insert(
        reqwest::header::HeaderName::from_static("sec-fetch-dest"),
        reqwest::header::HeaderValue::from_static("empty"),
    );

    Ok(reqwest::Client::builder()
        .default_headers(headers)
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()?)
}

fn external_elo_request_nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn parse_external_json<T>(text: &str, context: &str) -> AppResult<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    serde_json::from_str(text)
        .map_err(|error| AppError::InvalidInput(format!("{context} 解析失败：{error}")))
}

fn fetch_external_info_text_via_powershell(area: &EloArea, normalized_id: &str) -> AppResult<String> {
    let nickname = normalized_id.replace('\'', "''");
    let area_name = area.name.replace('\'', "''");
    let script = format!(
        "$nickname='{nickname}';$areaId='{area_id}';$areaName='{area_name}';$date=Get-Date;$m=$date.Month.ToString();$d=$date.Day.ToString();$h=$date.Hour.ToString();$mi=$date.Minute.ToString();$s=$date.Second.ToString();$signSource=('dld{{0:d2}}o{{1:d2}}u{{2:d2}}d{{3:d2}}o{{4:d2}}dld' -f $date.Month,$date.Day,$date.Hour,$date.Minute,$date.Second);$md5=[System.Security.Cryptography.MD5]::Create();$bytes=[Text.Encoding]::UTF8.GetBytes($signSource);$hash=-join ($md5.ComputeHash($bytes) | ForEach-Object {{ $_.ToString('x2') }});$signStr=\"$m$d$h$mi$s$($m.Length*3)$($d.Length*3)$($h.Length*3)$($mi.Length*3)$($s.Length*3)\";$query=\"nickname=$([uri]::EscapeDataString($nickname))&allCount=10&areaId=$areaId&areaName=$([uri]::EscapeDataString($areaName))&seleMe=1&filter=5&openId=&lzyumiSign=$hash&signStr=$signStr\";$headers=@{{ 'User-Agent'='{user_agent}'; 'Referer'='{referer}'; 'Accept'='application/json, text/plain, */*'; 'Accept-Language'='zh-CN,zh;q=0.9'; 'Pragma'='no-cache'; 'Cache-Control'='no-cache'; 'Sec-Fetch-Site'='same-origin'; 'Sec-Fetch-Mode'='cors'; 'Sec-Fetch-Dest'='empty'; 'X-Requested-With'='XMLHttpRequest' }};$url='https://a.lzyumi.top/lzyumi/lol/info?'+$query;(Invoke-WebRequest -Uri $url -Headers $headers -UseBasicParsing).Content",
        nickname = nickname,
        area_id = area.id,
        area_name = area_name,
        user_agent = EXTERNAL_ELO_USER_AGENT,
        referer = EXTERNAL_ELO_REFERER,
    );
    run_powershell_script(&script)
}

fn fetch_external_rank_text_via_powershell(
    open_id: &str,
    area_id: u32,
    filter: u32,
) -> AppResult<String> {
    let open_id = open_id.replace('\'', "''");
    let script = format!(
        "$openId='{open_id}';$areaId='{area_id}';$filter='{filter}';$date=Get-Date;$m=$date.Month.ToString();$d=$date.Day.ToString();$h=$date.Hour.ToString();$mi=$date.Minute.ToString();$s=$date.Second.ToString();$signSource=('dld{{0:d2}}o{{1:d2}}u{{2:d2}}d{{3:d2}}o{{4:d2}}dld' -f $date.Month,$date.Day,$date.Hour,$date.Minute,$date.Second);$md5=[System.Security.Cryptography.MD5]::Create();$bytes=[Text.Encoding]::UTF8.GetBytes($signSource);$hash=-join ($md5.ComputeHash($bytes) | ForEach-Object {{ $_.ToString('x2') }});$signStr=\"$m$d$h$mi$s$($m.Length*3)$($d.Length*3)$($h.Length*3)$($mi.Length*3)$($s.Length*3)\";$query=\"openId=$([uri]::EscapeDataString($openId))&areaId=$areaId&lzyumiSign=$hash&signStr=$signStr&filter=$filter\";$headers=@{{ 'User-Agent'='{user_agent}'; 'Referer'='{referer}'; 'Accept'='application/json, text/plain, */*'; 'Accept-Language'='zh-CN,zh;q=0.9'; 'Pragma'='no-cache'; 'Cache-Control'='no-cache'; 'Sec-Fetch-Site'='same-origin'; 'Sec-Fetch-Mode'='cors'; 'Sec-Fetch-Dest'='empty'; 'X-Requested-With'='XMLHttpRequest' }};$url='https://a.lzyumi.top/lzyumi/lol/info/getRankEloInfo?'+$query;(Invoke-WebRequest -Uri $url -Headers $headers -UseBasicParsing).Content",
        open_id = open_id,
        area_id = area_id,
        filter = filter,
        user_agent = EXTERNAL_ELO_USER_AGENT,
        referer = EXTERNAL_ELO_REFERER,
    );
    run_powershell_script(&script)
}

fn run_powershell_script(script: &str) -> AppResult<String> {
    let output = Command::new("powershell")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::InvalidInput(format!(
            "外部 ELO 系统请求失败：{}",
            if stderr.is_empty() {
                "PowerShell 返回非零退出码".to_string()
            } else {
                stderr
            }
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn lzyumi_sign_components() -> (String, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let total_seconds = now % 86_400;
    let hours = (total_seconds / 3_600) as u32;
    let minutes = ((total_seconds % 3_600) / 60) as u32;
    let seconds = (total_seconds % 60) as u32;
    let days_since_epoch = (now / 86_400) as i64;
    let (_, month, day) = civil_from_days(days_since_epoch);

    let month_raw = month.to_string();
    let day_raw = day.to_string();
    let hour_raw = hours.to_string();
    let minute_raw = minutes.to_string();
    let second_raw = seconds.to_string();

    let sign_source = format!(
        "dld{:02}o{:02}u{:02}d{:02}o{:02}dld",
        month, day, hours, minutes, seconds
    );
    let sign = format!("{:x}", md5::compute(sign_source));
    let sign_str = format!(
        "{}{}{}{}{}{}{}{}{}{}",
        month_raw,
        day_raw,
        hour_raw,
        minute_raw,
        second_raw,
        month_raw.len() * 3,
        day_raw.len() * 3,
        hour_raw.len() * 3,
        minute_raw.len() * 3,
        second_raw.len() * 3,
    );

    (sign, sign_str)
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u32, day as u32)
}

fn parse_lzyumi_elo(value: &str) -> Option<f64> {
    let digits = value
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();

    if digits.is_empty() {
        return None;
    }

    digits.parse::<f64>().ok()
}

fn display_name_from_parts(summoner_name: &str, game_name: &str, tag_line: &str) -> String {
    if !summoner_name.trim().is_empty() {
        return summoner_name.trim().to_string();
    }

    if !game_name.trim().is_empty() && !tag_line.trim().is_empty() {
        return format!("{}#{}", game_name.trim(), tag_line.trim());
    }

    if !game_name.trim().is_empty() {
        return game_name.trim().to_string();
    }

    tag_line.trim().to_string()
}

fn normalized_riot_id(game_name: &str, tag_line: &str) -> String {
    let normalized_game_name = game_name.trim();
    let normalized_tag_line = tag_line.trim();

    if normalized_game_name.is_empty() || normalized_tag_line.is_empty() {
        return String::new();
    }

    format!("{normalized_game_name}#{normalized_tag_line}")
}

fn normalized_riot_id_with_fallback(
    game_name: &str,
    tag_line: &str,
    display_name: Option<&str>,
) -> String {
    let direct = normalized_riot_id(game_name, tag_line);
    if !direct.is_empty() {
        return direct;
    }

    let Some(display_name) = display_name.map(str::trim) else {
        return String::new();
    };
    let Some((fallback_game_name, fallback_tag_line)) = display_name.split_once('#') else {
        return String::new();
    };

    normalized_riot_id(fallback_game_name, fallback_tag_line)
}

fn normalize_search_name(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_real_puuid(puuid: &str) -> bool {
    !puuid.trim().is_empty() && puuid != "00000000-0000-0000-0000-000000000000"
}

fn normalize_puuid(puuid: &str) -> String {
    puuid.trim().to_ascii_lowercase()
}

fn identity_for_participant(game: &Game, participant_id: u32) -> IdentityPlayer {
    game.participant_identities
        .iter()
        .find(|identity| identity.participant_id == participant_id)
        .map(|identity| identity.player.clone())
        .unwrap_or_default()
}

fn team_name(team_id: u32) -> String {
    match team_id {
        100 => "蓝方".to_string(),
        200 => "红方".to_string(),
        _ => format!("队伍 {team_id}"),
    }
}

fn champion_accumulator_to_stat(c: ChampionAccumulator, total_games: usize) -> ChampionStat {
    let losses = c.games.saturating_sub(c.wins);
    let damage_share = ratio_u64(c.damage_to_champions, c.team_damage_to_champions);
    let gold_share = ratio_u64(c.gold_earned, c.team_gold_earned);
    let damage_conversion_rate = if gold_share == 0.0 {
        0.0
    } else {
        round2(damage_share / gold_share)
    };

    ChampionStat {
        champion_id: c.champion_id,
        games: c.games,
        wins: c.wins,
        losses,
        win_rate: ratio(c.wins, c.games),
        pick_rate: ratio(c.games, total_games),
        average_kda: calc_kda(c.kills, c.deaths, c.assists),
        average_kills: average(c.kills as u64, c.games),
        average_deaths: average(c.deaths as u64, c.games),
        average_assists: average(c.assists as u64, c.games),
        average_damage_to_champions: average(c.damage_to_champions, c.games),
        average_cs: average(c.cs, c.games),
        damage_share,
        damage_conversion_rate,
        mitigation_share: ratio_u64(
            c.damage_self_mitigated.saturating_add(c.total_damage_taken),
            c.team_damage_self_mitigated
                .saturating_add(c.team_total_damage_taken),
        ),
        healing_share: ratio_u64(c.total_heal, c.team_total_heal),
        gold_share,
        last_played_at: c.last_played_at,
    }
}

fn nonzero_values<const N: usize>(values: [u32; N]) -> Vec<u32> {
    values.into_iter().filter(|value| *value > 0).collect()
}

fn participant_for_puuid<'a>(game: &'a Game, puuid: &str) -> Option<&'a Participant> {
    if let Some(identity) = game
        .participant_identities
        .iter()
        .find(|i| i.player.puuid.eq_ignore_ascii_case(puuid))
    {
        return game
            .participants
            .iter()
            .find(|p| p.participant_id == identity.participant_id);
    }

    // LCU 的玩家个人战绩 summary 通常把目标玩家放在 participants[0]。
    game.participants.first()
}

fn team_totals_for_participant(game: &Game, participant: &Participant) -> TeamTotals {
    game.participants
        .iter()
        .filter(|candidate| same_stat_team(game, participant, candidate))
        .fold(TeamTotals::default(), |mut totals, candidate| {
            totals.damage_to_champions += candidate.stats.total_damage_dealt_to_champions;
            totals.gold_earned += candidate.stats.gold_earned;
            totals.damage_self_mitigated += candidate.stats.damage_self_mitigated;
            totals.total_damage_taken += candidate.stats.total_damage_taken;
            totals.total_heal += candidate.stats.total_heal;
            totals.enemy_champion_immobilizations += candidate.stats.enemy_champion_immobilizations;
            totals.immobilize_and_kill_with_ally += candidate.stats.immobilize_and_kill_with_ally;
            totals.kills += candidate.stats.kills;
            totals.deaths += candidate.stats.deaths;
            totals
        })
}

fn game_totals(game: &Game) -> TeamTotals {
    game.participants
        .iter()
        .fold(TeamTotals::default(), |mut totals, participant| {
            totals.damage_to_champions += participant.stats.total_damage_dealt_to_champions;
            totals.gold_earned += participant.stats.gold_earned;
            totals.damage_self_mitigated += participant.stats.damage_self_mitigated;
            totals.total_damage_taken += participant.stats.total_damage_taken;
            totals.total_heal += participant.stats.total_heal;
            totals.enemy_champion_immobilizations +=
                participant.stats.enemy_champion_immobilizations;
            totals.immobilize_and_kill_with_ally += participant.stats.immobilize_and_kill_with_ally;
            totals.kills += participant.stats.kills;
            totals.deaths += participant.stats.deaths;
            totals
        })
}

fn tower_kills_for_team(game: &Game, team_id: u32) -> u32 {
    game.teams
        .iter()
        .find(|team| team.team_id == team_id)
        .map(|team| team.tower_kills)
        .unwrap_or_else(|| {
            game.participants
                .iter()
                .filter(|participant| participant.team_id == team_id)
                .map(|participant| participant.stats.turret_kills)
                .sum()
        })
}

fn game_tower_kills(game: &Game) -> u32 {
    if game.teams.is_empty() {
        return game
            .participants
            .iter()
            .map(|participant| participant.stats.turret_kills)
            .sum();
    }

    game.teams.iter().map(|team| team.tower_kills).sum()
}

fn game_accolades_for_participant(
    game: &Game,
    participant: &Participant,
    team_totals: &TeamTotals,
) -> GameAccolades {
    let team_participants = game
        .participants
        .iter()
        .filter(|candidate| same_stat_team(game, participant, candidate))
        .collect::<Vec<_>>();

    let team_damage_max = team_participants
        .iter()
        .map(|candidate| candidate.stats.total_damage_dealt_to_champions)
        .max()
        .unwrap_or(0);
    let game_damage_max = game
        .participants
        .iter()
        .map(|candidate| candidate.stats.total_damage_dealt_to_champions)
        .max()
        .unwrap_or(0);
    let team_mitigation_max = team_participants
        .iter()
        .map(|candidate| participant_mitigation_value(candidate))
        .max()
        .unwrap_or(0);
    let team_healing_max = team_participants
        .iter()
        .map(|candidate| candidate.stats.total_heal)
        .max()
        .unwrap_or(0);
    let team_gold_max = team_participants
        .iter()
        .map(|candidate| candidate.stats.gold_earned)
        .max()
        .unwrap_or(0);
    let team_damage_conversion_max = team_participants
        .iter()
        .map(|candidate| participant_damage_conversion(candidate, team_totals))
        .fold(0.0, f64::max);

    let my_damage = participant.stats.total_damage_dealt_to_champions;
    let my_damage_conversion = participant_damage_conversion(participant, team_totals);

    GameAccolades {
        team_damage_leader: is_u32_leader(my_damage, team_damage_max),
        game_damage_leader: is_u32_leader(my_damage, game_damage_max),
        team_mitigation_leader: is_u32_leader(
            participant_mitigation_value(participant),
            team_mitigation_max,
        ),
        team_healing_leader: is_u32_leader(participant.stats.total_heal, team_healing_max),
        team_damage_conversion_leader: team_damage_conversion_max > 0.0
            && my_damage_conversion >= team_damage_conversion_max,
        team_gold_leader: is_u32_leader(participant.stats.gold_earned, team_gold_max),
    }
}

fn participant_damage_conversion(participant: &Participant, team_totals: &TeamTotals) -> f64 {
    let damage_share = if team_totals.damage_to_champions == 0 {
        0.0
    } else {
        participant.stats.total_damage_dealt_to_champions as f64
            / team_totals.damage_to_champions as f64
    };
    let gold_share = if team_totals.gold_earned == 0 {
        0.0
    } else {
        participant.stats.gold_earned as f64 / team_totals.gold_earned as f64
    };

    if gold_share == 0.0 {
        0.0
    } else {
        damage_share / gold_share
    }
}

fn participant_mitigation_value(participant: &Participant) -> u32 {
    participant
        .stats
        .damage_self_mitigated
        .saturating_add(participant.stats.total_damage_taken)
}

fn is_u32_leader(value: u32, max: u32) -> bool {
    max > 0 && value >= max
}

fn same_stat_team(game: &Game, me: &Participant, other: &Participant) -> bool {
    if should_group_by_subteam(game) && me.stats.player_subteam_id > 0 {
        return me.stats.player_subteam_id == other.stats.player_subteam_id;
    }

    me.team_id == other.team_id
}

fn should_group_by_subteam(game: &Game) -> bool {
    matches!(game.game_mode.as_str(), "CHERRY" | "STRAWBERRY" | "KIWI")
        || matches!(game.queue_id, 1700 | 1710 | 1711 | 1712 | 2400)
}

fn calc_kda(kills: u32, deaths: u32, assists: u32) -> f64 {
    round2((kills + assists) as f64 / deaths.max(1) as f64)
}

fn average(total: u64, count: usize) -> f64 {
    if count == 0 {
        0.0
    } else {
        round2(total as f64 / count as f64)
    }
}

fn ratio(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        round2(part as f64 / total as f64)
    }
}

fn ratio_u64(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        round2(part as f64 / total as f64)
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn is_pve_queue(queue_id: u32) -> bool {
    matches!(
        queue_id,
        31 | 32
            | 33
            | 34
            | 35
            | 36
            | 52
            | 90
            | 91
            | 92
            | 800
            | 801
            | 810
            | 820
            | 830
            | 831
            | 832
            | 840
            | 841
            | 842
            | 850
            | 851
            | 852
            | 860
            | 870
            | 880
            | 890
            | 950
            | 951
            | 960
            | 961
            | 981
            | 982
            | 990
            | 1030
            | 1031
            | 1032
            | 1040
            | 1041
            | 1050
            | 1051
            | 1060
            | 1061
            | 1070
            | 1071
            | 1800
            | 1810
            | 1820
            | 1830
            | 1840
            | 1850
            | 1860
            | 1870
            | 1880
            | 1890
            | 2000
            | 2010
            | 2020
            | 3140
    )
}

pub fn normalize_depth(depth: usize) -> usize {
    depth.clamp(1, MAX_DEPTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::client_discovery::discover_primary_client;
    use crate::services::lcu::LcuClient;

    fn sample_recent_game(game_id: u64, champion_id: u32) -> RecentGame {
        RecentGame {
            game_id,
            champion_id,
            queue_id: 450,
            game_mode: "ARAM".to_string(),
            win: true,
            team_id: 100,
            spell1_id: 0,
            spell2_id: 0,
            item_ids: Vec::new(),
            perk_ids: Vec::new(),
            augment_ids: Vec::new(),
            team_damage_leader: false,
            game_damage_leader: false,
            team_mitigation_leader: false,
            team_healing_leader: false,
            team_damage_conversion_leader: false,
            team_gold_leader: false,
            kills: 0,
            deaths: 0,
            assists: 0,
            team_kills: 0,
            team_deaths: 0,
            game_kills: 0,
            game_deaths: 0,
            team_tower_kills: 0,
            game_tower_kills: 0,
            team_player_count: 5,
            game_player_count: 10,
            team_puuids: Vec::new(),
            team_composition: Vec::new(),
            game_composition: Vec::new(),
            premade_teammate_puuids: Vec::new(),
            ally_players: Vec::new(),
            enemy_players: Vec::new(),
            premade_source: String::new(),
            premade_together_times: 0,
            kda: 0.0,
            cs: 0,
            gold_earned: 0,
            damage_to_champions: 0,
            team_damage_to_champions: 0,
            game_damage_to_champions: 0,
            damage_self_mitigated: 0,
            total_damage_taken: 0,
            team_damage_self_mitigated: 0,
            team_total_damage_taken: 0,
            total_heal: 0,
            team_total_heal: 0,
            team_gold_earned: 0,
            game_gold_earned: 0,
            enemy_champion_immobilizations: 0,
            team_enemy_champion_immobilizations: 0,
            immobilize_and_kill_with_ally: 0,
            team_immobilize_and_kill_with_ally: 0,
            game_creation: 0,
            game_duration: 1_200,
        }
    }

    fn sample_detail_player(
        participant_id: u32,
        puuid: &str,
        game_name: &str,
        tag_line: &str,
        summoner_name: &str,
        player_subteam_id: u32,
    ) -> MatchDetailPlayer {
        MatchDetailPlayer {
            participant_id,
            puuid: puuid.to_string(),
            game_name: game_name.to_string(),
            tag_line: tag_line.to_string(),
            summoner_name: summoner_name.to_string(),
            team_id: 100,
            player_subteam_id,
            record: sample_recent_game(1, 266),
        }
    }

    #[test]
    fn normalized_riot_id_with_fallback_prefers_direct_fields() {
        let value = normalized_riot_id_with_fallback("Alpha", "1234", Some("Beta#9999"));
        assert_eq!(value, "Alpha#1234");
    }

    #[test]
    fn normalized_riot_id_with_fallback_uses_display_name_when_needed() {
        let value = normalized_riot_id_with_fallback("", "", Some("离于爱者无忧无怖#53258"));
        assert_eq!(value, "离于爱者无忧无怖#53258");
    }

    #[test]
    fn lzyumi_area_candidates_include_probe_list_for_formal_servers() {
        let areas = lzyumi_area_candidates_for_sgp_server("TENCENT_NJ100");
        assert!(!areas.is_empty());
        assert!(areas.iter().any(|area| area.name == "艾欧尼亚"));
        assert!(areas.iter().any(|area| area.name == "黑色玫瑰"));
    }

    #[test]
    fn lzyumi_area_candidates_reject_non_formal_servers() {
        let areas = lzyumi_area_candidates_for_sgp_server("TENCENT_PBE");
        assert!(areas.is_empty());
    }

    #[test]
    fn is_solo_detail_player_respects_subteam_grouping() {
        let solo_player = sample_detail_player(1, "puuid-1", "Solo", "1111", "", 7);
        let duo_left = sample_detail_player(2, "puuid-2", "DuoA", "2222", "", 9);
        let duo_right = sample_detail_player(3, "puuid-3", "DuoB", "3333", "", 9);
        let players = vec![solo_player.clone(), duo_left.clone(), duo_right.clone()];

        assert!(is_solo_detail_player(&solo_player, &players));
        assert!(!is_solo_detail_player(&duo_left, &players));
        assert!(!is_solo_detail_player(&duo_right, &players));
    }

    #[test]
    fn is_solo_detail_player_treats_zero_subteam_as_solo() {
        let player = sample_detail_player(1, "puuid-1", "Solo", "1111", "", 0);
        let teammate = sample_detail_player(2, "puuid-2", "Other", "2222", "", 0);
        let players = vec![player.clone(), teammate];

        assert!(is_solo_detail_player(&player, &players));
    }

    #[tokio::test]
    #[ignore = "手动触发：诊断当前角色的外部大乱斗 ELO 查询"]
    async fn manual_fetch_current_owner_external_elo() {
        let auth = discover_primary_client().expect("应能发现客户端");
        let lcu = LcuClient::new(&auth).expect("应能创建 LCU client");
        let summoner = lcu.current_summoner().await.expect("应能读取当前召唤师");
        let sgp_server_id = resolve_sgp_server_id(&auth, None);
        let area = lzyumi_area_for_sgp_server(&sgp_server_id).expect("当前大区应支持外部 ELO");

        let elo = fetch_external_aram_elo(
            &area,
            summoner.game_name.as_str(),
            summoner.tag_line.as_str(),
            Some(summoner.display_name.as_str()),
        )
        .await
        .expect("外部 ELO 查询不应报错");

        println!(
            "manual_fetch_current_owner_external_elo => server='{}' area='{}' summoner='{}#{}' display='{}' elo={:?}",
            sgp_server_id,
            area.name,
            summoner.game_name,
            summoner.tag_line,
            summoner.display_name,
            elo
        );
    }

    #[tokio::test]
    #[ignore = "手动触发：打印外部 ELO 接口原始返回"]
    async fn manual_dump_current_owner_external_elo_payloads() {
        let auth = discover_primary_client().expect("应能发现客户端");
        let lcu = LcuClient::new(&auth).expect("应能创建 LCU client");
        let summoner = lcu.current_summoner().await.expect("应能读取当前召唤师");
        let sgp_server_id = resolve_sgp_server_id(&auth, None);
        let area = lzyumi_area_for_sgp_server(&sgp_server_id).expect("当前大区应支持外部 ELO");
        let http =
            build_external_elo_http_client(PLAYER_ELO_OWNER_HTTP_TIMEOUT).expect("应能创建 HTTP client");
        let (sign, sign_str) = lzyumi_sign_components();
        let area_id = area.id.to_string();
        let normalized_id = normalized_riot_id_with_fallback(
            summoner.game_name.as_str(),
            summoner.tag_line.as_str(),
            Some(summoner.display_name.as_str()),
        );

        let info_text = http
            .get("https://a.lzyumi.top/lzyumi/lol/info")
            .query(&vec![
                ("nickname", normalized_id.clone()),
                ("allCount", "10".to_string()),
                ("areaId", area_id.clone()),
                ("areaName", area.name.to_string()),
                ("seleMe", "1".to_string()),
                ("filter", "5".to_string()),
                ("openId", String::new()),
                ("lzyumiSign", sign.clone()),
                ("signStr", sign_str.clone()),
                ("_t", external_elo_request_nonce()),
            ])
            .send()
            .await
            .expect("info 请求应成功")
            .text()
            .await
            .expect("应能读取 info 文本");

        println!("info payload => {}", info_text);

        let info = serde_json::from_str::<LzyumiInfoResponse>(&info_text).expect("info JSON 应可解析");
        let Some(battle_info) = info.battle_info else {
            return;
        };

        let (sign, sign_str) = lzyumi_sign_components();
        let elo_text = http
            .get("https://a.lzyumi.top/lzyumi/lol/info/getRankEloInfo")
            .query(&vec![
                ("openId", battle_info.open_id),
                ("areaId", area_id),
                ("lzyumiSign", sign),
                ("signStr", sign_str),
                ("filter", "5".to_string()),
                ("_t", external_elo_request_nonce()),
            ])
            .send()
            .await
            .expect("elo 请求应成功")
            .text()
            .await
            .expect("应能读取 elo 文本");

        println!("elo payload => {}", elo_text);
    }

    #[test]
    #[ignore = "手动触发：打印 PowerShell 回退请求的原始返回"]
    fn manual_dump_external_elo_powershell_payloads() {
        let area = EloArea {
            id: 14,
            name: "黑色玫瑰",
        };
        let info_text = fetch_external_info_text_via_powershell(
            &area,
            "离于爱者无忧无怖#53258",
        )
        .expect("PowerShell info 请求应成功");

        println!("powershell info payload => {}", info_text);
    }
}
