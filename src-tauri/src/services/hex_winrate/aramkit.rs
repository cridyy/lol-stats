use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;

const CONFIG_URL: &str = "https://aramkit.com/zh-CN/champions/vayne";
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

static DATASET_CACHE: LazyLock<Mutex<Option<DatasetCache>>> = LazyLock::new(|| Mutex::new(None));
static ENDPOINT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"aramKitDataEndpoint\s*:\s*"([^"]+)""#).expect("AramKit endpoint 正则应合法")
});
static DATA_PATH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"dataPath\s*:\s*"([^"]+)""#).expect("AramKit dataPath 正则应合法")
});

#[derive(Clone)]
struct DatasetCache {
    endpoint: String,
    data_path: String,
    fetched_at: Instant,
}

#[derive(Clone)]
pub(crate) struct ChampionAugmentStats {
    pub(crate) base_win_rate: f64,
    pub(crate) augment_win_rates: HashMap<u32, f64>,
}

#[derive(Debug, Deserialize)]
struct ChampionDetailsResponse {
    #[serde(default)]
    champion: ChampionData,
    #[serde(default)]
    augments: AugmentGroups,
}

#[derive(Debug, Default, Deserialize)]
struct ChampionData {
    #[serde(default)]
    stats: WinRateStats,
}

#[derive(Debug, Default, Deserialize)]
struct AugmentGroups {
    #[serde(default)]
    all: Vec<AugmentStat>,
}

#[derive(Debug, Default, Deserialize)]
struct WinRateStats {
    #[serde(default, rename = "winRate")]
    win_rate: f64,
}

#[derive(Debug, Default, Deserialize)]
struct AugmentStat {
    #[serde(default)]
    id: u32,
    #[serde(default, rename = "winRate")]
    win_rate: f64,
}

pub(crate) async fn load_champion_augments(
    champion_id: u32,
) -> Result<ChampionAugmentStats, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("lol-stats hex-winrate-overlay")
        .build()
        .map_err(|error| format!("创建 AramKit 请求失败：{error}"))?;
    let dataset = load_dataset(&client).await?;
    let url = format!(
        "{}/{}/stats/all/champion-details/{}.json",
        dataset.endpoint.trim_end_matches('/'),
        dataset.data_path.trim_matches('/'),
        champion_id
    );
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("读取 AramKit 英雄数据失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("AramKit 英雄数据返回 HTTP {}", response.status()));
    }

    let payload = response
        .json::<ChampionDetailsResponse>()
        .await
        .map_err(|error| format!("解析 AramKit 英雄数据失败：{error}"))?;
    if payload.champion.stats.win_rate <= 0.0 {
        return Err("AramKit 返回的英雄基础胜率无效".to_string());
    }

    let augment_win_rates = payload
        .augments
        .all
        .into_iter()
        .filter(|augment| augment.id > 0 && augment.win_rate > 0.0)
        .map(|augment| (augment.id, augment.win_rate))
        .collect();

    Ok(ChampionAugmentStats {
        base_win_rate: payload.champion.stats.win_rate,
        augment_win_rates,
    })
}

async fn load_dataset(client: &reqwest::Client) -> Result<DatasetCache, String> {
    if let Some(cached) = DATASET_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.clone())
        .filter(|cache| cache.fetched_at.elapsed() < CACHE_TTL)
    {
        return Ok(cached);
    }

    let response = client
        .get(CONFIG_URL)
        .send()
        .await
        .map_err(|error| format!("读取 AramKit 数据版本失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("AramKit 数据版本返回 HTTP {}", response.status()));
    }

    let page = response
        .text()
        .await
        .map_err(|error| format!("读取 AramKit 数据版本内容失败：{error}"))?;
    let (endpoint, data_path) = parse_dataset_config(&page)?;

    let dataset = DatasetCache {
        endpoint,
        data_path,
        fetched_at: Instant::now(),
    };
    if let Ok(mut cache) = DATASET_CACHE.lock() {
        *cache = Some(dataset.clone());
    }
    Ok(dataset)
}

fn parse_dataset_config(page: &str) -> Result<(String, String), String> {
    let endpoint = ENDPOINT_PATTERN
        .captures(page)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
        .ok_or_else(|| "AramKit 页面未找到数据端点".to_string())?;
    let data_path = DATA_PATH_PATTERN
        .captures(page)
        .and_then(|captures| captures.get(1))
        .map(|capture| capture.as_str().to_string())
        .ok_or_else(|| "AramKit 页面未找到数据版本".to_string())?;
    Ok((endpoint, data_path))
}

#[cfg(test)]
mod tests {
    use super::parse_dataset_config;

    #[test]
    fn parses_minified_nuxt_dataset_config() {
        let html = r#"<script>aramKitDataEndpoint:"https://data.aramkit.com",aramKitDataVersions:{latest:"16.16",versions:[{dataPath:"data/16.16-build"}]}</script>"#;
        assert_eq!(
            parse_dataset_config(html).unwrap(),
            (
                "https://data.aramkit.com".to_string(),
                "data/16.16-build".to_string()
            )
        );
    }
}
