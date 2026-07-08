#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// LeagueClientUx.exe 命令行中解析出的连接参数。
///
/// 这些字段对应 LeagueAkari 的 UxCommandLine。国服环境下 `region` 一般为
/// `TENCENT`，`rso_platform_id` 是具体大区标识。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAuth {
    pub port: u16,
    pub pid: u32,
    pub auth_token: String,
    pub region: String,
    pub rso_platform_id: String,
    pub riot_client_port: Option<u16>,
    pub riot_client_auth_token: Option<String>,
    pub source: String,
}

/// 只返回非敏感连接信息给前端。端口可见，token 永远不出后端。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeClientInfo {
    pub pid: u32,
    pub port: u16,
    pub riot_client_port: Option<u16>,
    pub region: String,
    pub rso_platform_id: String,
    pub source: String,
}

impl From<&ClientAuth> for SafeClientInfo {
    fn from(value: &ClientAuth) -> Self {
        Self {
            pid: value.pid,
            port: value.port,
            riot_client_port: value.riot_client_port,
            region: value.region.clone(),
            rso_platform_id: value.rso_platform_id.clone(),
            source: value.source.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub connected: bool,
    pub client: Option<SafeClientInfo>,
    pub summoner: Option<SummonerInfo>,
    pub gameflow_phase: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedStatsResponse {
    #[serde(default)]
    pub queue_map: RankedQueueMap,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RankedQueueMap {
    #[serde(default, rename = "RANKED_SOLO_5x5")]
    pub ranked_solo_5x5: RankedQueueEntry,
    #[serde(default, rename = "RANKED_FLEX_SR")]
    pub ranked_flex_sr: RankedQueueEntry,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedQueueEntry {
    #[serde(default)]
    pub queue_type: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub division: String,
    #[serde(default)]
    pub highest_tier: String,
    #[serde(default)]
    pub highest_division: String,
    #[serde(default)]
    pub previous_season_highest_tier: String,
    #[serde(default)]
    pub previous_season_highest_division: String,
    #[serde(default)]
    pub previous_season_end_tier: String,
    #[serde(default)]
    pub previous_season_end_division: String,
    #[serde(default)]
    pub league_points: i32,
    #[serde(default)]
    pub wins: i32,
    #[serde(default)]
    pub losses: i32,
    #[serde(default)]
    pub is_provisional: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerInfo {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub game_name: String,
    #[serde(default)]
    pub tag_line: String,
    #[serde(default)]
    pub internal_name: String,
    #[serde(default)]
    pub profile_icon_id: u32,
    #[serde(default)]
    pub puuid: String,
    #[serde(default)]
    pub summoner_id: u64,
    #[serde(default)]
    pub summoner_level: u32,
    #[serde(default)]
    pub privacy: String,
}

impl SummonerInfo {
    pub fn riot_id(&self) -> String {
        if !self.game_name.is_empty() && !self.tag_line.is_empty() {
            format!("{}#{}", self.game_name, self.tag_line)
        } else if !self.display_name.is_empty() {
            self.display_name.clone()
        } else {
            self.internal_name.clone()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchHistoryResponse {
    #[serde(default)]
    pub games: MatchHistoryPage,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchHistoryPage {
    #[serde(default)]
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    #[serde(default)]
    pub game_id: u64,
    #[serde(default)]
    pub game_creation: i64,
    #[serde(default)]
    pub game_duration: i64,
    #[serde(default)]
    pub game_mode: String,
    #[serde(default)]
    pub game_type: String,
    #[serde(default)]
    pub queue_id: u32,
    #[serde(default)]
    pub participant_identities: Vec<ParticipantIdentity>,
    #[serde(default)]
    pub participants: Vec<Participant>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantIdentity {
    #[serde(default)]
    pub participant_id: u32,
    #[serde(default)]
    pub player: IdentityPlayer,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPlayer {
    #[serde(default)]
    pub puuid: String,
    #[serde(default)]
    pub game_name: String,
    #[serde(default)]
    pub tag_line: String,
    #[serde(default)]
    pub summoner_name: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    #[serde(default)]
    pub participant_id: u32,
    #[serde(default)]
    pub champion_id: u32,
    #[serde(default)]
    pub spell1_id: u32,
    #[serde(default)]
    pub spell2_id: u32,
    #[serde(default)]
    pub team_id: u32,
    #[serde(default)]
    pub stats: ParticipantStats,
    #[serde(default)]
    pub timeline: ParticipantTimeline,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantStats {
    #[serde(default)]
    pub win: bool,
    #[serde(default)]
    pub kills: u32,
    #[serde(default)]
    pub deaths: u32,
    #[serde(default)]
    pub assists: u32,
    #[serde(default)]
    pub gold_earned: u32,
    #[serde(default)]
    pub total_minions_killed: u32,
    #[serde(default)]
    pub neutral_minions_killed: u32,
    #[serde(default)]
    pub total_damage_dealt_to_champions: u32,
    #[serde(default)]
    pub damage_self_mitigated: u32,
    #[serde(default)]
    pub total_damage_taken: u32,
    #[serde(default)]
    pub total_heal: u32,
    #[serde(default)]
    pub enemy_champion_immobilizations: u32,
    #[serde(default)]
    pub immobilize_and_kill_with_ally: u32,
    #[serde(default)]
    pub vision_score: u32,
    #[serde(default)]
    pub champ_level: u32,
    #[serde(default)]
    pub player_subteam_id: u32,
    #[serde(default)]
    pub item0: u32,
    #[serde(default)]
    pub item1: u32,
    #[serde(default)]
    pub item2: u32,
    #[serde(default)]
    pub item3: u32,
    #[serde(default)]
    pub item4: u32,
    #[serde(default)]
    pub item5: u32,
    #[serde(default)]
    pub item6: u32,
    #[serde(default)]
    pub perk0: u32,
    #[serde(default)]
    pub perk1: u32,
    #[serde(default)]
    pub perk2: u32,
    #[serde(default)]
    pub perk3: u32,
    #[serde(default)]
    pub perk4: u32,
    #[serde(default)]
    pub perk5: u32,
    #[serde(default)]
    pub player_augment1: u32,
    #[serde(default)]
    pub player_augment2: u32,
    #[serde(default)]
    pub player_augment3: u32,
    #[serde(default)]
    pub player_augment4: u32,
    #[serde(default)]
    pub player_augment5: u32,
    #[serde(default)]
    pub player_augment6: u32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantTimeline {
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStatsResponse {
    pub summoner: SummonerInfo,
    pub depth_requested: usize,
    pub depth_loaded: usize,
    pub summary: PlayerSummary,
    pub champion_stats: Vec<ChampionStat>,
    pub recent_games: Vec<RecentGame>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionSummaryItem {
    #[serde(default)]
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub square_portrait_path: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSummary {
    pub games: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub average_kda: f64,
    pub average_kills: f64,
    pub average_deaths: f64,
    pub average_assists: f64,
    pub unique_champions: usize,
    pub most_played_champion_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionStat {
    pub champion_id: u32,
    pub games: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub pick_rate: f64,
    pub average_kda: f64,
    pub average_kills: f64,
    pub average_deaths: f64,
    pub average_assists: f64,
    pub average_damage_to_champions: f64,
    pub average_cs: f64,
    #[serde(default)]
    pub damage_share: f64,
    #[serde(default)]
    pub damage_conversion_rate: f64,
    #[serde(default)]
    pub mitigation_share: f64,
    #[serde(default)]
    pub healing_share: f64,
    #[serde(default)]
    pub gold_share: f64,
    pub last_played_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentGame {
    pub game_id: u64,
    pub champion_id: u32,
    pub queue_id: u32,
    pub game_mode: String,
    pub win: bool,
    #[serde(default)]
    pub spell1_id: u32,
    #[serde(default)]
    pub spell2_id: u32,
    #[serde(default)]
    pub item_ids: Vec<u32>,
    #[serde(default)]
    pub perk_ids: Vec<u32>,
    #[serde(default)]
    pub augment_ids: Vec<u32>,
    #[serde(default)]
    pub team_damage_leader: bool,
    #[serde(default)]
    pub game_damage_leader: bool,
    #[serde(default)]
    pub team_mitigation_leader: bool,
    #[serde(default)]
    pub team_healing_leader: bool,
    #[serde(default)]
    pub team_damage_conversion_leader: bool,
    #[serde(default)]
    pub team_gold_leader: bool,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    #[serde(default)]
    pub team_kills: u32,
    #[serde(default)]
    pub team_deaths: u32,
    /// 这局里与当前玩家同一统计队伍的 PUUID 列表。
    ///
    /// 普通 5v5 按 teamId 计算；CHERRY / 海克斯等多小队模式按 playerSubteamId 计算。
    /// 实时战绩的组排推断会用它统计“当前对局玩家近期是否反复同队”。
    #[serde(default)]
    pub team_puuids: Vec<String>,
    pub kda: f64,
    pub cs: u32,
    #[serde(default)]
    pub gold_earned: u32,
    pub damage_to_champions: u32,
    #[serde(default)]
    pub team_damage_to_champions: u32,
    #[serde(default)]
    pub damage_self_mitigated: u32,
    #[serde(default)]
    pub total_damage_taken: u32,
    #[serde(default)]
    pub team_damage_self_mitigated: u32,
    #[serde(default)]
    pub team_total_damage_taken: u32,
    #[serde(default)]
    pub total_heal: u32,
    #[serde(default)]
    pub team_total_heal: u32,
    #[serde(default)]
    pub team_gold_earned: u32,
    #[serde(default)]
    pub enemy_champion_immobilizations: u32,
    #[serde(default)]
    pub team_enemy_champion_immobilizations: u32,
    #[serde(default)]
    pub immobilize_and_kill_with_ally: u32,
    #[serde(default)]
    pub team_immobilize_and_kill_with_ally: u32,
    pub game_creation: i64,
    pub game_duration: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetailResponse {
    pub game_id: u64,
    pub queue_id: u32,
    pub game_mode: String,
    pub game_creation: i64,
    pub game_duration: i64,
    pub teams: Vec<MatchDetailTeam>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetailTeam {
    pub team_id: u32,
    pub name: String,
    pub win: bool,
    pub players: Vec<MatchDetailPlayer>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetailPlayer {
    pub participant_id: u32,
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub summoner_name: String,
    pub team_id: u32,
    pub player_subteam_id: u32,
    #[serde(flatten)]
    pub record: RecentGame,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAssetBundle {
    pub summoner_spells: Vec<GameAssetEntry>,
    pub items: Vec<GameAssetEntry>,
    pub perks: Vec<GameAssetEntry>,
    pub augments: Vec<GameAssetEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameAssetEntry {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub icon_path: String,
    #[serde(default)]
    pub rarity: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub price: u32,
    #[serde(default)]
    pub price_total: u32,
    #[serde(default)]
    pub in_store: bool,
    #[serde(default)]
    pub display_in_item_sets: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameflowSession {
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub game_data: GameflowGameData,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameflowGameData {
    #[serde(default)]
    pub game_id: u64,
    #[serde(default)]
    pub queue: GameflowQueue,
    /// 游戏中阶段更可信的“玩家 -> 英雄”映射，实时战绩用它反补英雄选择。
    #[serde(default)]
    pub player_champion_selections: Vec<PlayerChampionSelection>,
    #[serde(default)]
    pub team_one: Vec<GameflowPlayer>,
    #[serde(default)]
    pub team_two: Vec<GameflowPlayer>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerChampionSelection {
    #[serde(default)]
    pub puuid: String,
    #[serde(default)]
    pub champion_id: u32,
    #[serde(default)]
    pub summoner_id: u64,
    #[serde(default)]
    pub summoner_internal_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameflowQueue {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub game_mode: String,
    #[serde(default, rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameflowPlayer {
    #[serde(default)]
    pub puuid: String,
    #[serde(default)]
    pub champion_id: u32,
    #[serde(default)]
    pub selected_position: String,
    #[serde(default)]
    pub selected_role: String,
    #[serde(default)]
    pub summoner_id: u64,
    #[serde(default)]
    pub summoner_internal_name: String,
    #[serde(default)]
    pub summoner_name: String,
    #[serde(default)]
    pub team_participant_id: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSession {
    #[serde(default)]
    pub my_team: Vec<ChampSelectPlayer>,
    #[serde(default)]
    pub their_team: Vec<ChampSelectPlayer>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectPlayer {
    #[serde(default)]
    pub cell_id: u32,
    #[serde(default)]
    pub puuid: String,
    #[serde(default)]
    pub champion_id: u32,
    #[serde(default)]
    pub champion_pick_intent: u32,
    #[serde(default)]
    pub assigned_position: String,
    #[serde(default)]
    pub summoner_id: u64,
    #[serde(default)]
    pub team: u32,
    #[serde(default)]
    pub player_type: String,
    #[serde(default)]
    pub name_visibility_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveGameResponse {
    /// LCU 原始 gameflow phase，例如 ChampSelect / InProgress。
    pub phase: String,
    /// 面向实时战绩的简化阶段：champ-select / in-game。
    pub query_stage: String,
    pub game_id: Option<u64>,
    pub queue_id: Option<u32>,
    pub queue_type: Option<String>,
    pub game_mode: Option<String>,
    pub teams: Vec<LiveTeam>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTeam {
    pub name: String,
    pub players: Vec<LivePlayer>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePlayer {
    pub puuid: String,
    pub champion_id: u32,
    pub champion_pick_intent: u32,
    pub position: String,
    pub selected_role: String,
    pub summoner_id: u64,
    pub team_participant_id: Option<u32>,
    pub is_placeholder: bool,
    pub premade: Option<LivePremadeMarker>,
    pub summoner: Option<SummonerInfo>,
    pub stats: Option<PlayerStatsResponse>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePremadeMarker {
    pub group_id: String,
    pub label: String,
    pub source: String,
    pub together_times: usize,
    pub member_puuids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiotAlias {
    pub puuid: String,
    pub alias: RiotAliasName,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiotAliasName {
    pub game_name: String,
    pub tag_line: String,
}
