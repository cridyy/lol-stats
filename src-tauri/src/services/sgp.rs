use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use super::error::{AppError, AppResult};
use super::lcu::LcuClient;
use super::models::{
    ClientAuth, Game, IdentityPlayer, Participant, ParticipantIdentity, ParticipantStats,
    ParticipantTimeline, RankedQueueEntry, RankedQueueMap, RankedStatsResponse, SummonerInfo,
};

const USER_AGENT: &str = "LeagueOfLegendsClient/14.13.596.7996 (rcp-be-lol-match-history)";
const MAX_RETRIES: usize = 3;

/// 国服 SGP（Service Gateway Proxy）深战绩客户端。
///
/// LCU 的 `/lol-match-history/v1/products/lol/{puuid}/matches` 在部分国服客户端上会忽略
/// `begIndex/endIndex`，只能稳定拿到第一页。Akari 的深分页走 SGP，这里只实现统计需要的
/// SUMMARY 接口，并把返回值映射成项目内部已有的 `Game` 结构。
pub struct SgpClient {
    http: Client,
    base_url: String,
    access_token: String,
    league_session_token: String,
    sgp_server_id: String,
}

impl SgpClient {
    pub async fn new(
        lcu: &LcuClient,
        auth: &ClientAuth,
        sgp_server_id: Option<&str>,
    ) -> AppResult<Self> {
        let sgp_server_id = resolve_sgp_server_id(auth, sgp_server_id);
        let base_url = tencent_sgp_base_url(&sgp_server_id)?;
        let access_token = lcu.entitlements_access_token().await?;
        let league_session_token = lcu
            .league_session_token()
            .await
            .unwrap_or_else(|_| access_token.clone());
        let http = Client::builder()
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .timeout(std::time::Duration::from_secs(14))
            .build()
            .map_err(AppError::Http)?;

        Ok(Self {
            http,
            base_url: base_url.to_string(),
            access_token,
            league_session_token,
            sgp_server_id,
        })
    }

    pub async fn ranked_stats(&self, puuid: &str) -> AppResult<RankedStatsResponse> {
        let response = self
            .http
            .get(format!(
                "{}/leagues-ledge/v2/rankedStats/puuid/{}",
                self.base_url, puuid
            ))
            .header(
                "Authorization",
                format!("Bearer {}", self.league_session_token),
            )
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(AppError::PlayerNotFound(puuid.to_string()));
        }

        if !response.status().is_success() {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 段位返回 {}",
                response.status()
            )));
        }

        let ranked = parse_sgp_json::<SgpRankedStats>(response, "SGP 段位").await?;

        Ok(sgp_ranked_stats_to_ranked_response(ranked))
    }

    pub async fn summoners_by_puuids(&self, puuids: &[String]) -> AppResult<Vec<SummonerInfo>> {
        if puuids.is_empty() {
            return Ok(Vec::new());
        }

        let sub_id = self
            .sgp_server_id
            .strip_prefix("TENCENT_")
            .unwrap_or(&self.sgp_server_id)
            .to_ascii_lowercase();

        let response = self
            .http
            .post(format!(
                "{}/summoner-ledge/v1/regions/{}/summoners/puuids",
                self.base_url, sub_id
            ))
            .header(
                "Authorization",
                format!("Bearer {}", self.league_session_token),
            )
            .header("User-Agent", USER_AGENT)
            .json(puuids)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 召唤师查询返回 {}",
                response.status()
            )));
        }

        let summoners = parse_sgp_json::<Vec<SgpSummoner>>(response, "SGP 召唤师").await?;
        Ok(summoners.into_iter().map(sgp_summoner_to_lcu).collect())
    }

    pub async fn match_history(
        &self,
        puuid: &str,
        start_index: usize,
        count: usize,
    ) -> AppResult<Vec<Game>> {
        let mut last_status = None;
        let mut last_error = None;

        for attempt in 0..=MAX_RETRIES {
            let response = self
                .http
                .get(format!(
                    "{}/match-history-query/v1/products/lol/player/{}/SUMMARY",
                    self.base_url, puuid
                ))
                .header("Authorization", format!("Bearer {}", self.access_token))
                .header("User-Agent", USER_AGENT)
                .query(&[
                    ("startIndex", start_index.to_string()),
                    ("count", count.to_string()),
                ])
                .send()
                .await;

            let response = match response {
                Ok(response) => response,
                Err(error) if attempt < MAX_RETRIES => {
                    last_error = Some(error);
                    wait_before_retry(attempt).await;
                    continue;
                }
                Err(error) => return Err(AppError::Http(error)),
            };

            if should_retry_status(response.status()) && attempt < MAX_RETRIES {
                last_status = Some(response.status());
                wait_before_retry(attempt).await;
                continue;
            }

            return self.parse_match_history_response(response, puuid).await;
        }

        if let Some(status) = last_status {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 深战绩多次返回 {}",
                status
            )));
        }

        if let Some(error) = last_error {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 深战绩网络请求失败：{}",
                error
            )));
        }

        Err(AppError::SgpUnavailable(
            "SGP 深战绩请求未返回结果".to_string(),
        ))
    }

    pub async fn game_summary(&self, game_id: u64) -> AppResult<Game> {
        let sub_id = self
            .sgp_server_id
            .strip_prefix("TENCENT_")
            .unwrap_or(&self.sgp_server_id)
            .to_ascii_uppercase();

        let response = self
            .http
            .get(format!(
                "{}/match-history-query/v1/products/lol/{}_{}/SUMMARY",
                self.base_url, sub_id, game_id
            ))
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        self.parse_game_summary_response(response, game_id).await
    }

    async fn parse_match_history_response(
        &self,
        response: reqwest::Response,
        puuid: &str,
    ) -> AppResult<Vec<Game>> {
        if response.status() == StatusCode::NOT_FOUND {
            return Err(AppError::PlayerNotFound(puuid.to_string()));
        }

        if !response.status().is_success() {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 深战绩返回 {}",
                response.status()
            )));
        }

        let history = parse_sgp_json::<SgpMatchHistoryResponse>(response, "SGP 深战绩").await?;

        Ok(history
            .games
            .into_iter()
            .filter_map(|game| game.json.map(sgp_game_json_to_lcu_game))
            .collect())
    }

    async fn parse_game_summary_response(
        &self,
        response: reqwest::Response,
        game_id: u64,
    ) -> AppResult<Game> {
        if response.status() == StatusCode::NOT_FOUND {
            return Err(AppError::PlayerNotFound(game_id.to_string()));
        }

        if !response.status().is_success() {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 对局详情返回 {}",
                response.status()
            )));
        }

        let summary = parse_sgp_json::<SgpGameEnvelope>(response, "SGP 对局详情").await?;

        let Some(summary) = summary.json else {
            return Err(AppError::SgpUnavailable(format!(
                "SGP 对局详情 {game_id} 缺少 json 数据"
            )));
        };

        Ok(sgp_game_json_to_lcu_game(summary))
    }
}

pub fn default_sgp_server_id(auth: &ClientAuth) -> String {
    if auth.region.eq_ignore_ascii_case("TENCENT") {
        format!("TENCENT_{}", auth.rso_platform_id.to_ascii_uppercase())
    } else {
        auth.region.to_ascii_uppercase()
    }
}

pub fn resolve_sgp_server_id(auth: &ClientAuth, sgp_server_id: Option<&str>) -> String {
    sgp_server_id
        .filter(|value| !value.trim().is_empty())
        .map(normalize_sgp_server_id)
        .unwrap_or_else(|| default_sgp_server_id(auth))
}

pub fn normalize_sgp_server_id(sgp_server_id: &str) -> String {
    sgp_server_id.trim().to_ascii_uppercase()
}

#[derive(Debug, Default, Deserialize)]
struct SgpMatchHistoryResponse {
    #[serde(default)]
    games: Vec<SgpGameEnvelope>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SgpRankedStats {
    #[serde(default)]
    queues: Vec<SgpRankedQueue>,
    #[serde(default)]
    queue_map: Option<SgpRankedQueueMap>,
}

#[derive(Debug, Default, Deserialize)]
struct SgpRankedQueueMap {
    #[serde(default, rename = "RANKED_SOLO_5x5", alias = "RANKED_SOLO_5X5")]
    ranked_solo_5x5: SgpRankedQueue,
    #[serde(default, rename = "RANKED_FLEX_SR")]
    ranked_flex_sr: SgpRankedQueue,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SgpRankedQueue {
    #[serde(default)]
    queue_type: String,
    #[serde(default)]
    tier: String,
    #[serde(default, alias = "division")]
    rank: String,
    #[serde(default)]
    league_points: i32,
    #[serde(default)]
    wins: i32,
    #[serde(default)]
    losses: i32,
    #[serde(default)]
    provisional_games_remaining: i32,
    #[serde(default)]
    highest_tier: String,
    #[serde(default, alias = "highestDivision")]
    highest_rank: String,
    #[serde(default)]
    previous_season_end_tier: String,
    #[serde(default, alias = "previousSeasonEndDivision")]
    previous_season_end_rank: String,
    #[serde(default)]
    previous_season_highest_tier: String,
    #[serde(default, alias = "previousSeasonHighestDivision")]
    previous_season_highest_rank: String,
    #[serde(default)]
    previous_season_achieved_tier: String,
    #[serde(default, alias = "previousSeasonAchievedDivision")]
    previous_season_achieved_rank: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SgpSummoner {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    puuid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    internal_name: String,
    #[serde(default)]
    profile_icon_id: u32,
    #[serde(default)]
    level: u32,
    #[serde(default)]
    privacy: String,
}

#[derive(Debug, Default, Deserialize)]
struct SgpGameEnvelope {
    #[serde(default)]
    json: Option<SgpGameJson>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SgpGameJson {
    #[serde(default)]
    game_id: u64,
    #[serde(default)]
    game_creation: i64,
    #[serde(default)]
    game_duration: i64,
    #[serde(default)]
    game_mode: String,
    #[serde(default)]
    game_type: String,
    #[serde(default)]
    queue_id: u32,
    #[serde(default)]
    participants: Vec<SgpParticipant>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SgpParticipant {
    #[serde(default)]
    puuid: String,
    #[serde(default)]
    riot_id_game_name: String,
    #[serde(default)]
    riot_id_tagline: String,
    #[serde(default)]
    summoner_name: String,
    #[serde(default)]
    participant_id: u32,
    #[serde(default)]
    champion_id: u32,
    #[serde(default)]
    spell1_id: u32,
    #[serde(default)]
    spell2_id: u32,
    #[serde(default)]
    team_id: u32,
    #[serde(default)]
    win: bool,
    #[serde(default)]
    kills: u32,
    #[serde(default)]
    deaths: u32,
    #[serde(default)]
    assists: u32,
    #[serde(default)]
    gold_earned: u32,
    #[serde(default)]
    total_minions_killed: u32,
    #[serde(default)]
    neutral_minions_killed: u32,
    #[serde(default)]
    total_damage_dealt_to_champions: u32,
    #[serde(default)]
    damage_self_mitigated: u32,
    #[serde(default)]
    total_damage_taken: u32,
    #[serde(default)]
    total_heal: u32,
    #[serde(default)]
    vision_score: u32,
    #[serde(default)]
    champ_level: u32,
    #[serde(default)]
    player_subteam_id: u32,
    #[serde(default)]
    challenges: Value,
    #[serde(default)]
    item0: u32,
    #[serde(default)]
    item1: u32,
    #[serde(default)]
    item2: u32,
    #[serde(default)]
    item3: u32,
    #[serde(default)]
    item4: u32,
    #[serde(default)]
    item5: u32,
    #[serde(default)]
    item6: u32,
    #[serde(default)]
    perk0: u32,
    #[serde(default)]
    perk1: u32,
    #[serde(default)]
    perk2: u32,
    #[serde(default)]
    perk3: u32,
    #[serde(default)]
    perk4: u32,
    #[serde(default)]
    perk5: u32,
    #[serde(default)]
    player_augment1: u32,
    #[serde(default)]
    player_augment2: u32,
    #[serde(default)]
    player_augment3: u32,
    #[serde(default)]
    player_augment4: u32,
    #[serde(default)]
    player_augment5: u32,
    #[serde(default)]
    player_augment6: u32,
    #[serde(default)]
    lane: String,
    #[serde(default)]
    role: String,
}

async fn parse_sgp_json<T>(response: reqwest::Response, context: &str) -> AppResult<T>
where
    T: DeserializeOwned,
{
    let body = response.text().await.map_err(AppError::Http)?;
    let mut deserializer = serde_json::Deserializer::from_str(&body);

    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = error.path().to_string();
        let path = if path.trim().is_empty() {
            "<root>".to_string()
        } else {
            path
        };

        AppError::SgpUnavailable(format!(
            "{context}解析失败：路径 {path}，{}；响应片段：{}",
            error.inner(),
            response_preview(&body)
        ))
    })
}

fn response_preview(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let preview = compact.chars().take(240).collect::<String>();

    if compact.chars().count() > 240 {
        format!("{preview}...")
    } else if preview.is_empty() {
        "<empty>".to_string()
    } else {
        preview
    }
}

fn challenge_u32(challenges: &Value, key: &str) -> u32 {
    challenges
        .get(key)
        .and_then(json_value_to_u32)
        .unwrap_or_default()
}

fn json_value_to_u32(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|value| u64::try_from(value).ok()))
            .or_else(|| number.as_f64().map(|value| value.max(0.0).round() as u64))
            .and_then(|value| u32::try_from(value).ok()),
        Value::String(value) => value.trim().parse::<u32>().ok().or_else(|| {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .map(|value| value.max(0.0).round() as u32)
        }),
        _ => None,
    }
}

fn sgp_game_json_to_lcu_game(game: SgpGameJson) -> Game {
    let identities = game
        .participants
        .iter()
        .map(|participant| ParticipantIdentity {
            participant_id: participant.participant_id,
            player: IdentityPlayer {
                puuid: participant.puuid.clone(),
                game_name: participant.riot_id_game_name.clone(),
                tag_line: participant.riot_id_tagline.clone(),
                summoner_name: participant.summoner_name.clone(),
            },
        })
        .collect();

    let participants = game
        .participants
        .into_iter()
        .map(|participant| {
            let enemy_champion_immobilizations =
                challenge_u32(&participant.challenges, "enemyChampionImmobilizations");
            let immobilize_and_kill_with_ally =
                challenge_u32(&participant.challenges, "immobilizeAndKillWithAlly");

            Participant {
                participant_id: participant.participant_id,
                champion_id: participant.champion_id,
                spell1_id: participant.spell1_id,
                spell2_id: participant.spell2_id,
                team_id: participant.team_id,
                stats: ParticipantStats {
                    win: participant.win,
                    kills: participant.kills,
                    deaths: participant.deaths,
                    assists: participant.assists,
                    gold_earned: participant.gold_earned,
                    total_minions_killed: participant.total_minions_killed,
                    neutral_minions_killed: participant.neutral_minions_killed,
                    total_damage_dealt_to_champions: participant.total_damage_dealt_to_champions,
                    damage_self_mitigated: participant.damage_self_mitigated,
                    total_damage_taken: participant.total_damage_taken,
                    total_heal: participant.total_heal,
                    enemy_champion_immobilizations,
                    immobilize_and_kill_with_ally,
                    vision_score: participant.vision_score,
                    champ_level: participant.champ_level,
                    player_subteam_id: participant.player_subteam_id,
                    item0: participant.item0,
                    item1: participant.item1,
                    item2: participant.item2,
                    item3: participant.item3,
                    item4: participant.item4,
                    item5: participant.item5,
                    item6: participant.item6,
                    perk0: participant.perk0,
                    perk1: participant.perk1,
                    perk2: participant.perk2,
                    perk3: participant.perk3,
                    perk4: participant.perk4,
                    perk5: participant.perk5,
                    player_augment1: participant.player_augment1,
                    player_augment2: participant.player_augment2,
                    player_augment3: participant.player_augment3,
                    player_augment4: participant.player_augment4,
                    player_augment5: participant.player_augment5,
                    player_augment6: participant.player_augment6,
                },
                timeline: ParticipantTimeline {
                    lane: participant.lane,
                    role: participant.role,
                },
            }
        })
        .collect();

    Game {
        game_id: game.game_id,
        game_creation: game.game_creation,
        game_duration: game.game_duration,
        game_mode: game.game_mode,
        game_type: game.game_type,
        queue_id: game.queue_id,
        participant_identities: identities,
        participants,
    }
}

fn sgp_summoner_to_lcu(summoner: SgpSummoner) -> SummonerInfo {
    SummonerInfo {
        display_name: summoner.name,
        internal_name: summoner.internal_name,
        profile_icon_id: summoner.profile_icon_id,
        puuid: summoner.puuid,
        summoner_id: summoner.id,
        summoner_level: summoner.level,
        privacy: summoner.privacy,
        ..Default::default()
    }
}

fn sgp_ranked_stats_to_ranked_response(ranked: SgpRankedStats) -> RankedStatsResponse {
    let mut queue_map = RankedQueueMap::default();

    if let Some(raw_queue_map) = ranked.queue_map {
        if sgp_ranked_queue_has_data(&raw_queue_map.ranked_solo_5x5) {
            queue_map.ranked_solo_5x5 =
                sgp_ranked_queue_to_entry(&raw_queue_map.ranked_solo_5x5, "RANKED_SOLO_5x5");
        }
        if sgp_ranked_queue_has_data(&raw_queue_map.ranked_flex_sr) {
            queue_map.ranked_flex_sr =
                sgp_ranked_queue_to_entry(&raw_queue_map.ranked_flex_sr, "RANKED_FLEX_SR");
        }
    }

    for queue in ranked.queues {
        let queue_type = normalize_ranked_queue_type(&queue.queue_type);
        if !sgp_ranked_queue_has_data(&queue) {
            continue;
        }
        let entry = sgp_ranked_queue_to_entry(&queue, queue.queue_type.trim());
        match queue_type.as_str() {
            "RANKED_SOLO_5x5" => queue_map.ranked_solo_5x5 = entry,
            "RANKED_FLEX_SR" => queue_map.ranked_flex_sr = entry,
            _ => {}
        }
    }

    RankedStatsResponse { queue_map }
}

fn sgp_ranked_queue_has_data(queue: &SgpRankedQueue) -> bool {
    ranked_text_has_value(&queue.tier)
        || ranked_text_has_value(&queue.highest_tier)
        || ranked_text_has_value(&queue.previous_season_end_tier)
        || ranked_text_has_value(&queue.previous_season_highest_tier)
        || ranked_text_has_value(&queue.previous_season_achieved_tier)
        || queue.league_points != 0
        || queue.wins != 0
        || queue.losses != 0
        || queue.provisional_games_remaining != 0
}

fn ranked_text_has_value(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "" | "NONE" | "NA"
    )
}

fn sgp_ranked_queue_to_entry(
    queue: &SgpRankedQueue,
    fallback_queue_type: &str,
) -> RankedQueueEntry {
    let queue_type = if queue.queue_type.trim().is_empty() {
        fallback_queue_type.to_string()
    } else {
        queue.queue_type.trim().to_string()
    };

    RankedQueueEntry {
        queue_type,
        tier: queue.tier.clone(),
        division: queue.rank.clone(),
        highest_tier: first_non_empty(&[
            &queue.highest_tier,
            &queue.previous_season_achieved_tier,
            &queue.previous_season_highest_tier,
            &queue.previous_season_end_tier,
        ]),
        highest_division: first_non_empty(&[
            &queue.highest_rank,
            &queue.previous_season_achieved_rank,
            &queue.previous_season_highest_rank,
            &queue.previous_season_end_rank,
        ]),
        previous_season_highest_tier: first_non_empty(&[
            &queue.previous_season_highest_tier,
            &queue.previous_season_achieved_tier,
        ]),
        previous_season_highest_division: first_non_empty(&[
            &queue.previous_season_highest_rank,
            &queue.previous_season_achieved_rank,
        ]),
        previous_season_end_tier: queue.previous_season_end_tier.clone(),
        previous_season_end_division: queue.previous_season_end_rank.clone(),
        league_points: queue.league_points,
        wins: queue.wins,
        losses: queue.losses,
        is_provisional: queue.provisional_games_remaining > 0,
    }
}

fn normalize_ranked_queue_type(queue_type: &str) -> String {
    match queue_type.trim().to_ascii_uppercase().as_str() {
        "RANKED_SOLO_5X5" => "RANKED_SOLO_5x5".to_string(),
        other => other.to_string(),
    }
}

fn first_non_empty(values: &[&String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| (*value).clone())
        .unwrap_or_default()
}

fn tencent_sgp_base_url(sgp_server_id: &str) -> AppResult<&'static str> {
    let Some(rso_platform_id) = sgp_server_id.strip_prefix("TENCENT_") else {
        return Err(AppError::SgpUnavailable(
            "当前只支持国服 SGP 深战绩".to_string(),
        ));
    };

    match rso_platform_id {
        "HN1" => Ok("https://hn1-k8s-sgp.lol.qq.com:21019"),
        "HN10" => Ok("https://hn10-k8s-sgp.lol.qq.com:21019"),
        "TJ100" => Ok("https://tj100-sgp.lol.qq.com:21019"),
        "TJ101" => Ok("https://tj101-sgp.lol.qq.com:21019"),
        "NJ100" => Ok("https://nj100-sgp.lol.qq.com:21019"),
        "GZ100" => Ok("https://gz100-sgp.lol.qq.com:21019"),
        "CQ100" => Ok("https://cq100-sgp.lol.qq.com:21019"),
        "BGP2" => Ok("https://bgp2-k8s-sgp.lol.qq.com:21019"),
        "PBE" => Ok("https://pbe-sgp.lol.qq.com:21019"),
        "PREPBE" => Ok("https://prepbe-sgp.lol.qq.com:21019"),
        other => Err(AppError::SgpUnavailable(format!(
            "暂未配置国服大区 {other} 的 SGP 深战绩地址"
        ))),
    }
}

fn should_retry_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

async fn wait_before_retry(attempt: usize) {
    let delay_ms = 500 * (attempt as u64 + 1);
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
}
