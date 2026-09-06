export interface SummonerInfo {
  displayName: string
  gameName: string
  tagLine: string
  internalName: string
  profileIconId: number
  puuid: string
  summonerId: number
  summonerLevel: number
  privacy: string
}

export interface SafeClientInfo {
  pid: number
  port: number
  riotClientPort?: number
  region: string
  rsoPlatformId: string
  source: string
}

export interface ConnectionStatus {
  connected: boolean
  client?: SafeClientInfo
  summoner?: SummonerInfo
  gameflowPhase?: string
  message: string
}

export interface ChatStatus {
  availability: string
  statusMessage: string
}

export interface FriendToolEntry {
  id: string
  puuid: string
  gameName: string
  tagLine: string
  displayName: string
  iconId: number
  summonerId: number
  availability: string
  statusMessage: string
  friendsSince?: string | null
}

export interface AppUpdateInfo {
  currentVersion: string
  latestVersion: string
  hasUpdate: boolean
  releasePageUrl: string
  releaseName?: string
  releaseMessage?: string
  releaseNotes: string[]
}

export interface AramkitProxySettings {
  enabled: boolean
  address: string
  wtlsEnabled: boolean
  timeoutSeconds: number
}

export interface AramkitConnectionTest {
  outcome: "success" | "warning" | "error"
  elapsedMs: number
  championCount: number
  augmentCount: number
  baseWinRate: number | null
  summary: string
  report: string
}

export interface RankedStatsResponse {
  queueMap: RankedQueueMap
  unavailableReason?: string
}

export interface RankedQueueMap {
  RANKED_SOLO_5x5: RankedQueueEntry
  RANKED_FLEX_SR: RankedQueueEntry
}

export interface RankedQueueEntry {
  queueType: string
  tier: string
  division: string
  highestTier: string
  highestDivision: string
  previousSeasonHighestTier: string
  previousSeasonHighestDivision: string
  previousSeasonEndTier: string
  previousSeasonEndDivision: string
  leaguePoints: number
  wins: number
  losses: number
  isProvisional: boolean
}

export interface SummonerSearchCandidate {
  puuid: string
  gameName: string
  tagLine: string
  sgpServerId: string
  profileIconId: number
  summonerLevel: number
  privacy: string
}

export interface PlayerSummary {
  games: number
  wins: number
  losses: number
  winRate: number
  averageKda: number
  averageKills: number
  averageDeaths: number
  averageAssists: number
  uniqueChampions: number
  mostPlayedChampionId?: number
}

export interface ChampionStat {
  championId: number
  games: number
  wins: number
  losses: number
  winRate: number
  pickRate: number
  averageKda: number
  averageKills: number
  averageDeaths: number
  averageAssists: number
  averageDamageToChampions: number
  averageCs: number
  damageShare: number
  damageConversionRate: number
  mitigationShare: number
  healingShare: number
  goldShare: number
  lastPlayedAt: number
}

export interface RatingCompositionEntry {
  championId: number
  itemIds: number[]
}

export interface RecentGameParticipantSummary {
  puuid: string
  summonerName: string
  championId: number
}

export interface RecentGame {
  gameId: number
  championId: number
  queueId: number
  gameMode: string
  win: boolean
  teamId: number
  playerSubteamId?: number
  spell1Id: number
  spell2Id: number
  itemIds: number[]
  perkIds: number[]
  augmentIds: number[]
  teamDamageLeader: boolean
  gameDamageLeader: boolean
  teamMitigationLeader: boolean
  teamHealingLeader: boolean
  teamDamageConversionLeader: boolean
  teamGoldLeader: boolean
  kills: number
  deaths: number
  assists: number
  teamKills: number
  teamDeaths: number
  gameKills?: number
  gameDeaths?: number
  teamTowerKills: number
  gameTowerKills: number
  loserFirstTowerTimeSeconds?: number
  teamPlayerCount?: number
  gamePlayerCount?: number
  teamPuuids: string[]
  teamComposition?: RatingCompositionEntry[]
  gameComposition?: RatingCompositionEntry[]
  /** 本局所有参与者的可评分快照；旧缓存可能没有该字段。 */
  matchParticipants?: RecentGameParticipantRecord[]
  premadeTeammatePuuids?: string[]
  allyPlayers?: RecentGameParticipantSummary[]
  enemyPlayers?: RecentGameParticipantSummary[]
  premadeSource?: string
  premadeTogetherTimes?: number
  kda: number
  cs: number
  goldEarned: number
  damageToChampions: number
  teamDamageToChampions: number
  gameDamageToChampions?: number
  damageSelfMitigated: number
  totalDamageTaken: number
  teamDamageSelfMitigated: number
  teamTotalDamageTaken: number
  totalHeal: number
  totalHealsOnTeammates?: number
  totalDamageShieldedOnTeammates: number
  teamTotalHeal: number
  teamGoldEarned: number
  gameGoldEarned?: number
  enemyChampionImmobilizations: number
  teamEnemyChampionImmobilizations: number
  immobilizeAndKillWithAlly: number
  teamImmobilizeAndKillWithAlly: number
  timeCCingOthers?: number
  gameCreation: number
  gameDuration: number
}

/** 同局玩家的身份与完整评分输入，字段与 RecentGame 保持一致。 */
export interface RecentGameParticipantRecord extends RecentGame {
  participantId: number
  puuid: string
  gameName: string
  tagLine: string
  summonerName: string
  isOwner: boolean
}

export interface GlobalBaselineSource {
  name: string
  mode: string
  indexUrl: string
  sourceDataDate: string
}

/** 当前全服基准快照支持的全部指标。 */
export type GlobalBaselineMetricKey =
  | "averageKda"
  | "averageKills"
  | "averageDeaths"
  | "averageAssists"
  | "participationRatePercent"
  | "multikills"
  | "damageToChampions"
  | "physicalDamageToChampions"
  | "magicDamageToChampions"
  | "trueDamageToChampions"
  | "damageSharePercent"
  | "turretDamage"
  | "damageTaken"
  | "selfMitigatedDamage"
  | "allyHealing"
  | "allyShielding"
  | "crowdControlDurationSeconds"
  | "goldEarned"
  | "cs"
  | "gameDurationSeconds"
  | "damageGoldConversionRatePercent"

export type GlobalBaselineMetricValues = Partial<
  Record<GlobalBaselineMetricKey, number>
>

export interface GlobalAverageEstimation {
  method?: string
  formula?: {
    relativePercent?: string
    percentagePointDifference?: string
  }
  percentagePointMetrics?: readonly GlobalBaselineMetricKey[]
  candidateCounts?: Partial<Record<GlobalBaselineMetricKey, number>>
  excludedCandidateCounts?: Partial<Record<GlobalBaselineMetricKey, number>>
  incompleteMetrics?: readonly GlobalBaselineMetricKey[]
}

export interface GlobalChampionBaseline {
  id: number
  slug: string
  name: string
  /** 英雄全服平均的 21 项原始数据；旧快照可能没有这些扩展字段。 */
  averageKda?: number
  averageKills?: number
  averageDeaths?: number
  averageAssists?: number
  participationRatePercent?: number
  multikills?: number
  damageToChampions: number
  physicalDamageToChampions?: number
  magicDamageToChampions?: number
  trueDamageToChampions?: number
  damageSharePercent?: number
  turretDamage?: number
  damageTaken: number
  selfMitigatedDamage: number
  allyHealing: number
  allyShielding: number
  crowdControlDurationSeconds: number
  goldEarned?: number
  cs?: number
  gameDurationSeconds: number
  damageGoldConversionRatePercent?: number
  relativeToGlobalAveragePercent?: GlobalBaselineMetricValues
  sourceDataDate: string
}

export interface GlobalBaselineDataset {
  schemaVersion: number
  datasetVersion: string
  generatedAt: string
  source: GlobalBaselineSource
  metricDescriptions?: Partial<Record<GlobalBaselineMetricKey, string>>
  /** 根据英雄“较平均”字段反算后的全服人均，通常包含 21 项。 */
  globalAverages?: GlobalBaselineMetricValues
  globalAverageEstimation?: GlobalAverageEstimation
  expectedChampionCount: number
  championCount: number
  unavailableChampionCount: number
  partial: boolean
  champions: readonly GlobalChampionBaseline[]
}

export interface MatchDetailResponse {
  gameId: number
  queueId: number
  gameMode: string
  gameCreation: number
  gameDuration: number
  teams: MatchDetailTeam[]
}

export interface MatchDetailTeam {
  teamId: number
  name: string
  win: boolean
  towerKills: number
  players: MatchDetailPlayer[]
}

export interface MatchDetailPlayer extends RecentGame {
  participantId: number
  puuid: string
  gameName: string
  tagLine: string
  summonerName: string
  teamId: number
  playerSubteamId: number
  premade?: LivePremadeMarker
  summonerLevel?: number
  rankedStats?: RankedStatsResponse
}

export interface OpenMatchPayload {
  game: RecentGame
  ownerLabel: string
  ownerPuuid?: string
  sgpServerId?: string
}

export interface PlayerStatsResponse {
  summoner: SummonerInfo
  depthRequested: number
  depthLoaded: number
  summary: PlayerSummary
  championStats: ChampionStat[]
  recentGames: RecentGame[]
}

export interface PlayerEloEstimateParticipant {
  puuid: string
  summonerName: string
  gameName: string
  tagLine: string
  championId: number
  elo: number
}

export interface PlayerEloEstimateGameSample {
  gameId: number
  gameCreation: number
  maxElo: number
  soloPlayers: PlayerEloEstimateParticipant[]
}

export interface PlayerEloEstimateResponse {
  sampleTarget: number
  sampleGames: number
  trimmedGames: number
  averageElo: number
  medianElo: number
  finalElo: number
  rawGameMaxElos: number[]
  trimmedGameMaxElos: number[]
  gameSamples: PlayerEloEstimateGameSample[]
}

export interface PlayerEloEstimateTaskStatus {
  jobId: string
  status: string
  cached: boolean
  result: PlayerEloEstimateResponse | null
  error: string | null
}

export interface LivePlayer {
  puuid: string
  championId: number
  championPickIntent: number
  position: string
  selectedRole: string
  summonerId: number
  teamParticipantId?: number
  isPlaceholder: boolean
  premade?: LivePremadeMarker
  summoner?: SummonerInfo
  stats?: PlayerStatsResponse
  error?: string
}

export interface LivePremadeMarker {
  groupId: string
  label: string
  source: string
  togetherTimes: number
  memberPuuids: string[]
}

export interface LiveTeam {
  name: string
  players: LivePlayer[]
}

export interface LiveGameResponse {
  phase: string
  queryStage: string
  gameId?: number
  queueId?: number
  queueType?: string
  gameMode?: string
  teams: LiveTeam[]
}

export interface ChampionSummaryItem {
  id: number
  name: string
  alias: string
  title: string
  squarePortraitPath: string
  roles: string[]
}

export interface GameAssetBundle {
  summonerSpells: GameAssetEntry[]
  items: GameAssetEntry[]
  perks: GameAssetEntry[]
  augments: GameAssetEntry[]
}

export interface GameAssetEntry {
  id: number
  name: string
  description: string
  iconPath: string
  rarity: string
  categories: string[]
  price: number
  priceTotal: number
  inStore: boolean
  displayInItemSets: boolean
}

export interface StatsLoadProgress {
  requestId: string
  loaded: number
  total: number
}

export interface ShareSettings {
  championAnalysisLimit: number
  championGamesAnalysisLimit: number
  mobileShareLayout: boolean
}
