import {
  calculateOfficialRating,
  scoreEvaluationLabel,
  type OutputRating,
} from "./scoring"
import type { PlayerRole, RatingContext } from "./roleClassifier"
import type { GlobalChampionBaseline, RecentGame } from "./types"

export type AbilityKey = "carry" | "frontline" | "support"
export type ChampionLabelBand = "excellent" | "normal" | "low" | "disaster" | "empty"

export interface AbilityProfile {
  key: AbilityKey
  label: string
  games: number
  averageScore: number
  medianScore: number
  highlightRate: number
  disasterRate: number
  volatility: number
  averageDamageShare: number
  averageDamageConversion: number
  averageMitigationShare: number
  averageHealingShare: number
}

export interface RoleDistributionItem {
  label: string
  games: number
  wins: number
  rate: number
  winRate: number
}

export interface PlayerProfile {
  games: number
  overallScore: number
  medianScore: number
  highlightRate: number
  disasterRate: number
  volatility: number
  mainRoleLabel: string
  roleDistribution: RoleDistributionItem[]
  abilities: Record<AbilityKey, AbilityProfile>
  tags: string[]
}

export interface ChampionProfile {
  championId: number
  games: number
  averageScore: number
  mainRoleLabel: string
  label: string
  labelBand: ChampionLabelBand
  highlightRate: number
  disasterRate: number
  averageDamageShare: number
  averageDamageConversion: number
  averageMitigationShare: number
  averageHealingShare: number
}

/**
 * 大样本数据统计会将长期表现和近期状态混合；实时战绩显式关闭此项，
 * 保持原本的短样本均匀平均，避免 20-50 场样本被再次放大波动。
 */
export interface ProfileAggregationOptions {
  timeWeighted?: boolean
  now?: number
  globalBaselines?: ReadonlyMap<number, GlobalChampionBaseline>
}

interface RatedGame {
  game: RecentGame
  rating: OutputRating
  family: AbilityKey
}

interface ProfileAggregation {
  /** 混合后的最终权重，用于综合分和画像标签。 */
  weights: Map<RatedGame, number>
  /** 纯近期衰减权重，用于单英雄评分中的近期项。 */
  recencyWeights: Map<RatedGame, number>
  /** 实际启用的近期混合比例，休眠玩家会降为 0。 */
  recencyMix: number
}

const EMPTY_ABILITY: Record<AbilityKey, string> = {
  carry: "输出能力",
  frontline: "前排能力",
  support: "辅助能力",
}
const DISASTER_BAND_DOMINANT_RATE = 0.3
const DISASTER_RATE_SCORE_LIMIT = 60
const DEFAULT_PROFILE_RATING_CONTEXT: RatingContext = {}

export function buildPlayerProfile(
  games: RecentGame[] = [],
  context: RatingContext = DEFAULT_PROFILE_RATING_CONTEXT,
  options: ProfileAggregationOptions = {},
): PlayerProfile {
  // 画像只聚合调用方已经加载好的对局，不在前端额外触发战绩请求。
  const ratedGames = rateGames(games, context, options.globalBaselines)
  const aggregation = createProfileAggregation(ratedGames, options)
  const abilities = {
    carry: buildAbilityProfile("carry", ratedGames, options),
    frontline: buildAbilityProfile("frontline", ratedGames, options),
    support: buildAbilityProfile("support", ratedGames, options),
  }
  const roleDistribution = buildRoleDistribution(ratedGames)
  const overallScore = weightedEntryAverage(
    ratedGames,
    (entry) => entry.rating.score,
    aggregation,
  )

  return {
    games: ratedGames.length,
    overallScore,
    medianScore: median(ratedGames.map((entry) => entry.rating.score)),
    highlightRate: weightedEntryRate(ratedGames, (entry) => entry.rating.score >= 80, aggregation),
    disasterRate: weightedEntryRate(
      ratedGames,
      (entry) => isDisasterRateScore(entry.rating.score),
      aggregation,
    ),
    volatility: weightedEntryStandardDeviation(
      ratedGames,
      (entry) => entry.rating.score,
      aggregation,
    ),
    mainRoleLabel: resolveMainRoleLabel(roleDistribution),
    roleDistribution,
    abilities,
    tags: buildPlayerTags(ratedGames, roleDistribution, overallScore, aggregation),
  }
}

export function buildChampionProfiles(
  games: RecentGame[] = [],
  context: RatingContext = DEFAULT_PROFILE_RATING_CONTEXT,
  options: ProfileAggregationOptions = {},
): ChampionProfile[] {
  const byChampion = new Map<number, RatedGame[]>()
  for (const entry of rateGames(games, context, options.globalBaselines)) {
    const list = byChampion.get(entry.game.championId) || []
    list.push(entry)
    byChampion.set(entry.game.championId, list)
  }

  return Array.from(byChampion.entries())
    .map(([championId, entries]) => {
      const roleDistribution = buildRoleDistribution(entries)
      const aggregation = createProfileAggregation(entries, options)
      const summary = summarizeRatedGames(entries, aggregation)
      const averageScore = championProfileScore(entries, aggregation)

      return {
        championId,
        games: entries.length,
        averageScore,
        mainRoleLabel: resolveMainRoleLabel(roleDistribution),
        label: aggregateProfileLabel(entries, aggregation),
        labelBand: dominantScoreBandKey(entries, aggregation),
        highlightRate: summary.highlightRate,
        disasterRate: summary.disasterRate,
        averageDamageShare: summary.damageShare,
        averageDamageConversion: summary.damageConversion,
        averageMitigationShare: summary.mitigationShare,
        averageHealingShare: summary.healingShare,
      }
    })
    .sort((a, b) => b.games - a.games || b.averageScore - a.averageScore)
}

export function profileScoreLevel(score: number) {
  if (score >= 80) return "excellent"
  if (score >= 65) return "good"
  if (score >= 40) return "average"
  return "poor"
}

export function profileTierLabel(score: number) {
  return scoreEvaluationLabel(score)
}

export function profileTierClass(score: number) {
  if (score >= 80) return "profile-tier-apex"
  if (score >= 65) return "profile-tier-steady"
  if (score >= 40) return "profile-tier-normal"
  return "profile-tier-big-pit"
}

function rateGames(
  games: RecentGame[],
  context: RatingContext,
  globalBaselines?: ReadonlyMap<number, GlobalChampionBaseline>,
) {
  return games.map((game) => {
    const rating = calculateOfficialRating(game, context, globalBaselines)
    return {
      game,
      rating,
      family: abilityFamily(rating.role.role),
    }
  })
}

function abilityFamily(role: PlayerRole): AbilityKey {
  // 先把具体定位收敛成三条能力线，后续做坦克/辅助独立公式时可以从这里扩展。
  switch (role) {
    case "tank":
    case "bruiser":
      return "frontline"
    case "support":
    case "utilityCarry":
      return "support"
    default:
      return "carry"
  }
}

function buildAbilityProfile(
  key: AbilityKey,
  entries: RatedGame[],
  options: ProfileAggregationOptions,
): AbilityProfile {
  const selected = entries.filter((entry) => entry.family === key)
  const aggregation = createProfileAggregation(selected, options)
  const summary = summarizeRatedGames(selected, aggregation)

  return {
    key,
    label: EMPTY_ABILITY[key],
    games: selected.length,
    averageScore: summary.averageScore,
    medianScore: summary.medianScore,
    highlightRate: summary.highlightRate,
    disasterRate: summary.disasterRate,
    volatility: summary.volatility,
    averageDamageShare: summary.damageShare,
    averageDamageConversion: summary.damageConversion,
    averageMitigationShare: summary.mitigationShare,
    averageHealingShare: summary.healingShare,
  }
}

function buildRoleDistribution(entries: RatedGame[]) {
  const counts = new Map<string, { games: number; wins: number }>()
  for (const entry of entries) {
    const current = counts.get(entry.rating.role.label) || { games: 0, wins: 0 }
    current.games += 1
    current.wins += entry.game.win ? 1 : 0
    counts.set(entry.rating.role.label, current)
  }

  return Array.from(counts.entries())
    .map(([label, value]) => ({
      label,
      games: value.games,
      wins: value.wins,
      rate: rate(value.games, entries.length),
      winRate: rate(value.wins, value.games),
    }))
    .sort((a, b) => b.games - a.games || b.rate - a.rate)
}

function resolveMainRoleLabel(distribution: RoleDistributionItem[]) {
  const top = distribution[0]
  if (!top) return "样本不足"
  return top.label
}

function buildPlayerTags(
  entries: RatedGame[],
  roleDistribution: RoleDistributionItem[],
  overallScore: number,
  aggregation: ProfileAggregation,
) {
  if (!entries.length) return ["样本不足"]

  const summary = summarizeRatedGames(entries, aggregation)
  const positiveEligible = overallScore >= 65
  const tags: string[] = [aggregateProfileLabel(entries, aggregation)]

  if (positiveEligible) {
    tags.push(...positiveProfileTags(entries, summary, roleDistribution, overallScore, aggregation))
  }
  if (overallScore >= 40 && overallScore < 65) tags.push(...controlProfileTags(summary))
  tags.push(...negativeProfileTags(summary, overallScore))
  tags.push(...stabilityProfileTags(summary, overallScore))

  const topRole = roleDistribution[0]
  if (topRole) tags.push(`常用${topRole.label}`)

  return dedupe(tags).slice(0, 10)
}

function positiveProfileTags(
  entries: RatedGame[],
  summary: ReturnType<typeof summarizeRatedGames>,
  roleDistribution: RoleDistributionItem[],
  overallScore: number,
  aggregation: ProfileAggregation,
) {
  const tags: string[] = []
  const role = dominantPlayerRole(entries, aggregation)
  const mainRoleLabel = resolveMainRoleLabel(roleDistribution)
  const carryGamesRate = abilityRate(entries, "carry", aggregation)
  const frontlineGamesRate = abilityRate(entries, "frontline", aggregation)
  const supportGamesRate = abilityRate(entries, "support", aggregation)

  const primaryTag = positivePrimaryProfileTag(summary, role, overallScore)
  if (primaryTag) tags.push(primaryTag)

  if (
    summary.highlightRate >= 0.28 ||
    (summary.damageShare >= 0.28 && summary.effectiveDamageConversion >= 1.05)
  ) {
    tags.push("核心大C")
  }
  if (summary.deathShare >= 0.27 && summary.damageShare >= 0.25) tags.push("浴血奋战")
  if (summary.mitigationShare >= 0.29 && summary.damageShare >= 0.23) tags.push("半肉战神")
  else if (summary.mitigationShare >= 0.29 || frontlineGamesRate >= 0.35) tags.push("哪来的城墙")
  if (summary.healingShare >= 0.25 || supportGamesRate >= 0.35 || mainRoleLabel === "辅助") {
    tags.push("团队功能")
  }
  tags.push(...controlProfileTags(summary))
  if (carryGamesRate >= 0.65 && summary.damageShare >= 0.24) {
    tags.push(overallScore >= 80 ? "输出机器" : "稳定火力")
  }

  return dedupe(tags)
}

function positivePrimaryProfileTag(
  summary: ReturnType<typeof summarizeRatedGames>,
  role: PlayerRole,
  overallScore: number,
) {
  if (role === "tank") {
    if (
      summary.mitigationShare >= 0.3 &&
      summary.mitigationPerDeath >= 3 &&
      summary.killParticipation >= 0.68
    ) {
      return "叹息之墙"
    }
    if (
      summary.mitigationShare >= 0.3 &&
      summary.mitigationPerDeath >= 1.05 &&
      summary.killParticipation >= 0.68
    ) {
      return "faker加里奥"
    }
    if (summary.mitigationShare >= 0.27 && summary.damageShare >= 0.22) return "半肉战神"
    if (summary.mitigationShare >= 0.27) return "哪来的城墙"
    return controlProfileTags(summary)[0] || "顶级前锋"
  }

  if (role === "fighter" || role === "bruiser" || role === "fighterAssassin") {
    if (summary.damageShare >= 0.35 && summary.effectiveDamageConversion >= 1.5) return "大魔王"
    if (
      summary.damageShare >= 0.32 &&
      summary.effectiveDamageConversion > 1.3 &&
      summary.killParticipation >= 0.68
    ) {
      return "恐怖利刃"
    }
    if (
      summary.kda >= 4 &&
      summary.damageShare >= 0.26 &&
      summary.effectiveDamageConversion >= 1.12 &&
      summary.deathShare <= 0.18
    ) {
      return "我chovy!"
    }
    if (
      role === "fighterAssassin" &&
      summary.killShare >= 0.28 &&
      summary.effectiveDamageConversion >= 1 &&
      summary.effectiveDamageConversion <= 1.2
    ) {
      return "无情收割者"
    }
    if (
      summary.damageShare >= 0.25 &&
      summary.mitigationShare >= 0.25 &&
      summary.effectiveDamageConversion >= 0.95
    ) {
      return "半肉战神"
    }
    if (
      summary.damageShare >= 0.27 &&
      summary.effectiveDamageConversion >= 1.1 &&
      summary.deathShare >= 0.24
    ) {
      return "刀尖舔血"
    }
    if (summary.damageShare >= 0.25 && summary.killParticipation >= 0.68) {
      return overallScore >= 80 ? "最强前锋" : "冲阵好手"
    }
    return controlProfileTags(summary)[0] || (overallScore >= 80 ? "最强前锋" : "冲阵好手")
  }

  if (role === "support" || role === "utilityCarry") {
    if (summary.goldShare <= 0.18 && summary.killParticipation >= 0.7) return "吃草挤奶"
    if (summary.damageShare >= 0.24 && summary.effectiveDamageConversion >= 1.05) return "核心大C"
    return controlProfileTags(summary)[0] || (overallScore >= 80 ? "团队发动机" : "功能担当")
  }

  if (summary.damageShare >= 0.35 && summary.effectiveDamageConversion >= 1.5) return "大魔王"
  if (
    summary.damageShare >= 0.32 &&
    summary.effectiveDamageConversion > 1.3 &&
    summary.killParticipation >= 0.68
  ) {
    return "爆炸核弹"
  }
  if (
    summary.kda >= 4 &&
    summary.damageShare >= 0.26 &&
    summary.effectiveDamageConversion >= 1.12 &&
    summary.deathShare <= 0.18
  ) {
    return "我chovy!"
  }
  if (
    role === "pureAssassin" &&
    summary.killShare >= 0.28 &&
    summary.effectiveDamageConversion >= 1 &&
    summary.effectiveDamageConversion <= 1.2
  ) {
    return "无情收割者"
  }
  if (
    summary.goldShare <= 0.19 &&
    summary.damageShare >= 0.25 &&
    summary.effectiveDamageConversion >= 1.15
  ) {
    return "吃草挤奶"
  }
  if (summary.damageShare >= 0.28 && summary.effectiveDamageConversion >= 1.12 && summary.kda >= 3) {
    return "无解主c"
  }
  if (summary.damageShare >= 0.28 && summary.effectiveDamageConversion >= 1.05) return "核心大C"
  if (summary.damageShare >= 0.29 && summary.killParticipation >= 0.68) return "全场火力点"
  if (
    summary.damageShare >= 0.27 &&
    summary.effectiveDamageConversion >= 1.08 &&
    summary.deathShare >= 0.24
  ) {
    return "浴血奋战"
  }

  return controlProfileTags(summary)[0] || (overallScore >= 80 ? "输出机器" : "稳定火力")
}

function negativeProfileTags(summary: ReturnType<typeof summarizeRatedGames>, overallScore: number) {
  const tags: string[] = []

  if (summary.damageShare < 0.2 && summary.effectiveDamageConversion < 0.95) tags.push("毫无存在感")
  if (summary.killStealGap >= 0.08 && summary.damageShare < 0.24) tags.push("k头狗")
  if (summary.killParticipation < 0.52 && summary.damageShare < 0.23) tags.push("开游戏的")
  if (summary.goldShare >= 0.24 && summary.effectiveDamageConversion < 1) tags.push("拿钱不干事")
  if (summary.disasterRate >= 0.25 || (summary.disasterRate >= 0.18 && overallScore < 65)) {
    tags.push("开游戏的")
  } else if (summary.disasterRate > 0 && overallScore >= 70) {
    tags.push("偶有拉胯")
  }

  return dedupe(tags)
}

function stabilityProfileTags(summary: ReturnType<typeof summarizeRatedGames>, overallScore: number) {
  const tags: string[] = []

  if (overallScore >= 65 && summary.volatility <= 9 && summary.games >= 8) tags.push("发挥稳定")
  if (summary.volatility >= 18 && summary.games >= 8) tags.push("上下限很大")

  return tags
}

function controlProfileTags(summary: ReturnType<typeof summarizeRatedGames>) {
  return profileControlTags(
    summary.immobilizationsPerMinute,
    summary.immobilizationShare,
    summary.immobilizeKillConversion,
    summary.controlQuality,
  )
}

function profileControlTags(
  immobilizationsPerMinute: number,
  immobilizationShare: number,
  immobilizeKillConversion: number,
  controlQuality: number,
) {
  if (immobilizationsPerMinute <= 2) return []

  const tags: string[] = []

  const highConversion = immobilizeKillConversion > 0.3
  const highShare = immobilizationShare > 0.3
  const highFrequency = immobilizationsPerMinute > 3

  if (highConversion && highShare && highFrequency) tags.push("控制之神")
  else if ((highConversion && highShare) || (highConversion && highFrequency)) {
    tags.push("控杀狙神")
  } else if (highShare && highFrequency) tags.push("控制天花板")
  else if (highConversion) tags.push("精准控杀")
  else if (highShare) tags.push("控制大师")
  else if (highFrequency) tags.push("控制永动机")

  if (!tags.length && controlQuality >= 0.5) tags.push("控制稳定")

  return tags
}

function createProfileAggregation(
  entries: RatedGame[],
  options: ProfileAggregationOptions,
): ProfileAggregation {
  const weights = new Map<RatedGame, number>()
  const recencyWeights = new Map<RatedGame, number>()
  if (!entries.length) return { weights, recencyWeights, recencyMix: 0 }

  const uniformWeight = 1 / entries.length
  const timeWeighted = options.timeWeighted !== false
  if (!timeWeighted) {
    for (const entry of entries) {
      weights.set(entry, uniformWeight)
      recencyWeights.set(entry, uniformWeight)
    }
    return { weights, recencyWeights, recencyMix: 0 }
  }

  const sorted = [...entries].sort((left, right) => {
    const timeDelta = normalizedGameTime(right.game.gameCreation) - normalizedGameTime(left.game.gameCreation)
    return timeDelta || right.game.gameId - left.game.gameId
  })
  const rawWeights = sorted.map((_, index) => 2 ** (-index / 20))
  const rawWeightTotal = rawWeights.reduce((sum, value) => sum + value, 0)

  sorted.forEach((entry, index) => {
    recencyWeights.set(entry, rawWeights[index] / rawWeightTotal)
  })

  const freshness = profileFreshness(sorted[0]?.game.gameCreation, options.now)
  const countBlend = clamp(0.3 + 0.1 * Math.log2(entries.length / 50), 0.3, 0.55)
  const recencyMix = countBlend * freshness

  for (const entry of entries) {
    const recentWeight = recencyWeights.get(entry) || uniformWeight
    weights.set(entry, (1 - recencyMix) * uniformWeight + recencyMix * recentWeight)
  }

  return { weights, recencyWeights, recencyMix }
}

function normalizedGameTime(value: number) {
  if (!Number.isFinite(value) || value <= 0) return 0
  return value < 1_000_000_000_000 ? value * 1000 : value
}

function profileFreshness(latestGameTime: number | undefined, now = Date.now()) {
  const timestamp = normalizedGameTime(Number(latestGameTime || 0))
  if (!timestamp) return 1

  const daysSinceLastGame = Math.max(0, (now - timestamp) / 86_400_000)
  if (daysSinceLastGame <= 7) return 1
  if (daysSinceLastGame >= 30) return 0

  const progress = (daysSinceLastGame - 7) / 23
  const smoothProgress = progress * progress * (3 - 2 * progress)
  return 1 - smoothProgress
}

function summarizeRatedGames(entries: RatedGame[], aggregation: ProfileAggregation) {
  return {
    games: entries.length,
    averageScore: weightedEntryAverage(entries, (entry) => entry.rating.score, aggregation),
    medianScore: median(entries.map((entry) => entry.rating.score)),
    highlightRate: weightedEntryRate(entries, (entry) => entry.rating.score >= 80, aggregation),
    goodRate: weightedEntryRate(entries, (entry) => entry.rating.score >= 70, aggregation),
    poorRate: weightedEntryRate(entries, (entry) => entry.rating.score < 55, aggregation),
    disasterRate: weightedEntryRate(
      entries,
      (entry) => isDisasterRateScore(entry.rating.score),
      aggregation,
    ),
    volatility: weightedEntryStandardDeviation(entries, (entry) => entry.rating.score, aggregation),
    kda: weightedEntryAverage(entries, (entry) => entry.rating.metrics.kda, aggregation),
    damageShare: weightedEntryAverage(entries, (entry) => entry.rating.metrics.damageShare, aggregation),
    damageConversion: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.damageConversion,
      aggregation,
    ),
    effectiveDamageConversion: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.effectiveDamageConversion,
      aggregation,
    ),
    goldShare: weightedEntryAverage(entries, (entry) => entry.rating.metrics.goldShare, aggregation),
    killShare: weightedEntryAverage(entries, (entry) => entry.rating.metrics.killShare, aggregation),
    killParticipation: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.killParticipation,
      aggregation,
    ),
    killStealGap: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.killShare - entry.rating.metrics.damageShare,
      aggregation,
    ),
    deathShare: weightedEntryAverage(entries, (entry) => entry.rating.metrics.deathShare, aggregation),
    mitigationShare: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.mitigationShare,
      aggregation,
    ),
    mitigationPerDeath: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.mitigationPerDeath,
      aggregation,
    ),
    healingShare: weightedEntryAverage(entries, (entry) => entry.rating.metrics.healingShare, aggregation),
    immobilizationsPerMinute: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.immobilizationsPerMinute,
      aggregation,
    ),
    immobilizationShare: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.immobilizationShare,
      aggregation,
    ),
    immobilizeKillConversion: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.immobilizeKillConversion,
      aggregation,
    ),
    controlQuality: weightedEntryAverage(
      entries,
      (entry) => entry.rating.metrics.controlQuality,
      aggregation,
    ),
  }
}

function weightedEntryAverage(
  entries: RatedGame[],
  selector: (entry: RatedGame) => number,
  aggregation: ProfileAggregation,
  source: "mixed" | "recency" = "mixed",
) {
  let total = 0
  let totalWeight = 0
  const weights = source === "recency" ? aggregation.recencyWeights : aggregation.weights

  for (const entry of entries) {
    const value = selector(entry)
    if (!Number.isFinite(value)) continue
    const weight = weights.get(entry) || 0
    total += value * weight
    totalWeight += weight
  }

  return totalWeight > 0 ? round2(total / totalWeight) : 0
}

function weightedEntryRate(
  entries: RatedGame[],
  predicate: (entry: RatedGame) => boolean,
  aggregation: ProfileAggregation,
) {
  let total = 0
  let matched = 0

  for (const entry of entries) {
    const weight = aggregation.weights.get(entry) || 0
    total += weight
    if (predicate(entry)) matched += weight
  }

  return total > 0 ? round2(matched / total) : 0
}

function weightedEntryStandardDeviation(
  entries: RatedGame[],
  selector: (entry: RatedGame) => number,
  aggregation: ProfileAggregation,
) {
  const mean = weightedEntryAverage(entries, selector, aggregation)
  let variance = 0
  let totalWeight = 0

  for (const entry of entries) {
    const value = selector(entry)
    if (!Number.isFinite(value)) continue
    const weight = aggregation.weights.get(entry) || 0
    variance += weight * (value - mean) ** 2
    totalWeight += weight
  }

  return totalWeight > 0 ? round2(Math.sqrt(variance / totalWeight)) : 0
}

function abilityRate(entries: RatedGame[], family: AbilityKey, aggregation: ProfileAggregation) {
  return weightedEntryRate(entries, (entry) => entry.family === family, aggregation)
}

function dominantPlayerRole(entries: RatedGame[], aggregation: ProfileAggregation): PlayerRole {
  const counts = new Map<PlayerRole, number>()
  for (const entry of entries) {
    const weight = aggregation.weights.get(entry) || 0
    counts.set(entry.rating.role.role, (counts.get(entry.rating.role.role) || 0) + weight)
  }

  return Array.from(counts.entries()).sort((a, b) => b[1] - a[1])[0]?.[0] || "unknown"
}

function aggregateProfileLabel(entries: RatedGame[], aggregation: ProfileAggregation) {
  if (!entries.length) return "样本不足"

  const band = dominantScoreBand(entries, aggregation)
  if (!band.length) return "样本不足"

  return dominantLabelInBand(band, aggregation)
}

function championProfileScore(entries: RatedGame[], aggregation: ProfileAggregation) {
  if (!entries.length) return 0

  const scores = entries.map((entry) => entry.rating.score)
  const band = dominantScoreBand(entries, aggregation)
  const averageScore = average(scores)
  const bandAverage = band.length ? average(band.map((entry) => entry.rating.score)) : averageScore
  const recentWeight = entries.length >= 10 && aggregation.recencyMix > 0 ? 0.2 : 0
  const recentScore = weightedEntryAverage(
    entries,
    (entry) => entry.rating.score,
    aggregation,
    "recency",
  )

  return round2(
    averageScore * (0.6 - recentWeight) +
      median(scores) * 0.25 +
      bandAverage * 0.15 +
      recentScore * recentWeight,
  )
}

function dominantScoreBand(entries: RatedGame[], aggregation: ProfileAggregation) {
  return dominantScoreBandGroup(entries, aggregation)?.entries || []
}

function dominantScoreBandKey(entries: RatedGame[], aggregation: ProfileAggregation): ChampionLabelBand {
  return dominantScoreBandGroup(entries, aggregation)?.key || "empty"
}

function dominantScoreBandGroup(entries: RatedGame[], aggregation: ProfileAggregation) {
  const bands: Array<{
    key: Exclude<ChampionLabelBand, "empty">
    entries: RatedGame[]
    weight: number
  }> = [
    { key: "excellent", entries: [], weight: 0 },
    { key: "normal", entries: [], weight: 0 },
    { key: "low", entries: [], weight: 0 },
    { key: "disaster", entries: [], weight: 0 },
  ]

  for (const entry of entries) {
    const score = entry.rating.score
    const target = score >= 80 ? bands[0] : score >= 60 ? bands[1] : score >= 40 ? bands[2] : bands[3]
    target.entries.push(entry)
    target.weight += aggregation.weights.get(entry) || 0
  }

  const totalWeight = bands.reduce((sum, band) => sum + band.weight, 0)
  const disasterRate = totalWeight > 0 ? bands[3].weight / totalWeight : 0
  const eligibleBands = bands.filter((band) => {
    if (!band.entries.length) return false
    return band.key !== "disaster" || disasterRate >= DISASTER_BAND_DOMINANT_RATE
  })

  return [...eligibleBands].sort((a, b) => b.weight - a.weight || b.entries.length - a.entries.length)[0]
}

function dominantLabelInBand(entries: RatedGame[], aggregation: ProfileAggregation) {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    const weight = aggregation.weights.get(entry) || 0
    counts.set(entry.rating.label, (counts.get(entry.rating.label) || 0) + weight)
  }

  return (
    Array.from(counts.entries()).sort((a, b) => b[1] - a[1])[0]?.[0] ||
    "样本不足"
  )
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

function isDisasterRateScore(score: number) {
  return score < DISASTER_RATE_SCORE_LIMIT
}

function average(values: number[]) {
  const safeValues = values.filter((value) => Number.isFinite(value))
  if (!safeValues.length) return 0
  return round2(safeValues.reduce((sum, value) => sum + value, 0) / safeValues.length)
}

function median(values: number[]) {
  const safeValues = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b)
  if (!safeValues.length) return 0
  const middle = Math.floor(safeValues.length / 2)
  if (safeValues.length % 2) return round2(safeValues[middle])
  return round2((safeValues[middle - 1] + safeValues[middle]) / 2)
}

function rate(part: number, total: number) {
  return total > 0 ? round2(part / total) : 0
}

function round2(value: number) {
  return Math.round(value * 100) / 100
}

function dedupe(values: string[]) {
  return Array.from(new Set(values.filter(Boolean)))
}
