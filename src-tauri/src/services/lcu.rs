use std::collections::HashMap;

use base64::Engine;
use reqwest::{header::CONTENT_TYPE, Client, StatusCode};
use serde::de::DeserializeOwned;

use super::error::{AppError, AppResult};
use super::models::{
    ChampSelectSession, ChampionSummaryItem, ClientAuth, GameAssetBundle, GameAssetEntry,
    GameflowSession, MatchHistoryResponse, RankedStatsResponse, RiotAlias, SummonerInfo,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntitlementsToken {
    #[serde(default)]
    access_token: String,
}

fn basic_auth_header(token: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("riot:{token}"));
    format!("Basic {encoded}")
}

fn https_client() -> AppResult<Client> {
    // LCU/RC 使用本地自签名证书。只允许访问 127.0.0.1，证书校验在这里放宽。
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .timeout(std::time::Duration::from_secs(14))
        .build()
        .map_err(AppError::Http)
}

#[derive(Clone)]
pub struct LcuClient {
    http: Client,
    base_url: String,
    auth_header: String,
}

impl LcuClient {
    pub fn new(auth: &ClientAuth) -> AppResult<Self> {
        Ok(Self {
            http: https_client()?,
            base_url: format!("https://127.0.0.1:{}", auth.port),
            auth_header: basic_auth_header(&auth.auth_token),
        })
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", &self.auth_header)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(AppError::PlayerNotFound(path.to_string()));
        }

        if !response.status().is_success() {
            return Err(AppError::LcuUnavailable(format!(
                "{} 返回 {}",
                path,
                response.status()
            )));
        }

        response.json::<T>().await.map_err(AppError::Http)
    }

    async fn get_optional_json<T: DeserializeOwned>(&self, path: &str) -> AppResult<Option<T>> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", &self.auth_header)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(AppError::LcuUnavailable(format!(
                "{} 返回 {}",
                path,
                response.status()
            )));
        }

        response.json::<T>().await.map(Some).map_err(AppError::Http)
    }

    async fn get_bytes(&self, path: &str) -> AppResult<(Vec<u8>, String)> {
        let response = self
            .http
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", &self.auth_header)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::LcuUnavailable(format!(
                "{} 返回 {}",
                path,
                response.status()
            )));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("image/png")
            .to_string();
        let bytes = response.bytes().await?.to_vec();

        Ok((bytes, content_type))
    }

    pub async fn ping(&self) -> AppResult<()> {
        let _: serde_json::Value = self.get_json("/riotclient/auth-token").await?;
        Ok(())
    }

    pub async fn entitlements_access_token(&self) -> AppResult<String> {
        let token: EntitlementsToken = self.get_json("/entitlements/v1/token").await?;
        if token.access_token.trim().is_empty() {
            return Err(AppError::LcuUnavailable(
                "entitlements token 为空，无法读取深度战绩".to_string(),
            ));
        }

        Ok(token.access_token)
    }

    pub async fn league_session_token(&self) -> AppResult<String> {
        let token: String = self
            .get_json("/lol-league-session/v1/league-session-token")
            .await?;
        if token.trim().is_empty() {
            return Err(AppError::LcuUnavailable(
                "league session token 为空，无法读取段位".to_string(),
            ));
        }

        Ok(token)
    }

    pub async fn current_summoner(&self) -> AppResult<SummonerInfo> {
        self.get_json("/lol-summoner/v1/current-summoner").await
    }

    pub async fn summoner_by_puuid(&self, puuid: &str) -> AppResult<SummonerInfo> {
        self.get_json(&format!("/lol-summoner/v2/summoners/puuid/{puuid}"))
            .await
    }

    pub async fn summoner_by_name(&self, name: &str) -> AppResult<SummonerInfo> {
        let encoded = urlencoding::encode(name);
        self.get_json(&format!("/lol-summoner/v1/summoners?name={encoded}"))
            .await
    }

    pub async fn ranked_stats(&self, puuid: &str) -> AppResult<RankedStatsResponse> {
        self.get_json(&format!("/lol-ranked/v1/ranked-stats/{puuid}"))
            .await
    }

    pub async fn gameflow_phase(&self) -> AppResult<String> {
        self.get_json("/lol-gameflow/v1/gameflow-phase").await
    }

    pub async fn gameflow_session(&self) -> AppResult<Option<GameflowSession>> {
        self.get_optional_json("/lol-gameflow/v1/session").await
    }

    pub async fn champ_select_session(&self) -> AppResult<Option<ChampSelectSession>> {
        self.get_optional_json("/lol-champ-select/v1/session").await
    }

    pub async fn match_history(
        &self,
        puuid: &str,
        begin_index: usize,
        end_index: usize,
    ) -> AppResult<MatchHistoryResponse> {
        self.get_json(&format!(
            "/lol-match-history/v1/products/lol/{puuid}/matches?begIndex={begin_index}&endIndex={end_index}"
        ))
        .await
    }

    pub async fn match_history_game(&self, game_id: u64) -> AppResult<super::models::Game> {
        self.get_json(&format!("/lol-match-history/v1/games/{game_id}"))
            .await
    }

    pub async fn champion_summary(&self) -> AppResult<Vec<ChampionSummaryItem>> {
        let champions: Vec<ChampionSummaryItem> = self
            .get_json("/lol-game-data/assets/v1/champion-summary.json")
            .await?;

        // LCU 的 champion-summary 第一项通常是 id=-1 的 None 占位项。
        // 前端统计只需要真实英雄，过滤掉占位项也能避免无效数据进入映射表。
        Ok(champions
            .into_iter()
            .filter(|champion| champion.id > 0)
            .collect())
    }

    pub async fn game_assets(&self) -> AppResult<GameAssetBundle> {
        let summoner_spells = self
            .load_game_asset_entries("/lol-game-data/assets/v1/summoner-spells.json")
            .await;
        let items = self
            .load_game_asset_entries("/lol-game-data/assets/v1/items.json")
            .await;
        let mut perks = self
            .load_game_asset_entries("/lol-game-data/assets/v1/perks.json")
            .await;
        let augments = self
            .load_game_asset_entries("/lol-game-data/assets/v1/cherry-augments.json")
            .await;

        for augment in &augments {
            if !perks.iter().any(|perk| perk.id == augment.id) {
                perks.push(augment.clone());
            }
        }

        Ok(GameAssetBundle {
            summoner_spells,
            items,
            perks,
            augments,
        })
    }

    async fn load_game_asset_entries(&self, path: &str) -> Vec<GameAssetEntry> {
        let Ok(entries) = self.get_json::<Vec<serde_json::Value>>(path).await else {
            return Vec::new();
        };

        entries
            .into_iter()
            .filter_map(game_asset_entry_from_value)
            .collect()
    }

    pub async fn asset_data_url(&self, path: &str) -> AppResult<String> {
        let path = normalize_lcu_asset_path(path)?;
        let (bytes, content_type) = self.get_bytes(&path).await?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(format!("data:{content_type};base64,{encoded}"))
    }

    pub async fn asset_data_urls(&self, paths: Vec<String>) -> AppResult<HashMap<String, String>> {
        let mut result = HashMap::new();

        for path in paths {
            if result.contains_key(&path) {
                continue;
            }

            if let Ok(data_url) = self.asset_data_url(&path).await {
                result.insert(path, data_url);
            }
        }

        Ok(result)
    }
}

fn game_asset_entry_from_value(value: serde_json::Value) -> Option<GameAssetEntry> {
    let id = read_u32_field(&value, "id")?;
    if id == 0 {
        return None;
    }

    Some(GameAssetEntry {
        id,
        name: read_string_field(&value, &["name", "nameTRA", "nameCn", "displayName"]),
        description: read_string_field(&value, &["description", "longDesc", "shortDesc"]),
        icon_path: read_string_field(
            &value,
            &[
                "iconPath",
                "augmentSmallIconPath",
                "iconSmall",
                "iconLargePath",
                "iconLarge",
            ],
        ),
        rarity: read_rarity_field(&value),
        categories: read_string_vec_field(&value, "categories"),
        price: read_u32_field(&value, "price").unwrap_or_default(),
        price_total: read_u32_field(&value, "priceTotal").unwrap_or_default(),
        in_store: read_bool_field(&value, "inStore"),
        display_in_item_sets: read_bool_field(&value, "displayInItemSets"),
    })
}

fn read_u32_field(value: &serde_json::Value, field: &str) -> Option<u32> {
    let raw = value.get(field)?;
    raw.as_u64()
        .and_then(|id| u32::try_from(id).ok())
        .or_else(|| raw.as_str()?.parse::<u32>().ok())
}

fn read_string_field(value: &serde_json::Value, fields: &[&str]) -> String {
    fields
        .iter()
        .find_map(|field| value.get(*field)?.as_str())
        .map(str::to_string)
        .unwrap_or_default()
}

fn read_string_vec_field(value: &serde_json::Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(|raw| raw.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn read_bool_field(value: &serde_json::Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(|raw| {
            raw.as_bool().or_else(|| {
                raw.as_str()
                    .map(|text| matches!(text.to_ascii_lowercase().as_str(), "true" | "1"))
            })
        })
        .unwrap_or(false)
}

fn read_rarity_field(value: &serde_json::Value) -> String {
    let Some(raw) = value.get("rarity") else {
        return String::new();
    };

    if let Some(rarity) = raw.as_str() {
        return rarity.to_string();
    }

    let rarity_index = raw
        .as_u64()
        .or_else(|| raw.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| {
            raw.as_f64()
                .filter(|value| value.is_finite())
                .map(|value| value as u64)
        });

    match rarity_index {
        Some(0) => "kSilver".to_string(),
        Some(1) => "kGold".to_string(),
        Some(2) => "kPrismatic".to_string(),
        Some(3) => "kBronze".to_string(),
        _ => String::new(),
    }
}

fn normalize_lcu_asset_path(path: &str) -> AppResult<String> {
    let path = path.trim();
    if !path.starts_with("/lol-game-data/assets/") || path.contains("..") {
        return Err(AppError::LcuUnavailable(
            "拒绝读取非 lol-game-data 资源路径".to_string(),
        ));
    }

    Ok(path.to_string())
}

#[derive(Clone)]
pub struct RiotClient {
    http: Client,
    base_url: String,
    auth_header: String,
}

impl RiotClient {
    pub fn new(auth: &ClientAuth) -> AppResult<Option<Self>> {
        let Some(port) = auth.riot_client_port else {
            return Ok(None);
        };

        let Some(token) = &auth.riot_client_auth_token else {
            return Ok(None);
        };

        Ok(Some(Self {
            http: https_client()?,
            base_url: format!("https://127.0.0.1:{port}"),
            auth_header: basic_auth_header(token),
        }))
    }

    pub async fn lookup_alias(
        &self,
        game_name: &str,
        tag_line: Option<&str>,
    ) -> AppResult<Vec<RiotAlias>> {
        let mut request = self
            .http
            .get(format!(
                "{}/player-account/aliases/v1/lookup",
                self.base_url
            ))
            .header("Authorization", &self.auth_header)
            .query(&[("gameName", game_name)]);

        if let Some(tag_line) = tag_line {
            request = request.query(&[("tagLine", tag_line)]);
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(AppError::RiotClientUnavailable(format!(
                "alias lookup 返回 {}",
                response.status()
            )));
        }

        response
            .json::<Vec<RiotAlias>>()
            .await
            .map_err(AppError::Http)
    }
}
