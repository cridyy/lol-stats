use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use super::error::AppResult;
use super::lcu::LcuClient;
use super::models::{
    ChampionStat, ClientAuth, Game, IdentityPlayer, MatchDetailPlayer, MatchDetailResponse,
    MatchDetailTeam, Participant, PlayerStatsResponse, PlayerSummary, RecentGame, SummonerInfo,
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
const CACHE_SCHEMA_VERSION: i64 = 8;
const DATABASE_FILE_NAME: &str = "lol-stats.sqlite3";

static STATS_CACHE: LazyLock<Mutex<HashMap<StatsCacheKey, StatsCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct StatsCacheKey {
    sgp_server_id: String,
    puuid: String,
    depth: usize,
}

#[derive(Clone)]
struct StatsCacheEntry {
    stored_at: Instant,
    response: PlayerStatsResponse,
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
    team_damage_self_mitigated: u64,
    total_heal: u64,
    team_total_heal: u64,
    last_played_at: i64,
}

#[derive(Default)]
struct TeamTotals {
    damage_to_champions: u32,
    gold_earned: u32,
    damage_self_mitigated: u32,
    total_heal: u32,
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

    serde_json::from_str(&response_json).ok()
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
        "#,
    )
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
        entry.team_damage_self_mitigated += team_totals.damage_self_mitigated as u64;
        entry.total_heal += participant.stats.total_heal as u64;
        entry.team_total_heal += team_totals.total_heal as u64;
        entry.last_played_at = entry.last_played_at.max(game.game_creation);

        recent_games.push(participant_to_recent_game(&game, participant));
    }

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
        kda,
        cs,
        gold_earned: participant.stats.gold_earned,
        damage_to_champions: participant.stats.total_damage_dealt_to_champions,
        team_damage_to_champions: team_totals.damage_to_champions,
        damage_self_mitigated: participant.stats.damage_self_mitigated,
        team_damage_self_mitigated: team_totals.damage_self_mitigated,
        total_heal: participant.stats.total_heal,
        team_total_heal: team_totals.total_heal,
        team_gold_earned: team_totals.gold_earned,
        game_creation: game.game_creation,
        game_duration: game.game_duration,
    }
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
        mitigation_share: ratio_u64(c.damage_self_mitigated, c.team_damage_self_mitigated),
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
            totals.total_heal += candidate.stats.total_heal;
            totals
        })
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
        .map(|candidate| candidate.stats.damage_self_mitigated)
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
            participant.stats.damage_self_mitigated,
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
