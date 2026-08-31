import assert from "node:assert/strict"
import Module from "node:module"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { build } from "esbuild"

const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(scriptDir, "..")
const virtualFilename = path.join(rootDir, ".scoring-verification.cjs")

const bundle = await build({
  absWorkingDir: rootDir,
  stdin: {
    contents: [
      'export { calculateOfficialRating, calculateOutputRating, globalRatingTitle } from "./src/scoring.ts"',
      'export { buildChampionProfiles, buildPlayerProfile } from "./src/playerProfile.ts"',
      'export { buildGameTitleSet, titleRuleNames } from "./src/titles.ts"',
    ].join("\n"),
    resolveDir: rootDir,
    sourcefile: "scoring-verification.ts",
  },
  bundle: true,
  format: "cjs",
  platform: "node",
  write: false,
})

const verificationModule = new Module(virtualFilename)
verificationModule.filename = virtualFilename
verificationModule.paths = Module._nodeModulePaths(rootDir)
verificationModule._compile(bundle.outputFiles[0].text, virtualFilename)

const {
  buildChampionProfiles,
  buildPlayerProfile,
  calculateOfficialRating,
  calculateOutputRating,
  buildGameTitleSet,
  titleRuleNames,
  globalRatingTitle,
} = verificationModule.exports

function sampleGame(loserTowerKills, loserFirstTowerTimeSeconds, overrides = {}) {
  return {
    gameId: 1,
    championId: 1,
    queueId: 2400,
    gameMode: "KIWI",
    win: true,
    teamId: 100,
    spell1Id: 0,
    spell2Id: 0,
    itemIds: [],
    perkIds: [],
    augmentIds: [],
    teamDamageLeader: false,
    gameDamageLeader: false,
    teamMitigationLeader: false,
    teamHealingLeader: false,
    teamDamageConversionLeader: false,
    teamGoldLeader: false,
    kills: 1,
    deaths: 1,
    assists: 4,
    teamKills: 5,
    teamDeaths: 5,
    gameKills: 10,
    gameDeaths: 10,
    teamTowerKills: 2,
    gameTowerKills: 2 + loserTowerKills,
    loserFirstTowerTimeSeconds,
    teamPlayerCount: 5,
    gamePlayerCount: 10,
    teamPuuids: [],
    teamComposition: [],
    gameComposition: [],
    kda: 5,
    cs: 20,
    goldEarned: 20,
    damageToChampions: 20,
    teamDamageToChampions: 100,
    gameDamageToChampions: 200,
    damageSelfMitigated: 10,
    totalDamageTaken: 10,
    teamDamageSelfMitigated: 50,
    teamTotalDamageTaken: 50,
    totalHeal: 0,
    totalHealsOnTeammates: 0,
    totalDamageShieldedOnTeammates: 0,
    teamTotalHeal: 0,
    teamGoldEarned: 100,
    gameGoldEarned: 200,
    enemyChampionImmobilizations: 0,
    teamEnemyChampionImmobilizations: 0,
    immobilizeAndKillWithAlly: 0,
    teamImmobilizeAndKillWithAlly: 0,
    timeCCingOthers: 0,
    gameCreation: 0,
    gameDuration: 16 * 60,
    ...overrides,
  }
}

function adjustment(loserTowerKills, loserFirstTowerTimeSeconds, overrides) {
  return calculateOutputRating(
    sampleGame(loserTowerKills, loserFirstTowerTimeSeconds, overrides),
    {},
  ).contextAdjustment
}

assert.equal(adjustment(0).active, true)
assert.equal(adjustment(0).maxKdaWeight, 35)
assert.equal(adjustment(1, 12 * 60).active, false)
assert.equal(adjustment(1, 12 * 60).maxKdaWeight, 0)
assert.equal(adjustment(1, 14 * 60).active, true)
assert.equal(adjustment(1, 14 * 60).maxKdaWeight, 17.5)
assert.equal(adjustment(1, 16 * 60).maxKdaWeight, 35)
assert.equal(adjustment(1).active, false)
const longZeroTowerAdjustment = adjustment(0, undefined, { gameDuration: 17 * 60 })
assert.equal(longZeroTowerAdjustment.active, true)
assert.equal(longZeroTowerAdjustment.loserTowerKills, 0)
assert.equal(longZeroTowerAdjustment.towerKdaWeight, 35)
assert.match(longZeroTowerAdjustment.reason, /超过16分钟零塔例外/)
const longOneTowerAdjustment = adjustment(1, 15 * 60, { gameDuration: 17 * 60 })
assert.equal(longOneTowerAdjustment.active, false)
assert.equal(longOneTowerAdjustment.reason, "对局时长超过16分钟")

const twoTowerGame = adjustment(2, 15 * 60, {
  teamKills: 20,
  teamDeaths: 2,
  gameKills: 22,
  gameDeaths: 22,
  teamDamageToChampions: 150,
  gameDamageToChampions: 200,
})
assert.equal(twoTowerGame.active, false)
assert.match(twoTowerGame.reason, /2/)

const baseline = {
  id: 1,
  slug: "test-support",
  name: "Test Support",
  damageToChampions: 16_000,
  damageTaken: 18_000,
  selfMitigatedDamage: 12_000,
  allyHealing: 10_000,
  allyShielding: 8_000,
  crowdControlDurationSeconds: 100,
  gameDurationSeconds: 16 * 60,
  sourceDataDate: "2026-08-30",
}
const baselines = new Map([[1, baseline]])
const supportContext = { champions: { 1: { roles: ["Support"] } } }
const outputContext = { champions: { 1: { roles: ["Marksman"] } } }
const formalGame = sampleGame(2, 15 * 60, { gameDuration: 20 * 60 })

const supportBaseRating = calculateOutputRating(formalGame, supportContext)
const supportOfficialRating = calculateOfficialRating(formalGame, supportContext, baselines)
assert.notEqual(supportOfficialRating.score, supportBaseRating.score)
assert.equal(Object.hasOwn(supportOfficialRating.parts, "globalBaseline"), true)
assert.equal(Object.hasOwn(supportOfficialRating.parts, "killQuality"), false)
assert.equal(Object.hasOwn(supportOfficialRating.parts, "effectiveDamage"), false)

const supportRatingTitle = globalRatingTitle(formalGame, supportContext, baselines, "test")
assert.equal((supportRatingTitle.match(/^全服得分 /gm) || []).length, 1)
assert.match(supportRatingTitle, /^全服得分 \d+(?:\.\d+)?\/25分$/m)
assert.doesNotMatch(
  supportRatingTitle,
  /综合完成度|被替换小项|分池来源|计分曲线|同英雄基准|动态采用|全服分均时长折算|分支指标/,
)

const outputBaseRating = calculateOutputRating(formalGame, outputContext)
const outputOfficialRating = calculateOfficialRating(formalGame, outputContext, baselines)
assert.strictEqual(outputOfficialRating, outputBaseRating)

// 国服称号使用独立的英雄全服证据，不改变非辅助正式评分。
const nationalBaseline = {
  ...baseline,
  name: "测试英雄称号",
  averageKda: 3,
  averageKills: 10,
  averageDeaths: 10,
  averageAssists: 20,
  participationRatePercent: 65,
  damageSharePercent: 20,
  damageGoldConversionRatePercent: 200,
  goldEarned: 16_000,
  cs: 35,
}
const nationalBaselines = new Map([[1, nationalBaseline]])
const nationalContext = {
  champions: {
    1: { name: "疾风剑豪", title: "亚索", alias: "Yasuo", roles: ["Marksman"] },
  },
}
const nationalGame = sampleGame(2, 15 * 60, {
  win: true,
  kills: 30,
  deaths: 2,
  assists: 30,
  teamKills: 40,
  teamDeaths: 10,
  kda: 30,
  teamDamageLeader: true,
  gameDamageLeader: true,
  damageToChampions: 50_000,
  teamDamageToChampions: 100_000,
  goldEarned: 20_000,
  teamGoldEarned: 100_000,
  damageSelfMitigated: 2_000,
  totalDamageTaken: 5_000,
  teamDamageSelfMitigated: 10_000,
  teamTotalDamageTaken: 50_000,
  enemyChampionImmobilizations: 8,
  teamEnemyChampionImmobilizations: 20,
  gameDuration: 16 * 60,
})
const nationalRating = calculateOfficialRating(nationalGame, nationalContext, nationalBaselines)
assert.ok(nationalRating.score >= 90)
const nationalTitles = buildGameTitleSet(nationalGame, nationalRating, {
  ratingContext: nationalContext,
  globalBaselines: nationalBaselines,
})
assert.equal(nationalTitles.titles.includes("国服最强亚索"), true)
assert.equal(nationalTitles.titles.includes(nationalRating.label), true)
assert.equal(nationalTitles.details.some((detail) => detail.name === nationalRating.label && detail.category === "评分"), true)
assert.equal(titleRuleNames().includes("区区！？"), false)

// 国服输出称号额外要求至少 1.40 的修正伤转；只堆伤害占比而伤转不足时不能命中。
const lowConversionNationalGame = {
  ...nationalGame,
  goldEarned: 37_000,
  teamGoldEarned: 100_000,
}
const lowConversionNationalRating = calculateOfficialRating(
  lowConversionNationalGame,
  nationalContext,
  nationalBaselines,
)
const lowConversionNationalTitles = buildGameTitleSet(lowConversionNationalGame, lowConversionNationalRating, {
  ratingContext: nationalContext,
  globalBaselines: nationalBaselines,
})
assert.ok(lowConversionNationalRating.score >= 90)
assert.ok(lowConversionNationalRating.metrics.effectiveDamageConversion < 1.4)
assert.equal(lowConversionNationalTitles.titles.includes("国服最强亚索"), false)

// 普通称号明确位于 60 分以下；60 分及以上不能命中这两个称号。
const belowSixtyGame = sampleGame(2, 15 * 60, {
  gameId: 2,
  gameDuration: 12 * 60,
})
const belowSixtyRating = {
  ...calculateOfficialRating(belowSixtyGame, {}),
  score: 59,
  metrics: {
    ...calculateOfficialRating(belowSixtyGame, {}).metrics,
    damageShare: 0.2,
    effectiveDamageConversion: 1,
    killParticipation: 0.6,
    deathShare: 0.2,
  },
}
const belowSixtyTitles = buildGameTitleSet(belowSixtyGame, belowSixtyRating)
assert.equal(belowSixtyTitles.titles.includes("一般人机"), true)
assert.equal(belowSixtyTitles.titles.includes("小泯一把"), true)
const atSixtyTitles = buildGameTitleSet(belowSixtyGame, { ...belowSixtyRating, score: 60 })
assert.equal(atSixtyTitles.titles.includes("一般人机"), false)
assert.equal(atSixtyTitles.titles.includes("小泯一把"), false)

// 低分调侃/错位称号的分界统一为 60 分以下；60 分及以上不能命中。
const tankContext = { champions: { 1: { roles: ["Tank"] } } }
const tankGame = sampleGame(0, undefined, {
  win: true,
  deaths: 15,
  gameDeaths: 30,
  itemIds: [],
})
const tankBaseRating = calculateOfficialRating(tankGame, tankContext)
const tankLowRating = {
  ...tankBaseRating,
  score: 59,
  metrics: {
    ...tankBaseRating.metrics,
    mitigationShare: 0.34,
    killParticipation: 0.6,
    deathShare: 0.3,
  },
}
const tankLowTitles = buildGameTitleSet(tankGame, tankLowRating, { ratingContext: tankContext })
assert.equal(tankLowTitles.titles.includes("自转的陀螺"), true)
assert.equal(tankLowTitles.titles.includes("发癫了？"), true)
assert.equal(tankLowTitles.titles.includes("泉水指挥官"), true)
const tankAtSixtyTitles = buildGameTitleSet(tankGame, { ...tankLowRating, score: 60 }, { ratingContext: tankContext })
assert.equal(tankAtSixtyTitles.titles.includes("自转的陀螺"), false)
assert.equal(tankAtSixtyTitles.titles.includes("发癫了？"), false)
assert.equal(tankAtSixtyTitles.titles.includes("泉水指挥官"), false)

const profileOptions = { timeWeighted: false, globalBaselines: baselines }
const playerProfile = buildPlayerProfile([formalGame], supportContext, profileOptions)
const championProfile = buildChampionProfiles([formalGame], supportContext, profileOptions)[0]
assert.equal(playerProfile.overallScore, supportOfficialRating.score)
assert.equal(championProfile.averageScore, supportOfficialRating.score)

console.log("Scoring verification passed")
