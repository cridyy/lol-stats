import {
  buildChampionProfiles,
  buildPlayerProfile,
  type ChampionProfile,
  type PlayerProfile,
} from "./playerProfile"
import { refreshGameTitleSelection } from "./gamePresentation"
import {
  calculateGlobalRating,
  calculateOfficialRating,
  globalRatingTitle,
  scoreEvaluationLabel,
} from "./scoring"
import { buildGameTitleSet } from "./titles"
import type { RatingContext } from "./roleClassifier"
import type {
  ChampionSummaryItem,
  GlobalBaselineDataset,
  GlobalChampionBaseline,
  PlayerStatsResponse,
  RecentGame,
} from "./types"
import { championName, mitigationValue, riotId } from "./utils"

export type LocalApiMode = "hex-aram" | "aram" | "ranked" | "normal" | "arena" | "all"

export interface LocalApiRequestPayload {
  requestId: string
  gameName: string
  tagLine: string
  server: string
  mode: LocalApiMode
  count: number
}

export interface LocalApiBuildOptions {
  champions: Record<number, ChampionSummaryItem>
  ratingContext: RatingContext
  globalBaselines: ReadonlyMap<number, GlobalChampionBaseline>
  globalBaselineDataset?: Readonly<GlobalBaselineDataset> | null
  globalBaselineDatasetVersion?: string
  scannedGames: number
  exhausted: boolean
}

export function localApiModeMatches(game: RecentGame, mode: LocalApiMode) {
  const gameMode = String(game.gameMode || "").toUpperCase()
  const isHexAram = game.queueId === 2400 || gameMode === "STRAWBERRY" || gameMode === "KIWI"

  switch (mode) {
    case "hex-aram":
      return isHexAram
    case "aram":
      return !isHexAram && (game.queueId === 450 || gameMode === "ARAM")
    case "ranked":
      return game.queueId === 420 || game.queueId === 440
    case "normal":
      return (
        [400, 430, 480, 490].includes(game.queueId) ||
        (gameMode === "CLASSIC" && game.queueId !== 420 && game.queueId !== 440)
      )
    case "arena":
      return gameMode === "CHERRY"
    case "all":
      return true
  }
}

export function buildLocalApiResponse(
  request: LocalApiRequestPayload,
  stats: PlayerStatsResponse,
  games: RecentGame[],
  options: LocalApiBuildOptions,
) {
  // 每次接口刷新重新抽取展示称号；同一响应内仍保持稳定，便于列表和悬浮窗对应。
  refreshGameTitleSelection()
  const profile = buildPlayerProfile(games, options.ratingContext, {
    globalBaselines: options.globalBaselines,
  })
  const championProfiles = buildChampionProfiles(games, options.ratingContext, {
    globalBaselines: options.globalBaselines,
  })

  return {
    apiVersion: 1,
    generatedAt: new Date().toISOString(),
    request: {
      gameName: request.gameName,
      tagLine: request.tagLine,
      server: request.server,
      mode: request.mode,
      requestedCount: request.count,
    },
    coverage: {
      returnedGames: games.length,
      scannedGames: options.scannedGames,
      exhausted: options.exhausted,
      complete: games.length >= request.count,
    },
    player: {
      riotId: riotId(stats.summoner),
      gameName: stats.summoner.gameName,
      tagLine: stats.summoner.tagLine,
      displayName: stats.summoner.displayName,
      profileIconId: stats.summoner.profileIconId,
      summonerLevel: stats.summoner.summonerLevel,
    },
    globalBaseline: buildGlobalBaselineView(options),
    summary: buildSummary(games, profile),
    profile,
    championProfiles: buildChampionViews(games, championProfiles, options),
    games: games.map((game) => buildGameView(game, options)),
  }
}

function buildGlobalBaselineView(options: LocalApiBuildOptions) {
  const dataset = options.globalBaselineDataset
  if (!dataset) {
    return {
      available: false,
      datasetVersion: options.globalBaselineDatasetVersion || "",
      championCount: options.globalBaselines.size,
      champions: Array.from(options.globalBaselines.values()),
    }
  }

  return {
    available: true,
    schemaVersion: dataset.schemaVersion,
    datasetVersion: dataset.datasetVersion,
    generatedAt: dataset.generatedAt,
    source: dataset.source,
    metricDescriptions: dataset.metricDescriptions || {},
    globalAverages: dataset.globalAverages || {},
    globalAverageEstimation: dataset.globalAverageEstimation || null,
    champions: dataset.champions,
    expectedChampionCount: dataset.expectedChampionCount,
    championCount: dataset.championCount,
    unavailableChampionCount: dataset.unavailableChampionCount,
    partial: dataset.partial,
  }
}

function buildGameView(game: RecentGame, options: LocalApiBuildOptions) {
  const rating = calculateOfficialRating(
    game,
    options.ratingContext,
    options.globalBaselines,
  )
  const globalRating = calculateGlobalRating(
    game,
    options.ratingContext,
    options.globalBaselines,
  )
  const { baseRating: _baseRating, ...globalBenchmark } = globalRating
  const metrics = rating.metrics
  const mitigation = mitigationValue(game)
  const match = buildMatchComparison(game, options)
  const titleSet = buildGameTitleSet(game, rating, {
    ratingContext: options.ratingContext,
    globalBaselines: options.globalBaselines,
  }, match.comparison)
  const championBaseline = options.globalBaselines.get(game.championId)

  return {
    match: {
      gameId: game.gameId,
      playedAt: game.gameCreation,
      durationSeconds: game.gameDuration,
      queueId: game.queueId,
      mode: game.gameMode,
      win: game.win,
      participants: match.participants,
      comparison: match.comparison,
    },
    champion: championView(game.championId, options.champions),
    performance: {
      combat: {
        kills: game.kills,
        deaths: game.deaths,
        assists: game.assists,
        kda: game.kda,
        killShare: metrics.killShare,
        killParticipation: metrics.killParticipation,
        deathShare: metrics.deathShare,
      },
      damage: {
        value: game.damageToChampions,
        share: metrics.damageShare,
        conversion: metrics.damageConversion,
        effectiveConversion: metrics.effectiveDamageConversion,
        teamLeader: game.teamDamageLeader,
        gameLeader: game.gameDamageLeader,
      },
      gold: {
        value: game.goldEarned,
        share: metrics.goldShare,
        teamLeader: game.teamGoldLeader,
      },
      mitigation: {
        value: mitigation,
        selfMitigated: game.damageSelfMitigated,
        received: game.totalDamageTaken,
        share: metrics.mitigationShare,
        perDeath: metrics.mitigationPerDeath,
        teamLeader: game.teamMitigationLeader,
      },
      healing: {
        value: game.totalHeal,
        teammates: game.totalHealsOnTeammates || 0,
        share: metrics.healingShare,
        teamLeader: game.teamHealingLeader,
      },
      shielding: {
        teammates: game.totalDamageShieldedOnTeammates,
      },
      control: {
        immobilizations: game.enemyChampionImmobilizations,
        perMinute: metrics.immobilizationsPerMinute,
        share: metrics.immobilizationShare,
        assistedKills: game.immobilizeAndKillWithAlly,
        assistedKillShare: metrics.immobilizeKillShare,
        conversion: metrics.immobilizeKillConversion,
        durationSeconds: game.timeCCingOthers || 0,
      },
    },
      score: {
      value: rating.score,
      evaluation: scoreEvaluationLabel(rating.score),
        tag: rating.label,
        title: titleSet.displayTitle,
        titles: titleSet.titles,
        titleDetails: titleSet.details,
        factTags: titleSet.factTags,
        level: rating.level,
      role: rating.role,
      parts: rating.parts,
      metrics: rating.metrics,
      scoringMetrics: rating.scoringMetrics,
      matchContext: rating.contextAdjustment,
        globalBenchmark,
        championBaseline,
      details: globalRatingTitle(
        game,
        options.ratingContext,
        options.globalBaselines,
        options.globalBaselineDatasetVersion,
      ).split("\n"),
    },
  }
}

/**
 * 计算同局队伍对比。评分接口本身只需要当前玩家，但称号判断还需要知道
 * 队友平均分、队内名次和对手强弱，因此这里复用同一套前端评分函数处理
 * 后端附带的参与者快照。
 */
function buildMatchComparison(game: RecentGame, options: LocalApiBuildOptions) {
  const records = (game.matchParticipants || []).filter((entry) => entry)
  const ownerKey = participantTeamKey(game)
  const participants = records.map((entry) => {
    const record = entry
    const rating = calculateOfficialRating(record, options.ratingContext, options.globalBaselines)
    const globalRating = calculateGlobalRating(record, options.ratingContext, options.globalBaselines)
    const metrics = rating.metrics
    return {
      participantId: entry.participantId,
      puuid: entry.puuid,
      gameName: entry.gameName,
      tagLine: entry.tagLine,
      summonerName: entry.summonerName,
      owner: Boolean(entry.isOwner) || sameParticipant(record, game),
      teamId: record.teamId,
      playerSubteamId: record.playerSubteamId || 0,
      teamKey: participantTeamKey(record),
      win: record.win,
      champion: championView(record.championId, options.champions),
      score: rating.score,
      baseScore: globalRating.baseRating.score,
      globalScore: globalRating.available ? globalRating.score : undefined,
      evaluation: scoreEvaluationLabel(rating.score),
      tag: rating.label,
      role: rating.role,
      metrics: {
        kda: record.kda,
        kills: record.kills,
        deaths: record.deaths,
        assists: record.assists,
        killParticipation: metrics.killParticipation,
        damageShare: metrics.damageShare,
        damageConversion: metrics.damageConversion,
        effectiveDamageConversion: metrics.effectiveDamageConversion,
        mitigationShare: metrics.mitigationShare,
        mitigationPerDeath: metrics.mitigationPerDeath,
        deathShare: metrics.deathShare,
        healingShare: metrics.healingShare,
        controlPerMinute: metrics.immobilizationsPerMinute,
        controlShare: metrics.immobilizationShare,
      },
    }
  })

  const owner = participants.find((entry) => entry.owner)
    || participants.find((entry) => entry.teamKey === ownerKey && sameParticipant(entry, game))
  const ownerTeamKey = owner?.teamKey || ownerKey
  const ownTeam = participants.filter((entry) => entry.teamKey === ownerTeamKey)
  const teammates = ownTeam.filter((entry) => !entry.owner)
  const opponents = participants.filter((entry) => entry.teamKey !== ownerTeamKey)
  const ownScore = owner?.score ?? calculateOfficialRating(
    game,
    options.ratingContext,
    options.globalBaselines,
  ).score

  const rankOf = (value: number, values: number[]) =>
    1 + values.filter((candidate) => candidate > value + 0.0001).length
  const mean = (entries: Array<{ score: number }>) =>
    entries.length ? round2(entries.reduce((sum, entry) => sum + entry.score, 0) / entries.length) : null
  const min = (entries: Array<{ score: number }>) =>
    entries.length ? Math.min(...entries.map((entry) => entry.score)) : null
  const max = (entries: Array<{ score: number }>) =>
    entries.length ? Math.max(...entries.map((entry) => entry.score)) : null
  const allScores = participants.map((entry) => entry.score)
  const teamScores = ownTeam.map((entry) => entry.score)
  const teammatesAverageScore = mean(teammates)
  const opponentAverageScore = mean(opponents)

  return {
    participants,
    comparison: {
      participantCount: participants.length,
      teamKey: ownerTeamKey,
      teamCount: ownTeam.length,
      teammateCount: teammates.length,
      opponentCount: opponents.length,
      ownScore,
      teamAverageScore: mean(ownTeam),
      teammatesAverageScore,
      opponentAverageScore,
      teamScoreRank: rankOf(ownScore, teamScores),
      gameScoreRank: rankOf(ownScore, allScores),
      teamScoreLeader: ownTeam.length > 0 && ownScore >= Math.max(...teamScores),
      gameScoreLeader: participants.length > 0 && ownScore >= Math.max(...allScores),
      teamScoreGapToTeammates: teammatesAverageScore === null ? null : round2(ownScore - teammatesAverageScore),
      scoreGapToOpponents: opponentAverageScore === null ? null : round2(ownScore - opponentAverageScore),
      teammateLowestScore: min(teammates),
      teammateHighestScore: max(teammates),
      opponentLowestScore: min(opponents),
      opponentHighestScore: max(opponents),
      teamLowestScore: min(ownTeam),
      teamHighestScore: max(ownTeam),
    },
  }
}

function participantTeamKey(game: Pick<RecentGame, "teamId" | "playerSubteamId">) {
  const subteam = Number(game.playerSubteamId || 0)
  return subteam > 0 ? `subteam:${subteam}` : `team:${Number(game.teamId || 0)}`
}

function sameParticipant(left: RecentGame | { puuid?: string; participantId?: number }, right: RecentGame | { puuid?: string; participantId?: number }) {
  const leftPuuid = "puuid" in left ? String(left.puuid || "").trim().toLowerCase() : ""
  const rightPuuid = "puuid" in right ? String(right.puuid || "").trim().toLowerCase() : ""
  if (leftPuuid && rightPuuid) return leftPuuid === rightPuuid
  const leftId = "participantId" in left ? Number(left.participantId || 0) : 0
  const rightId = "participantId" in right ? Number(right.participantId || 0) : 0
  return leftId > 0 && rightId > 0 && leftId === rightId
}

function buildSummary(games: RecentGame[], profile: PlayerProfile) {
  const wins = games.filter((game) => game.win).length
  const kills = sum(games, (game) => game.kills)
  const deaths = sum(games, (game) => game.deaths)
  const assists = sum(games, (game) => game.assists)

  return {
    games: games.length,
    wins,
    losses: games.length - wins,
    winRate: ratio(wins, games.length),
    kda: calculateKda(kills, deaths, assists),
    score: profile.overallScore,
    evaluation: scoreEvaluationLabel(profile.overallScore),
    mainRole: profile.mainRoleLabel,
    tags: profile.tags,
  }
}

function buildChampionViews(
  games: RecentGame[],
  profiles: ChampionProfile[],
  options: LocalApiBuildOptions,
) {
  const gamesByChampion = new Map<number, RecentGame[]>()
  for (const game of games) {
    const entries = gamesByChampion.get(game.championId) || []
    entries.push(game)
    gamesByChampion.set(game.championId, entries)
  }

  return profiles.map((profile) => {
    const championGames = gamesByChampion.get(profile.championId) || []
    const wins = championGames.filter((game) => game.win).length
    const ratings = championGames.map((game) =>
      calculateOfficialRating(game, options.ratingContext, options.globalBaselines),
    )

    return {
      champion: championView(profile.championId, options.champions),
      games: championGames.length,
      wins,
      losses: championGames.length - wins,
      winRate: ratio(wins, championGames.length),
      kda: calculateKda(
        sum(championGames, (game) => game.kills),
        sum(championGames, (game) => game.deaths),
        sum(championGames, (game) => game.assists),
      ),
      profile,
      averages: {
        damageShare: average(ratings, (rating) => rating.metrics.damageShare),
        damageConversion: average(ratings, (rating) => rating.metrics.damageConversion),
        effectiveDamageConversion: average(
          ratings,
          (rating) => rating.metrics.effectiveDamageConversion,
        ),
        mitigationShare: average(ratings, (rating) => rating.metrics.mitigationShare),
        healingShare: average(ratings, (rating) => rating.metrics.healingShare),
        killParticipation: average(ratings, (rating) => rating.metrics.killParticipation),
      },
    }
  })
}

function championView(championId: number, champions: Record<number, ChampionSummaryItem>) {
  const champion = champions[championId]
  return {
    id: championId,
    name: championName(champions, championId),
    alias: champion?.alias || "",
    title: champion?.title || "",
    portraitPath: champion?.squarePortraitPath || "",
  }
}

function sum<T>(entries: T[], selector: (entry: T) => number) {
  return entries.reduce((total, entry) => total + Number(selector(entry) || 0), 0)
}

function average<T>(entries: T[], selector: (entry: T) => number) {
  return entries.length === 0 ? 0 : round2(sum(entries, selector) / entries.length)
}

function ratio(value: number, total: number) {
  return total > 0 ? round2(value / total) : 0
}

function calculateKda(kills: number, deaths: number, assists: number) {
  return round2((kills + assists) / Math.max(deaths, 1))
}

function round2(value: number) {
  return Math.round(value * 100) / 100
}
