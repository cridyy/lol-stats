import {
  calculateOutputRating,
  scoreEvaluationLabel,
  type OutputRating,
} from "./scoring"
import type { PlayerRole, RatingContext } from "./roleClassifier"
import type { RecentGame } from "./types"

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
    tags: buildPlayerTags(ratedGames, roleDistribution, overallScore),
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
      const averageScore = weightedProfileScore(entries)

      return {
        championId,
        games: entries.length,
        averageScore,
        mainRoleLabel: resolveMainRoleLabel(roleDistribution),
        label: aggregateProfileLabel(entries),
        labelBand: dominantScoreBandKey(entries),
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
  roleDistribution: RoleDistributionItem[],
  overallScore: number,
) {
  if (!entries.length) return ["样本不足"]

  const summary = summarizeRatedGames(entries)
  const positiveEligible = overallScore >= 65
  const tags: string[] = [aggregateProfileLabel(entries)]

  if (positiveEligible) {
    tags.push(...positiveProfileTags(entries, summary, roleDistribution, overallScore))
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
) {
  const tags: string[] = []
  const role = dominantPlayerRole(entries)
  const mainRoleLabel = resolveMainRoleLabel(roleDistribution)
  const carryGamesRate = abilityRate(entries, "carry")
  const frontlineGamesRate = abilityRate(entries, "frontline")
  const supportGamesRate = abilityRate(entries, "support")

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
    volatility: standardDeviation(scores),
    kda: average(metrics.map((item) => item.kda)),
    damageShare: average(metrics.map((item) => item.damageShare)),
    damageConversion: average(metrics.map((item) => item.damageConversion)),
    effectiveDamageConversion: average(metrics.map((item) => item.effectiveDamageConversion)),
    goldShare: average(metrics.map((item) => item.goldShare)),
    killShare: average(metrics.map((item) => item.killShare)),
    killParticipation: average(metrics.map((item) => item.killParticipation)),
    killStealGap: average(metrics.map((item) => item.killShare - item.damageShare)),
    deathShare: average(metrics.map((item) => item.deathShare)),
    mitigationShare: average(metrics.map((item) => item.mitigationShare)),
    mitigationPerDeath: average(metrics.map((item) => item.mitigationPerDeath)),
    healingShare: average(metrics.map((item) => item.healingShare)),
    immobilizationsPerMinute: average(metrics.map((item) => item.immobilizationsPerMinute)),
    immobilizationShare: average(metrics.map((item) => item.immobilizationShare)),
    immobilizeKillConversion: average(metrics.map((item) => item.immobilizeKillConversion)),
    controlQuality: average(metrics.map((item) => item.controlQuality)),
  }
}

function abilityRate(entries: RatedGame[], family: AbilityKey) {
  return rate(entries.filter((entry) => entry.family === family).length, entries.length)
}

function dominantPlayerRole(entries: RatedGame[]): PlayerRole {
  const counts = new Map<PlayerRole, number>()
  for (const entry of entries) {
    counts.set(entry.rating.role.role, (counts.get(entry.rating.role.role) || 0) + 1)
  }

  return Array.from(counts.entries()).sort((a, b) => b[1] - a[1])[0]?.[0] || "unknown"
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
  return dominantScoreBandGroup(entries)?.entries || []
}

function dominantScoreBandKey(entries: RatedGame[]): ChampionLabelBand {
  return dominantScoreBandGroup(entries)?.key || "empty"
}

function dominantScoreBandGroup(entries: RatedGame[]) {
  const bands: Array<{ key: Exclude<ChampionLabelBand, "empty">; entries: RatedGame[] }> = [
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

  return [...eligibleBands].sort((a, b) => b.entries.length - a.entries.length)[0]
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
