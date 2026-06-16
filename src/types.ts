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

export interface RankedStatsResponse {
  queueMap: RankedQueueMap
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

export interface RecentGame {
  gameId: number
  championId: number
  queueId: number
  gameMode: string
  win: boolean
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
  kda: number
  cs: number
  goldEarned: number
  damageToChampions: number
  teamDamageToChampions: number
  damageSelfMitigated: number
  teamDamageSelfMitigated: number
  totalHeal: number
  teamTotalHeal: number
  teamGoldEarned: number
  gameCreation: number
  gameDuration: number
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

export interface LivePlayer {
  puuid: string
  championId: number
  championPickIntent: number
  position: string
  selectedRole: string
  summonerId: number
  teamParticipantId?: number
  isPlaceholder: boolean
  summoner?: SummonerInfo
  stats?: PlayerStatsResponse
  error?: string
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
