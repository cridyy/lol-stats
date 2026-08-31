import {
  calculateGlobalRating,
  calculateOfficialRating,
  type GlobalRating,
  type OutputRating,
  type OutputRatingMetrics,
} from "./scoring"
import type { RatingContext, PlayerRole } from "./roleClassifier"
import type {
  GlobalBaselineMetricKey,
  GlobalChampionBaseline,
  RecentGame,
  RecentGameParticipantRecord,
} from "./types"
import { mitigationValue } from "./utils"

/** 称号稀有度只影响展示池抽取概率，不影响是否命中。 */
export type GameTitleTier = "神话" | "高光" | "特征" | "普通" | "调侃"
export type GameTitleCategory = "通用" | "定位" | "英雄" | "对局"

export interface GameTitleDetail {
  name: string
  tier: GameTitleTier
  category: GameTitleCategory
  /** 给接口分析用的简短理由，界面默认只展示称号名称。 */
  reason: string
}

export interface GameTitleSet {
  /** 当前会显示的一个称号；没有命中专属称号时回退到评分标签。 */
  displayTitle: string
  /** 当前对局命中的全部专属称号，顺序稳定，便于悬浮窗展示。 */
  titles: string[]
  details: GameTitleDetail[]
  /** 伤害第一、队内第一等事实标签，与娱乐称号分开。 */
  factTags: string[]
}

export interface TitleMatchComparison {
  participantCount?: number
  teamCount?: number
  teammateCount?: number
  opponentCount?: number
  ownScore?: number
  teamAverageScore?: number | null
  teammatesAverageScore?: number | null
  opponentAverageScore?: number | null
  teamScoreRank?: number | null
  gameScoreRank?: number | null
  teamScoreLeader?: boolean
  gameScoreLeader?: boolean
  teamScoreGapToTeammates?: number | null
  scoreGapToOpponents?: number | null
  teammateLowestScore?: number | null
  teammateHighestScore?: number | null
  opponentLowestScore?: number | null
  opponentHighestScore?: number | null
}

export interface GameTitleOptions {
  ratingContext?: RatingContext
  globalBaselines?: ReadonlyMap<number, GlobalChampionBaseline>
  comparison?: TitleMatchComparison | null
}

interface TitleFacts {
  game: RecentGame
  rating: OutputRating
  globalRating: GlobalRating
  metrics: OutputRatingMetrics
  role: PlayerRole
  family: "carry" | "fighter" | "frontline" | "mage" | "support"
  score: number
  gameMinutes: number
  damagePerMinute: number
  mitigationPerMinute: number
  healingPerMinute: number
  shieldingPerMinute: number
  weakSignals: number
  gameDamageLeader: boolean
  teamDamageLeader: boolean
  comparison: TitleMatchComparison | null
  matchAvailable: boolean
  winnerTowerKills: number
  loserTowerKills: number
  firstTowerTimeRatio: number
  globalRatios: Partial<Record<GlobalBaselineMetricKey, number>>
  protectionRatio: number
  allyCarryCount: number
  allyMageOrAdcCount: number
}

interface TitleRule {
  name: string
  tier: GameTitleTier
  category: GameTitleCategory
  test: (facts: TitleFacts) => boolean
  reason: (facts: TitleFacts) => string
}

const DEFAULT_RATING_CONTEXT: RatingContext = {}

function randomTitleSeed() {
  const cryptoApi = globalThis.crypto
  if (cryptoApi?.getRandomValues) {
    const values = new Uint32Array(1)
    cryptoApi.getRandomValues(values)
    return values[0] >>> 0
  }
  return Math.floor(Math.random() * 0x1_0000_0000) >>> 0
}

/** 当前响应/刷新使用的称号随机种子；重开软件时模块会重新生成。 */
let titleSelectionSeed = randomTitleSeed()

/**
 * 让一次新的战绩刷新重新抽取展示称号。
 *
 * 具体命中的全部称号不变，只改变从称号池中挑出的那一个显示名。
 */
export function refreshTitleSelectionSeed() {
  titleSelectionSeed = randomTitleSeed()
}

const ROLE_FIGHTERS = new Set<PlayerRole>(["fighter", "bruiser", "fighterAssassin"])
const ROLE_RANGED = new Set<PlayerRole>(["adc", "mage"])

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value)
}

function numberOr(value: unknown, fallback = 0) {
  return finite(Number(value)) ? Number(value) : fallback
}

function atMost(value: number | null | undefined, limit: number) {
  return finite(value) && value <= limit
}

function atLeast(value: number | null | undefined, limit: number) {
  return finite(value) && value >= limit
}

function average(values: number[]) {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null
}

function gameMinutes(game: RecentGame) {
  return Math.max(1, numberOr(game.gameDuration) / 60)
}

function weakSignalCount(metrics: OutputRatingMetrics) {
  return [
    metrics.damageShare <= 0.16,
    metrics.effectiveDamageConversion <= 0.8,
    metrics.killParticipation <= 0.52,
    metrics.deathShare >= 0.29,
  ].filter(Boolean).length
}

function isFighter(role: PlayerRole) {
  return ROLE_FIGHTERS.has(role)
}

function isRanged(role: PlayerRole) {
  return ROLE_RANGED.has(role)
}

function isCarryLike(role: PlayerRole) {
  return isRanged(role) || isFighter(role) || role === "pureAssassin" || role === "assassin"
}

function isTank(role: PlayerRole) {
  return role === "tank"
}

function isSupport(role: PlayerRole) {
  return role === "support"
}

function titleRoleFamily(role: PlayerRole): TitleFacts["family"] {
  if (role === "tank") return "frontline"
  if (role === "mage") return "mage"
  if (isFighter(role)) return "fighter"
  if (role === "support") return "support"
  return "carry"
}

function globalMetricRatio(globalRating: GlobalRating, key: GlobalBaselineMetricKey) {
  const metric = globalRating.metrics.find((entry) => entry.key === key)
  return metric && finite(metric.ratio) ? metric.ratio : undefined
}

function durationMultiplier(seconds: number) {
  const minutes = seconds / 60
  if (minutes <= 8) return 0.6
  if (minutes <= 16) return 0.6 + ((minutes - 8) / 8) * 0.4
  if (minutes <= 20) return 1 + ((minutes - 16) / 4) * 0.1
  return 1.1
}

function genericBaselineRatio(
  game: RecentGame,
  baseline: GlobalChampionBaseline | undefined,
  field: "damageToChampions" | "damageTaken" | "selfMitigatedDamage" | "allyHealing" | "allyShielding" | "crowdControlDurationSeconds",
  actual: number | undefined,
) {
  if (!baseline || !finite(actual) || actual < 0) return undefined
  const base = numberOr(baseline[field])
  const baseMinutes = numberOr(baseline.gameDurationSeconds) / 60
  const minutes = gameMinutes(game)
  if (base <= 0 || baseMinutes <= 0) return undefined
  const expectedPerMinute = (base / baseMinutes) * durationMultiplier(numberOr(game.gameDuration))
  return expectedPerMinute > 0 ? (actual / minutes) / expectedPerMinute : undefined
}

function sameParticipant(left: RecentGame | RecentGameParticipantRecord, right: RecentGame) {
  const leftPuuid = String((left as RecentGameParticipantRecord).puuid || "").trim().toLowerCase()
  const rightPuuid = String((right as RecentGameParticipantRecord).puuid || "").trim().toLowerCase()
  if (leftPuuid && rightPuuid) return leftPuuid === rightPuuid
  const leftId = numberOr((left as RecentGameParticipantRecord).participantId)
  const rightId = numberOr((right as RecentGameParticipantRecord).participantId)
  return leftId > 0 && rightId > 0 && leftId === rightId
}

function participantTeamKey(game: Pick<RecentGame, "teamId" | "playerSubteamId">) {
  const subteam = numberOr(game.playerSubteamId)
  return subteam > 0 ? `subteam:${subteam}` : `team:${numberOr(game.teamId)}`
}

function deriveComparison(
  game: RecentGame,
  context: RatingContext,
  baselines: ReadonlyMap<number, GlobalChampionBaseline> | undefined,
): TitleMatchComparison | null {
  const records = (game.matchParticipants || []).filter(Boolean)
  if (records.length < 2) return null

  const entries = records.map((record) => ({
    record,
    rating: calculateOfficialRating(record, context, baselines),
    teamKey: participantTeamKey(record),
  }))
  const owner = entries.find((entry) => entry.record.isOwner)
    || entries.find((entry) => sameParticipant(entry.record, game))
  if (!owner) return null

  const ownTeam = entries.filter((entry) => entry.teamKey === owner.teamKey)
  const teammates = ownTeam.filter((entry) => entry !== owner)
  const opponents = entries.filter((entry) => entry.teamKey !== owner.teamKey)
  if (!teammates.length || !opponents.length) return null

  const ownScore = owner.rating.score
  const scores = entries.map((entry) => entry.rating.score)
  const teamScores = ownTeam.map((entry) => entry.rating.score)
  const rank = (value: number, values: number[]) => 1 + values.filter((candidate) => candidate > value + 0.0001).length
  return {
    participantCount: entries.length,
    teamCount: ownTeam.length,
    teammateCount: teammates.length,
    opponentCount: opponents.length,
    ownScore,
    teamAverageScore: average(teamScores),
    teammatesAverageScore: average(teammates.map((entry) => entry.rating.score)),
    opponentAverageScore: average(opponents.map((entry) => entry.rating.score)),
    teamScoreRank: rank(ownScore, teamScores),
    gameScoreRank: rank(ownScore, scores),
    teamScoreLeader: ownScore >= Math.max(...teamScores),
    gameScoreLeader: ownScore >= Math.max(...scores),
    teamScoreGapToTeammates: ownScore - (average(teammates.map((entry) => entry.rating.score)) || 0),
    scoreGapToOpponents: ownScore - (average(opponents.map((entry) => entry.rating.score)) || 0),
    teammateLowestScore: Math.min(...teammates.map((entry) => entry.rating.score)),
    teammateHighestScore: Math.max(...teammates.map((entry) => entry.rating.score)),
    opponentLowestScore: Math.min(...opponents.map((entry) => entry.rating.score)),
    opponentHighestScore: Math.max(...opponents.map((entry) => entry.rating.score)),
  }
}

function deriveAllyComposition(
  game: RecentGame,
  context: RatingContext,
  baselines: ReadonlyMap<number, GlobalChampionBaseline> | undefined,
) {
  const records = (game.matchParticipants || []).filter(Boolean)
  const owner = records.find((record) => record.isOwner) || records.find((record) => sameParticipant(record, game))
  if (!owner) return { allyCarryCount: 0, allyMageOrAdcCount: 0 }
  const ownerTeam = participantTeamKey(owner)
  const allies = records.filter((record) => participantTeamKey(record) === ownerTeam && !sameParticipant(record, game))
  let allyCarryCount = 0
  let allyMageOrAdcCount = 0
  for (const ally of allies) {
    const role = calculateOfficialRating(ally, context, baselines).role.role
    if (isCarryLike(role)) allyCarryCount += 1
    if (role === "mage" || role === "adc") allyMageOrAdcCount += 1
  }
  return { allyCarryCount, allyMageOrAdcCount }
}

function buildFacts(
  game: RecentGame,
  rating: OutputRating,
  options: GameTitleOptions,
  suppliedComparison?: TitleMatchComparison | null,
): TitleFacts {
  const context = options.ratingContext || DEFAULT_RATING_CONTEXT
  const baselines = options.globalBaselines
  const globalRating = calculateGlobalRating(game, context, baselines)
  const metrics = rating.metrics
  const minutes = gameMinutes(game)
  const baseline = baselines?.get(game.championId)
  const genericRatios: Partial<Record<GlobalBaselineMetricKey, number>> = {
    damageToChampions: genericBaselineRatio(game, baseline, "damageToChampions", numberOr(game.damageToChampions)),
    damageTaken: genericBaselineRatio(game, baseline, "damageTaken", numberOr(game.totalDamageTaken)),
    selfMitigatedDamage: genericBaselineRatio(game, baseline, "selfMitigatedDamage", numberOr(game.damageSelfMitigated)),
    allyHealing: genericBaselineRatio(game, baseline, "allyHealing", game.totalHealsOnTeammates),
    allyShielding: genericBaselineRatio(game, baseline, "allyShielding", numberOr(game.totalDamageShieldedOnTeammates)),
    crowdControlDurationSeconds: genericBaselineRatio(game, baseline, "crowdControlDurationSeconds", game.timeCCingOthers),
  }
  for (const key of Object.keys(genericRatios) as GlobalBaselineMetricKey[]) {
    genericRatios[key] = globalMetricRatio(globalRating, key) ?? genericRatios[key]
  }
  const comparison = suppliedComparison === undefined
    ? (options.comparison === undefined ? deriveComparison(game, context, baselines) : options.comparison || null)
    : suppliedComparison || null
  const composition = deriveAllyComposition(game, context, baselines)
  const healingRatio = genericRatios.allyHealing || 0
  const shieldingRatio = genericRatios.allyShielding || 0

  return {
    game,
    rating,
    globalRating,
    metrics,
    role: rating.role.role,
    family: titleRoleFamily(rating.role.role),
    score: rating.score,
    gameMinutes: minutes,
    damagePerMinute: numberOr(game.damageToChampions) / minutes,
    mitigationPerMinute: mitigationValue(game) / minutes,
    healingPerMinute: numberOr(game.totalHealsOnTeammates) / minutes,
    shieldingPerMinute: numberOr(game.totalDamageShieldedOnTeammates) / minutes,
    weakSignals: weakSignalCount(metrics),
    gameDamageLeader: Boolean(game.gameDamageLeader),
    teamDamageLeader: Boolean(game.teamDamageLeader),
    comparison,
    matchAvailable: Boolean(comparison && (comparison.participantCount || 0) >= 2 && (comparison.teammateCount || 0) > 0),
    winnerTowerKills: numberOr(rating.contextAdjustment.winnerTowerKills),
    loserTowerKills: numberOr(rating.contextAdjustment.loserTowerKills),
    firstTowerTimeRatio: numberOr(rating.contextAdjustment.loserFirstTowerTimeRatio),
    globalRatios: genericRatios,
    protectionRatio: Math.max(healingRatio, shieldingRatio),
    allyCarryCount: composition.allyCarryCount,
    allyMageOrAdcCount: composition.allyMageOrAdcCount,
  }
}

function championAlias(facts: TitleFacts) {
  return String(facts.game.championId || "")
}

function hasMetric(facts: TitleFacts, key: GlobalBaselineMetricKey, minimum: number) {
  return atLeast(facts.globalRatios[key], minimum)
}

function rule(
  name: string,
  tier: GameTitleTier,
  category: GameTitleCategory,
  test: (facts: TitleFacts) => boolean,
  reason: string | ((facts: TitleFacts) => string),
): TitleRule {
  return {
    name,
    tier,
    category,
    test,
    reason: typeof reason === "function" ? reason : () => reason,
  }
}

/**
 * 规则故意分成“普通可见、强表现、极少数神话、负面调侃”四层。
 * 普通玩家的称号不依赖 95/100 分或 45 分队友均值这类极端条件；相对比较只在
 * 能识别完整十人数据时启用，旧缓存不会被缺失值误触发。
 */
const TITLE_RULES: TitleRule[] = [
  // 通用：先保证普通玩家也有可见的评价。
  rule("显示器开了吗", "调侃", "通用", (f) => f.score <= 45 && f.weakSignals >= 2, "评分较低且至少两项核心表现偏弱"),
  rule("卧底找到了", "调侃", "对局", (f) => f.game.win && f.score <= 50 && f.weakSignals >= 2
    && (!f.matchAvailable || (atLeast(f.comparison?.teamScoreRank, 4) && atMost(f.comparison?.teamScoreGapToTeammates, -6))), "赢了但位于队伍后段"),
  rule("少冰七分糖", "普通", "通用", (f) => f.score >= 68 && f.score <= 80 && f.weakSignals <= 1
    && f.metrics.killParticipation >= 0.55 && f.metrics.killParticipation <= 0.82
    && !f.gameDamageLeader && !f.teamDamageLeader, "表现稳定，既不离谱也不抢镜"),
  rule("一般人机", "普通", "通用", (f) => f.score >= 55 && f.score <= 72 && f.weakSignals <= 1
    && f.metrics.killParticipation >= 0.5 && f.metrics.killParticipation <= 0.8
    && !f.gameDamageLeader && !f.teamDamageLeader, "大多数指标处在正常区间"),
  rule("小泯一把", "普通", "对局", (f) => f.game.win && f.score >= 50 && f.score <= 72
    && !f.gameDamageLeader && !f.teamDamageLeader
    // teamScoreRank 是 1 到 5，排名 3 以后才表示“没有成为队伍核心”。
    && (!f.matchAvailable || atLeast(f.comparison?.teamScoreRank, 3)), "赢下对局但没有成为队伍核心"),
  rule("悲情英雄", "高光", "对局", (f) => !f.game.win && f.matchAvailable && f.score >= 65
    && (atMost(f.comparison?.teamScoreRank, 2) || atLeast(f.comparison?.teamScoreGapToTeammates, 8))
    && (atMost(f.comparison?.teammatesAverageScore, 62) || atLeast(f.comparison?.scoreGapToOpponents, 8)), "败方仍是队内前列，队友或对手差距明显"),
  rule("女武神", "高光", "对局", (f) => !f.game.win && f.matchAvailable
    && f.score >= 68 && atMost(f.comparison?.teamScoreRank, 1)
    && (atLeast(f.comparison?.teamScoreGapToTeammates, 5) || atLeast(f.comparison?.scoreGapToOpponents, 5)), "败方队内第一且个人表现明显高于队友"),
  rule("渡劫失败", "高光", "对局", (f) => !f.game.win && f.matchAvailable
    && f.score >= 65
    // 与“女武神”错开：这里强调队内第二、第三名也尽力了，但队友整体掉队。
    && atLeast(f.comparison?.teamScoreRank, 2)
    && atMost(f.comparison?.teamScoreRank, 3)
    && (atMost(f.comparison?.teammatesAverageScore, 55)
      || atLeast(f.comparison?.teamScoreGapToTeammates, 15)), "队友整体掉队，自己已经尽力但仍然落败"),
  rule("本质高手", "高光", "通用", (f) => f.score >= 85
    && (!f.matchAvailable || atMost(f.comparison?.gameScoreRank, 3) || atMost(f.comparison?.teamScoreRank, 2))
    && (f.metrics.damageShare >= 0.28 || f.metrics.mitigationShare >= 0.32 || f.metrics.killParticipation >= 0.78 || f.protectionRatio >= 1.2), "高分且在至少一个关键维度领先"),
  rule("已击败全服1%的对手", "调侃", "通用", (f) => f.score <= 40 && f.weakSignals >= 2
    && ["damageToChampions", "damageTaken", "selfMitigatedDamage", "allyHealing", "allyShielding", "crowdControlDurationSeconds"]
      .filter((key) => atMost(f.globalRatios[key as GlobalBaselineMetricKey], 0.7)).length >= 2, "相对英雄全服均值的核心表现明显偏低（娱乐称号）"),
  rule("很难想象屏幕前是人类", "调侃", "通用", (f) => f.score <= 35 && f.weakSignals >= 3, "评分很低且多项核心指标同时失守"),
  rule("不是哥们", "调侃", "通用", (f) => f.score <= 52 && (
    (f.metrics.damageShare >= 0.28 && f.metrics.effectiveDamageConversion <= 0.92)
    || (f.metrics.killParticipation >= 0.78 && f.metrics.damageShare <= 0.16)
    || (f.metrics.mitigationShare >= 0.35 && f.metrics.damageShare <= 0.16)
  ), "资源、击杀或承伤与实际产出明显错位"),
  rule("泉水指挥官", "调侃", "通用", (f) => numberOr(f.game.deaths) >= 14
    && f.metrics.deathShare >= 0.26 && numberOr(f.game.deaths) / f.gameMinutes >= 0.7, "死亡次数和死亡负担都很高"),
  rule("喜欢K？", "调侃", "对局", (f) => !f.game.win && f.score <= 65
    && f.metrics.killShare >= 0.28
    && f.metrics.killShare - f.metrics.damageShare >= 0.1
    && f.metrics.damageShare < 0.25 && f.metrics.effectiveDamageConversion < 1.1, "败方击杀占比高于伤害贡献"),
  rule("对面卧底？", "调侃", "对局", (f) => !f.game.win && f.score <= 58
    && f.metrics.killShare >= 0.28
    && f.metrics.killShare - f.metrics.damageShare >= 0.13
    && f.metrics.damageShare < 0.24 && f.metrics.effectiveDamageConversion < 1.0, "败方拿到不少人头但没有对应输出"),
  rule("你不来，对面都不敢动筷子", "调侃", "对局", (f) => !f.game.win && f.score <= 65
    && f.metrics.killShare >= 0.38 && f.metrics.killShare - f.metrics.damageShare >= 0.12, "败方主要负责收尾"),
  rule("爹来了", "神话", "对局", (f) => f.game.win && f.score >= 90
    && (f.comparison?.gameScoreLeader === true || f.gameDamageLeader)
    && (f.metrics.damageShare >= 0.3 || f.metrics.killParticipation >= 0.8), "获胜且成为全场核心"),
  rule("莫欺少年穷", "高光", "对局", (f) => f.game.win && f.matchAvailable
    && f.score >= 75 && f.winnerTowerKills >= 2
    && f.comparison?.teamScoreLeader === true
    // 有记录时要求首塔在对局前四分之一；败方零塔是同一含义的可靠兜底。
    && (f.loserTowerKills === 0
      || (f.firstTowerTimeRatio > 0 && f.firstTowerTimeRatio <= 0.25)), "获胜、队内第一并在前期拿到关键防御塔"),

  // 坦克 / 前排。
  rule("自转的陀螺", "特征", "定位", (f) => isTank(f.role) && f.game.win
    && f.score >= 60 && f.metrics.mitigationShare >= 0.32 && f.metrics.killParticipation <= 0.68, "赢下对局但主要靠承伤旋转战场"),
  rule("旋转至败", "调侃", "定位", (f) => isTank(f.role) && !f.game.win
    && f.metrics.mitigationShare >= 0.3 && f.metrics.killParticipation <= 0.68, "败方仍然承担了大量伤害"),
  rule("一夫当关", "高光", "定位", (f) => isTank(f.role) && f.score >= 78
    && f.metrics.mitigationShare >= 0.35
    && (f.metrics.immobilizationShare >= 0.3 || f.metrics.killParticipation >= 0.68), "承伤与开团/参团都很突出"),
  rule("剑气长城", "神话", "定位", (f) => isTank(f.role) && f.score >= 78
    && f.metrics.mitigationShare >= 0.45 && f.mitigationPerMinute >= 6000, "承伤占比和分均承伤都达到前排高位"),
  rule("你看你都保护了什么", "特征", "对局", (f) => (isTank(f.role) || isSupport(f.role)) && !f.game.win
    && f.score >= 68 && f.matchAvailable
    && (atMost(f.comparison?.teamScoreRank, 2) || atLeast(f.comparison?.teamScoreGapToTeammates, 10))
    && (atMost(f.comparison?.teammatesAverageScore, 62) || atLeast(f.comparison?.teamScoreGapToTeammates, 14)), "自己表现靠前但队友整体偏低"),
  rule("喜欢当Tes？", "调侃", "对局", (f) => isTank(f.role) && !f.game.win
    && f.score >= 62 && f.matchAvailable && f.allyMageOrAdcCount >= 3
    && (atMost(f.comparison?.teamScoreRank, 2) || atLeast(f.comparison?.teamScoreGapToTeammates, 8)), "前排队友全是输出位且最终落败"),
  rule("发癫了？", "调侃", "定位", (f) => (isTank(f.role) || isFighter(f.role))
    && numberOr(f.game.deaths) >= 15 && f.metrics.deathShare >= 0.27
    && numberOr(f.game.deaths) / f.gameMinutes >= 0.75, "冲得很勤，回城也很勤"),

  // 战士 / 战刺。
  rule("神王", "神话", "定位", (f) => isFighter(f.role) && f.score >= 90
    && f.metrics.damageShare >= 0.32 && f.metrics.effectiveDamageConversion >= 1.45
    && f.metrics.killParticipation >= 0.68, "战士定位下输出、伤转和参团同时突出"),
  rule("最终兵器", "高光", "定位", (f) => isFighter(f.role) && f.score >= 85
    && f.metrics.damageShare >= 0.3 && f.metrics.effectiveDamageConversion >= 1.35
    && f.metrics.mitigationShare >= 0.22 && f.metrics.killParticipation >= 0.65, "伤害与承伤兼顾的战士"),
  rule("游龙归海", "高光", "定位", (f) => isFighter(f.role) && f.game.win && f.score >= 82
    && f.metrics.kda >= 4 && f.metrics.deathShare <= 0.2
    && f.metrics.damageShare >= 0.27 && f.metrics.killParticipation >= 0.68, "战士高机动切入且很少白给"),
  rule("吾乃常山赵子龙", "高光", "定位", (f) => isFighter(f.role) && f.score >= 78
    && f.metrics.deathShare <= 0.18 && f.metrics.mitigationShare >= 0.28
    && f.metrics.healingShare >= 0.2 && f.metrics.killParticipation >= 0.65, "少死、能扛、还能持续参团"),
  rule("区区！？", "特征", "定位", (f) => isFighter(f.role) && f.game.win && f.score >= 78
    && f.metrics.mitigationShare >= 0.28 && f.metrics.deathShare <= 0.2
    && f.metrics.kda >= 3.5, "战士赢下正面碰撞"),
  rule("独断万古荒天帝", "神话", "定位", (f) => isFighter(f.role) && f.game.win && f.score >= 95
    && (f.comparison?.gameScoreLeader === true || f.gameDamageLeader)
    && f.metrics.damageShare >= 0.35 && f.metrics.effectiveDamageConversion >= 1.6, "战士获胜且全场输出登顶"),
  rule("484848", "调侃", "定位", (f) => isFighter(f.role) && f.score <= 55 && f.weakSignals >= 2, "战士核心指标同时偏低"),

  // 射手 / 法师。
  rule("木已成舟", "高光", "定位", (f) => f.role === "adc" && f.score >= 78
    && f.metrics.damageShare >= 0.3 && f.metrics.effectiveDamageConversion >= 1.2, "射手稳定提供高额输出"),
  rule("viper3？", "高光", "定位", (f) => f.role === "adc" && f.score >= 85
    && f.metrics.damageShare >= 0.32 && f.metrics.effectiveDamageConversion >= 1.45
    && f.metrics.deathShare <= 0.24 && f.metrics.killParticipation >= 0.65, "射手高输出且死亡控制良好"),
  rule("极道帝兵", "神话", "定位", (f) => f.role === "adc" && f.score >= 90
    && (f.gameDamageLeader || atMost(f.comparison?.gameScoreRank, 2))
    && f.metrics.damageShare >= 0.36 && f.metrics.effectiveDamageConversion >= 1.65, "射手输出和伤转达到全场顶尖"),
  rule("射程之内皆真理", "神话", "定位", (f) => f.role === "adc" && f.score >= 82
    && f.metrics.damageShare >= 0.34 && (f.gameDamageLeader || atMost(f.comparison?.gameScoreRank, 2)), "射手用输出解决问题"),
  rule("玻璃大炮", "特征", "定位", (f) => isRanged(f.role) && f.score >= 60
    // 普通玩家的高输出通常在 27%～33% 之间，不能把 30% 写成几乎唯一入口。
    && f.metrics.damageShare >= 0.27 && f.metrics.effectiveDamageConversion >= 1.1
    && numberOr(f.game.deaths) >= 10 && f.metrics.deathShare >= 0.22, "输出很高但代价也很高"),
  rule("喜欢翻山", "调侃", "定位", (f) => f.role === "adc" && f.score <= 60
    && numberOr(f.game.deaths) >= 15 && f.metrics.deathShare >= 0.25
    && f.metrics.damageShare <= 0.25, "射手频繁阵亡且没有换来足够输出"),
  rule("Rookie？", "高光", "定位", (f) => f.role === "mage" && f.score >= 84
    && (f.gameDamageLeader || atMost(f.comparison?.gameScoreRank, 2))
    && f.metrics.damageShare >= 0.3 && f.metrics.effectiveDamageConversion >= 1.35
    && f.metrics.killParticipation >= 0.62, "法师输出、伤转和参团同时在线"),
  rule("无害的微风", "调侃", "定位", (f) => f.role === "mage" && f.score <= 62
    && f.metrics.damageShare <= 0.18 && f.metrics.effectiveDamageConversion <= 0.95, "法师没有打出应有的伤害"),
  rule("胜天半子", "高光", "定位", (f) => f.role === "mage" && !f.game.win && f.score >= 74
    && (f.gameDamageLeader || atMost(f.comparison?.gameScoreRank, 3))
    && f.metrics.damageShare >= 0.27 && f.metrics.effectiveDamageConversion >= 1.15, "败方法师仍然打穿了对局"),
  rule("怎么有人名字叫击飞", "特征", "定位", (f) => f.score >= 58
    && f.metrics.immobilizationShare >= 0.35
    && f.metrics.immobilizationsPerMinute >= 2.5
    && f.metrics.killParticipation >= 0.55, "控制覆盖率和分均控制都很突出"),
  rule("合着就打我是吧", "特征", "定位", (f) => isRanged(f.role)
    && f.metrics.mitigationShare >= 0.27 && f.mitigationPerMinute >= 6000, "远程英雄承受了异常多的伤害"),
  rule("针对我，有用吗", "神话", "定位", (f) => isRanged(f.role) && f.game.win && f.score >= 82
    && f.metrics.mitigationShare >= 0.25 && f.metrics.damageShare >= 0.3
    && f.metrics.effectiveDamageConversion >= 1.3, "被集火仍然赢下并打出高额输出"),

  // 辅助：全服基准是证据，不是硬性把所有辅助都按同一套数字比较。
  rule("移动泉水", "神话", "定位", (f) => isSupport(f.role) && f.score >= 72
    && f.metrics.healingShare >= 0.32 && hasMetric(f, "allyHealing", 1.15), "治疗量达到该英雄全服均值的较高水平"),
  rule("全心为你", "高光", "定位", (f) => isSupport(f.role) && f.score >= 70
    && f.metrics.damageShare <= 0.12 && f.metrics.killParticipation >= 0.65
    // 这个称号是治疗向专属：护盾型辅助不能仅凭“保护总量”误触发。
    && f.protectionRatio >= 1.0 && f.healingPerMinute > 0
    && hasMetric(f, "allyHealing", 0.9), "把主要资源投入治疗和保护队友"),
  rule("医学奇迹", "高光", "定位", (f) => isSupport(f.role) && f.metrics.healingShare >= 0.5
    && (hasMetric(f, "allyHealing", 1.0) || f.healingPerMinute >= 100), "团队治疗占比很高"),
  rule("绝望大白盾", "神话", "定位", (f) => isSupport(f.role) && f.score >= 72
    && hasMetric(f, "allyShielding", 1.15) && f.shieldingPerMinute >= 350, "护盾量达到英雄均值高位"),
  rule("盾比血条厚", "高光", "定位", (f) => isSupport(f.role) && f.shieldingPerMinute >= 300
    && hasMetric(f, "allyShielding", 1.0) && f.shieldingPerMinute >= f.healingPerMinute * 0.45, "护盾成为主要保护手段"),
  rule("什么都守护不了", "调侃", "定位", (f) => isSupport(f.role) && f.score <= 68
    && f.protectionRatio > 0 && f.protectionRatio < 0.75 && f.metrics.damageShare < 0.15, "治疗和护盾都低于英雄常态"),
  rule("感觉不如血包", "调侃", "定位", (f) => isSupport(f.role) && f.score <= 72
    && f.metrics.healingShare < 0.3 && atMost(f.globalRatios.allyHealing, 0.8), "治疗贡献偏低"),

  // 英雄专属称号：只要求“这个英雄打得不错”，不要求全服前百分位。
  rule("国服刘备", "特征", "英雄", (f) => championAlias(f) === "104" && f.score >= 78
    && f.metrics.damageShare >= 0.28 && f.metrics.effectiveDamageConversion >= 1.2, "男枪本局输出和伤转都不错"),
  rule("国服亚瑟", "特征", "英雄", (f) => championAlias(f) === "86" && f.score >= 78
    && f.metrics.damageShare >= 0.24 && f.metrics.mitigationShare >= 0.25, "盖伦同时能打能扛"),
  rule("国服达摩", "特征", "英雄", (f) => championAlias(f) === "64" && f.score >= 78
    && f.metrics.killParticipation >= 0.65 && f.metrics.damageShare >= 0.24, "李青高参团且有输出"),
  rule("国服瑶", "特征", "英雄", (f) => championAlias(f) === "350" && f.score >= 70
    && isSupport(f.role) && f.protectionRatio >= 1.0, "猫咪保护量达到合格线"),
  rule("国服蔡文姬", "特征", "英雄", (f) => championAlias(f) === "16" && f.score >= 72
    && isSupport(f.role) && hasMetric(f, "allyHealing", 0.95), "索拉卡治疗达到英雄常态"),
  rule("妈！", "神话", "英雄", (f) => championAlias(f) === "16" && f.score >= 80
    && isSupport(f.role) && hasMetric(f, "allyHealing", 1.2) && f.metrics.healingShare >= 0.42, "索拉卡把队友血线拉回来了"),
]

function tierWeight(tier: GameTitleTier) {
  switch (tier) {
    case "神话": return 5
    case "高光": return 4
    case "特征": return 3
    case "普通": return 2
    case "调侃": return 1
  }
}

function hash32(value: number) {
  let x = (value ^ titleSelectionSeed) >>> 0
  x ^= x << 13
  x ^= x >>> 17
  x ^= x << 5
  return x >>> 0
}

function pickDisplayTitle(details: GameTitleDetail[], game: RecentGame, fallback: string) {
  if (!details.length) return fallback
  const weighted: GameTitleDetail[] = []
  for (const detail of details) {
    for (let index = 0; index < tierWeight(detail.tier); index += 1) weighted.push(detail)
  }
  const key = numberOr(game.gameId) ^ numberOr(game.gameCreation)
  return weighted[hash32(key) % weighted.length]?.name || details[0].name
}

function factTags(game: RecentGame, comparison: TitleMatchComparison | null) {
  const tags: string[] = []
  if (game.gameDamageLeader) tags.push("伤害全场第一")
  else if (game.teamDamageLeader) tags.push("伤害第一")
  if (game.teamMitigationLeader) tags.push("承伤第一")
  if (game.teamHealingLeader) tags.push("治疗第一")
  if (game.teamDamageConversionLeader) tags.push("伤转第一")
  if (game.teamGoldLeader) tags.push("经济第一")
  if (comparison?.teamScoreLeader) tags.push("评分队内第一")
  if (comparison?.gameScoreLeader) tags.push("评分全场第一")
  return tags
}

export function buildGameTitleSet(
  game: RecentGame,
  rating: OutputRating = calculateOfficialRating(game),
  options: GameTitleOptions = {},
  suppliedComparison?: TitleMatchComparison | null,
): GameTitleSet {
  const facts = buildFacts(game, rating, options, suppliedComparison)
  const details = TITLE_RULES
    .filter((candidate) => candidate.test(facts))
    .map((candidate) => ({
      name: candidate.name,
      tier: candidate.tier,
      category: candidate.category,
      reason: candidate.reason(facts),
    }))
  const fallback = rating.label
  return {
    displayTitle: pickDisplayTitle(details, game, fallback),
    titles: details.map((detail) => detail.name),
    details,
    factTags: factTags(game, facts.comparison),
  }
}

export function titleRuleNames() {
  return TITLE_RULES.map((rule) => rule.name)
}
