//! 海克斯大乱斗英雄全服基线数据的本地同步。
//!
//! 该数据集与软件版本更新完全独立：应用每天首次启动时仅请求一次远端清单，只有
//! 数据版本、哈希或本地文件状态发生变化时才下载新的快照。

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DATA_MANIFEST_URL: &str =
    "https://gitee.com/Crescenre/lol-stats-data/raw/master/manifest.json";
const DATA_RAW_BASE_URL: &str = "https://gitee.com/Crescenre/lol-stats-data/raw/master/";
const USER_AGENT: &str = "lol-stats global-baselines";
const REQUEST_TIMEOUT_SECONDS: u64 = 12;
const SCHEMA_VERSION: u32 = 1;
const MAX_DATASET_BYTES: u64 = 16 * 1024 * 1024;
const STORAGE_DIRECTORY_NAME: &str = "global-baselines";
const LOCAL_MANIFEST_FILE_NAME: &str = "manifest.json";
const LOCAL_DATASET_FILE_NAME: &str = "current.json";
const CHECK_STATE_FILE_NAME: &str = "check-state.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalBaselineSource {
    name: String,
    mode: String,
    index_url: String,
    source_data_date: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalChampionBaseline {
    id: u32,
    slug: String,
    name: String,
    /// ARAMKit 当前快照中的扩展英雄均值。旧快照没有这些字段时仍可读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    average_kda: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    average_kills: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    average_deaths: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    average_assists: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    participation_rate_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    multikills: Option<f64>,
    damage_to_champions: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    physical_damage_to_champions: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    magic_damage_to_champions: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    true_damage_to_champions: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    damage_share_percent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    turret_damage: Option<f64>,
    damage_taken: f64,
    self_mitigated_damage: f64,
    ally_healing: f64,
    ally_shielding: f64,
    crowd_control_duration_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gold_earned: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cs: Option<f64>,
    game_duration_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    damage_gold_conversion_rate_percent: Option<f64>,
    #[serde(default)]
    relative_to_global_average_percent: HashMap<String, f64>,
    source_data_date: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalBaselineDataset {
    schema_version: u32,
    dataset_version: String,
    generated_at: String,
    source: GlobalBaselineSource,
    #[serde(default)]
    metric_descriptions: HashMap<String, String>,
    #[serde(default)]
    global_averages: HashMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    global_average_estimation: Option<serde_json::Value>,
    expected_champion_count: u32,
    champion_count: u32,
    unavailable_champion_count: u32,
    #[serde(default)]
    unavailable_champions: Vec<serde_json::Value>,
    partial: bool,
    champions: Vec<GlobalChampionBaseline>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteManifest {
    schema_version: u32,
    dataset_version: String,
    data_path: String,
    data_bytes: u64,
    data_sha256: String,
    expected_champion_count: u32,
    champion_count: u32,
    unavailable_champion_count: u32,
    partial: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatasetHeader {
    schema_version: u32,
    dataset_version: String,
    expected_champion_count: u32,
    champion_count: u32,
    unavailable_champion_count: u32,
    partial: bool,
    #[serde(default)]
    champions: Vec<serde_json::Value>,
    #[serde(default)]
    unavailable_champions: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyCheckState {
    last_checked_on: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshResult {
    SkippedToday,
    Unchanged,
    Updated,
}

/// 在应用启动后异步检查一次数据仓库。网络或本地文件失败不会影响主窗口启动。
pub(crate) fn start_daily_update_check() {
    tauri::async_runtime::spawn(async {
        match refresh_if_due().await {
            Ok(RefreshResult::SkippedToday) => {
                eprintln!("[global-baselines] 今天已经检查过数据更新");
            }
            Ok(RefreshResult::Unchanged) => {
                eprintln!("[global-baselines] 全服基线数据已是最新版本");
            }
            Ok(RefreshResult::Updated) => {
                eprintln!("[global-baselines] 全服基线数据已更新");
            }
            Err(error) => {
                eprintln!("[global-baselines] 检查数据更新失败：{error}");
            }
        }
    });
}

pub(crate) fn load_current_dataset() -> Result<GlobalBaselineDataset, String> {
    let storage_directory = storage_directory()?;
    if !local_dataset_is_valid(&storage_directory) {
        return Err("本地全服基线数据不存在或校验失败".to_string());
    }

    let dataset =
        read_json::<GlobalBaselineDataset>(&storage_directory.join(LOCAL_DATASET_FILE_NAME))?;
    validate_scoring_dataset(&dataset)?;
    Ok(dataset)
}

fn validate_scoring_dataset(dataset: &GlobalBaselineDataset) -> Result<(), String> {
    if dataset.schema_version != SCHEMA_VERSION
        || dataset.partial
        || dataset.champion_count == 0
        || dataset.champions.len() != dataset.champion_count as usize
        || dataset.unavailable_champions.len() != dataset.unavailable_champion_count as usize
        || dataset.champion_count + dataset.unavailable_champion_count
            != dataset.expected_champion_count
    {
        return Err("本地全服基线数据结构无效".to_string());
    }

    let mut champion_ids = std::collections::HashSet::new();
    for champion in &dataset.champions {
        let required_values = [
            champion.damage_to_champions,
            champion.damage_taken,
            champion.self_mitigated_damage,
            champion.ally_healing,
            champion.ally_shielding,
            champion.crowd_control_duration_seconds,
            champion.game_duration_seconds,
        ];
        if champion.id == 0
            || !champion_ids.insert(champion.id)
            || champion.game_duration_seconds <= 0.0
            || required_values
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(format!("英雄 {} 的全服基线数据无效", champion.id));
        }

        let optional_values = [
            champion.average_kda,
            champion.average_kills,
            champion.average_deaths,
            champion.average_assists,
            champion.participation_rate_percent,
            champion.multikills,
            champion.physical_damage_to_champions,
            champion.magic_damage_to_champions,
            champion.true_damage_to_champions,
            champion.damage_share_percent,
            champion.turret_damage,
            champion.gold_earned,
            champion.cs,
            champion.damage_gold_conversion_rate_percent,
        ];
        if optional_values.iter().flatten().any(|value| !value.is_finite() || *value < 0.0)
            || champion
                .relative_to_global_average_percent
                .values()
                .any(|value| !value.is_finite())
        {
            return Err(format!("英雄 {} 的扩展全服基线数据无效", champion.id));
        }
    }

    if dataset
        .global_averages
        .values()
        .any(|value| !value.is_finite())
    {
        return Err("全服人均基准数据无效".to_string());
    }

    Ok(())
}

async fn refresh_if_due() -> Result<RefreshResult, String> {
    let storage_directory = storage_directory()?;
    fs::create_dir_all(&storage_directory)
        .map_err(|error| format!("创建全服基线数据目录失败：{error}"))?;

    let today = local_date_key();
    let state_path = storage_directory.join(CHECK_STATE_FILE_NAME);
    let state = read_json::<DailyCheckState>(&state_path).unwrap_or_default();
    let local_ready = local_dataset_is_valid(&storage_directory);

    if state.last_checked_on.as_deref() == Some(today.as_str()) && local_ready {
        return Ok(RefreshResult::SkippedToday);
    }

    let result = refresh_from_remote(&storage_directory).await;
    let next_state = DailyCheckState {
        last_checked_on: Some(today),
    };
    if let Err(error) = write_json_atomic(&state_path, &next_state) {
        eprintln!("[global-baselines] 记录每日检查状态失败：{error}");
    }

    result
}

async fn refresh_from_remote(storage_directory: &Path) -> Result<RefreshResult, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("创建数据请求客户端失败：{error}"))?;

    let remote_manifest_bytes = fetch_bytes(&client, DATA_MANIFEST_URL, "数据清单").await?;
    let remote_manifest = serde_json::from_slice::<RemoteManifest>(&remote_manifest_bytes)
        .map_err(|error| format!("解析远端数据清单失败：{error}"))?;
    validate_manifest(&remote_manifest)?;

    if local_manifest_matches(storage_directory, &remote_manifest)
        && local_dataset_is_valid(storage_directory)
    {
        return Ok(RefreshResult::Unchanged);
    }

    let data_url = remote_data_url(&remote_manifest)?;
    let dataset_bytes = fetch_bytes(&client, &data_url, "全服基线数据").await?;
    validate_dataset_bytes(&dataset_bytes, &remote_manifest)?;

    // 先替换数据快照，再写入清单；异常退出时客户端仍可继续使用旧快照。
    write_bytes_atomic(
        &storage_directory.join(LOCAL_DATASET_FILE_NAME),
        &dataset_bytes,
    )?;
    write_bytes_atomic(
        &storage_directory.join(LOCAL_MANIFEST_FILE_NAME),
        &remote_manifest_bytes,
    )?;

    Ok(RefreshResult::Updated)
}

async fn fetch_bytes(
    client: &reqwest::Client,
    url: &str,
    resource_name: &str,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("请求{resource_name}失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("请求{resource_name}失败：{error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取{resource_name}内容失败：{error}"))?;
    Ok(bytes.to_vec())
}

fn validate_manifest(manifest: &RemoteManifest) -> Result<(), String> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!("不支持的数据清单版本：{}", manifest.schema_version));
    }
    if manifest.dataset_version.trim().is_empty() {
        return Err("数据清单缺少 datasetVersion".to_string());
    }
    if manifest.partial {
        return Err("远端全服基线数据不是完整快照".to_string());
    }
    if manifest.champion_count == 0
        || manifest.expected_champion_count == 0
        || manifest.champion_count + manifest.unavailable_champion_count
            != manifest.expected_champion_count
    {
        return Err("远端数据清单中的英雄数量无效".to_string());
    }
    if manifest.data_bytes == 0 || manifest.data_bytes > MAX_DATASET_BYTES {
        return Err(format!("远端数据文件大小无效：{}", manifest.data_bytes));
    }
    if !is_sha256(&manifest.data_sha256) {
        return Err("远端数据清单中的 SHA-256 格式无效".to_string());
    }
    validate_remote_data_path(&manifest.data_path)?;
    Ok(())
}

fn validate_dataset_bytes(bytes: &[u8], manifest: &RemoteManifest) -> Result<(), String> {
    if bytes.len() as u64 != manifest.data_bytes {
        return Err(format!(
            "下载的数据大小不匹配：期望 {}，实际 {}",
            manifest.data_bytes,
            bytes.len()
        ));
    }

    let actual_hash = sha256_hex(bytes);
    if !actual_hash.eq_ignore_ascii_case(manifest.data_sha256.trim()) {
        return Err("下载的数据 SHA-256 校验失败".to_string());
    }

    let header = serde_json::from_slice::<DatasetHeader>(bytes)
        .map_err(|error| format!("解析下载的数据文件失败：{error}"))?;
    if header.schema_version != manifest.schema_version
        || header.dataset_version != manifest.dataset_version
        || header.expected_champion_count != manifest.expected_champion_count
        || header.champion_count != manifest.champion_count
        || header.unavailable_champion_count != manifest.unavailable_champion_count
        || header.partial != manifest.partial
        || header.champions.len() != manifest.champion_count as usize
        || header.unavailable_champions.len() != manifest.unavailable_champion_count as usize
    {
        return Err("下载的数据文件与清单不一致".to_string());
    }
    Ok(())
}

fn local_manifest_matches(storage_directory: &Path, remote: &RemoteManifest) -> bool {
    let path = storage_directory.join(LOCAL_MANIFEST_FILE_NAME);
    let Ok(local) = read_json::<RemoteManifest>(&path) else {
        return false;
    };

    local.dataset_version == remote.dataset_version
        && local.data_path == remote.data_path
        && local.data_bytes == remote.data_bytes
        && local.data_sha256.eq_ignore_ascii_case(&remote.data_sha256)
}

fn local_dataset_is_valid(storage_directory: &Path) -> bool {
    let manifest_path = storage_directory.join(LOCAL_MANIFEST_FILE_NAME);
    let dataset_path = storage_directory.join(LOCAL_DATASET_FILE_NAME);
    let Ok(manifest) = read_json::<RemoteManifest>(&manifest_path) else {
        return false;
    };
    validate_manifest(&manifest).is_ok()
        && fs::read(dataset_path)
            .ok()
            .and_then(|bytes| validate_dataset_bytes(&bytes, &manifest).ok())
            .is_some()
}

fn remote_data_url(manifest: &RemoteManifest) -> Result<String, String> {
    let path = validate_remote_data_path(&manifest.data_path)?;
    Ok(format!("{DATA_RAW_BASE_URL}{path}"))
}

fn validate_remote_data_path(path: &str) -> Result<&str, String> {
    let trimmed = path.trim();
    let file_name = trimmed
        .strip_prefix("datasets/")
        .filter(|file_name| !file_name.is_empty())
        .ok_or_else(|| "远端数据路径必须位于 datasets/ 目录".to_string())?;
    if file_name.contains(['/', '\\'])
        || file_name.contains("..")
        || !file_name.ends_with(".json")
        || !file_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("远端数据路径不安全或格式不正确".to_string());
    }
    Ok(trimmed)
}

fn is_sha256(value: &str) -> bool {
    let value = value.trim();
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn storage_directory() -> Result<PathBuf, String> {
    let mut directory = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    directory.push("lol-stats");
    directory.push(STORAGE_DIRECTORY_NAME);
    Ok(directory)
}

fn read_json<T>(path: &Path) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let content =
        fs::read(path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    serde_json::from_slice(&content)
        .map_err(|error| format!("解析 {} 失败：{error}", path.display()))
}

fn write_json_atomic<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("序列化 {} 失败：{error}", path.display()))?;
    write_bytes_atomic(path, &bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定 {} 的父目录", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("创建 {} 失败：{error}", parent.display()))?;

    let temporary_path = temporary_path(path)?;
    let write_result = (|| -> Result<(), String> {
        let mut file = File::create(&temporary_path)
            .map_err(|error| format!("创建临时数据文件失败：{error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("写入临时数据文件失败：{error}"))?;
        file.sync_all()
            .map_err(|error| format!("同步临时数据文件失败：{error}"))?;
        replace_file(&temporary_path, path)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn temporary_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("无法创建 {} 的临时文件名", path.display()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(path.with_file_name(format!(".{file_name}.{nonce}.tmp")))
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target_wide = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let succeeded = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            target_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        return Err(format!(
            "替换 {} 失败：{}",
            target.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|error| format!("替换 {} 失败：{error}", target.display()))
}

#[cfg(target_os = "windows")]
fn local_date_key() -> String {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    let mut local_time = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    unsafe { GetLocalTime(&mut local_time) };
    format!(
        "{:04}-{:02}-{:02}",
        local_time.wYear, local_time.wMonth, local_time.wDay
    )
}

#[cfg(not(target_os = "windows"))]
fn local_date_key() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    format!("utc-day-{days}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_scoring_dataset() -> GlobalBaselineDataset {
        GlobalBaselineDataset {
            schema_version: SCHEMA_VERSION,
            dataset_version: "dataset-1".to_string(),
            generated_at: "2026-08-29T00:00:00Z".to_string(),
            source: GlobalBaselineSource {
                name: "test".to_string(),
                mode: "hex-aram".to_string(),
                index_url: "https://example.com".to_string(),
                source_data_date: "2026-08-28".to_string(),
            },
            metric_descriptions: HashMap::new(),
            global_averages: HashMap::new(),
            global_average_estimation: None,
            expected_champion_count: 1,
            champion_count: 1,
            unavailable_champion_count: 0,
            unavailable_champions: Vec::new(),
            partial: false,
            champions: vec![GlobalChampionBaseline {
                id: 1,
                slug: "annie".to_string(),
                name: "黑暗之女".to_string(),
                average_kda: None,
                average_kills: None,
                average_deaths: None,
                average_assists: None,
                participation_rate_percent: None,
                multikills: None,
                damage_to_champions: 36_912.0,
                physical_damage_to_champions: None,
                magic_damage_to_champions: None,
                true_damage_to_champions: None,
                damage_share_percent: None,
                turret_damage: None,
                damage_taken: 27_053.0,
                self_mitigated_damage: 18_415.0,
                ally_healing: 18.0,
                ally_shielding: 4_495.0,
                crowd_control_duration_seconds: 43.9,
                gold_earned: None,
                cs: None,
                game_duration_seconds: 1_032.0,
                damage_gold_conversion_rate_percent: None,
                relative_to_global_average_percent: HashMap::new(),
                source_data_date: "2026-08-28".to_string(),
            }],
        }
    }

    fn valid_manifest() -> RemoteManifest {
        RemoteManifest {
            schema_version: SCHEMA_VERSION,
            dataset_version: "aramkit-hex-aram-2026-08-28".to_string(),
            data_path: "datasets/aramkit-hex-aram-2026-08-28.json".to_string(),
            data_bytes: 1024,
            data_sha256: "d7162da6a7805521be9cedce24493e34e9b44b42642ca7eb2dfa4c1e73a27703"
                .to_string(),
            expected_champion_count: 173,
            champion_count: 172,
            unavailable_champion_count: 1,
            partial: false,
        }
    }

    #[test]
    fn accepts_valid_remote_path() {
        assert_eq!(
            validate_remote_data_path("datasets/data-2026_08.json").unwrap(),
            "datasets/data-2026_08.json"
        );
    }

    #[test]
    fn rejects_unsafe_remote_path() {
        for path in [
            "../manifest.json",
            "datasets/../manifest.json",
            "datasets/nested/data.json",
            "datasets/data.json?cache=1",
            "datasets/data.exe",
        ] {
            assert!(validate_remote_data_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn validates_manifest_counts_and_sha() {
        assert!(validate_manifest(&valid_manifest()).is_ok());

        let mut manifest = valid_manifest();
        manifest.unavailable_champion_count = 2;
        assert!(validate_manifest(&manifest).is_err());

        let mut manifest = valid_manifest();
        manifest.data_sha256 = "invalid".to_string();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn sha256_matches_known_value() {
        assert_eq!(
            sha256_hex(b"lol-stats"),
            "f2254a0d88fd609ab941deec1f001e294f1095d98cfad07c83e7b6b0ff0e906c"
        );
    }

    #[test]
    fn validates_dataset_header_against_manifest() {
        let mut manifest = valid_manifest();
        let dataset = serde_json::json!({
            "schemaVersion": 1,
            "datasetVersion": manifest.dataset_version,
            "expectedChampionCount": 173,
            "championCount": 172,
            "unavailableChampionCount": 1,
            "partial": false,
            "champions": vec![serde_json::json!({}); 172],
            "unavailableChampions": [serde_json::json!({})],
        });
        let bytes = serde_json::to_vec(&dataset).unwrap();
        manifest.data_bytes = bytes.len() as u64;
        manifest.data_sha256 = sha256_hex(&bytes);

        assert!(validate_dataset_bytes(&bytes, &manifest).is_ok());

        manifest.champion_count = 171;
        assert!(validate_dataset_bytes(&bytes, &manifest).is_err());
    }

    #[test]
    fn validates_scoring_dataset_values_and_unique_champions() {
        let dataset = valid_scoring_dataset();
        assert!(validate_scoring_dataset(&dataset).is_ok());

        let mut duplicate = dataset.clone();
        duplicate.expected_champion_count = 2;
        duplicate.champion_count = 2;
        duplicate.champions.push(duplicate.champions[0].clone());
        assert!(validate_scoring_dataset(&duplicate).is_err());

        let mut invalid_duration = dataset;
        invalid_duration.champions[0].game_duration_seconds = 0.0;
        assert!(validate_scoring_dataset(&invalid_duration).is_err());
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let directory = std::env::temp_dir().join(format!(
            "lol-stats-global-baselines-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let file_path = directory.join("current.json");
        fs::write(&file_path, b"old-data").unwrap();

        write_bytes_atomic(&file_path, b"new-data").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"new-data");

        fs::remove_dir_all(directory).unwrap();
    }
}
