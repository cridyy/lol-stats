import { analyzePlayerRole, type PlayerRole, type RatingContext } from "./roleClassifier"
import type { RecentGame } from "./types"

export interface FunStatsBucket {
  games: number
  wins: number
  rate: number
  winRate: number
  averageEconomyDelta?: number
}

export type FunEquipmentRole =
  | "adc"
  | "mage"
  | "support"
  | "tank"
  | "skirmisher"
  | "bruiser"

export interface FunGoldRange {
  min: number | null
  max: number | null
}

export interface FunEquipmentFilter extends FunGoldRange {
  itemId: number
  championId: number | null
  role: FunEquipmentRole | null
}

export interface FunEquipmentStats {
  filter: FunEquipmentFilter
  eligibleGames: number
  withItem: FunStatsBucket
  withoutItem: FunStatsBucket
}

export type FunAugmentRarity = "silver" | "gold" | "prismatic" | "bronze"

export interface FunAugmentFilter {
  augmentId: number | null
  championId: number | null
  role: FunEquipmentRole | null
}

export interface FunAugmentStats extends FunAugmentFilter {
  rarity: FunAugmentRarity | null
  eligibleGames: number
  appearances: number
  wins: number
  opportunities: number
  pickRate: number
  winRate: number
}

export interface FunStatsResult {
  generatedAt: number
  sampleGames: number
  side: {
    blue: FunStatsBucket
    red: FunStatsBucket
  }
  equipment: FunEquipmentStats
  augment: FunAugmentStats
  penetration: {
    goldRange: FunGoldRange
    eligibleGames: number
    withItem: FunStatsBucket
    withoutItem: FunStatsBucket
  }
  leaders: {
    gold: FunStatsBucket
    damage: FunStatsBucket
  }
}

interface FunStatsCacheEntry {
  updatedAt: number
  result: FunStatsResult
}

interface FunStatsCacheStore {
  version: number
  entries: Record<string, FunStatsCacheEntry>
}

const FUN_STATS_STORAGE_KEY = "lol-stats.fun-stats"
const FUN_STATS_CACHE_VERSION = 7
const FUN_STATS_CACHE_LIMIT = 24
const COLLECTOR_ITEM_ID = 6676
const PERCENT_PENETRATION_ITEM_IDS = new Set([
  3033,
  3036,
  3071,
  3135,
  3302,
  4010,
  4630,
  4635,
  8010,
  223071,
  223302,
])
const PERCENT_PENETRATION_ITEM_NAMES =
  /黑色切割者|放血者的诅咒|界弓|black cleaver|bloodletter'?s curse|terminus/i

const outputRoles = new Set<PlayerRole>([
  "adc",
  "mage",
  "fighter",
  "assassin",
  "pureAssassin",
  "fighterAssassin",
  "utilityCarry",
  "unknown",
])

export const DEFAULT_FUN_EQUIPMENT_FILTER: FunEquipmentFilter = {
  itemId: COLLECTOR_ITEM_ID,
  championId: null,
  role: "adc",
  min: null,
  max: null,
}

export const DEFAULT_PENETRATION_GOLD_RANGE: FunGoldRange = {
  min: 12_000,
  max: null,
}

export const DEFAULT_FUN_AUGMENT_FILTER: FunAugmentFilter = {
  augmentId: null,
  championId: null,
  role: null,
}

export const FUN_EQUIPMENT_ROLE_OPTIONS: Array<{
  value: FunEquipmentRole
  label: string
}> = [
  { value: "adc", label: "ADC" },
  { value: "mage", label: "法师" },
  { value: "support", label: "辅助" },
  { value: "tank", label: "坦克" },
  { value: "skirmisher", label: "战刺" },
  { value: "bruiser", label: "半肉战士" },
]

export function calculateFunStats(
  games: RecentGame[],
  context: RatingContext,
  equipmentFilter: FunEquipmentFilter = DEFAULT_FUN_EQUIPMENT_FILTER,
  penetrationGoldRange: FunGoldRange = DEFAULT_PENETRATION_GOLD_RANGE,
  augmentFilter: FunAugmentFilter = DEFAULT_FUN_AUGMENT_FILTER,
): FunStatsResult {
  const classified = games.map((game) => ({
    game,
    role: analyzePlayerRole(game, context).role,
  }))
  const normalizedPenetrationGoldRange = normalizeGoldRange(penetrationGoldRange)
  const highEconomyOutput = classified.filter(
    (entry) =>
      outputRoles.has(entry.role) &&
      matchesGoldRange(entry.game.goldEarned, normalizedPenetrationGoldRange),
  )
  const outputWithPenetration = highEconomyOutput.filter((entry) =>
    hasPercentPenetration(entry.game, context),
  )

  return {
    generatedAt: Date.now(),
    sampleGames: games.length,
    side: {
      blue: bucket(games.filter((game) => game.teamId === 100), games.length),
      red: bucket(games.filter((game) => game.teamId === 200), games.length),
    },
    equipment: calculateEquipmentStatsFromClassified(classified, equipmentFilter),
    augment: calculateAugmentStats(games, context, augmentFilter),
    penetration: {
      goldRange: normalizedPenetrationGoldRange,
      eligibleGames: highEconomyOutput.length,
      withItem: classifiedBucket(outputWithPenetration, highEconomyOutput.length),
      withoutItem: classifiedBucket(
        highEconomyOutput.filter((entry) => !outputWithPenetration.includes(entry)),
        highEconomyOutput.length,
      ),
    },
    leaders: {
      gold: bucket(games.filter((game) => game.teamGoldLeader), games.length),
      damage: bucket(games.filter((game) => game.teamDamageLeader), games.length),
    },
  }
}

export function calculateEquipmentStats(
  games: RecentGame[],
  context: RatingContext,
  filter: FunEquipmentFilter,
) {
  const classified = games.map((game) => ({
    game,
    role: analyzePlayerRole(game, context).role,
  }))
  return calculateEquipmentStatsFromClassified(classified, filter)
}

export function calculatePenetrationStats(
  games: RecentGame[],
  context: RatingContext,
  goldRange: FunGoldRange,
) {
  const normalizedRange = normalizeGoldRange(goldRange)
  const eligible = games.filter((game) => {
    const role = analyzePlayerRole(game, context).role
    return outputRoles.has(role) && matchesGoldRange(game.goldEarned, normalizedRange)
  })
  const withItem = eligible.filter((game) => hasPercentPenetration(game, context))

  return {
    goldRange: normalizedRange,
    eligibleGames: eligible.length,
    withItem: bucket(withItem, eligible.length),
    withoutItem: bucket(
      eligible.filter((game) => !withItem.includes(game)),
      eligible.length,
    ),
  }
}

export function calculateAugmentStats(
  games: RecentGame[],
  context: RatingContext,
  filter: FunAugmentFilter = DEFAULT_FUN_AUGMENT_FILTER,
): FunAugmentStats {
  const observedAugmentIds = games.flatMap((game) => game.augmentIds)
  const augmentId = resolveSelectedAugmentId(observedAugmentIds, context, filter.augmentId)
  const rarity = augmentId ? normalizeAugmentRarity(context.augments?.[augmentId]?.rarity) : null
  const championId = filter.championId ? Math.trunc(filter.championId) : null
  const role = filter.role || null
  const eligibleGames = games.filter((game) =>
    (!championId || game.championId === championId) &&
    (!role || matchesEquipmentRole(analyzePlayerRole(game, context).role, role)),
  )
  if (!augmentId || !rarity) {
    return {
      augmentId: null,
      championId,
      role,
      rarity: null,
      eligibleGames: eligibleGames.length,
      appearances: 0,
      wins: 0,
      opportunities: 0,
      pickRate: 0,
      winRate: 0,
    }
  }

  let appearances = 0
  let wins = 0
  let opportunities = 0

  for (const game of eligibleGames) {
    for (const candidateId of game.augmentIds) {
      if (normalizeAugmentRarity(context.augments?.[candidateId]?.rarity) !== rarity) continue

      opportunities += 1
      if (candidateId !== augmentId) continue

      appearances += 1
      if (game.win) wins += 1
      // 同局不能重复选择相同强化，命中后后续同色槽位不再是有效机会。
      break
    }
  }

  return {
    augmentId,
    championId,
    role,
    rarity,
    eligibleGames: eligibleGames.length,
    appearances,
    wins,
    opportunities,
    pickRate: ratio(appearances, opportunities),
    winRate: ratio(wins, appearances),
  }
}

export function funStatsCacheKey(
  games: RecentGame[],
  ownerPuuid = "",
  ownerLabel = "",
  sgpServerId = "",
) {
  const owner = ownerPuuid.trim().toLowerCase() || ownerLabel.trim().toLowerCase() || "unknown"
  const sample = games
    .map((game) => [
      game.gameId,
      game.gameCreation,
      game.teamId,
      Number(game.win),
      game.goldEarned,
      game.teamGoldEarned,
      game.itemIds.join("."),
    ].join(":"))
    .join("|")
  return `${FUN_STATS_CACHE_VERSION}:${hashText(`${sgpServerId}:${owner}`)}:${games.length}:${hashText(sample)}`
}

export function loadCachedFunStats(key: string) {
  return loadCacheStore().entries[key]?.result || null
}

export function persistFunStats(key: string, result: FunStatsResult) {
  const store = loadCacheStore()
  store.entries[key] = { updatedAt: Date.now(), result }

  const retained = Object.entries(store.entries)
    .sort((left, right) => right[1].updatedAt - left[1].updatedAt)
    .slice(0, FUN_STATS_CACHE_LIMIT)
  store.entries = Object.fromEntries(retained)
  localStorage.setItem(FUN_STATS_STORAGE_KEY, JSON.stringify(store))
}

function classifiedBucket(
  entries: Array<{ game: RecentGame }>,
  denominator: number,
  includeEconomyDelta = false,
) {
  return bucket(entries.map((entry) => entry.game), denominator, includeEconomyDelta)
}

function bucket(
  games: RecentGame[],
  denominator: number,
  includeEconomyDelta = false,
): FunStatsBucket {
  const wins = games.filter((game) => game.win).length
  const result: FunStatsBucket = {
    games: games.length,
    wins,
    rate: ratio(games.length, denominator),
    winRate: ratio(wins, games.length),
  }
  if (includeEconomyDelta) {
    result.averageEconomyDelta = average(
      games.map((game) => (ratio(game.goldEarned, game.teamGoldEarned) - 0.2) * 5),
    )
  }
  return result
}

function hasPercentPenetration(game: RecentGame, context: RatingContext) {
  return game.itemIds.some((itemId) => {
    if (PERCENT_PENETRATION_ITEM_IDS.has(itemId)) return true
    const item = context.items?.[itemId]
    if (!item) return false
    if (PERCENT_PENETRATION_ITEM_NAMES.test(item.name)) return true
    const text = `${item.name} ${item.description || ""}`
      .replace(/<[^>]*>/g, " ")
      .replace(/\s+/g, " ")
    return (
      /\d+(?:\.\d+)?%\s*(?:护甲穿透|法术穿透)/i.test(text) ||
      /\d+(?:\.\d+)?%\s*(?:armor|magic)\s+penetration/i.test(text)
    )
  })
}

function calculateEquipmentStatsFromClassified(
  classified: Array<{ game: RecentGame; role: PlayerRole }>,
  filter: FunEquipmentFilter,
): FunEquipmentStats {
  const normalizedFilter: FunEquipmentFilter = {
    itemId: Math.max(0, Math.trunc(filter.itemId)),
    championId: filter.championId ? Math.trunc(filter.championId) : null,
    role: filter.role || null,
    ...normalizeGoldRange(filter),
  }
  const eligible = classified.filter(({ game, role }) =>
    (!normalizedFilter.role || matchesEquipmentRole(role, normalizedFilter.role)) &&
    (!normalizedFilter.championId || game.championId === normalizedFilter.championId) &&
    matchesGoldRange(game.goldEarned, normalizedFilter),
  )
  const withItem = eligible.filter(({ game }) => game.itemIds.includes(normalizedFilter.itemId))
  const withoutItem = eligible.filter((entry) => !withItem.includes(entry))

  return {
    filter: normalizedFilter,
    eligibleGames: eligible.length,
    withItem: classifiedBucket(withItem, eligible.length, true),
    withoutItem: classifiedBucket(withoutItem, eligible.length, true),
  }
}

function matchesEquipmentRole(role: PlayerRole, selected: FunEquipmentRole) {
  if (selected === "skirmisher") {
    return ["fighter", "assassin", "pureAssassin", "fighterAssassin"].includes(role)
  }
  return role === selected
}

export function normalizeAugmentRarity(rarity: string | undefined): FunAugmentRarity | null {
  switch ((rarity || "").toLowerCase()) {
    case "ksilver":
    case "silver":
      return "silver"
    case "kgold":
    case "gold":
      return "gold"
    case "kprismatic":
    case "prismatic":
      return "prismatic"
    case "kbronze":
    case "bronze":
      return "bronze"
    default:
      return null
  }
}

function resolveSelectedAugmentId(
  observedAugmentIds: number[],
  context: RatingContext,
  selectedAugmentId?: number | null,
) {
  const observed = new Set(observedAugmentIds)
  if (
    selectedAugmentId &&
    observed.has(selectedAugmentId) &&
    normalizeAugmentRarity(context.augments?.[selectedAugmentId]?.rarity)
  ) {
    return selectedAugmentId
  }
  return observedAugmentIds.find((augmentId) =>
    normalizeAugmentRarity(context.augments?.[augmentId]?.rarity),
  ) || null
}

function normalizeGoldRange(range: FunGoldRange): FunGoldRange {
  const min = range.min === null ? null : Math.max(0, Math.trunc(range.min))
  const max = range.max === null ? null : Math.max(0, Math.trunc(range.max))
  if (min !== null && max !== null && min > max) {
    return { min: max, max: min }
  }
  return { min, max }
}

function matchesGoldRange(gold: number, range: FunGoldRange) {
  return (range.min === null || gold >= range.min) && (range.max === null || gold <= range.max)
}

function loadCacheStore(): FunStatsCacheStore {
  try {
    const parsed = JSON.parse(localStorage.getItem(FUN_STATS_STORAGE_KEY) || "null")
    if (parsed?.version === FUN_STATS_CACHE_VERSION && parsed.entries) {
      return parsed as FunStatsCacheStore
    }
  } catch {
    // 损坏的本地缓存直接舍弃，下次点击会重新计算。
  }
  return { version: FUN_STATS_CACHE_VERSION, entries: {} }
}

function ratio(part: number, total: number) {
  return total > 0 ? part / total : 0
}

function average(values: number[]) {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : 0
}

function hashText(value: string) {
  let hash = 2166136261
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return (hash >>> 0).toString(36)
}
