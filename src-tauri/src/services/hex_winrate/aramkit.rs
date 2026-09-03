use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;

// 英雄总表页包含当前数据版本和 CDN 入口，不依赖任何单英雄路由。
const CONFIG_URL: &str = "https://aramkit.com/zh-CN/champions";
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

static DATASET_CACHE: LazyLock<Mutex<Option<DatasetCache>>> = LazyLock::new(|| Mutex::new(None));
// 总表只在当前进程第一次成功加载后缓存。失败的初始化不会写入 OnceCell，
// 由上层按退避策略重试；成功后整个进程只保留这一份数据。
static CHAMPION_RANKINGS_CACHE: tokio::sync::OnceCell<ChampionWinrateTable> =
    tokio::sync::OnceCell::const_new();
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

#[derive(Clone, Default)]
pub(crate) struct ChampionWinrateTable {
    win_rates: HashMap<u32, f64>,
}

impl ChampionWinrateTable {
    pub(crate) fn win_rate(&self, champion_id: u32) -> Option<f64> {
        self.win_rates.get(&champion_id).copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.win_rates.len()
    }
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

#[derive(Debug, Deserialize)]
struct ChampionRankingsResponse {
    #[serde(default)]
    rows: Vec<ChampionRankingRow>,
}

#[derive(Debug, Deserialize)]
struct ChampionRankingRow {
    #[serde(default)]
    id: u32,
    #[serde(default, rename = "winRate")]
    win_rate: f64,
}

fn aramkit_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("lol-stats hex-winrate-overlay")
        .build()
        .map_err(|error| format!("创建 AramKit 请求失败：{error}"))
}

/// 加载当前版本的全英雄胜率总表。
///
/// 这个缓存只存在于进程内，不写入本地数据库；软件退出后自然失效。只有成功结果
/// 会写入 OnceCell，失败时由调用方负责稍后重试。
pub(crate) async fn load_champion_rankings_once() -> Result<ChampionWinrateTable, String> {
    CHAMPION_RANKINGS_CACHE
        .get_or_try_init(|| async { fetch_champion_rankings().await })
        .await
        .cloned()
}

async fn fetch_champion_rankings() -> Result<ChampionWinrateTable, String> {
    let client = aramkit_client()?;
    let dataset = load_dataset(&client).await?;
    let url = format!(
        "{}/{}/stats/all/champion-rankings.json",
        dataset.endpoint.trim_end_matches('/'),
        dataset.data_path.trim_matches('/')
    );
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("读取 AramKit 英雄总表失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("AramKit 英雄总表返回 HTTP {}", response.status()));
    }

    let body = response
        .text()
        .await
        .map_err(|error| format!("读取 AramKit 英雄总表内容失败：{error}"))?;
    parse_champion_rankings(&body)
}

fn parse_champion_rankings(body: &str) -> Result<ChampionWinrateTable, String> {
    let payload = serde_json::from_str::<ChampionRankingsResponse>(body)
        .map_err(|error| format!("解析 AramKit 英雄总表失败：{error}"))?;
    let win_rates = payload
        .rows
        .into_iter()
        .filter_map(|row| {
            if row.id == 0 || !row.win_rate.is_finite() || row.win_rate <= 0.0 {
                return None;
            }
            // 当前接口使用 0.5789；兼容少数返回 57.89 的镜像格式。
            let win_rate = if row.win_rate > 1.0 && row.win_rate <= 100.0 {
                row.win_rate / 100.0
            } else {
                row.win_rate
            };
            (win_rate <= 1.0).then_some((row.id, win_rate))
        })
        .collect::<HashMap<_, _>>();

    if win_rates.is_empty() {
        return Err("AramKit 英雄总表没有有效数据".to_string());
    }

    Ok(ChampionWinrateTable { win_rates })
}

pub(crate) async fn load_champion_augments(
    champion_id: u32,
) -> Result<ChampionAugmentStats, String> {
    let client = aramkit_client()?;
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
    use super::{parse_champion_rankings, parse_dataset_config};

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

    #[test]
    fn parses_champion_rankings_by_id() {
        let table = parse_champion_rankings(
            r#"{"rows":[{"id":67,"winRate":0.5789},{"id":104,"winRate":57.53},{"id":0,"winRate":0}]}"#,
        )
        .expect("总表应能解析");

        assert_eq!(table.len(), 2);
        assert!((table.win_rate(67).unwrap() - 0.5789).abs() < f64::EPSILON);
        assert!((table.win_rate(104).unwrap() - 0.5753).abs() < f64::EPSILON);
        assert_eq!(table.win_rate(0), None);
    }

    #[test]
    fn rejects_empty_champion_rankings() {
        assert!(parse_champion_rankings(r#"{"rows":[]}"#).is_err());
    }
}
