import type { RecentGame } from "./types"
import {
  analyzePlayerRole,
  roleVectorText,
  type RatingContext,
  type RoleAnalysis,
  type RoleKey,
  type PlayerRole,
} from "./roleClassifier"
import { fixed } from "./utils"

export interface OutputRatingMetrics {
  kda: number
  immobilizationsPerMinute: number
  damageShare: number
  goldShare: number
  damageConversion: number
  effectiveDamageConversion: number
  killShare: number
  killParticipation: number
  deathShare: number
  mitigationShare: number
  mitigationPerDeath: number
  healingShare: number
  immobilizationShare: number
  immobilizeKillShare: number
  immobilizeKillConversion: number
  controlQuality: number
}

export interface OutputRatingParts {
  [key: string]: number
}

export interface OutputRating {
  score: number
  label: string
  level: "excellent" | "good" | "average" | "poor"
  role: RoleAnalysis
  metrics: OutputRatingMetrics
  parts: OutputRatingParts
}

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min
  return Math.min(max, Math.max(min, value))
}

function clamp01(value: number) {
  return clamp(value, 0, 1)
}

function ratio(part: number | undefined, total: number | undefined) {
  const safePart = Number(part || 0)
  const safeTotal = Number(total || 0)
  return safeTotal > 0 ? safePart / safeTotal : 0
}

export function outputRatingMetrics(game: RecentGame): OutputRatingMetrics {
  const damageShare = ratio(game.damageToChampions, game.teamDamageToChampions)
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  const killShare = ratio(game.kills, game.teamKills)
  const killParticipation = ratio(game.kills + game.assists, game.teamKills)
  const deathShare = ratio(game.deaths, game.teamDeaths)
  const mitigationShare = ratio(game.damageSelfMitigated, game.teamDamageSelfMitigated)
  const gameMinutes = Math.max(Number(game.gameDuration || 0) / 60, 1)
  const immobilizationsPerMinute = Number(game.enemyChampionImmobilizations || 0) / gameMinutes
  const rawDamageConversion = goldShare > 0 ? damageShare / goldShare : 0
  const effectiveGoldShare = killAdjustedGoldShare(damageShare, goldShare, killShare)
  const personalMitigationPerDeath = Number(game.damageSelfMitigated || 0) / Math.max(game.deaths, 1)
  const teamMitigationPerDeath =
    Number(game.teamDamageSelfMitigated || 0) / Math.max(game.teamDeaths, 1)
  const immobilizationShare = ratio(
    game.enemyChampionImmobilizations,
    game.teamEnemyChampionImmobilizations,
  )
  const immobilizeKillShare = ratio(
    game.immobilizeAndKillWithAlly,
    game.teamImmobilizeAndKillWithAlly,
  )
  const immobilizeKillConversion = ratio(
    game.immobilizeAndKillWithAlly,
    game.enemyChampionImmobilizations,
  )

  return {
    kda: Number(game.kda || 0),
    immobilizationsPerMinute,
    damageShare,
    goldShare,
    damageConversion: rawDamageConversion,
    effectiveDamageConversion: effectiveGoldShare > 0 ? damageShare / effectiveGoldShare : 0,
    killShare,
    killParticipation,
    deathShare,
    mitigationShare,
    mitigationPerDeath:
      teamMitigationPerDeath > 0 ? personalMitigationPerDeath / teamMitigationPerDeath : 0,
    healingShare: ratio(game.totalHeal, game.teamTotalHeal),
    immobilizationShare,
    immobilizeKillShare,
    immobilizeKillConversion,
    controlQuality: controlQualityScore(
      immobilizationShare,
      immobilizeKillConversion,
      immobilizationsPerMinute,
    ),
  }
}

type RatingFamily = "carry" | "fighter" | "frontline" | "mage" | "support"

export function calculateOutputRating(game: RecentGame, context: RatingContext = {}): OutputRating {
  const metrics = outputRatingMetrics(game)
  const role = analyzePlayerRole(game, context)
  const family = ratingFamily(role.role)
  const parts = calculateRatingParts(metrics, role, family)
  const rawScore = Object.values(parts).reduce((sum, value) => sum + value, 0)
  const score = Math.round(clamp(rawScore, 0, 100))

  return {
    score,
    label: outputRatingLabel(game, metrics, score, role, family),
    level: outputRatingLevel(score),
    role,
    metrics,
    parts,
  }
}

function ratingFamily(role: PlayerRole): RatingFamily {
  switch (role) {
    case "tank":
      return "frontline"
    case "mage":
      return "mage"
    case "fighter":
    case "bruiser":
    case "fighterAssassin":
      return "fighter"
    case "support":
      return "support"
    default:
      return "carry"
  }
}

function dynamicControlMaxWeight(role: RoleAnalysis) {
  let maxWeight = 0

  if (isSupportHybridControlCandidate(role)) {
    maxWeight = Math.max(maxWeight, 30)
  }

  if (role.role === "mage" || primaryChampionRole(role) === "mage") {
    maxWeight = Math.max(maxWeight, 30)
  }

  return maxWeight
}

function isSupportHybridControlCandidate(role: RoleAnalysis) {
  if (role.role === "support") return false

  const sorted = (Object.keys(role.championWeights) as RoleKey[])
    .map((key) => ({ key, value: role.championWeights[key] }))
    .sort((a, b) => b.value - a.value)
  const top = sorted[0]
  const second = sorted[1]

  return top?.key === "support" && !!second && second.key !== "support" && second.value > 0.01
}

function primaryChampionRole(role: RoleAnalysis): RoleKey | undefined {
  const primary = role.championRoles[0]?.toLowerCase()
  switch (primary) {
    case "marksman":
      return "adc"
    case "mage":
      return "mage"
    case "assassin":
      return "assassin"
    case "fighter":
      return "fighter"
    case "tank":
      return "tank"
    case "support":
      return "support"
    default:
      return undefined
  }
}

function dynamicControlPart(role: RoleAnalysis, value: number): OutputRatingParts {
  if (isSupportHybridControlCandidate(role)) return { supportControl: value }
  if (role.role === "mage" || primaryChampionRole(role) === "mage") return { mageControl: value }
  return { control: value }
}

function calculateRatingParts(
  metrics: OutputRatingMetrics,
  role: RoleAnalysis,
  family: RatingFamily,
): OutputRatingParts {
  if (family === "frontline") {
    const damageControl = frontlineDamageControlParts(metrics)

    return {
      mitigation: 30 * gatedMitigationQualityScore(metrics, 0.34, 1.2),
      effectiveDamage: damageControl.effectiveDamage,
      participation: 15 * clamp01((metrics.killParticipation - 0.42) / 0.38),
      deathControl: 10 * survivalScore(metrics, 0.28, 0.22),
      roleFit: clamp01((role.finalWeights.tank + role.finalWeights.fighter) / 0.65),
      efficiency: 9 * damageConversionScore(metrics, 0.65, 0.5),
      healing: 5 * clamp01(metrics.healingShare / 0.25),
      control: damageControl.control,
    }
  }

  if (family === "fighter") {
    const combat = dynamicCombatContributionParts(metrics, {
      pool: 79,
      maxMitigationWeight: 22,
      damageWeightRatio: 36 / 58,
      damageTarget: 0.28,
      efficiencyBase: 0.72,
      efficiencyRange: 0.58,
      mitigationTargetShare: 0.3,
      mitigationTargetPerDeath: 1.15,
      mitigationShareStart: 0.18,
      mitigationShareRange: 0.22,
      mitigationPerDeathStart: 0.7,
      mitigationPerDeathRange: 0.6,
      advantageRange: 0.35,
      maxControlWeight: dynamicControlMaxWeight(role),
      controlAdvantageRange: 0.45,
    })

    return {
      damage: combat.damage,
      efficiency: combat.efficiency,
      mitigation: combat.mitigation,
      ...dynamicControlPart(role, combat.control),
      participation: 8 * clamp01((metrics.killParticipation - 0.43) / 0.37),
      survival: 8 * survivalScore(metrics, 0.24, 0.2),
      killQuality: 5 * killQualityScore(metrics),
    }
  }

  if (family === "mage") {
    const combat = dynamicCombatContributionParts(
      metrics,
      {
        pool: 63,
        maxMitigationWeight: 0,
        damageWeightRatio: 28 / 55,
        damageTarget: 0.3,
        efficiencyBase: 0.78,
        efficiencyRange: 0.57,
        mitigationTargetShare: 0.22,
        mitigationTargetPerDeath: 1.25,
        mitigationShareStart: 0.22,
        mitigationShareRange: 0.24,
        mitigationPerDeathStart: 0.9,
        mitigationPerDeathRange: 0.65,
        advantageRange: 0.45,
        maxControlWeight: 30,
        controlAdvantageRange: 0.45,
      },
      mageControlQualityScore(metrics),
    )

    return {
      damage: combat.damage,
      efficiency: combat.efficiency,
      ...dynamicControlPart(role, combat.control),
      participation: 14 * clamp01((metrics.killParticipation - 0.45) / 0.35),
      killQuality: 9 * killQualityScore(metrics),
      survival: 8 * survivalScore(metrics, 0.2, 0.18),
      economy: 6 * damageConversionScore(metrics, 0.75, 0.55),
    }
  }

  if (family === "support") {
    const protection = Math.max(metrics.healingShare, metrics.mitigationShare * 0.75)

    return {
      participation: 23 * clamp01((metrics.killParticipation - 0.45) / 0.35),
      healing: 18 * clamp01(metrics.healingShare / 0.32),
      protection: 14 * clamp01(protection / 0.3),
      effectiveDamage: 13 * clamp01(metrics.damageShare / 0.22),
      deathControl: 9 * survivalScore(metrics, 0.24, 0.2),
      economy: 6 * (1 - clamp01((metrics.goldShare - 0.23) / 0.18)),
      killQuality: 7 * killQualityScore(metrics),
      control: 10 * metrics.controlQuality,
    }
  }

  const combat = dynamicCombatContributionParts(metrics, {
    pool: 69,
    maxMitigationWeight: 20,
    damageWeightRatio: 28 / 55,
    damageTarget: 0.3,
    efficiencyBase: 0.78,
    efficiencyRange: 0.57,
    mitigationTargetShare: 0.22,
    mitigationTargetPerDeath: 1.25,
    mitigationShareStart: 0.22,
    mitigationShareRange: 0.24,
    mitigationPerDeathStart: 0.9,
    mitigationPerDeathRange: 0.65,
    advantageRange: 0.45,
    maxControlWeight: dynamicControlMaxWeight(role),
    controlAdvantageRange: 0.45,
  })

  return {
    damage: combat.damage,
    efficiency: combat.efficiency,
    participation: 8 * clamp01((metrics.killParticipation - 0.45) / 0.35),
    killQuality: 9 * killQualityScore(metrics),
    survival: 8 * survivalScore(metrics, 0.2, 0.18),
    mitigation: combat.mitigation,
    ...dynamicControlPart(role, combat.control),
    economy: 6 * damageConversionScore(metrics, 0.75, 0.55),
  }
}

interface DynamicCombatConfig {
  pool: number
  maxMitigationWeight: number
  damageWeightRatio: number
  damageTarget: number
  efficiencyBase: number
  efficiencyRange: number
  mitigationTargetShare: number
  mitigationTargetPerDeath: number
  mitigationShareStart: number
  mitigationShareRange: number
  mitigationPerDeathStart: number
  mitigationPerDeathRange: number
  advantageRange: number
  maxControlWeight?: number
  controlAdvantageRange?: number
}

function dynamicCombatContributionParts(
  metrics: OutputRatingMetrics,
  config: DynamicCombatConfig,
  controlScore = metrics.controlQuality,
) {
  // 输出/战士共享伤害、伤转、承伤/控制分池；功能项只有质量更高时才借权重。
  const damageScore = clamp01(metrics.damageShare / config.damageTarget)
  const efficiencyScore = damageConversionScore(
    metrics,
    config.efficiencyBase,
    config.efficiencyRange,
  )
  const mitigationScore = mitigationQualityScore(
    metrics,
    config.mitigationTargetShare,
    config.mitigationTargetPerDeath,
  )
  const nonMitigationScore =
    damageScore * config.damageWeightRatio + efficiencyScore * (1 - config.damageWeightRatio)
  const mitigationAdvantage = mitigationScore - nonMitigationScore

  let mitigationWeight = 0
  if (mitigationAdvantage > 0) {
    const shareGate = clamp01(
      (metrics.mitigationShare - config.mitigationShareStart) / config.mitigationShareRange,
    )
    const perDeathGate = clamp01(
      (metrics.mitigationPerDeath - config.mitigationPerDeathStart) /
        config.mitigationPerDeathRange,
    )
    const scoreGate = clamp01(mitigationAdvantage / config.advantageRange)
    const boost = shareGate * perDeathGate * scoreGate
    mitigationWeight = config.maxMitigationWeight * boost
  }

  const controlAdvantage = controlScore - nonMitigationScore
  let controlWeight = 0
  if ((config.maxControlWeight || 0) > 0 && controlAdvantage > 0) {
    const qualityGate = clamp01(controlScore)
    const scoreGate = clamp01(controlAdvantage / (config.controlAdvantageRange || 0.45))
    controlWeight = (config.maxControlWeight || 0) * qualityGate * scoreGate
  }

  const specialWeight = Math.min(config.pool, mitigationWeight + controlWeight)
  if (specialWeight < mitigationWeight + controlWeight && mitigationWeight + controlWeight > 0) {
    const scale = specialWeight / (mitigationWeight + controlWeight)
    mitigationWeight *= scale
    controlWeight *= scale
  }

  const baseWeight = config.pool - mitigationWeight - controlWeight
  const damageWeight = baseWeight * config.damageWeightRatio
  const efficiencyWeight = baseWeight - damageWeight

  return {
    damage: damageWeight * damageScore,
    efficiency: efficiencyWeight * efficiencyScore,
    mitigation: mitigationWeight * mitigationScore,
    control: controlWeight * controlScore,
  }
}

function frontlineDamageControlParts(metrics: OutputRatingMetrics) {
  const pool = 30
  const damageScore = clamp01(metrics.damageShare / 0.24)
  const controlScore = metrics.controlQuality
  const totalScore = damageScore + controlScore

  if (totalScore <= 0) {
    return { effectiveDamage: 0, control: 0 }
  }

  const controlWeight = pool * (controlScore / totalScore)
  const damageWeight = pool - controlWeight

  return {
    effectiveDamage: damageWeight * damageScore,
    control: controlWeight * controlScore,
  }
}

function killQualityScore(metrics: OutputRatingMetrics) {
  return killQualityFromShares(
    metrics.killShare,
    metrics.damageShare,
    performanceProtection(metrics),
  )
}

function damageConversionScore(metrics: OutputRatingMetrics, base: number, range: number) {
  const rawScore = clamp01((metrics.effectiveDamageConversion - base) / range)
  if (rawScore <= 0) return 0

  // High damage conversion is only reliable when deaths are controlled, or when
  // the player dealt enough team damage to justify the risk. This prevents
  // low-gold/high-death games from getting inflated by damage conversion alone.
  const deathPressure = clamp01((metrics.deathShare - 0.2) / 0.12)
  const damageProtection = clamp01((metrics.damageShare - 0.24) / 0.12)
  const deathDamageGapRisk = clamp01((metrics.deathShare - metrics.damageShare - 0.02) / 0.12)
  const feedRisk = Math.max(
    deathPressure * (1 - damageProtection),
    deathDamageGapRisk * (1 - damageProtection * 0.4),
  )
  const protectedFeedRisk = feedRisk * (1 - performanceProtection(metrics) * 0.65)
  const retain = clamp(1 - protectedFeedRisk * 0.82, 0.18, 1)

  return rawScore * retain
}

function survivalScore(metrics: OutputRatingMetrics, start: number, range: number) {
  const rawPenalty = clamp01((metrics.deathShare - start) / range)
  const protectedPenalty = rawPenalty * (1 - performanceProtection(metrics) * 0.65)
  return 1 - protectedPenalty
}

function performanceProtection(metrics: OutputRatingMetrics) {
  const kdaProtection = clamp01((metrics.kda - 3) / 4)
  const damageProtection = clamp01((metrics.damageShare - 0.24) / 0.12)

  return kdaProtection * damageProtection
}

function killQualityFromShares(killShare: number, damageShare: number, protection = 0) {
  const killStealGap = Math.max(0, killShare - damageShare)
  const adjustedGap = killStealGap * (1 - protection * 0.65)
  return clamp01(1 - adjustedGap / 0.18)
}

function controlQualityScore(
  immobilizationShare: number,
  immobilizeKillConversion: number,
  immobilizationsPerMinute: number,
) {
  if (immobilizationsPerMinute <= 1) return 0

  const shareScore = clamp01(immobilizationShare / 0.3)
  const conversionScore = clamp01(immobilizeKillConversion / 0.3)
  const frequencyScore = clamp01((immobilizationsPerMinute - 1) / 2)
  const frequencyGate = 0.4 + frequencyScore * 0.6
  const controlCore = conversionScore * 0.55 + shareScore * 0.45

  return clamp01(controlCore * frequencyGate * 0.7 + frequencyScore * 0.3)
}

function mageControlQualityScore(metrics: OutputRatingMetrics) {
  return metrics.controlQuality
}

function killAdjustedGoldShare(damageShare: number, goldShare: number, killShare: number) {
  if (goldShare <= 0) return 0

  const killQuality = killQualityFromShares(killShare, damageShare)
  const productiveKillShare = Math.min(killShare, damageShare + 0.04)
  const killPressure = clamp01((productiveKillShare - 0.18) / 0.18)
  const killGoldRelief = 0.06 * killPressure * killQuality

  return Math.max(goldShare - killGoldRelief, goldShare * 0.78, 0.08)
}

function mitigationQualityScore(
  metrics: OutputRatingMetrics,
  targetShare: number,
  targetPerDeath: number,
) {
  const shareScore = clamp01(metrics.mitigationShare / targetShare)
  const perDeathScore = clamp01((metrics.mitigationPerDeath - 0.55) / (targetPerDeath - 0.55))
  return shareScore * 0.65 + perDeathScore * 0.35
}

function gatedMitigationQualityScore(
  metrics: OutputRatingMetrics,
  targetShare: number,
  targetPerDeath: number,
) {
  const shareScore = clamp01(metrics.mitigationShare / targetShare)
  const perDeathScore = clamp01((metrics.mitigationPerDeath - 0.55) / (targetPerDeath - 0.55))
  const gatedShareScore = shareScore * (0.35 + perDeathScore * 0.65)

  return gatedShareScore * 0.7 + perDeathScore * 0.3
}

function controlRatingLabel(metrics: OutputRatingMetrics) {
  if (metrics.immobilizationsPerMinute <= 2) return undefined

  const highConversion = metrics.immobilizeKillConversion > 0.3
  const highShare = metrics.immobilizationShare > 0.3
  const highFrequency = metrics.immobilizationsPerMinute > 3

  if (highConversion && highShare && highFrequency) return "控制之神"
  if ((highConversion && highShare) || (highConversion && highFrequency)) return "控杀狙神"
  if (highShare && highFrequency) return "控制天花板"
  if (highConversion) return "精准控杀"
  if (highShare) return "控制大师"
  if (highFrequency) return "控制永动机"
  return undefined
}

function outputRatingLabel(
  game: RecentGame,
  metrics: OutputRatingMetrics,
  score: number,
  role: RoleAnalysis,
  family: RatingFamily,
) {
  if (score < 40) return "纯战犯"

  const controlLabel = controlRatingLabel(metrics)
  if (controlLabel) return controlLabel

  if (family === "frontline") {
    if (metrics.mitigationShare >= 0.34 && metrics.killParticipation >= 0.7) {
      return score >= 88 ? "铜墙铁壁" : "可靠前排"
    }
    if (metrics.mitigationShare <= 0.18 && metrics.damageShare <= 0.18) return "纸糊前排"
    if (score >= 88) return "开团核心"
    if (score >= 75) return "可靠前排"
    if (score >= 62) return "普通前排"
    if (score >= 50) return "有点发软"
    return "低能前排"
  }

  if (family === "fighter") {
    if (
      metrics.damageShare >= 0.25 &&
      metrics.mitigationShare >= 0.28 &&
      metrics.effectiveDamageConversion >= 0.95
    ) {
      return score >= 88 ? "战士核心" : "能抗能打"
    }
    if (
      metrics.damageShare >= 0.27 &&
      metrics.effectiveDamageConversion >= 1.1 &&
      metrics.deathShare >= 0.24
    ) {
      return "冲锋战神"
    }
    if (metrics.mitigationShare <= 0.18 && metrics.damageShare <= 0.2) return "低能战士"
    if (score >= 88) return "战士核心"
    if (score >= 75) return "能抗能打"
    if (score >= 62) return "普通战士"
    if (score >= 50) return "有点莽"
    return "低能战士"
  }

  if (family === "support") {
    if (metrics.healingShare >= 0.32 && metrics.killParticipation >= 0.72) {
      return score >= 88 ? "团队发动机" : "靠谱辅助"
    }
    if (metrics.damageShare >= 0.25 && metrics.damageConversion >= 1.1) return "功能大腿"
    if (score >= 88) return "团队发动机"
    if (score >= 75) return "靠谱辅助"
    if (score >= 62) return "普通人"
    if (score >= 50) return "低能辅助"
    return "开游戏的"
  }

  if (
    game.kda >= 3 &&
    (metrics.damageShare <= 0.2 || metrics.effectiveDamageConversion <= 0.95) &&
    metrics.killParticipation < 0.68
  ) {
    return "保KDA混子"
  }

  if (
    metrics.killShare - metrics.damageShare >= 0.08 &&
    metrics.damageShare < 0.24 &&
    metrics.damageConversion < 1.05
  ) {
    return "只会K头"
  }

  if (
    metrics.damageShare >= 0.27 &&
    metrics.effectiveDamageConversion >= 1.1 &&
    metrics.deathShare >= 0.24
  ) {
    return "冲锋战神"
  }

  if (
    metrics.damageShare >= 0.27 &&
    metrics.killShare + 0.08 < metrics.damageShare &&
    metrics.effectiveDamageConversion >= 1
  ) {
    return "打工皇帝"
  }

  if (metrics.damageShare >= 0.28 && metrics.effectiveDamageConversion >= 1.18 && game.kda >= 3) {
    return score >= 90 ? "通天代" : "核心大腿"
  }

  if (metrics.damageShare <= 0.2 && metrics.effectiveDamageConversion <= 0.9) {
    return "低能输出"
  }

  if (metrics.killParticipation <= 0.5 && metrics.damageShare <= 0.22) {
    return "开游戏的"
  }

  if (score >= 90) return "通天代"
  if (score >= 82) return "核心大腿"
  if (score >= 72) return "优质输出"
  if (
    role.role === "pureAssassin" &&
    metrics.killShare >= 0.28 &&
    metrics.effectiveDamageConversion >= 1
  ) {
    return "收割机器"
  }
  if (score >= 62) return "普通人"
  if (score >= 52) return "开游戏的"
  return "低能输出"
}

function outputRatingLevel(score: number): OutputRating["level"] {
  if (score >= 82) return "excellent"
  if (score >= 65) return "good"
  if (score >= 41) return "average"
  return "poor"
}

const RATING_PART_LABELS: Record<string, string> = {
  damage: "伤害",
  efficiency: "伤转",
  mitigation: "承伤",
  control: "控制分",
  mageControl: "法师控制分",
  supportControl: "功能控制分",
  participation: "参团",
  killQuality: "人头质量",
  survival: "生存",
  economy: "经济效率",
  effectiveDamage: "有效伤害",
  deathControl: "死亡控制",
  roleFit: "定位匹配",
  healing: "治疗",
  protection: "保护",
}

function ratingPartLines(parts: OutputRatingParts) {
  const entries = Object.entries(parts).filter(([, value]) => Math.abs(value) >= 0.005)
  if (!entries.length) return ["评分小项 暂无"]

  const total = entries.reduce((sum, [, value]) => sum + value, 0)
  return [
    `评分小项 合计 ${fixed(total, 1)}`,
    ...entries.map(([key, value]) => `${RATING_PART_LABELS[key] || key} ${fixed(value, 1)}`),
  ]
}

export function outputRatingTitle(game: RecentGame, context: RatingContext = {}) {
  const rating = calculateOutputRating(game, context)
  const m = rating.metrics
  return [
    `得分 ${rating.score} · ${rating.label}`,
    ...ratingPartLines(rating.parts),
    `定位 ${rating.role.label} · 置信度 ${Math.round(rating.role.confidence * 100)}%`,
    `定位来源 装备 ${Math.round(rating.role.itemWeightRatio * 100)}% / 英雄 ${Math.round(
      rating.role.championWeightRatio * 100,
    )}%`,
    `装备倾向 ${roleVectorText(rating.role.itemWeights) || "暂无"}`,
    `英雄倾向 ${roleVectorText(rating.role.championWeights) || "暂无"}`,
    `KDA ${fixed(game.kda)}`,
    `伤害占比 ${Math.round(m.damageShare * 100)}%`,
    `伤转 ${fixed(m.damageConversion)}`,
    `修正伤转 ${fixed(m.effectiveDamageConversion)}`,
    `参团率 ${Math.round(m.killParticipation * 100)}%`,
    `人头占比 ${Math.round(m.killShare * 100)}%`,
    `死亡占比 ${Math.round(m.deathShare * 100)}%`,
    `承伤占比 ${Math.round(m.mitigationShare * 100)}%`,
    `每死承伤 ${fixed(m.mitigationPerDeath)}`,
    `分均控制 ${fixed(m.immobilizationsPerMinute)}`,
    `控制占比 ${Math.round(m.immobilizationShare * 100)}%`,
    `控杀占比 ${Math.round(m.immobilizeKillShare * 100)}%`,
    `控制转化 ${Math.round(m.immobilizeKillConversion * 100)}%`,
  ].join("\n")
}
