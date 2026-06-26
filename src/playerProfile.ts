import { calculateOutputRating, outputRatingMetrics, type OutputRating } from "./scoring"
import type { PlayerRole, RatingContext } from "./roleClassifier"
import type { RecentGame } from "./types"

export type AbilityKey = "carry" | "frontline" | "support"

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
  highlightRate: number
  disasterRate: number
  averageDamageShare: number
  averageDamageConversion: number
  averageMitigationShare: number
  averageHealingShare: number
}

interface RatedGame {
  game: RecentGame
  rating: OutputRating
  family: AbilityKey
}

const EMPTY_ABILITY: Record<AbilityKey, string> = {
  carry: "输出能力",
  frontline: "前排能力",
  support: "辅助能力",
}
const DISASTER_BAND_DOMINANT_RATE = 0.3
const DISASTER_RATE_SCORE_LIMIT = 60

export function buildPlayerProfile(
  games: RecentGame[] = [],
  context: RatingContext = {},
): PlayerProfile {
  // 画像只聚合调用方已经加载好的对局，不在前端额外触发战绩请求。
  const ratedGames = rateGames(games, context)
  const abilities = {
    carry: buildAbilityProfile("carry", ratedGames),
    frontline: buildAbilityProfile("frontline", ratedGames),
    support: buildAbilityProfile("support", ratedGames),
  }
  const roleDistribution = buildRoleDistribution(ratedGames)
  const overallScore = average(ratedGames.map((entry) => entry.rating.score))

  return {
    games: ratedGames.length,
    overallScore,
    medianScore: median(ratedGames.map((entry) => entry.rating.score)),
    highlightRate: rate(
      ratedGames.filter((entry) => entry.rating.score >= 80).length,
      ratedGames.length,
    ),
    disasterRate: rate(
      ratedGames.filter((entry) => isDisasterRateScore(entry.rating.score)).length,
      ratedGames.length,
    ),
    volatility: standardDeviation(ratedGames.map((entry) => entry.rating.score)),
    mainRoleLabel: resolveMainRoleLabel(roleDistribution),
    roleDistribution,
    abilities,
    tags: buildPlayerTags(ratedGames, abilities, roleDistribution, overallScore),
  }
}

export function buildChampionProfiles(
  games: RecentGame[] = [],
  context: RatingContext = {},
): ChampionProfile[] {
  const byChampion = new Map<number, RatedGame[]>()
  for (const entry of rateGames(games, context)) {
    const list = byChampion.get(entry.game.championId) || []
    list.push(entry)
    byChampion.set(entry.game.championId, list)
  }

  return Array.from(byChampion.entries())
    .map(([championId, entries]) => {
      const roleDistribution = buildRoleDistribution(entries)
      const summary = summarizeRatedGames(entries)

      return {
        championId,
        games: entries.length,
        averageScore: weightedProfileScore(entries),
        mainRoleLabel: resolveMainRoleLabel(roleDistribution),
        label: aggregateProfileLabel(entries),
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
  if (score >= 82) return "excellent"
  if (score >= 65) return "good"
  if (score >= 41) return "average"
  return "poor"
}

export function profileTierLabel(score: number) {
  if (score >= 90) return "通天代"
  if (score >= 82) return "小代"
  if (score >= 76) return "实力强劲"
  if (score >= 65) return "正常玩家"
  if (score >= 55) return "小坑比"
  return "大坑比"
}

export function profileTierClass(score: number) {
  if (score >= 82) return "profile-tier-apex"
  if (score >= 65) return "profile-tier-steady"
  if (score >= 41) return "profile-tier-normal"
  return "profile-tier-big-pit"
}

function rateGames(games: RecentGame[], context: RatingContext) {
  return games.map((game) => {
    const rating = calculateOutputRating(game, context)
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

function buildAbilityProfile(key: AbilityKey, entries: RatedGame[]): AbilityProfile {
  const selected = entries.filter((entry) => entry.family === key)

  return {
    key,
    label: EMPTY_ABILITY[key],
    games: selected.length,
    averageScore: average(selected.map((entry) => entry.rating.score)),
    medianScore: median(selected.map((entry) => entry.rating.score)),
    highlightRate: rate(selected.filter((entry) => entry.rating.score >= 80).length, selected.length),
    disasterRate: rate(
      selected.filter((entry) => isDisasterRateScore(entry.rating.score)).length,
      selected.length,
    ),
    volatility: standardDeviation(selected.map((entry) => entry.rating.score)),
    averageDamageShare: average(selected.map((entry) => entry.rating.metrics.damageShare)),
    averageDamageConversion: average(selected.map((entry) => entry.rating.metrics.damageConversion)),
    averageMitigationShare: average(selected.map((entry) => entry.rating.metrics.mitigationShare)),
    averageHealingShare: average(selected.map((entry) => entry.rating.metrics.healingShare)),
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
  abilities: Record<AbilityKey, AbilityProfile>,
  roleDistribution: RoleDistributionItem[],
  overallScore: number,
) {
  if (!entries.length) return ["样本不足"]

  // 标签负责给用户一个可读结论，具体分数仍以 scoring.ts 的单局评分为准。
  const metrics = entries.map((entry) => outputRatingMetrics(entry.game))
  const damageShare = average(metrics.map((item) => item.damageShare))
  const effectiveDamageConversion = average(metrics.map((item) => item.effectiveDamageConversion))
  const goldShare = average(metrics.map((item) => item.goldShare))
  const mitigationShare = average(metrics.map((item) => item.mitigationShare))
  const healingShare = average(metrics.map((item) => item.healingShare))
  const controlQuality = average(metrics.map((item) => item.controlQuality))
  const immobilizationsPerMinute = average(metrics.map((item) => item.immobilizationsPerMinute))
  const immobilizationShare = average(metrics.map((item) => item.immobilizationShare))
  const immobilizeKillConversion = average(metrics.map((item) => item.immobilizeKillConversion))
  const killParticipation = average(metrics.map((item) => item.killParticipation))
  const killStealGap = average(metrics.map((item) => item.killShare - item.damageShare))
  const deathShare = average(metrics.map((item) => item.deathShare))
  const carryGamesRate = rate(abilities.carry.games, entries.length)
  const frontlineGamesRate = rate(abilities.frontline.games, entries.length)
  const supportGamesRate = rate(abilities.support.games, entries.length)
  const highlightRate = rate(entries.filter((entry) => entry.rating.score >= 80).length, entries.length)
  const disasterRate = rate(
    entries.filter((entry) => isDisasterRateScore(entry.rating.score)).length,
    entries.length,
  )
  const volatility = standardDeviation(entries.map((entry) => entry.rating.score))
  const tags: string[] = []

  tags.push(aggregateProfileLabel(entries))

  if (damageShare >= 0.27 && effectiveDamageConversion >= 1.15) tags.push("输出爆表")
  else if (damageShare < 0.2 && effectiveDamageConversion < 0.95) tags.push("低能输出")
  else if (effectiveDamageConversion >= 1.18) tags.push("伤转优秀")

  if (killStealGap >= 0.08 && damageShare < 0.24) tags.push("只会K头")
  if (killParticipation < 0.52 && damageShare < 0.23) tags.push("开游戏的")
  if (killParticipation >= 0.72) tags.push("团战积极")
  if (highlightRate >= 0.28) tags.push("高光多")
  if (disasterRate >= 0.25 || (disasterRate >= 0.18 && overallScore < 65)) {
    tags.push("战犯偏多")
  } else if (disasterRate > 0 && overallScore >= 70) {
    tags.push("偶有拉胯")
  }
  if (volatility <= 9 && entries.length >= 8) tags.push("发挥稳定")
  if (volatility >= 18 && entries.length >= 8) tags.push("上下限很大")
  if (goldShare >= 0.24 && effectiveDamageConversion < 1) tags.push("吃资源")
  if (deathShare >= 0.27 && damageShare >= 0.25) tags.push("冲锋型")
  if (mitigationShare >= 0.32 || frontlineGamesRate >= 0.35) tags.push("敢吃伤害")
  if (healingShare >= 0.25 || supportGamesRate >= 0.35) tags.push("团队功能")
  tags.push(
    ...profileControlTags(
      immobilizationsPerMinute,
      immobilizationShare,
      immobilizeKillConversion,
      controlQuality,
    ),
  )
  if (carryGamesRate >= 0.65 && damageShare >= 0.24) tags.push("输出型")

  const topRole = roleDistribution[0]
  if (topRole) tags.push(`常用${topRole.label}`)

  return dedupe(tags).slice(0, 10)
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

function summarizeRatedGames(entries: RatedGame[]) {
  const scores = entries.map((entry) => entry.rating.score)
  const metrics = entries.map((entry) => entry.rating.metrics)

  return {
    games: entries.length,
    averageScore: average(scores),
    medianScore: median(scores),
    highlightRate: rate(scores.filter((score) => score >= 80).length, entries.length),
    goodRate: rate(scores.filter((score) => score >= 70).length, entries.length),
    poorRate: rate(scores.filter((score) => score < 55).length, entries.length),
    disasterRate: rate(scores.filter(isDisasterRateScore).length, entries.length),
    damageShare: average(metrics.map((item) => item.damageShare)),
    damageConversion: average(metrics.map((item) => item.damageConversion)),
    killParticipation: average(metrics.map((item) => item.killParticipation)),
    killStealGap: average(metrics.map((item) => item.killShare - item.damageShare)),
    deathShare: average(metrics.map((item) => item.deathShare)),
    mitigationShare: average(metrics.map((item) => item.mitigationShare)),
    healingShare: average(metrics.map((item) => item.healingShare)),
  }
}

function aggregateProfileLabel(entries: RatedGame[]) {
  if (!entries.length) return "样本不足"

  const band = dominantScoreBand(entries)
  if (!band.length) return "样本不足"

  return dominantLabelInBand(band)
}

function weightedProfileScore(entries: RatedGame[]) {
  if (!entries.length) return 0

  const scores = entries.map((entry) => entry.rating.score)
  const band = dominantScoreBand(entries)
  const bandAverage = band.length ? average(band.map((entry) => entry.rating.score)) : average(scores)

  return round2(average(scores) * 0.6 + median(scores) * 0.25 + bandAverage * 0.15)
}

function dominantScoreBand(entries: RatedGame[]) {
  const bands = [
    { key: "excellent", entries: [] as RatedGame[] },
    { key: "normal", entries: [] as RatedGame[] },
    { key: "low", entries: [] as RatedGame[] },
    { key: "disaster", entries: [] as RatedGame[] },
  ]

  for (const entry of entries) {
    const score = entry.rating.score
    if (score >= 80) bands[0].entries.push(entry)
    else if (score >= 60) bands[1].entries.push(entry)
    else if (score >= 40) bands[2].entries.push(entry)
    else bands[3].entries.push(entry)
  }

  const disasterRate = rate(bands[3].entries.length, entries.length)
  const eligibleBands = bands.filter((band) => {
    if (!band.entries.length) return false
    return band.key !== "disaster" || disasterRate >= DISASTER_BAND_DOMINANT_RATE
  })

  return [...eligibleBands].sort((a, b) => b.entries.length - a.entries.length)[0]?.entries || []
}

function dominantLabelInBand(entries: RatedGame[]) {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    counts.set(entry.rating.label, (counts.get(entry.rating.label) || 0) + 1)
  }

  return (
    Array.from(counts.entries()).sort((a, b) => b[1] - a[1])[0]?.[0] ||
    "样本不足"
  )
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

function standardDeviation(values: number[]) {
  const safeValues = values.filter((value) => Number.isFinite(value))
  if (safeValues.length <= 1) return 0
  const mean = average(safeValues)
  const variance =
    safeValues.reduce((sum, value) => sum + (value - mean) ** 2, 0) / safeValues.length
  return round2(Math.sqrt(variance))
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
