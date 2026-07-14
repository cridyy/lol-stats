import { invoke } from "@tauri-apps/api/core"
import type {
  AppUpdateInfo,
  ChampionSummaryItem,
  ChatStatus,
  ConnectionStatus,
  FriendToolEntry,
  GameAssetBundle,
  LiveGameResponse,
  MatchDetailResponse,
  PlayerStatsResponse,
  RankedStatsResponse,
  SummonerSearchCandidate,
} from "./types"

export function acceptReadyCheck() {
  return invoke<void>("accept_ready_check")
}

export function dismissEndOfGame() {
  return invoke<void>("dismiss_end_of_game")
}

export function getGameSettingsLocked() {
  return invoke<boolean>("get_game_settings_locked")
}

export function setGameSettingsLocked(locked: boolean) {
  return invoke<boolean>("set_game_settings_locked", { locked })
}

export function getChatStatus() {
  return invoke<ChatStatus>("get_chat_status")
}

export function setChatAvailability(availability: string) {
  return invoke<void>("set_chat_availability", { availability })
}

export function setChatStatusMessage(statusMessage: string) {
  return invoke<void>("set_chat_status_message", { statusMessage })
}

export function loadFriends(includeSince = true) {
  return invoke<FriendToolEntry[]>("load_friends", { includeSince })
}

export function checkAppUpdate() {
  return invoke<AppUpdateInfo>("check_app_update")
}

export function appVersion() {
  return invoke<string>("app_version")
}

export function connectionStatus() {
  return invoke<ConnectionStatus>("connection_status")
}

export function loadChampions() {
  return invoke<ChampionSummaryItem[]>("load_champions")
}

export function loadGameAssets() {
  return invoke<GameAssetBundle>("load_game_assets")
}

export function loadCurrentRankedStats() {
  return invoke<RankedStatsResponse>("load_current_ranked_stats")
}

export function loadGameflowPhase() {
  return invoke<string | null>("load_gameflow_phase")
}

export function loadRankedStats(puuid: string, sgpServerId?: string) {
  return invoke<RankedStatsResponse>("load_ranked_stats", { puuid, sgpServerId })
}

export function loadLcuAsset(path: string) {
  return invoke<string>("load_lcu_asset", { path })
}

export function loadLcuAssets(paths: string[]) {
  return invoke<Record<string, string>>("load_lcu_assets", { paths })
}

export function searchSummonerCandidates(query: string, sgpServerId?: string) {
  return invoke<SummonerSearchCandidate[]>("search_summoner_candidates", { query, sgpServerId })
}

export function loadSelfStats(depth: number, forceRefresh = false, persistCache = true) {
  return invoke<PlayerStatsResponse>("load_self_stats", { depth, forceRefresh, persistCache })
}

export function loadSelfStatsWithProgress(
  depth: number,
  requestId: string,
  forceRefresh = false,
  persistCache = true,
) {
  return invoke<PlayerStatsResponse>("load_self_stats_with_progress", {
    depth,
    requestId,
    forceRefresh,
    persistCache,
  })
}

export function searchPlayer(
  query: string,
  depth: number,
  sgpServerId?: string,
  forceRefresh = false,
  persistCache = true,
) {
  return invoke<PlayerStatsResponse>("search_player", {
    query,
    depth,
    sgpServerId,
    forceRefresh,
    persistCache,
  })
}

export function searchPlayerWithProgress(
  query: string,
  depth: number,
  requestId: string,
  sgpServerId?: string,
  forceRefresh = false,
  persistCache = true,
) {
  return invoke<PlayerStatsResponse>("search_player_with_progress", {
    query,
    depth,
    requestId,
    sgpServerId,
    forceRefresh,
    persistCache,
  })
}

export function cancelStatsLoad(requestId: string) {
  return invoke<void>("cancel_stats_load", { requestId })
}

export function copyPngToClipboard(bytes: number[]) {
  return invoke<void>("copy_png_to_clipboard", { bytes })
}

export function loadMatchDetail(gameId: number, sgpServerId?: string) {
  return invoke<MatchDetailResponse>("load_match_detail", { gameId, sgpServerId })
}

export function loadLiveGame(depth: number) {
  return invoke<LiveGameResponse>("load_live_game", { depth })
}
