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

const profileOptions = { timeWeighted: false, globalBaselines: baselines }
const playerProfile = buildPlayerProfile([formalGame], supportContext, profileOptions)
const championProfile = buildChampionProfiles([formalGame], supportContext, profileOptions)[0]
assert.equal(playerProfile.overallScore, supportOfficialRating.score)
assert.equal(championProfile.averageScore, supportOfficialRating.score)

console.log("Scoring verification passed")
