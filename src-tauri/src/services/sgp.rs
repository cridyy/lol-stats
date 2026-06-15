use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::Deserialize;

use super::error::{AppError, AppResult};
use super::lcu::LcuClient;
use super::models::{
    ClientAuth, Game, IdentityPlayer, Participant, ParticipantIdentity, ParticipantStats,
    ParticipantTimeline,
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
            sgp_server_id,
        })
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

        let history = response
            .json::<SgpMatchHistoryResponse>()
            .await
            .map_err(AppError::Http)?;

        Ok(history
            .games
            .into_iter()
            .map(sgp_game_to_lcu_game)
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

        let summary = response
            .json::<SgpGameEnvelope>()
            .await
            .map_err(AppError::Http)?;

        Ok(sgp_game_to_lcu_game(summary))
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
struct SgpGameEnvelope {
    #[serde(default)]
    json: SgpGameJson,
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
    total_heal: u32,
    #[serde(default)]
    vision_score: u32,
    #[serde(default)]
    champ_level: u32,
    #[serde(default)]
    player_subteam_id: u32,
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

fn sgp_game_to_lcu_game(game: SgpGameEnvelope) -> Game {
    let identities = game
        .json
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
        .json
        .participants
        .into_iter()
        .map(|participant| Participant {
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
                total_heal: participant.total_heal,
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
        })
        .collect();

    Game {
        game_id: game.json.game_id,
        game_creation: game.json.game_creation,
        game_duration: game.json.game_duration,
        game_mode: game.json.game_mode,
        game_type: game.json.game_type,
        queue_id: game.json.queue_id,
        participant_identities: identities,
        participants,
    }
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
