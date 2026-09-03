import type { GlobalChampionBaseline, RecentGame } from "./types"
import {
  analyzePlayerRole,
  roleVectorText,
  type RatingContext,
  type RoleAnalysis,
  type RoleKey,
  type PlayerRole,
} from "./roleClassifier"
import { fixed, mitigationValue, teamMitigationValue } from "./utils"

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
  /** 原始队内指标，用于界面展示和多局画像。 */
  metrics: OutputRatingMetrics
  /** 本局评分和标签实际采用的指标；未采用碾压校准时与 metrics 相同。 */
  scoringMetrics: OutputRatingMetrics
  parts: OutputRatingParts
  contextAdjustment: MatchContextAdjustment
}

export type GlobalBenchmarkMetricKey =
  | "damageToChampions"
  | "damageTaken"
  | "selfMitigatedDamage"
  | "allyHealing"
  | "allyShielding"
  | "crowdControlDurationSeconds"

export interface GlobalBenchmarkMetricResult {
  key: GlobalBenchmarkMetricKey
  label: string
  ledger: "branch" | "addedBranch"
  referenceLabel: string
  actualPerMinute: number
  unadjustedBaselinePerMinute: number
  baselinePerMinute: number
  unadjustedRoleBaselinePerMinute?: number
  roleBaselinePerMinute?: number
  ratio: number
  weight: number
  perMinuteDigits: number
}

export interface GlobalRating {
  available: boolean
  reason: string
  baseRating: OutputRating
  score: number
  level: OutputRating["level"]
  globalScore: number
  globalScoreMax: number
  globalPassingScore: number
  replacedScore: number
  scoreDelta: number
  performanceRatio: number
  metrics: GlobalBenchmarkMetricResult[]
  eligibleMetricCount: number
  baselineName: string
  baselineSourceDate: string
  branchChanged: boolean
  nativeBranchLabel: string
  evaluatedBranchLabel: string
  baselineDurationMultiplier: number
}

export interface MatchContextAdjustment {
  active: boolean
  scoringApplied: boolean
  reason: string
  severity: number
  blendWeight: number
  imbalance: number
  winnerKillDeathRatio: number
  loserKillDeathRatio: number
  combatGap: number
  damageGap: number
  killDeathGateActive: boolean
  damageGateActive: boolean
  towerGateActive: boolean
  winnerTowerKills: number
  loserTowerKills: number
  loserFirstTowerTimeSeconds?: number
  loserFirstTowerTimeRatio: number
  towerKdaWeight: number
  maxKdaWeight: number
  kdaWeight: number
  kdaPoints: number
  originalScore: number
  contextualScore: number
  outcomeAdjustment: number
  maxScoreDelta: number
  unprotectedScoreDelta: number
  loserResistanceProtection: number
  scoreDelta: number
}

interface ContextRatingCandidate {
  scoringApplied: boolean
  kdaWeight: number
  parts: OutputRatingParts
  contextualTotal: number
  contextualScore: number
  outcomeAdjustment: number
  maxScoreDelta: number
  unprotectedScoreDelta: number
  loserResistanceProtection: number
  scoreDelta: number
  finalScore: number
}

interface StompMatchContext {
  active: boolean
  reason: string
  severity: number
  blendWeight: number
  imbalance: number
  winnerKillDeathRatio: number
  loserKillDeathRatio: number
  combatGap: number
  damageGap: number
  killDeathGateActive: boolean
  damageGateActive: boolean
  towerGateActive: boolean
  winnerTowerKills: number
  loserTowerKills: number
  loserFirstTowerTimeSeconds?: number
  loserFirstTowerTimeRatio: number
  towerKdaWeight: number
  participantScale: number
  teamKillStrength: number
}

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min
  return Math.min(max, Math.max(min, value))
}

function clamp01(value: number) {
  return clamp(value, 0, 1)
}

function smoothstep01(value: number) {
  const t = clamp01(value)
  return t * t * (3 - 2 * t)
}

function smoothstepRange(value: number, start: number, end: number) {
  if (end <= start) return value >= end ? 1 : 0
  return smoothstep01((value - start) / (end - start))
}

function mix(from: number, to: number, weight: number) {
  return from + (to - from) * clamp01(weight)
}

function ratio(part: number | undefined, total: number | undefined) {
  const safePart = Number(part || 0)
  const safeTotal = Number(total || 0)
  return safeTotal > 0 ? safePart / safeTotal : 0
}

export function scoreEvaluationLabel(score: number) {
  if (score >= 90) return "通天代"
  if (score >= 80) return "小代"
  if (score >= 70) return "小有实力"
  if (score >= 60) return "普通人"
  if (score >= 40) return "小坑比"
  return "大坑比"
}

export function outputRatingMetrics(game: RecentGame): OutputRatingMetrics {
  const damageShare = ratio(game.damageToChampions, game.teamDamageToChampions)
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  const killShare = ratio(game.kills, game.teamKills)
  const killParticipation = ratio(game.kills + game.assists, game.teamKills)
  const deathShare = ratio(game.deaths, game.teamDeaths)
  const mitigation = mitigationValue(game)
  const teamMitigation = teamMitigationValue(game)
  const mitigationShare = ratio(mitigation, teamMitigation)
  const gameMinutes = Math.max(Number(game.gameDuration || 0) / 60, 1)
  const immobilizationsPerMinute = Number(game.enemyChampionImmobilizations || 0) / gameMinutes
  const rawDamageConversion = goldShare > 0 ? damageShare / goldShare : 0
  const effectiveGoldShare = killAdjustedGoldShare(damageShare, goldShare, killShare)
  const personalMitigationPerDeath = mitigation / Math.max(game.deaths, 1)
  const teamMitigationPerDeath =
    teamMitigation / Math.max(game.teamDeaths, 1)
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

type GlobalBaselineValueField =
  | "damageToChampions"
  | "damageTaken"
  | "selfMitigatedDamage"
  | "allyHealing"
  | "allyShielding"
  | "crowdControlDurationSeconds"

interface GlobalBenchmarkMetricDefinition {
  key: GlobalBenchmarkMetricKey
  label: string
  baselineField: GlobalBaselineValueField
  minimumBaselineValue: number
  perMinuteDigits: number
  actualValue: (game: RecentGame) => number | undefined
}

interface GlobalSupportBenchmark {
  label: string
  metrics: Partial<Record<GlobalBenchmarkMetricKey, number>>
}

interface GlobalBranchSelection {
  nativeFamily: RatingFamily
  evaluatedFamily: RatingFamily
  branchChanged: boolean
}

interface GlobalMetricComparison {
  ledger: GlobalBenchmarkMetricResult["ledger"]
  referenceLabel: string
  actualPerMinute: number
  unadjustedBaselinePerMinute: number
  baselinePerMinute: number
  unadjustedRoleBaselinePerMinute?: number
  roleBaselinePerMinute?: number
  ratio: number
  allocationEffect: number
  scoringRatio: number
}

interface GlobalMetricRoute {
  eligible: boolean
  measured: boolean
  effect: number
  weights: Record<GlobalBenchmarkMetricKey, number>
}

const GLOBAL_SCORE_SUPPORT_MAX = 25
const GLOBAL_SCORE_SUPPORT_PASSING = 15
const GLOBAL_SCORE_FULL_PERFORMANCE_RATIO = 1.5
const GLOBAL_BASELINE_MIN_DURATION_MINUTES = 8
const GLOBAL_BASELINE_STANDARD_DURATION_MINUTES = 16
const GLOBAL_BASELINE_MAX_DURATION_MINUTES = 20
const GLOBAL_BASELINE_MIN_DURATION_MULTIPLIER = 0.6
const GLOBAL_BASELINE_MAX_DURATION_MULTIPLIER = 1.1

const GLOBAL_BENCHMARK_METRICS: GlobalBenchmarkMetricDefinition[] = [
  {
    key: "damageToChampions",
    label: "对英雄伤害",
    baselineField: "damageToChampions",
    minimumBaselineValue: 1_000,
    perMinuteDigits: 0,
    actualValue: (game) => Number(game.damageToChampions || 0),
  },
  {
    key: "damageTaken",
    label: "承受伤害",
    baselineField: "damageTaken",
    minimumBaselineValue: 1_000,
    perMinuteDigits: 0,
    actualValue: (game) => Number(game.totalDamageTaken || 0),
  },
  {
    key: "selfMitigatedDamage",
    label: "自我减免",
    baselineField: "selfMitigatedDamage",
    minimumBaselineValue: 1_000,
    perMinuteDigits: 0,
    actualValue: (game) => Number(game.damageSelfMitigated || 0),
  },
  {
    key: "allyHealing",
    label: "队友治疗",
    baselineField: "allyHealing",
    minimumBaselineValue: 100,
    perMinuteDigits: 0,
    actualValue: (game) => optionalMetricValue(game.totalHealsOnTeammates),
  },
  {
    key: "allyShielding",
    label: "队友护盾",
    baselineField: "allyShielding",
    minimumBaselineValue: 100,
    perMinuteDigits: 0,
    actualValue: (game) => Number(game.totalDamageShieldedOnTeammates || 0),
  },
  {
    key: "crowdControlDurationSeconds",
    label: "控制时长",
    baselineField: "crowdControlDurationSeconds",
    minimumBaselineValue: 1,
    perMinuteDigits: 1,
    actualValue: (game) => optionalMetricValue(game.timeCCingOthers),
  },
]

const GLOBAL_BRANCH_LABELS: Record<RatingFamily, string> = {
  carry: "输出",
  fighter: "战士",
  frontline: "前排",
  mage: "法师",
  support: "辅助",
}

function optionalMetricValue(value: number | null | undefined) {
  if (value === undefined || value === null) return undefined
  return Number(value)
}

function globalRatioEffect(value: number) {
  return clamp(Math.log2(Math.max(value, 0.5)), -1, 1)
}

function globalBaselineDurationMultiplier(gameSeconds: number) {
  const gameMinutes = gameSeconds / 60
  if (gameMinutes <= GLOBAL_BASELINE_STANDARD_DURATION_MINUTES) {
    const earlyDurationProgress = clamp(
      (gameMinutes - GLOBAL_BASELINE_MIN_DURATION_MINUTES)
        / (GLOBAL_BASELINE_STANDARD_DURATION_MINUTES - GLOBAL_BASELINE_MIN_DURATION_MINUTES),
      0,
      1,
    )
    return GLOBAL_BASELINE_MIN_DURATION_MULTIPLIER
      + earlyDurationProgress * (1 - GLOBAL_BASELINE_MIN_DURATION_MULTIPLIER)
  }

  const lateDurationProgress = clamp(
    (gameMinutes - GLOBAL_BASELINE_STANDARD_DURATION_MINUTES)
      / (GLOBAL_BASELINE_MAX_DURATION_MINUTES - GLOBAL_BASELINE_STANDARD_DURATION_MINUTES),
    0,
    1,
  )
  return 1 + lateDurationProgress * (GLOBAL_BASELINE_MAX_DURATION_MULTIPLIER - 1)
}

function globalScoreFromPerformance(
  performanceRatio: number,
) {
  if (performanceRatio <= 1) {
    return GLOBAL_SCORE_SUPPORT_PASSING * clamp01(performanceRatio)
  }

  const extraProgress = clamp01(
    (performanceRatio - 1) / (GLOBAL_SCORE_FULL_PERFORMANCE_RATIO - 1),
  )
  return GLOBAL_SCORE_SUPPORT_PASSING
    + (GLOBAL_SCORE_SUPPORT_MAX - GLOBAL_SCORE_SUPPORT_PASSING) * extraProgress
}

function supportGlobalSlotReplacedScore(baseRating: OutputRating) {
  const metrics = baseRating.scoringMetrics
  return 5 * clamp01((metrics.killParticipation - 0.45) / 0.35)
    + 7 * killQualityScore(metrics)
    + 13 * clamp01(metrics.damageShare / 0.22)
}

function globalMetricKeys() {
  return GLOBAL_BENCHMARK_METRICS.map((definition) => definition.key)
}

function championOnlyRatingFamily(
  championId: number,
  context: RatingContext,
  fallback: RatingFamily,
) {
  if (!context.champions?.[championId]) return fallback

  const nativeRole = analyzePlayerRole(
    { championId, itemIds: [] } as unknown as RecentGame,
    { champions: context.champions },
  )
  return ratingFamily(nativeRole.role)
}

function selectGlobalBranch(
  game: RecentGame,
  context: RatingContext,
  role: RoleAnalysis,
): GlobalBranchSelection {
  const evaluatedFamily = ratingFamily(role.role)
  const nativeFamily = championOnlyRatingFamily(game.championId, context, evaluatedFamily)
  return {
    nativeFamily,
    evaluatedFamily,
    branchChanged: nativeFamily !== evaluatedFamily,
  }
}

function medianValue(values: number[]) {
  if (!values.length) return 0
  const sorted = [...values].sort((left, right) => left - right)
  const middle = Math.floor(sorted.length / 2)
  return sorted.length % 2
    ? sorted[middle]
    : (sorted[middle - 1] + sorted[middle]) / 2
}

function buildGlobalSupportBenchmark(
  baselines: ReadonlyMap<number, GlobalChampionBaseline>,
  context: RatingContext,
) {
  const metricPool = new Map<GlobalBenchmarkMetricKey, number[]>()

  for (const baseline of baselines.values()) {
    if (!context.champions?.[baseline.id] || baseline.gameDurationSeconds <= 0) continue
    const family = championOnlyRatingFamily(baseline.id, context, "carry")
    if (family !== "support") continue

    for (const definition of GLOBAL_BENCHMARK_METRICS) {
      const totalValue = Number(baseline[definition.baselineField] || 0)
      if (totalValue < definition.minimumBaselineValue) continue

      const values = metricPool.get(definition.key) || []
      values.push(totalValue * 60 / baseline.gameDurationSeconds)
      metricPool.set(definition.key, values)
    }
  }

  const metrics = Object.fromEntries(
    globalMetricKeys().flatMap((key) => {
      const value = medianValue(metricPool.get(key) || [])
      return value > 0 ? [[key, value]] : []
    }),
  ) as Partial<Record<GlobalBenchmarkMetricKey, number>>
  return {
    label: GLOBAL_BRANCH_LABELS.support,
    metrics,
  }
}

function isHexAramGame(game: RecentGame) {
  const mode = String(game.gameMode || "").toUpperCase()
  return game.queueId === 2400 || mode === "STRAWBERRY" || mode === "KIWI"
}

const DEFAULT_RATING_CONTEXT: RatingContext = {}
const outputRatingCache = new WeakMap<RecentGame, WeakMap<RatingContext, OutputRating>>()
const globalRatingCache = new WeakMap<
  RecentGame,
  WeakMap<RatingContext, WeakMap<object, GlobalRating>>
>()
const globalSupportBenchmarkCache = new WeakMap<
  object,
  WeakMap<RatingContext, GlobalSupportBenchmark>
>()

export function calculateOutputRating(
  game: RecentGame,
  context: RatingContext = DEFAULT_RATING_CONTEXT,
): OutputRating {
  let contextCache = outputRatingCache.get(game)
  if (!contextCache) {
    contextCache = new WeakMap<RatingContext, OutputRating>()
    outputRatingCache.set(game, contextCache)
  }

  const cached = contextCache.get(context)
  if (cached) return cached

  const rating = calculateOutputRatingUncached(game, context)
  contextCache.set(context, rating)
  return rating
}

export function calculateGlobalRating(
  game: RecentGame,
  context: RatingContext = DEFAULT_RATING_CONTEXT,
  baselines?: ReadonlyMap<number, GlobalChampionBaseline>,
): GlobalRating {
  const baseRating = calculateOutputRating(game, context)
  if (ratingFamily(baseRating.role.role) !== "support") {
    return unavailableGlobalRating(baseRating, "仅辅助评分分支引入全服水平")
  }
  if (!isHexAramGame(game)) {
    return unavailableGlobalRating(baseRating, "全服基准目前只适用于海克斯大乱斗")
  }
  if (!baselines) {
    return unavailableGlobalRating(baseRating, "全服基准尚未加载")
  }
  const baseline = baselines.get(game.championId)
  if (!baseline) {
    return unavailableGlobalRating(baseRating, "该英雄暂无全服平均数据")
  }

  const baselineCacheKey = baselines as object
  let contextCache = globalRatingCache.get(game)
  if (!contextCache) {
    contextCache = new WeakMap<RatingContext, WeakMap<object, GlobalRating>>()
    globalRatingCache.set(game, contextCache)
  }
  let baselineCache = contextCache.get(context)
  if (!baselineCache) {
    baselineCache = new WeakMap<object, GlobalRating>()
    contextCache.set(context, baselineCache)
  }

  const cached = baselineCache.get(baselineCacheKey)
  if (cached) return cached

  const rating = calculateGlobalRatingUncached(
    game,
    context,
    baseRating,
    baseline,
    baselines,
  )
  baselineCache.set(baselineCacheKey, rating)
  return rating
}

export function calculateOfficialRating(
  game: RecentGame,
  context: RatingContext = DEFAULT_RATING_CONTEXT,
  baselines?: ReadonlyMap<number, GlobalChampionBaseline>,
): OutputRating {
  const globalRating = calculateGlobalRating(game, context, baselines)
  return officialRatingFromGlobal(game, globalRating)
}

function officialRatingFromGlobal(game: RecentGame, globalRating: GlobalRating): OutputRating {
  const baseRating = globalRating.baseRating
  if (!globalRating.available) return baseRating

  const metrics = baseRating.scoringMetrics
  const parts = { ...baseRating.parts }
  const replacedParticipation = 5 * clamp01((metrics.killParticipation - 0.45) / 0.35)
  parts.participation = Math.max(0, (parts.participation || 0) - replacedParticipation)
  delete parts.killQuality
  delete parts.effectiveDamage
  parts.globalBaseline = globalRating.globalScore

  return {
    ...baseRating,
    score: globalRating.score,
    level: globalRating.level,
    label: outputRatingLabel(
      game,
      metrics,
      globalRating.score,
      baseRating.role,
      ratingFamily(baseRating.role.role),
    ),
    parts,
  }
}

function unavailableGlobalRating(baseRating: OutputRating, reason: string): GlobalRating {
  return {
    available: false,
    reason,
    baseRating,
    score: baseRating.score,
    level: baseRating.level,
    globalScore: 0,
    globalScoreMax: 0,
    globalPassingScore: 0,
    replacedScore: 0,
    scoreDelta: 0,
    performanceRatio: 0,
    metrics: [],
    eligibleMetricCount: 0,
    baselineName: "",
    baselineSourceDate: "",
    branchChanged: false,
    nativeBranchLabel: "",
    evaluatedBranchLabel: "",
    baselineDurationMultiplier: 1,
  }
}

function calculateGlobalRatingUncached(
  game: RecentGame,
  context: RatingContext,
  baseRating: OutputRating,
  baseline: GlobalChampionBaseline,
  baselines: ReadonlyMap<number, GlobalChampionBaseline>,
): GlobalRating {
  const gameSeconds = Number(game.gameDuration || 0)
  const baselineSeconds = Number(baseline.gameDurationSeconds || 0)
  if (gameSeconds <= 0 || baselineSeconds <= 0) {
    return unavailableGlobalRating(baseRating, "本局或全服平均游戏时长无效")
  }

  const branch = selectGlobalBranch(game, context, baseRating.role)
  const baselineDurationMultiplier = globalBaselineDurationMultiplier(gameSeconds)
  const calculation = calculateSupportGlobalMetrics(
    game,
    context,
    baseline,
    baselines,
    gameSeconds,
    baselineSeconds,
    branch,
    baselineDurationMultiplier,
  )
  if (!calculation.metrics.length) {
    return unavailableGlobalRating(baseRating, "本局没有可与全服平均值比较的指标")
  }

  const metrics = calculation.metrics
  const globalScore = globalScoreFromPerformance(calculation.performanceRatio)
  const replacedScore = supportGlobalSlotReplacedScore(baseRating)
  const scoreDelta = globalScore - replacedScore
  const score = Math.round(clamp(baseRating.score + scoreDelta, 0, 100))
  return {
    available: true,
    reason: "",
    baseRating,
    score,
    level: outputRatingLevel(score),
    globalScore,
    globalScoreMax: GLOBAL_SCORE_SUPPORT_MAX,
    globalPassingScore: GLOBAL_SCORE_SUPPORT_PASSING,
    replacedScore,
    scoreDelta,
    performanceRatio: calculation.performanceRatio,
    metrics,
    eligibleMetricCount: calculation.eligibleMetricCount,
    baselineName: baseline.name,
    baselineSourceDate: baseline.sourceDataDate,
    branchChanged: branch.branchChanged,
    nativeBranchLabel: GLOBAL_BRANCH_LABELS[branch.nativeFamily],
    evaluatedBranchLabel: GLOBAL_BRANCH_LABELS[branch.evaluatedFamily],
    baselineDurationMultiplier,
  }
}

function calculateSupportGlobalMetrics(
  game: RecentGame,
  context: RatingContext,
  baseline: GlobalChampionBaseline,
  baselines: ReadonlyMap<number, GlobalChampionBaseline>,
  gameSeconds: number,
  baselineSeconds: number,
  branch: GlobalBranchSelection,
  baselineDurationMultiplier: number,
) {
  const { comparisons, eligibleKeys, baselineWeights } = sameChampionGlobalComparisons(
    game,
    baseline,
    gameSeconds,
    baselineSeconds,
    baselineDurationMultiplier,
  )

  if (branch.branchChanged) {
    const supportBenchmark = cachedGlobalSupportBenchmark(baselines, context)
    addChangedBranchComparisons(
      comparisons,
      eligibleKeys,
      baselineWeights,
      game,
      baseline,
      supportBenchmark,
      gameSeconds,
      baselineSeconds,
      baselineDurationMultiplier,
    )
  }

  const weights = supportGlobalMetricWeights(
    comparisons,
    eligibleKeys,
    baselineWeights,
  )
  const potentialKeys = supportGlobalMetricKeys()
  const metrics = GLOBAL_BENCHMARK_METRICS.flatMap<GlobalBenchmarkMetricResult>((definition) => {
    const comparison = comparisons[definition.key]
    const weight = weights[definition.key]
    if (!comparison || weight <= 0) return []

    return [{
      key: definition.key,
      label: definition.label,
      ledger: comparison.ledger,
      referenceLabel: comparison.referenceLabel,
      actualPerMinute: comparison.actualPerMinute,
      unadjustedBaselinePerMinute: comparison.unadjustedBaselinePerMinute,
      baselinePerMinute: comparison.baselinePerMinute,
      unadjustedRoleBaselinePerMinute: comparison.unadjustedRoleBaselinePerMinute,
      roleBaselinePerMinute: comparison.roleBaselinePerMinute,
      ratio: comparison.ratio,
      weight,
      perMinuteDigits: definition.perMinuteDigits,
    }]
  })
  const totalWeight = globalMetricKeys().reduce((sum, key) => sum + weights[key], 0)
  const measuredPerformance = globalMetricKeys().reduce((sum, key) => {
    const comparison = comparisons[key]
    return sum + weights[key] * (comparison?.scoringRatio ?? 1)
  }, 0)

  return {
    metrics,
    eligibleMetricCount: [...eligibleKeys].filter((key) => potentialKeys.has(key)).length,
    performanceRatio: measuredPerformance + Math.max(1 - totalWeight, 0),
  }
}

function sameChampionGlobalComparisons(
  game: RecentGame,
  baseline: GlobalChampionBaseline,
  gameSeconds: number,
  baselineSeconds: number,
  baselineDurationMultiplier: number,
) {
  const comparisons: Partial<Record<GlobalBenchmarkMetricKey, GlobalMetricComparison>> = {}
  const eligibleKeys = new Set<GlobalBenchmarkMetricKey>()
  const baselineWeights: Partial<Record<GlobalBenchmarkMetricKey, number>> = {}

  for (const definition of GLOBAL_BENCHMARK_METRICS) {
    const baselineValue = Number(baseline[definition.baselineField] || 0)
    if (baselineValue < definition.minimumBaselineValue) continue
    eligibleKeys.add(definition.key)
    baselineWeights[definition.key] = baselineValue * 60 / baselineSeconds
    const actualValue = definition.actualValue(game)
    if (actualValue === undefined || !Number.isFinite(actualValue) || actualValue < 0) continue

    const actualPerMinute = actualValue * 60 / gameSeconds
    const unadjustedBaselinePerMinute = baselineValue * 60 / baselineSeconds
    const baselinePerMinute = unadjustedBaselinePerMinute * baselineDurationMultiplier
    const metricRatio = baselinePerMinute > 0 ? actualPerMinute / baselinePerMinute : 0
    const metricEffect = globalRatioEffect(metricRatio)
    comparisons[definition.key] = {
      ledger: "branch",
      referenceLabel: `${baseline.name}全服平均`,
      actualPerMinute,
      unadjustedBaselinePerMinute,
      baselinePerMinute,
      ratio: metricRatio,
      allocationEffect: metricEffect,
      scoringRatio: metricRatio,
    }
  }

  return { comparisons, eligibleKeys, baselineWeights }
}

function addChangedBranchComparisons(
  comparisons: Partial<Record<GlobalBenchmarkMetricKey, GlobalMetricComparison>>,
  eligibleKeys: Set<GlobalBenchmarkMetricKey>,
  baselineWeights: Partial<Record<GlobalBenchmarkMetricKey, number>>,
  game: RecentGame,
  baseline: GlobalChampionBaseline,
  supportBenchmark: GlobalSupportBenchmark | undefined,
  gameSeconds: number,
  baselineSeconds: number,
  baselineDurationMultiplier: number,
) {
  if (!supportBenchmark) return

  for (const definition of GLOBAL_BENCHMARK_METRICS) {
    if (comparisons[definition.key]) continue
    const baselineValue = Number(baseline[definition.baselineField] || 0)
    if (baselineValue >= definition.minimumBaselineValue) continue

    const unadjustedRoleBaselinePerMinute = supportBenchmark.metrics[definition.key]
    if (!unadjustedRoleBaselinePerMinute || unadjustedRoleBaselinePerMinute <= 0) continue

    const actualValue = definition.actualValue(game)
    if (actualValue === undefined || !Number.isFinite(actualValue) || actualValue < 0) continue

    const actualPerMinute = actualValue * 60 / gameSeconds
    const unadjustedBaselinePerMinute = baselineValue * 60 / baselineSeconds
    const baselinePerMinute = unadjustedBaselinePerMinute * baselineDurationMultiplier
    const roleBaselinePerMinute = unadjustedRoleBaselinePerMinute
      * baselineDurationMultiplier
    const addedRoleWorkload = clamp(
      (actualPerMinute - baselinePerMinute) / roleBaselinePerMinute,
      0,
      GLOBAL_SCORE_FULL_PERFORMANCE_RATIO,
    )
    if (addedRoleWorkload <= 0) continue

    eligibleKeys.add(definition.key)
    baselineWeights[definition.key] = unadjustedRoleBaselinePerMinute
    comparisons[definition.key] = {
      ledger: "addedBranch",
      referenceLabel: `${supportBenchmark.label}英雄池基准`,
      actualPerMinute,
      unadjustedBaselinePerMinute,
      baselinePerMinute,
      unadjustedRoleBaselinePerMinute,
      roleBaselinePerMinute,
      ratio: addedRoleWorkload,
      allocationEffect: globalRatioEffect(addedRoleWorkload),
      scoringRatio: addedRoleWorkload,
    }
  }
}

function emptyGlobalMetricWeights() {
  return Object.fromEntries(
    globalMetricKeys().map((key) => [key, 0]),
  ) as Record<GlobalBenchmarkMetricKey, number>
}

function globalMetricRoute(
  keys: GlobalBenchmarkMetricKey[],
  comparisons: Partial<Record<GlobalBenchmarkMetricKey, GlobalMetricComparison>>,
  eligibleKeys: ReadonlySet<GlobalBenchmarkMetricKey>,
  baselineWeights?: Partial<Record<GlobalBenchmarkMetricKey, number>>,
): GlobalMetricRoute {
  // 同一路线内优先按英雄全服基准的固有构成分权，缺失字段保留为中性。
  const routeKeys = keys.filter((key) => eligibleKeys.has(key))
  const weights = emptyGlobalMetricWeights()
  if (!routeKeys.length) return { eligible: false, measured: false, effect: 0, weights }

  const effects = routeKeys.map((key) => comparisons[key]?.allocationEffect || 0)
  const strengths = baselineWeights
    ? routeKeys.map((key) => Math.max(baselineWeights[key] || 0, 0))
    : effects.map((effect) => Math.max(effect + 1, 0))
  const strengthTotal = strengths.reduce((sum, value) => sum + value, 0)
  routeKeys.forEach((key, index) => {
    weights[key] = strengthTotal > 0
      ? strengths[index] / strengthTotal
      : 1 / routeKeys.length
  })

  return {
    eligible: true,
    measured: routeKeys.some((key) => Boolean(comparisons[key])),
    effect: routeKeys.reduce(
      (sum, key) => sum + weights[key] * (comparisons[key]?.allocationEffect || 0),
      0,
    ),
    weights,
  }
}

function combineGlobalRoutes(routes: GlobalMetricRoute[]): GlobalMetricRoute {
  const eligibleRoutes = routes.filter((route) => route.eligible)
  const weights = emptyGlobalMetricWeights()
  if (!eligibleRoutes.length) return { eligible: false, measured: false, effect: 0, weights }

  const strengths = eligibleRoutes.map((route) => Math.max(route.effect + 1, 0))
  const strengthTotal = strengths.reduce((sum, value) => sum + value, 0)
  const routeWeights = eligibleRoutes.map((_, index) => (
    strengthTotal > 0 ? strengths[index] / strengthTotal : 1 / eligibleRoutes.length
  ))

  eligibleRoutes.forEach((route, index) => {
    for (const key of globalMetricKeys()) {
      weights[key] += route.weights[key] * routeWeights[index]
    }
  })

  return {
    eligible: true,
    measured: eligibleRoutes.some((route) => route.measured),
    effect: eligibleRoutes.reduce(
      (sum, route, index) => sum + route.effect * routeWeights[index],
      0,
    ),
    weights,
  }
}

function addGlobalRouteWeight(
  weights: Record<GlobalBenchmarkMetricKey, number>,
  route: GlobalMetricRoute,
  routeWeight: number,
) {
  if (!route.eligible || routeWeight <= 0) return
  for (const key of globalMetricKeys()) {
    weights[key] += route.weights[key] * routeWeight
  }
}

function optionalGlobalRouteWeight(
  baseRoute: GlobalMetricRoute,
  optionalRoute: GlobalMetricRoute,
  maxWeight: number,
  advantageRange: number,
) {
  if (!optionalRoute.measured || optionalRoute.effect <= baseRoute.effect) return 0
  const qualityGate = clamp01((optionalRoute.effect + 1) / 2)
  const advantageGate = clamp01(
    (optionalRoute.effect - baseRoute.effect) / advantageRange,
  )
  return maxWeight * qualityGate * advantageGate
}

function dynamicGlobalRouteWeights(
  baseRoute: GlobalMetricRoute,
  optionalRoutes: Array<{
    route: GlobalMetricRoute
    maxWeight: number
    advantageRange: number
  }>,
  maxOptionalWeight = 1,
) {
  const weights = emptyGlobalMetricWeights()
  const desiredWeights = optionalRoutes.map((entry) => optionalGlobalRouteWeight(
    baseRoute,
    entry.route,
    entry.maxWeight,
    entry.advantageRange,
  ))
  const desiredTotal = desiredWeights.reduce((sum, value) => sum + value, 0)
  const scale = desiredTotal > maxOptionalWeight ? maxOptionalWeight / desiredTotal : 1
  const optionalTotal = desiredTotal * scale

  addGlobalRouteWeight(weights, baseRoute, 1 - optionalTotal)
  optionalRoutes.forEach((entry, index) => {
    addGlobalRouteWeight(weights, entry.route, desiredWeights[index] * scale)
  })
  return weights
}

function supportGlobalMetricWeights(
  comparisons: Partial<Record<GlobalBenchmarkMetricKey, GlobalMetricComparison>>,
  eligibleKeys: ReadonlySet<GlobalBenchmarkMetricKey>,
  baselineWeights: Partial<Record<GlobalBenchmarkMetricKey, number>>,
) {
  // 全服数据只进入辅助分支，并在可观测的辅助职责之间动态分配。
  const damage = globalMetricRoute(["damageToChampions"], comparisons, eligibleKeys)
  const durability = globalMetricRoute(
    ["damageTaken", "selfMitigatedDamage"],
    comparisons,
    eligibleKeys,
    baselineWeights,
  )
  const protection = globalMetricRoute(
    ["allyHealing", "allyShielding"],
    comparisons,
    eligibleKeys,
    baselineWeights,
  )
  const control = globalMetricRoute(
    ["crowdControlDurationSeconds"],
    comparisons,
    eligibleKeys,
  )

  const supportWork = combineGlobalRoutes([protection, durability])
  return dynamicGlobalRouteWeights(
    supportWork,
    [
      { route: damage, maxWeight: 0.35, advantageRange: 0.8 },
      { route: control, maxWeight: 0.35, advantageRange: 0.8 },
    ],
    0.35,
  )
}

function supportGlobalMetricKeys() {
  return new Set<GlobalBenchmarkMetricKey>(globalMetricKeys())
}

function cachedGlobalSupportBenchmark(
  baselines: ReadonlyMap<number, GlobalChampionBaseline>,
  context: RatingContext,
) {
  const cacheKey = baselines as object
  let contextCache = globalSupportBenchmarkCache.get(cacheKey)
  if (!contextCache) {
    contextCache = new WeakMap()
    globalSupportBenchmarkCache.set(cacheKey, contextCache)
  }

  const cached = contextCache.get(context)
  if (cached) return cached

  const benchmark = buildGlobalSupportBenchmark(baselines, context)
  contextCache.set(context, benchmark)
  return benchmark
}

function calculateOutputRatingUncached(game: RecentGame, context: RatingContext): OutputRating {
  const metrics = outputRatingMetrics(game)
  const role = analyzePlayerRole(game, context)
  const family = ratingFamily(role.role)
  const rawParts = calculateRatingParts(metrics, role, family)
  const rawTotal = Object.values(rawParts).reduce((sum, value) => sum + value, 0)
  const originalScore = clamp(rawTotal, 0, 100)
  const calibrated = calibratedMetrics(game, metrics, context)
  // 碾压 KDA 是胜方专属的奖励项。败方仍可使用全场占比校准和失败保护，
  // 但不能通过 KDA 重分配改变评分小项。
  const maxKdaWeight = calibrated.context.active && game.win
    ? stompKdaWeight(calibrated.context, family)
    : 0
  const originalCandidate = originalRatingCandidate(rawParts, originalScore)
  const selectedCandidate = calibrated.context.active
    ? game.win
      ? selectBestWinnerCandidate(
        game,
        metrics,
        calibrated.metrics,
        role,
        family,
        calibrated.context,
        originalCandidate,
        maxKdaWeight,
      )
      : evaluateContextCandidate(
        game,
        metrics,
        calibrated.metrics,
        role,
        family,
        calibrated.context,
        originalScore,
        maxKdaWeight,
      )
    : originalCandidate
  const finalScore = selectedCandidate.finalScore
  const score = Math.round(finalScore)
  const parts = { ...selectedCandidate.parts }
  const finalReconciliation = selectedCandidate.scoringApplied
    ? finalScore - selectedCandidate.contextualTotal
    : 0
  if (Math.abs(finalReconciliation) >= 0.005) {
    parts.matchContext = finalReconciliation
  }

  const contextAdjustment: MatchContextAdjustment = {
    active: calibrated.context.active,
    scoringApplied: selectedCandidate.scoringApplied,
    reason: calibrated.context.reason,
    severity: calibrated.context.severity,
    blendWeight: calibrated.context.blendWeight,
    imbalance: calibrated.context.imbalance,
    winnerKillDeathRatio: calibrated.context.winnerKillDeathRatio,
    loserKillDeathRatio: calibrated.context.loserKillDeathRatio,
    combatGap: calibrated.context.combatGap,
    damageGap: calibrated.context.damageGap,
    killDeathGateActive: calibrated.context.killDeathGateActive,
    damageGateActive: calibrated.context.damageGateActive,
    towerGateActive: calibrated.context.towerGateActive,
    winnerTowerKills: calibrated.context.winnerTowerKills,
    loserTowerKills: calibrated.context.loserTowerKills,
    loserFirstTowerTimeSeconds: calibrated.context.loserFirstTowerTimeSeconds,
    loserFirstTowerTimeRatio: calibrated.context.loserFirstTowerTimeRatio,
    towerKdaWeight: calibrated.context.towerKdaWeight,
    maxKdaWeight,
    kdaWeight: selectedCandidate.kdaWeight,
    kdaPoints: selectedCandidate.parts.kda || 0,
    originalScore,
    contextualScore: selectedCandidate.contextualScore,
    outcomeAdjustment: selectedCandidate.outcomeAdjustment,
    maxScoreDelta: selectedCandidate.maxScoreDelta,
    unprotectedScoreDelta: selectedCandidate.unprotectedScoreDelta,
    loserResistanceProtection: selectedCandidate.loserResistanceProtection,
    scoreDelta: selectedCandidate.scoreDelta,
  }

  return {
    score,
    label: outputRatingLabel(
      game,
      selectedCandidate.scoringApplied ? calibrated.metrics : metrics,
      score,
      role,
      family,
    ),
    level: outputRatingLevel(score),
    role,
    metrics,
    scoringMetrics: selectedCandidate.scoringApplied ? calibrated.metrics : metrics,
    parts,
    contextAdjustment,
  }
}

function originalRatingCandidate(
  parts: OutputRatingParts,
  originalScore: number,
): ContextRatingCandidate {
  return {
    scoringApplied: false,
    kdaWeight: 0,
    parts,
    contextualTotal: originalScore,
    contextualScore: originalScore,
    outcomeAdjustment: 0,
    maxScoreDelta: 0,
    unprotectedScoreDelta: 0,
    loserResistanceProtection: 0,
    scoreDelta: 0,
    finalScore: originalScore,
  }
}

function evaluateContextCandidate(
  game: RecentGame,
  rawMetrics: OutputRatingMetrics,
  scoringMetrics: OutputRatingMetrics,
  role: RoleAnalysis,
  family: RatingFamily,
  matchContext: StompMatchContext,
  originalScore: number,
  kdaWeight: number,
): ContextRatingCandidate {
  const parts = calculateRatingParts(scoringMetrics, role, family, kdaWeight)
  const contextualTotal = Object.values(parts).reduce((sum, value) => sum + value, 0)
  const outcomeAdjustment = stompOutcomeAdjustment(
    game,
    contextualTotal,
    scoringMetrics,
    matchContext,
  )
  const contextualScore = clamp(contextualTotal + outcomeAdjustment, 0, 100)
  const maxScoreDelta = Math.max(matchContext.severity * 25, Math.min(kdaWeight, 25))
  const unprotectedScoreDelta = clamp(
    contextualScore - originalScore,
    -maxScoreDelta,
    maxScoreDelta,
  )
  const loserResistanceProtection = !game.win && unprotectedScoreDelta < 0
    ? stompLoserResistanceProtection(rawMetrics)
    : 0
  const scoreDelta = unprotectedScoreDelta < 0
    ? unprotectedScoreDelta * (1 - loserResistanceProtection)
    : unprotectedScoreDelta
  const unclampedFinalScore = clamp(originalScore + scoreDelta, 0, 100)
  const finalScore = game.win
    ? unclampedFinalScore
    : Math.min(originalScore, unclampedFinalScore)

  return {
    scoringApplied: true,
    kdaWeight,
    parts,
    contextualTotal,
    contextualScore,
    outcomeAdjustment,
    maxScoreDelta,
    unprotectedScoreDelta,
    loserResistanceProtection,
    scoreDelta: finalScore - originalScore,
    finalScore,
  }
}

function selectBestWinnerCandidate(
  game: RecentGame,
  rawMetrics: OutputRatingMetrics,
  scoringMetrics: OutputRatingMetrics,
  role: RoleAnalysis,
  family: RatingFamily,
  matchContext: StompMatchContext,
  originalCandidate: ContextRatingCandidate,
  maxKdaWeight: number,
) {
  let best = originalCandidate
  const weights = stompKdaWeightCandidates(maxKdaWeight)

  for (const kdaWeight of weights) {
    const candidate = evaluateContextCandidate(
      game,
      rawMetrics,
      scoringMetrics,
      role,
      family,
      matchContext,
      originalCandidate.finalScore,
      kdaWeight,
    )

    // 同分时保留原始算法或更低权重，避免为了相同结果引入额外校准。
    if (candidate.finalScore > best.finalScore + 0.0001) {
      best = candidate
    }
  }

  return best
}

function stompKdaWeightCandidates(maxKdaWeight: number) {
  const safeMax = Math.max(0, maxKdaWeight)
  const weights = [0]

  for (let weight = 0.5; weight < safeMax; weight += 0.5) {
    weights.push(weight)
  }
  if (safeMax > 0) weights.push(safeMax)

  return weights
}

function stompKdaWeight(context: StompMatchContext, family: RatingFamily) {
  if (family === "frontline" || family === "support") return 0
  return context.towerKdaWeight
}

function stompKdaQuality(metrics: OutputRatingMetrics) {
  return clamp01((metrics.kda - 1) / 4)
}

function stompLoserResistanceProtection(metrics: OutputRatingMetrics) {
  if (metrics.damageShare < 0.27 || metrics.damageConversion < 1.2) return 0

  const damageProgress = clamp01((metrics.damageShare - 0.27) / 0.08)
  const conversionProgress = clamp01((metrics.damageConversion - 1.2) / 0.25)
  return Math.sqrt(damageProgress * conversionProgress)
}

function calibratedMetrics(
  game: RecentGame,
  metrics: OutputRatingMetrics,
  ratingContext: RatingContext,
) {
  const context = stompMatchContext(game, ratingContext)
  if (!context.active) return { metrics, context }

  const globalDamageShare = clamp01(
    ratio(game.damageToChampions, game.gameDamageToChampions) * context.participantScale,
  )
  const globalGoldShare = clamp01(
    ratio(game.goldEarned, game.gameGoldEarned) * context.participantScale,
  )
  const globalKillShare = clamp01(
    ratio(game.kills, game.gameKills) * context.participantScale,
  )
  const globalDeathShare = clamp01(
    ratio(game.deaths, game.gameDeaths) * context.participantScale,
  )
  const damageShare = mix(metrics.damageShare, globalDamageShare, context.blendWeight)
  const goldShare = mix(metrics.goldShare, globalGoldShare, context.blendWeight)
  const killShare = mix(metrics.killShare, globalKillShare, context.blendWeight)
  const deathShare = mix(metrics.deathShare, globalDeathShare, context.blendWeight)
  const killParticipation = clamp01(
    metrics.killParticipation * mix(1, context.teamKillStrength, context.blendWeight),
  )
  const damageConversion = goldShare > 0 ? damageShare / goldShare : 0
  const effectiveGoldShare = killAdjustedGoldShare(damageShare, goldShare, killShare)

  return {
    context,
    metrics: {
      ...metrics,
      damageShare,
      goldShare,
      killShare,
      killParticipation,
      deathShare,
      damageConversion,
      effectiveDamageConversion: effectiveGoldShare > 0 ? damageShare / effectiveGoldShare : 0,
    },
  }
}

interface StompCompositionRoleCounts {
  tank: number
  support: number
}

function stompCompositionCounts(game: RecentGame, context: RatingContext) {
  const teamEntries = game.teamComposition || []
  const gameEntries = game.gameComposition || []
  const empty = (): StompCompositionRoleCounts => ({ tank: 0, support: 0 })

  // 旧缓存缺少完整阵容时回退到基础 115% 门槛，避免只识别到单边阵容。
  if (!teamEntries.length || gameEntries.length < teamEntries.length) {
    return { team: empty(), opponent: empty() }
  }

  const countRoles = (entries: typeof teamEntries) => {
    const counts = empty()
    for (const entry of entries) {
      const role = analyzePlayerRole(
        {
          ...game,
          championId: entry.championId,
          itemIds: entry.itemIds,
        },
        context,
      ).role
      if (role === "tank") counts.tank += 1
      if (role === "support") counts.support += 1
    }
    return counts
  }

  const team = countRoles(teamEntries)
  const all = countRoles(gameEntries)
  return {
    team,
    opponent: {
      tank: Math.max(0, all.tank - team.tank),
      support: Math.max(0, all.support - team.support),
    },
  }
}

function stompMatchContext(game: RecentGame, ratingContext: RatingContext): StompMatchContext {
  const inactive = (
    reason: string,
    imbalance = 0,
    diagnostics: Partial<
      Pick<
        StompMatchContext,
        | "winnerKillDeathRatio"
        | "loserKillDeathRatio"
        | "combatGap"
        | "damageGap"
        | "killDeathGateActive"
        | "damageGateActive"
        | "towerGateActive"
        | "winnerTowerKills"
        | "loserTowerKills"
        | "loserFirstTowerTimeSeconds"
        | "loserFirstTowerTimeRatio"
        | "towerKdaWeight"
      >
    > = {},
  ): StompMatchContext => ({
    active: false,
    reason,
    severity: 0,
    blendWeight: 0,
    imbalance,
    winnerKillDeathRatio: diagnostics.winnerKillDeathRatio ?? 0,
    loserKillDeathRatio: diagnostics.loserKillDeathRatio ?? 0,
    combatGap: diagnostics.combatGap ?? 0,
    damageGap: diagnostics.damageGap ?? 0,
    killDeathGateActive: diagnostics.killDeathGateActive ?? false,
    damageGateActive: diagnostics.damageGateActive ?? false,
    towerGateActive: diagnostics.towerGateActive ?? false,
    winnerTowerKills: diagnostics.winnerTowerKills ?? 0,
    loserTowerKills: diagnostics.loserTowerKills ?? 0,
    loserFirstTowerTimeSeconds: diagnostics.loserFirstTowerTimeSeconds,
    loserFirstTowerTimeRatio: diagnostics.loserFirstTowerTimeRatio ?? 0,
    towerKdaWeight: diagnostics.towerKdaWeight ?? 0,
    participantScale: 1,
    teamKillStrength: 1,
  })

  const gamePlayerCount = Number(game.gamePlayerCount || 0)
  const teamPlayerCount = Number(game.teamPlayerCount || 0)
  if (gamePlayerCount < 2 || teamPlayerCount < 1) return inactive("缺少全场数据")
  if (gamePlayerCount !== teamPlayerCount * 2) return inactive("不是两支等人数队伍")
  if (
    Number(game.gameDamageToChampions || 0) <= Number(game.teamDamageToChampions || 0) ||
    Number(game.gameGoldEarned || 0) <= Number(game.teamGoldEarned || 0)
  ) return inactive("全场伤害或经济数据不完整")

  const gameMinutes = Number(game.gameDuration || 0) / 60
  if (!Number.isFinite(gameMinutes) || gameMinutes <= 0) return inactive("对局时长无效")

  const teamDamage = Number(game.teamDamageToChampions || 0)
  const gameDamage = Number(game.gameDamageToChampions || 0)
  const opponentDamage = gameDamage - teamDamage
  const teamGold = Number(game.teamGoldEarned || 0)
  const gameGold = Number(game.gameGoldEarned || 0)
  const opponentGold = gameGold - teamGold
  const teamKills = Number(game.teamKills || 0)
  const gameKills = Number(game.gameKills || 0)
  const opponentKills = Math.max(0, gameKills - teamKills)
  const teamDeaths = Number(game.teamDeaths || 0)
  const gameDeaths = Number(game.gameDeaths || 0)
  const opponentDeaths = Math.max(0, gameDeaths - teamDeaths)
  const teamTowerKills = Number(game.teamTowerKills || 0)
  const gameTowerKills = Number(game.gameTowerKills || 0)
  const opponentTowerKills = Math.max(0, gameTowerKills - teamTowerKills)

  if (opponentDamage <= 0 || opponentGold <= 0 || gameDeaths <= 0) {
    return inactive("对方数据不完整")
  }

  const winnerDamage = game.win ? teamDamage : opponentDamage
  const loserDamage = game.win ? opponentDamage : teamDamage
  const winnerGold = game.win ? teamGold : opponentGold
  const loserGold = game.win ? opponentGold : teamGold
  const winnerKills = game.win ? teamKills : opponentKills
  const loserKills = game.win ? opponentKills : teamKills
  const winnerDeaths = game.win ? teamDeaths : opponentDeaths
  const loserDeaths = game.win ? opponentDeaths : teamDeaths
  const winnerTowerKills = game.win ? teamTowerKills : opponentTowerKills
  const loserTowerKills = game.win ? opponentTowerKills : teamTowerKills
  const rawLoserFirstTowerTimeSeconds = Number(game.loserFirstTowerTimeSeconds)
  const gameDurationSeconds = Number(game.gameDuration || 0)
  const loserFirstTowerTimeSeconds =
    Number.isFinite(rawLoserFirstTowerTimeSeconds) &&
    rawLoserFirstTowerTimeSeconds > 0 &&
    rawLoserFirstTowerTimeSeconds <= gameDurationSeconds
      ? rawLoserFirstTowerTimeSeconds
      : undefined
  const loserFirstTowerTimeRatio = loserFirstTowerTimeSeconds
    ? clamp01(loserFirstTowerTimeSeconds / gameDurationSeconds)
    : 0
  const towerKdaWeight = loserTowerKills === 0
    ? 35
    : loserTowerKills === 1 && loserFirstTowerTimeSeconds
      ? 35 * clamp01((loserFirstTowerTimeRatio - 0.75) / 0.25)
      : 0
  const damageRatio = winnerDamage / Math.max(loserDamage, 1)
  const damageGap = damageRatio - 1
  const composition = stompCompositionCounts(game, ratingContext)
  const winnerComposition = game.win ? composition.team : composition.opponent
  const loserComposition = game.win ? composition.opponent : composition.team
  const damageThresholdRatio =
    1.15 -
    (winnerComposition.tank - loserComposition.tank) * 0.1 -
    (winnerComposition.support - loserComposition.support) * 0.05
  const goldGap = Math.abs(teamGold - opponentGold) / Math.max(gameGold, 1)
  const winnerKillDeathRatio = winnerKills / Math.max(winnerDeaths, 1)
  const loserKillDeathRatio = loserKills / Math.max(loserDeaths, 1)
  const combatGap = clamp01(
    (winnerKillDeathRatio - loserKillDeathRatio) /
      Math.max(winnerKillDeathRatio + loserKillDeathRatio, 0.001),
  )
  const killLead = winnerKills - loserKills
  const killDeathGateActive =
    killLead >= 6 &&
    winnerKillDeathRatio >= 1.3
  const damageGateActive = damageRatio >= damageThresholdRatio
  const towerGateActive = towerKdaWeight > 0
  const imbalance = Math.max(combatGap, damageGap, 0) * 0.78 + goldGap * 0.22
  const diagnostics = {
    winnerKillDeathRatio,
    loserKillDeathRatio,
    combatGap,
    damageGap,
    killDeathGateActive,
    damageGateActive,
    towerGateActive,
    winnerTowerKills,
    loserTowerKills,
    loserFirstTowerTimeSeconds,
    loserFirstTowerTimeRatio,
    towerKdaWeight,
  }

  if (loserTowerKills >= 2) {
    return inactive("败方已推掉至少2座防御塔，不属于碾压局", imbalance, diagnostics)
  }
  // 败方零塔是最明确的推塔压制证据：即使对局拖过16分钟，也允许该门控继续生效。
  // 一塔局仍受16分钟硬截止约束，避免把晚期僵持误判成短时碾压。
  const zeroTowerDurationOverride = loserTowerKills === 0 && towerGateActive
  if (gameMinutes > 16 && !zeroTowerDurationOverride) {
    return inactive("对局时长超过16分钟", imbalance, diagnostics)
  }
  if (!killDeathGateActive && !damageGateActive && !towerGateActive) {
    return inactive("K/D、伤害差和推塔时间均未达到门槛", imbalance, diagnostics)
  }

  const durationFactor = gameMinutes <= 12
    ? 1
    : gameMinutes <= 16
      ? mix(1, 10 / 35, (gameMinutes - 12) / 4)
      // 零塔例外在16分钟后继续衰减，避免长局获得与短局相同的全场混合强度。
      : (10 / 35) * (16 / gameMinutes)
  const combatStrength = killDeathGateActive
    ? mix(0.18, 1, smoothstepRange(winnerKillDeathRatio, 1.3, 3))
    : 0
  const damageStrength = damageGateActive
    ? mix(
      0.18,
      1,
      smoothstepRange(damageRatio, damageThresholdRatio, damageThresholdRatio + 0.2),
    )
    : 0
  const primaryStrength = Math.max(combatStrength, damageStrength)
  const winnerGoldGap = (winnerGold - loserGold) / Math.max(gameGold, 1)
  const goldAmplifier = mix(0.82, 1, smoothstepRange(winnerGoldGap, 0.03, 0.18))
  const severity = durationFactor * primaryStrength * goldAmplifier
  if (severity < 0.01 && !towerGateActive) {
    return inactive("短时碾压强度不足", imbalance, diagnostics)
  }

  const calibrationGates = [
    killDeathGateActive ? "K/D差" : "",
    damageGateActive ? "伤害差" : "",
  ].filter(Boolean)
  const towerGateReason = loserTowerKills === 0
    ? "败方零塔门控"
    : "败方首塔偏晚门控"
  const towerGateReasonWithDuration = zeroTowerDurationOverride && gameMinutes > 16
    ? `${towerGateReason}（超过16分钟零塔例外）`
    : towerGateReason
  const reason = calibrationGates.length
    ? `已触发${calibrationGates.join("、")}校准${towerGateActive ? `，同时命中${towerGateReasonWithDuration}` : ""}`
    : `${towerGateReasonWithDuration}命中，仅启用K/D评分`

  return {
    active: true,
    reason,
    severity,
    blendWeight: severity * 0.65,
    imbalance,
    winnerKillDeathRatio,
    loserKillDeathRatio,
    combatGap,
    damageGap,
    killDeathGateActive,
    damageGateActive,
    towerGateActive,
    winnerTowerKills,
    loserTowerKills,
    loserFirstTowerTimeSeconds,
    loserFirstTowerTimeRatio,
    towerKdaWeight,
    participantScale: gamePlayerCount / teamPlayerCount,
    teamKillStrength: clamp(Math.sqrt((teamKills + 2) / (opponentKills + 2)), 0.65, 1.25),
  }
}

function stompOutcomeAdjustment(
  game: RecentGame,
  contextualScore: number,
  metrics: OutputRatingMetrics,
  context: StompMatchContext,
) {
  if (game.win) {
    const contributionEvidence = clamp01((contextualScore - 45) / 20)
    return 3 * context.severity * contributionEvidence
  }

  const expectedDamageShare = 1 / Math.max(Number(game.teamPlayerCount || 5), 1)
  const damageResistance = clamp01(
    (metrics.damageShare - expectedDamageShare * 1.1) / (expectedDamageShare * 0.65),
  )
  const scoreResistance = clamp01((contextualScore - 65) / 20)
  const resistance = Math.max(damageResistance, scoreResistance)
  return -3 * context.severity * (1 - resistance)
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
  kdaWeight = 0,
): OutputRatingParts {
  if (family === "frontline") {
    const damageControl = frontlineDamageControlParts(metrics)

    return {
      mitigation: 30 * gatedMitigationQualityScore(metrics, 0.3, 1.1),
      effectiveDamage: damageControl.effectiveDamage,
      participation: 15 * clamp01((metrics.killParticipation - 0.42) / 0.38),
      deathControl: 10 * survivalScore(metrics, 0.28, 0.22),
      roleFit: clamp01((role.finalWeights.tank + role.finalWeights.fighter) / 0.65),
      efficiency: 9 * damageConversionScore(metrics, 0.65, 0.55),
      healing: 5 * clamp01(metrics.healingShare / 0.25),
      control: damageControl.control,
    }
  }

  if (family === "fighter") {
    const combat = dynamicCombatContributionParts(metrics, {
      pool: 79,
      maxMitigationWeight: 22,
      damageWeightRatio: 36 / 58,
      damageTarget: 0.3,
      efficiencyBase: 0.72,
      efficiencyRange: 0.63,
      mitigationTargetShare: 0.27,
      mitigationTargetPerDeath: 1.08,
      mitigationShareStart: 0.16,
      mitigationShareRange: 0.2,
      mitigationPerDeathStart: 0.65,
      mitigationPerDeathRange: 0.55,
      advantageRange: 0.35,
      maxControlWeight: dynamicControlMaxWeight(role),
      controlAdvantageRange: 0.45,
      reservedWeight: kdaWeight,
    })

    return {
      damage: combat.damage,
      efficiency: combat.efficiency,
      mitigation: combat.mitigation,
      ...dynamicControlPart(role, combat.control),
      ...(kdaWeight > 0 ? { kda: kdaWeight * stompKdaQuality(metrics) } : {}),
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
        damageWeightShift: 3,
        damageTarget: 0.32,
        efficiencyBase: 0.78,
        efficiencyRange: 0.62,
        mitigationTargetShare: 0.22,
        mitigationTargetPerDeath: 1.25,
        mitigationShareStart: 0.22,
        mitigationShareRange: 0.24,
        mitigationPerDeathStart: 0.9,
        mitigationPerDeathRange: 0.65,
        advantageRange: 0.45,
        maxControlWeight: 30,
        controlAdvantageRange: 0.45,
        reservedWeight: kdaWeight,
      },
      mageControlQualityScore(metrics),
    )

    return {
      damage: combat.damage,
      efficiency: combat.efficiency,
      ...dynamicControlPart(role, combat.control),
      ...(kdaWeight > 0 ? { kda: kdaWeight * stompKdaQuality(metrics) } : {}),
      participation: 14 * clamp01((metrics.killParticipation - 0.45) / 0.35),
      killQuality: 9 * killQualityScore(metrics),
      survival: 8 * survivalScore(metrics, 0.2, 0.18),
      economy: 6 * damageConversionScore(metrics, 0.75, 0.65),
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
    damageWeightShift: 3,
    damageTarget: 0.32,
    efficiencyBase: 0.78,
    efficiencyRange: 0.62,
    mitigationTargetShare: 0.22,
    mitigationTargetPerDeath: 1.25,
    mitigationShareStart: 0.22,
    mitigationShareRange: 0.24,
    mitigationPerDeathStart: 0.9,
    mitigationPerDeathRange: 0.65,
    advantageRange: 0.45,
    maxControlWeight: dynamicControlMaxWeight(role),
    controlAdvantageRange: 0.45,
    reservedWeight: kdaWeight,
  })

  return {
    damage: combat.damage,
    efficiency: combat.efficiency,
    ...(kdaWeight > 0 ? { kda: kdaWeight * stompKdaQuality(metrics) } : {}),
    participation: 8 * clamp01((metrics.killParticipation - 0.45) / 0.35),
    killQuality: 9 * killQualityScore(metrics),
    survival: 8 * survivalScore(metrics, 0.2, 0.18),
    mitigation: combat.mitigation,
    ...dynamicControlPart(role, combat.control),
    economy: 6 * damageConversionScore(metrics, 0.75, 0.65),
  }
}

interface DynamicCombatConfig {
  pool: number
  maxMitigationWeight: number
  damageWeightRatio: number
  damageWeightShift?: number
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
  reservedWeight?: number
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
  const reservedWeight = clamp(config.reservedWeight || 0, 0, config.pool)
  const allocatablePool = config.pool - reservedWeight
  const comparisonDamageRatio = clamp01(
    config.damageWeightRatio + (config.damageWeightShift || 0) / config.pool,
  )
  const nonMitigationScore =
    damageScore * comparisonDamageRatio + efficiencyScore * (1 - comparisonDamageRatio)
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

  const specialWeight = Math.min(allocatablePool, mitigationWeight + controlWeight)
  if (specialWeight < mitigationWeight + controlWeight && mitigationWeight + controlWeight > 0) {
    const scale = specialWeight / (mitigationWeight + controlWeight)
    mitigationWeight *= scale
    controlWeight *= scale
  }

  const baseWeight = allocatablePool - mitigationWeight - controlWeight
  const damageWeightShift = Math.min(
    config.damageWeightShift || 0,
    baseWeight * (1 - config.damageWeightRatio),
  )
  const damageWeight = baseWeight * config.damageWeightRatio + damageWeightShift
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

  // 死亡只用于修正虚高伤转，不应抹掉真正打出高伤害的对局。
  // 正常局使用队内伤害占比；碾压局使用校准后的有效伤害占比。
  const deathPressure = clamp01((metrics.deathShare - 0.2) / 0.12)
  const deathDamageGapRisk = clamp01((metrics.deathShare - metrics.damageShare - 0.02) / 0.12)
  const feedRisk = Math.max(deathPressure, deathDamageGapRisk)
  const protectedFeedRisk = feedRisk * (1 - damageDeathProtection(metrics))
  const retain = clamp(1 - protectedFeedRisk * 0.65, 0.6, 1)

  return rawScore * retain
}

function survivalScore(metrics: OutputRatingMetrics, start: number, range: number) {
  const rawPenalty = clamp01((metrics.deathShare - start) / range)
  const protectedPenalty = rawPenalty * (1 - damageDeathProtection(metrics))
  return 1 - protectedPenalty
}

function damageDeathProtection(metrics: OutputRatingMetrics) {
  return clamp01((metrics.damageShare - 0.2) / 0.1)
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
  if (score >= 65) return positiveRatingLabel(game, metrics, score, role, family)

  const controlLabel = controlRatingLabel(metrics)
  if (score >= 40 && controlLabel) return controlLabel

  if (
    game.kda >= 3 &&
    (metrics.damageShare <= 0.2 || metrics.effectiveDamageConversion <= 0.95) &&
    metrics.killParticipation < 0.68
  ) {
    return metrics.deathShare <= 0.18 ? "美美隐身" : "保KDA混子"
  }

  if (
    metrics.killShare - metrics.damageShare >= 0.08 &&
    metrics.damageShare < 0.24 &&
    metrics.damageConversion < 1.05
  ) {
    return "k头狗"
  }

  if (metrics.goldShare >= 0.24 && metrics.effectiveDamageConversion < 1) return "拿钱不干事"

  if (metrics.deathShare >= 0.3 && metrics.damageShare >= 0.24 && score < 65) {
    return "自爆达人"
  }

  if (metrics.damageShare <= 0.18 && metrics.effectiveDamageConversion <= 0.9) {
    return metrics.killParticipation < 0.5 ? "开游戏的" : "毫无存在感"
  }

  if (metrics.killParticipation <= 0.5 && metrics.damageShare <= 0.22 && score < 65) {
    return metrics.deathShare <= 0.18 ? "美美隐身" : "开游戏的"
  }

  return score >= 60 ? "毫无存在感" : "开游戏的"
}

function positiveRatingLabel(
  game: RecentGame,
  metrics: OutputRatingMetrics,
  score: number,
  role: RoleAnalysis,
  family: RatingFamily,
) {
  const controlLabel = controlRatingLabel(metrics)

  if (family === "frontline") {
    if (
      metrics.mitigationShare >= 0.3 &&
      metrics.mitigationPerDeath >= 3 &&
      metrics.killParticipation >= 0.68
    ) {
      return "叹息之墙"
    }
    if (
      metrics.mitigationShare >= 0.3 &&
      metrics.mitigationPerDeath >= 1.05 &&
      metrics.killParticipation >= 0.68
    ) {
      return "faker加里奥"
    }
    if (metrics.mitigationShare >= 0.27 && metrics.damageShare >= 0.22) return "半肉战神"
    if (metrics.mitigationShare >= 0.27) return "哪来的城墙"
    if (controlLabel) return controlLabel
    return "顶级前锋"
  }

  if (family === "fighter") {
    if (metrics.damageShare >= 0.35 && metrics.effectiveDamageConversion >= 1.5) {
      return "大魔王"
    }
    if (
      metrics.damageShare >= 0.32 &&
      metrics.effectiveDamageConversion > 1.3 &&
      metrics.killParticipation >= 0.68
    ) {
      return "恐怖利刃"
    }
    if (
      game.kda >= 4 &&
      metrics.damageShare >= 0.26 &&
      metrics.effectiveDamageConversion >= 1.12 &&
      metrics.deathShare <= 0.18
    ) {
      return "我chovy!"
    }
    if (
      metrics.damageShare >= 0.25 &&
      metrics.mitigationShare >= 0.25 &&
      metrics.effectiveDamageConversion >= 0.95
    ) {
      return "半肉战神"
    }
    if (
      metrics.damageShare >= 0.27 &&
      metrics.effectiveDamageConversion >= 1.1 &&
      metrics.deathShare >= 0.24
    ) {
      return "刀尖舔血"
    }
    if (metrics.damageShare >= 0.25 && metrics.killParticipation >= 0.68) {
      return score >= 80 ? "最强前锋" : "冲阵好手"
    }
    if (controlLabel) return controlLabel
    return score >= 80 ? "最强前锋" : "冲阵好手"
  }

  if (family === "support") {
    if (metrics.goldShare <= 0.18 && metrics.killParticipation >= 0.7) return "吃草挤奶"
    if (metrics.damageShare >= 0.24 && metrics.effectiveDamageConversion >= 1.05) return "核心大C"
    if (controlLabel) return controlLabel
    return score >= 80 ? "团队发动机" : "功能担当"
  }

  if (metrics.damageShare >= 0.35 && metrics.effectiveDamageConversion >= 1.5) {
    return "大魔王"
  }

  if (
    metrics.damageShare >= 0.32 &&
    metrics.effectiveDamageConversion > 1.3 &&
    metrics.killParticipation >= 0.68
  ) {
    return "爆炸核弹"
  }

  if (
    game.kda >= 4 &&
    metrics.damageShare >= 0.26 &&
    metrics.effectiveDamageConversion >= 1.12 &&
    metrics.deathShare <= 0.18
  ) {
    return "我chovy!"
  }

  if (
    (role.role === "fighterAssassin" || role.role === "pureAssassin") &&
    metrics.killShare >= 0.28 &&
    metrics.effectiveDamageConversion >= 1 &&
    metrics.effectiveDamageConversion <= 1.2
  ) {
    return "无情收割者"
  }

  if (metrics.goldShare <= 0.19 && metrics.damageShare >= 0.25 && metrics.effectiveDamageConversion >= 1.15) {
    return "吃草挤奶"
  }

  if (metrics.damageShare >= 0.28 && metrics.effectiveDamageConversion >= 1.12 && game.kda >= 3) {
    return "无解主c"
  }

  if (metrics.damageShare >= 0.28 && metrics.effectiveDamageConversion >= 1.05) {
    return "核心大C"
  }

  if (metrics.damageShare >= 0.29 && metrics.killParticipation >= 0.68) return "全场火力点"

  if (
    metrics.damageShare >= 0.27 &&
    metrics.effectiveDamageConversion >= 1.08 &&
    metrics.deathShare >= 0.24
  ) {
    return "浴血奋战"
  }

  if (controlLabel) return controlLabel

  return score >= 80 ? "输出机器" : "稳定火力"
}

function outputRatingLevel(score: number): OutputRating["level"] {
  if (score >= 80) return "excellent"
  if (score >= 65) return "good"
  if (score >= 40) return "average"
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
  globalBaseline: "全服得分",
  kda: "碾压KDA",
  matchContext: "碾压校准",
}

function ratingPartLines(parts: OutputRatingParts) {
  const entries = Object.entries(parts).filter(
    ([key, value]) => key === "globalBaseline" || Math.abs(value) >= 0.005,
  )
  if (!entries.length) return ["评分小项 暂无"]

  const total = entries.reduce((sum, [, value]) => sum + value, 0)
  return [
    `评分小项 合计 ${fixed(total, 1)}`,
    ...entries.map(([key, value]) => key === "globalBaseline"
      ? `全服得分 ${fixed(value, 1)}/${GLOBAL_SCORE_SUPPORT_MAX}分`
      : `${RATING_PART_LABELS[key] || key} ${fixed(value, 1)}`),
  ]
}

function outputRatingDetailLines(game: RecentGame, rating: OutputRating) {
  const m = rating.metrics
  return [
    `得分 ${rating.score} · ${scoreEvaluationLabel(rating.score)} · ${rating.label}`,
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
    ...matchContextDetailLines(rating.contextAdjustment),
  ]
}

function matchContextDetailLines(adjustment: MatchContextAdjustment) {
  if (!adjustment.active) return []

  const towerProgress = adjustment.loserTowerKills === 0
    ? "败方未推塔"
    : adjustment.loserFirstTowerTimeSeconds
      ? `败方首塔 ${fixed(adjustment.loserFirstTowerTimeSeconds / 60, 1)} 分钟`
        + ` · 对局进度 ${Math.round(adjustment.loserFirstTowerTimeRatio * 100)}%`
      : `败方推塔 ${adjustment.loserTowerKills} 座 · 首塔时间缺失`

  return [
    `碾压校准 ${adjustment.reason}`,
    towerProgress,
    `KDA可借权上限 ${fixed(adjustment.maxKdaWeight, 1)}分`
      + ` · 实际采用 ${fixed(adjustment.kdaWeight, 1)}分`,
  ]
}

export function outputRatingTitle(
  game: RecentGame,
  context: RatingContext = DEFAULT_RATING_CONTEXT,
) {
  return outputRatingDetailLines(game, calculateOutputRating(game, context)).join("\n")
}

export function globalRatingTitle(
  game: RecentGame,
  context: RatingContext = DEFAULT_RATING_CONTEXT,
  baselines?: ReadonlyMap<number, GlobalChampionBaseline>,
  _datasetVersion = "",
) {
  const rating = calculateGlobalRating(game, context, baselines)
  const officialRating = officialRatingFromGlobal(game, rating)
  if (!rating.available) {
    const fallbackLines = outputRatingDetailLines(game, officialRating)
    if (ratingFamily(rating.baseRating.role.role) !== "support") {
      return fallbackLines.join("\n")
    }
    return [
      ...fallbackLines,
      "全服得分 暂不可用",
    ].join("\n")
  }

  return outputRatingDetailLines(game, officialRating).join("\n")
}
