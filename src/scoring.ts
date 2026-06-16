import type { RecentGame } from "./types"
import {
  analyzePlayerRole,
  roleVectorText,
  type RatingContext,
  type RoleAnalysis,
  type PlayerRole,
} from "./roleClassifier"
import { fixed } from "./utils"

export interface OutputRatingMetrics {
  damageShare: number
  goldShare: number
  damageConversion: number
  killShare: number
  killParticipation: number
  deathShare: number
  mitigationShare: number
  healingShare: number
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

  return {
    damageShare,
    goldShare,
    damageConversion: goldShare > 0 ? damageShare / goldShare : 0,
    killShare: ratio(game.kills, game.teamKills),
    killParticipation: ratio(game.kills + game.assists, game.teamKills),
    deathShare: ratio(game.deaths, game.teamDeaths),
    mitigationShare: ratio(game.damageSelfMitigated, game.teamDamageSelfMitigated),
    healingShare: ratio(game.totalHeal, game.teamTotalHeal),
  }
}

type RatingFamily = "carry" | "frontline" | "support"

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
    case "bruiser":
      return "frontline"
    case "support":
    case "utilityCarry":
      return "support"
    default:
      return "carry"
  }
}

function calculateRatingParts(
  metrics: OutputRatingMetrics,
  role: RoleAnalysis,
  family: RatingFamily,
): OutputRatingParts {
  if (family === "frontline") {
    return {
      mitigation: 30 * clamp01(metrics.mitigationShare / 0.34),
      effectiveDamage: 18 * clamp01(metrics.damageShare / 0.24),
      participation: 18 * clamp01((metrics.killParticipation - 0.42) / 0.38),
      deathControl: 12 * (1 - clamp01((metrics.deathShare - 0.28) / 0.22)),
      roleFit: 10 * clamp01((role.finalWeights.tank + role.finalWeights.fighter) / 0.65),
      efficiency: 7 * clamp01((metrics.damageConversion - 0.65) / 0.5),
      healing: 5 * clamp01(metrics.healingShare / 0.25),
      fighterMitigationBonus: fighterMitigationBonus(role, metrics),
    }
  }

  if (family === "support") {
    const protection = Math.max(metrics.healingShare, metrics.mitigationShare * 0.75)

    return {
      participation: 25 * clamp01((metrics.killParticipation - 0.45) / 0.35),
      healing: 20 * clamp01(metrics.healingShare / 0.32),
      protection: 15 * clamp01(protection / 0.3),
      effectiveDamage: 15 * clamp01(metrics.damageShare / 0.22),
      deathControl: 10 * (1 - clamp01((metrics.deathShare - 0.24) / 0.2)),
      economy: 8 * (1 - clamp01((metrics.goldShare - 0.23) / 0.18)),
      killQuality: 7 * killQualityScore(metrics),
    }
  }

  return {
    damage: 30 * clamp01(metrics.damageShare / 0.3),
    efficiency: 30 * clamp01((metrics.damageConversion - 0.78) / 0.57),
    participation: 15 * clamp01((metrics.killParticipation - 0.45) / 0.35),
    killQuality: 10 * killQualityScore(metrics),
    survival: 8 * (1 - clamp01((metrics.deathShare - 0.2) / 0.18)),
    economy: 7 * clamp01((metrics.damageConversion - 0.75) / 0.55),
    fighterMitigationBonus: fighterMitigationBonus(role, metrics),
  }
}

function killQualityScore(metrics: OutputRatingMetrics) {
  const killStealGap = Math.max(0, metrics.killShare - metrics.damageShare)
  return clamp01(1 - killStealGap / 0.18)
}

function fighterMitigationBonus(role: RoleAnalysis, metrics: OutputRatingMetrics) {
  if (role.role !== "fighter" && role.role !== "bruiser") return 0
  return 8 * clamp01((metrics.mitigationShare - 0.4) / 0.15)
}

function outputRatingLabel(
  game: RecentGame,
  metrics: OutputRatingMetrics,
  score: number,
  role: RoleAnalysis,
  family: RatingFamily,
) {
  if (score < 40) return "纯战犯"

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
    (metrics.damageShare <= 0.2 || metrics.damageConversion <= 0.95) &&
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
    metrics.damageConversion >= 1.1 &&
    metrics.deathShare >= 0.24
  ) {
    return "冲锋战神"
  }

  if (
    metrics.damageShare >= 0.27 &&
    metrics.killShare + 0.08 < metrics.damageShare &&
    metrics.damageConversion >= 1
  ) {
    return "打工皇帝"
  }

  if (metrics.damageShare >= 0.28 && metrics.damageConversion >= 1.18 && game.kda >= 3) {
    return score >= 90 ? "通天代" : "核心大腿"
  }

  if (metrics.damageShare <= 0.2 && metrics.damageConversion <= 0.9) {
    return "低能输出"
  }

  if (metrics.killParticipation <= 0.5 && metrics.damageShare <= 0.22) {
    return "开游戏的"
  }

  if (score >= 90) return "通天代"
  if (score >= 82) return "核心大腿"
  if (score >= 72) return "优质输出"
  if (role.role === "pureAssassin" && metrics.killShare >= 0.28 && metrics.damageConversion >= 1) {
    return "收割机器"
  }
  if (score >= 62) return "普通人"
  if (score >= 52) return "开游戏的"
  return "低能输出"
}

function outputRatingLevel(score: number): OutputRating["level"] {
  if (score >= 80) return "excellent"
  if (score >= 65) return "good"
  if (score >= 50) return "average"
  return "poor"
}

export function outputRatingTitle(game: RecentGame, context: RatingContext = {}) {
  const rating = calculateOutputRating(game, context)
  const m = rating.metrics
  return [
    `得分 ${rating.score} · ${rating.label}`,
    `定位 ${rating.role.label} · 置信度 ${Math.round(rating.role.confidence * 100)}%`,
    `定位来源 装备 ${Math.round(rating.role.itemWeightRatio * 100)}% / 英雄 ${Math.round(
      rating.role.championWeightRatio * 100,
    )}%`,
    `装备倾向 ${roleVectorText(rating.role.itemWeights) || "暂无"}`,
    `英雄倾向 ${roleVectorText(rating.role.championWeights) || "暂无"}`,
    `KDA ${fixed(game.kda)}`,
    `伤害占比 ${Math.round(m.damageShare * 100)}%`,
    `伤转 ${fixed(m.damageConversion)}`,
    `参团率 ${Math.round(m.killParticipation * 100)}%`,
    `人头占比 ${Math.round(m.killShare * 100)}%`,
    `死亡占比 ${Math.round(m.deathShare * 100)}%`,
  ].join("\n")
}
