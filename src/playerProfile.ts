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
  rate: number
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
      ratedGames.filter((entry) => entry.rating.score < 40).length,
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
      const labelCounts = new Map<string, number>()
      for (const entry of entries) {
        labelCounts.set(entry.rating.label, (labelCounts.get(entry.rating.label) || 0) + 1)
      }

      return {
        championId,
        games: entries.length,
        averageScore: average(entries.map((entry) => entry.rating.score)),
        mainRoleLabel: resolveMainRoleLabel(roleDistribution),
        label: topLabel(labelCounts),
        highlightRate: rate(entries.filter((entry) => entry.rating.score >= 80).length, entries.length),
        disasterRate: rate(entries.filter((entry) => entry.rating.score < 40).length, entries.length),
        averageDamageShare: average(entries.map((entry) => entry.rating.metrics.damageShare)),
        averageDamageConversion: average(entries.map((entry) => entry.rating.metrics.damageConversion)),
        averageMitigationShare: average(entries.map((entry) => entry.rating.metrics.mitigationShare)),
        averageHealingShare: average(entries.map((entry) => entry.rating.metrics.healingShare)),
      }
    })
    .sort((a, b) => b.games - a.games || b.averageScore - a.averageScore)
}

export function profileScoreLevel(score: number) {
  if (score >= 80) return "excellent"
  if (score >= 68) return "good"
  if (score >= 55) return "average"
  return "poor"
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
    disasterRate: rate(selected.filter((entry) => entry.rating.score < 40).length, selected.length),
    volatility: standardDeviation(selected.map((entry) => entry.rating.score)),
    averageDamageShare: average(selected.map((entry) => entry.rating.metrics.damageShare)),
    averageDamageConversion: average(selected.map((entry) => entry.rating.metrics.damageConversion)),
    averageMitigationShare: average(selected.map((entry) => entry.rating.metrics.mitigationShare)),
    averageHealingShare: average(selected.map((entry) => entry.rating.metrics.healingShare)),
  }
}

function buildRoleDistribution(entries: RatedGame[]) {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    counts.set(entry.rating.role.label, (counts.get(entry.rating.role.label) || 0) + 1)
  }

  return Array.from(counts.entries())
    .map(([label, games]) => ({
      label,
      games,
      rate: rate(games, entries.length),
    }))
    .sort((a, b) => b.games - a.games || b.rate - a.rate)
}

function resolveMainRoleLabel(distribution: RoleDistributionItem[]) {
  const top = distribution[0]
  if (!top) return "样本不足"
  return `${top.label}型玩家`
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
  const damageConversion = average(metrics.map((item) => item.damageConversion))
  const mitigationShare = average(metrics.map((item) => item.mitigationShare))
  const healingShare = average(metrics.map((item) => item.healingShare))
  const killParticipation = average(metrics.map((item) => item.killParticipation))
  const killStealGap = average(metrics.map((item) => item.killShare - item.damageShare))
  const carryGamesRate = rate(abilities.carry.games, entries.length)
  const frontlineGamesRate = rate(abilities.frontline.games, entries.length)
  const supportGamesRate = rate(abilities.support.games, entries.length)
  const tags: string[] = []

  if (overallScore >= 82) tags.push("核心大腿")
  else if (overallScore >= 70) tags.push("优质玩家")
  else if (overallScore < 45) tags.push("纯战犯")
  else tags.push("普通人")

  if (damageShare >= 0.27 && damageConversion >= 1.15) tags.push("输出爆表")
  else if (damageShare < 0.2 && damageConversion < 0.95) tags.push("低能输出")
  else if (damageConversion >= 1.18) tags.push("伤转优秀")

  if (killStealGap >= 0.08 && damageShare < 0.24) tags.push("只会K头")
  if (killParticipation < 0.52 && damageShare < 0.23) tags.push("开游戏的")
  if (mitigationShare >= 0.32 || frontlineGamesRate >= 0.35) tags.push("敢吃伤害")
  if (healingShare >= 0.25 || supportGamesRate >= 0.35) tags.push("团队功能")
  if (carryGamesRate >= 0.65 && damageShare >= 0.24) tags.push("输出型")

  const topRole = roleDistribution[0]
  if (topRole) tags.push(`常用${topRole.label}`)

  return dedupe(tags).slice(0, 6)
}

function topLabel(counts: Map<string, number>) {
  return (
    Array.from(counts.entries()).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))[0]?.[0] ||
    "样本不足"
  )
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
