<script setup lang="ts">
import { computed, inject, nextTick, onBeforeUnmount, ref, watch } from "vue"
import { ArrowDown, ArrowUp, ChevronsUpDown, ClipboardCopy, ExternalLink, Sparkles } from "lucide-vue-next"
import { openUrl } from "@tauri-apps/plugin-opener"
import {
  calculateAugmentStats,
  calculateEquipmentStats,
  calculateFunStats,
  calculatePenetrationStats,
  funStatsCacheKey,
  loadCachedFunStats,
  normalizeAugmentRarity,
  persistFunStats,
  type FunAugmentFilter,
  type FunAugmentRarity,
  type FunEquipmentFilter,
  type FunGoldRange,
  type FunStatsResult,
} from "../funStats"
import { copyElementAsPng } from "../imageShare"
import { globalBaselinesByChampionId } from "../globalBaselines"
import { notifyKey } from "../notifications"
import { getPlayerEloEstimateTask, startPlayerEloEstimateTask } from "../api"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  OpenMatchPayload,
  PlayerEloEstimateResponse,
  PlayerEloEstimateTaskStatus,
  PlayerStatsResponse,
  RecentGame,
  RecentGameParticipantSummary,
  ShareSettings,
} from "../types"
import AssetIcon from "./AssetIcon.vue"
import ChampionAvatar from "./ChampionAvatar.vue"
import FunStatsPanel from "./FunStatsPanel.vue"
import GameRecordList from "./GameRecordList.vue"
import {
  buildChampionProfiles,
  buildPlayerProfile,
  profileScoreLevel,
  profileTierLabel,
} from "../playerProfile"
import { championName, fixed, mitigationValue, percent, teamMitigationValue } from "../utils"

const DATA_FILTER_HINT_SEEN_KEY = "lol-stats.data-filter-hint-seen"
const PLAYER_ELO_POLL_INTERVAL_MS = 1200
const DETAIL_GAME_BATCH_SIZE = 20
const HIDDEN_RATING_URL = "https://aramkit.com/zh-CN/rating"
// 暂时隐藏角色 ELO 功能，但保留前后端计算、缓存和恢复入口。
const ELO_FEATURE_ENABLED = false

type StatsTableTab =
  | "champion"
  | "augment"
  | "item"
  | "allySummoner"
  | "enemySummoner"
  | "allyChampion"
  | "enemyChampion"

interface StatsGameSelection {
  kind: StatsTableTab
  id: number | string
  label: string
}

type SortDirection = "asc" | "desc"
type ChampionSortKey =
  | "name"
  | "games"
  | "winRate"
  | "averageScore"
  | "label"
  | "averageKda"
  | "damageShare"
  | "damageConversionRate"
  | "mitigationShare"
  | "healingShare"
type UsageSortKey =
  | "name"
  | "games"
  | "pickRate"
  | "winRate"
  | "label"
  | "topChampion"
  | "lastSeenAt"

interface UsageChampionLink {
  championId: number
  games: number
  wins: number
  winRate: number
}

interface UsageStatsRow {
  id: number
  name: string
  iconPath?: string
  games: number
  wins: number
  losses: number
  pickRate: number
  winRate: number
  label: string
  topChampions: UsageChampionLink[]
  lastSeenAt: number
  rarityClass?: string
  rarity?: FunAugmentRarity | null
}

interface EncounterStatsRow {
  key: string
  name: string
  entityType: "summoner" | "champion"
  championId?: number
  games: number
  wins: number
  losses: number
  pickRate: number
  winRate: number
  label: string
  topChampions: UsageChampionLink[]
  lastSeenAt: number
}

const props = defineProps<{
  stats: PlayerStatsResponse | null
  champions: Record<number, ChampionSummaryItem>
  gameAssets: GameAssetBundle
  shareSettings: ShareSettings
  sgpServerId?: string
  ownerLabel?: string
  ownerPuuid?: string
}>()

const emit = defineEmits<{
  openMatch: [payload: OpenMatchPayload]
}>()

const selectedStatsGamesFilter = ref<StatsGameSelection | null>(null)
const selectedStatsVisibleCount = ref(DETAIL_GAME_BATCH_SIZE)
const championSearch = ref("")
const activeStatsTable = ref<StatsTableTab>("champion")
const augmentRarityFilter = ref<FunAugmentRarity | "all">("all")
const statsOwnerLabel = computed(() => {
  if (props.ownerLabel?.trim()) return props.ownerLabel.trim()
  if (props.stats?.summoner.displayName?.trim()) return props.stats.summoner.displayName.trim()
  if (props.stats?.summoner.gameName?.trim() && props.stats.summoner.tagLine?.trim()) {
    return `${props.stats.summoner.gameName.trim()}#${props.stats.summoner.tagLine.trim()}`
  }
  if (props.stats?.summoner.gameName?.trim()) return props.stats.summoner.gameName.trim()
  return props.stats?.summoner.internalName?.trim() || "当前角色"
})
const championSort = ref<{ key: ChampionSortKey; direction: SortDirection }>({
  key: "games",
  direction: "desc",
})
const augmentSort = ref<{ key: UsageSortKey; direction: SortDirection }>({
  key: "games",
  direction: "desc",
})
const itemSort = ref<{ key: UsageSortKey; direction: SortDirection }>({
  key: "games",
  direction: "desc",
})
const encounterSort = ref<{ key: UsageSortKey; direction: SortDirection }>({
  key: "games",
  direction: "desc",
})
const shareBusy = ref<"champions" | "games" | null>(null)
const statsRootRef = ref<HTMLElement | null>(null)
const tableWrapRef = ref<HTMLElement | null>(null)
const detailLoadSentinelRef = ref<HTMLElement | null>(null)
const championStatsCaptureRef = ref<HTMLElement | null>(null)
const championGamesCaptureRef = ref<HTMLElement | null>(null)
const funStatsResult = ref<FunStatsResult | null>(null)
const funStatsVisible = ref(false)
const funStatsLoading = ref(false)
const playerEloEstimate = ref<PlayerEloEstimateResponse | null>(null)
const playerEloLoading = ref(false)
const playerEloError = ref("")
const dataFilterHintVisible = ref(localStorage.getItem(DATA_FILTER_HINT_SEEN_KEY) !== "1")
const notify = inject(notifyKey, () => 0)
let savedTableScrollTop = 0
let savedPageScrollTop = 0
let playerEloRequestToken = 0
let playerEloPollTimer: number | null = null
let detailLoadObserver: IntersectionObserver | null = null
const selectedChampionId = computed(() => {
  const selection = selectedStatsGamesFilter.value
  return selection?.kind === "champion" ? Number(selection.id) : null
})
const selectedStatsGames = computed(() => {
  const selection = selectedStatsGamesFilter.value
  if (!selection) return []
  return (props.stats?.recentGames || []).filter((game) => gameMatchesStatsSelection(game, selection))
})
const visibleSelectedStatsGames = computed(() =>
  selectedStatsGames.value.slice(0, selectedStatsVisibleCount.value),
)
const hasMoreSelectedStatsGames = computed(
  () => visibleSelectedStatsGames.value.length < selectedStatsGames.value.length,
)
const selectedChampionGames = computed(() => {
  if (!selectedChampionId.value) return []
  return selectedStatsGames.value
})
const selectedChampionLabel = computed(() =>
  selectedStatsGamesFilter.value?.kind === "champion"
    ? selectedStatsGamesFilter.value.label
    : championName(props.champions, selectedChampionId.value || undefined),
)
const selectedStatsGamesTitle = computed(() => {
  const selection = selectedStatsGamesFilter.value
  if (!selection) return "相关战绩"
  return `${selection.label} · ${statsSelectionTitle(selection.kind)}`
})
const topChampionIds = computed(
  () => props.stats?.championStats.slice(0, 5).map((champ) => champ.championId) || [],
)
const augmentStats = computed(() =>
  buildUsageStats(
    props.stats?.recentGames || [],
    (game) => Array.from(new Set(game.augmentIds || [])),
    combinedAugmentMap.value,
    "augment",
  ),
)
const itemStats = computed(() =>
  buildUsageStats(
    props.stats?.recentGames || [],
    (game) => Array.from(new Set((game.itemIds || []).filter((itemId) => itemId > 0))),
    itemMap.value,
    "item",
  ),
)
const allySummonerStats = computed(() => buildEncounterSummonerStats(props.stats?.recentGames || [], "ally"))
const enemySummonerStats = computed(() => buildEncounterSummonerStats(props.stats?.recentGames || [], "enemy"))
const allyChampionStats = computed(() => buildEncounterChampionStats(props.stats?.recentGames || [], "ally"))
const enemyChampionStats = computed(() => buildEncounterChampionStats(props.stats?.recentGames || [], "enemy"))
const filteredChampionStats = computed(() => {
  const champions = props.stats?.championStats || []
  const keyword = normalizeSearchText(championSearch.value)
  const filtered = !keyword
    ? champions
    : champions.filter((champ) => championSearchSummary(champ).includes(keyword))
  return [...filtered].sort(compareChampionStats)
})
const filteredAugmentStats = computed(() => {
  const keyword = normalizeSearchText(championSearch.value)
  const filteredByRarity =
    augmentRarityFilter.value === "all"
      ? augmentStats.value
      : augmentStats.value.filter((entry) => entry.rarity === augmentRarityFilter.value)
  const filtered = !keyword
    ? filteredByRarity
    : filteredByRarity.filter((entry) => usageSearchSummary(entry).includes(keyword))
  return [...filtered].sort((a, b) => compareUsageStats(a, b, augmentSort.value))
})
const filteredItemStats = computed(() => {
  const keyword = normalizeSearchText(championSearch.value)
  const filtered = !keyword
    ? itemStats.value
    : itemStats.value.filter((entry) => usageSearchSummary(entry).includes(keyword))
  return [...filtered].sort((a, b) => compareUsageStats(a, b, itemSort.value))
})
const currentEncounterStats = computed(() => {
  switch (activeStatsTable.value) {
    case "allySummoner":
      return allySummonerStats.value
    case "enemySummoner":
      return enemySummonerStats.value
    case "allyChampion":
      return allyChampionStats.value
    case "enemyChampion":
      return enemyChampionStats.value
    default:
      return []
  }
})
const filteredEncounterStats = computed(() => {
  const keyword = normalizeSearchText(championSearch.value)
  const filtered = !keyword
    ? currentEncounterStats.value
    : currentEncounterStats.value.filter((entry) => encounterSearchSummary(entry).includes(keyword))
  return [...filtered].sort((a, b) => compareEncounterStats(a, b, encounterSort.value))
})
const shareChampionStats = computed(
  () => filteredChampionStats.value.slice(0, props.shareSettings.championAnalysisLimit),
)
const shareSelectedChampionGames = computed(() =>
  selectedChampionGames.value.slice(0, props.shareSettings.championGamesAnalysisLimit),
)
const spellMap = computed(() => indexAssets(props.gameAssets.summonerSpells))
const itemMap = computed(() => indexAssets(props.gameAssets.items))
const perkMap = computed(() => indexAssets(props.gameAssets.perks))
const augmentMap = computed(() => indexAssets(props.gameAssets.augments))
const combinedAugmentMap = computed(() => mergeAssetMaps(perkMap.value, augmentMap.value))
const funAugmentAssets = computed(() => Object.values(combinedAugmentMap.value))
const ratingContext = computed(() => ({
  items: itemMap.value,
  champions: props.champions,
  augments: combinedAugmentMap.value,
}))
const playerProfile = computed(() =>
  buildPlayerProfile(props.stats?.recentGames || [], ratingContext.value, {
    globalBaselines: globalBaselinesByChampionId.value,
  }),
)
const championProfileMap = computed(() =>
  buildChampionProfiles(props.stats?.recentGames || [], ratingContext.value, {
    globalBaselines: globalBaselinesByChampionId.value,
  }).reduce<
    Record<number, ReturnType<typeof buildChampionProfiles>[number]>
  >((acc, profile) => {
    acc[profile.championId] = profile
    return acc
  }, {}),
)
const abilityCards = computed(() => [
  playerProfile.value.abilities.carry,
  playerProfile.value.abilities.frontline,
  playerProfile.value.abilities.support,
])
const roleBarColors = ["#2f78d6", "#d08a20", "#d44b7a", "#8b5cf6", "#d34f4f", "#5c6f7a"]
const displayRoleDistribution = computed(() => {
  const grouped = new Map<string, { games: number; wins: number }>()
  for (const role of playerProfile.value.roleDistribution) {
    const label = roleBarLabel(role.label)
    const current = grouped.get(label) || { games: 0, wins: 0 }
    current.games += role.games
    current.wins += role.wins
    grouped.set(label, current)
  }

  return Array.from(grouped.entries())
    .map(([label, value]) => ({
      label,
      games: value.games,
      wins: value.wins,
      rate: ratio(value.games, playerProfile.value.games),
      winRate: ratio(value.wins, value.games),
    }))
    .sort((a, b) => b.games - a.games || b.rate - a.rate)
})
const roleBars = computed(() =>
  displayRoleDistribution.value.slice(0, roleBarColors.length).map((role, index) => ({
    ...role,
    color: roleBarColors[index],
    width: `${Math.max(role.rate * 100, 4)}%`,
  })),
)

function indexAssets(entries: GameAssetBundle["items"]) {
  return entries.reduce<Record<number, GameAssetBundle["items"][number]>>((acc, entry) => {
    acc[entry.id] = entry
    return acc
  }, {})
}

function mergeAssetMaps(
  fallback: Record<number, GameAssetBundle["items"][number]>,
  preferred: Record<number, GameAssetBundle["items"][number]>,
) {
  const result = { ...fallback }
  for (const [rawId, entry] of Object.entries(preferred)) {
    const id = Number(rawId)
    const previous = result[id]
    result[id] = previous
      ? {
          ...previous,
          ...entry,
          name: entry.name || previous.name,
          description: entry.description || previous.description,
          iconPath: entry.iconPath || previous.iconPath,
          rarity: entry.rarity || previous.rarity,
          categories: entry.categories.length ? entry.categories : previous.categories,
          price: entry.price || previous.price,
          priceTotal: entry.priceTotal || previous.priceTotal,
        }
      : entry
  }
  return result
}

function normalizeSearchText(value: string) {
  return value.toLowerCase().replace(/[\s·.'’_-]/g, "")
}

function championSearchText(championId: number) {
  const champion = props.champions[championId]
  return normalizeSearchText(
    [
      champion?.name,
      champion?.title,
      champion?.alias,
      championName(props.champions, championId),
    ]
      .filter(Boolean)
      .join(" "),
  )
}

function championSearchSummary(champ: PlayerStatsResponse["championStats"][number]) {
  const profile = championProfile(champ.championId)
  return normalizeSearchText(
    [
      championSearchText(champ.championId),
      profile.mainRoleLabel,
      profile.label,
      profileEvaluationText(profile.averageScore, profile.games),
    ].join(" "),
  )
}

function usageSearchSummary(entry: UsageStatsRow) {
  return normalizeSearchText(
    [
      entry.name,
      entry.label,
      augmentRarityLabel(entry.rarity),
      ...entry.topChampions.map((champion) => championName(props.champions, champion.championId)),
    ].join(" "),
  )
}

function encounterSearchSummary(entry: EncounterStatsRow) {
  return normalizeSearchText(
    [
      entry.name,
      entry.label,
      ...entry.topChampions.map((champion) => championName(props.champions, champion.championId)),
    ].join(" "),
  )
}

function roleBarLabel(label: string) {
  return ["战士", "刺客", "纯刺客", "战士刺客", "半肉刺客"].includes(label) ? "战刺" : label
}

watch(
  () => props.stats,
  () => {
    selectedStatsGamesFilter.value = null
    selectedStatsVisibleCount.value = DETAIL_GAME_BATCH_SIZE
    championSearch.value = ""
    funStatsResult.value = null
    funStatsVisible.value = false
  },
)

watch(
  () => [props.stats?.summoner.puuid, props.stats?.summoner.gameName, props.stats?.summoner.tagLine, props.sgpServerId],
  () => {
    if (!ELO_FEATURE_ENABLED) {
      stopPlayerEloPolling()
      playerEloEstimate.value = null
      playerEloLoading.value = false
      playerEloError.value = ""
      return
    }
    void refreshPlayerEloEstimate()
  },
  { immediate: true },
)

async function refreshPlayerEloEstimate() {
  if (!ELO_FEATURE_ENABLED) return
  await refreshPlayerEloEstimateInternal(false)
}

async function refreshPlayerEloEstimateFromSource() {
  if (!ELO_FEATURE_ENABLED) return
  await refreshPlayerEloEstimateInternal(true)
}

async function refreshPlayerEloEstimateInternal(forceRefresh: boolean) {
  const stats = props.stats
  if (!stats?.summoner?.puuid) {
    stopPlayerEloPolling()
    playerEloEstimate.value = null
    playerEloError.value = ""
    return
  }

  const requestToken = ++playerEloRequestToken
  stopPlayerEloPolling()
  playerEloLoading.value = true
  playerEloError.value = ""

  try {
    const task = await startPlayerEloEstimateTask(
      stats.summoner.puuid,
      stats.summoner.gameName || "",
      stats.summoner.tagLine || "",
      statsOwnerLabel.value,
      props.sgpServerId,
      forceRefresh,
    )
    await handlePlayerEloTaskStatus(task, requestToken, forceRefresh)
  } catch (error) {
    if (requestToken !== playerEloRequestToken) return
    playerEloEstimate.value = null
    playerEloError.value = errorMessage(error)
  } finally {
    if (requestToken === playerEloRequestToken) {
      playerEloLoading.value = false
    }
  }
}

async function handlePlayerEloTaskStatus(
  task: PlayerEloEstimateTaskStatus,
  requestToken: number,
  forceRefresh: boolean,
) {
  if (requestToken !== playerEloRequestToken) return

  if (task.result) {
    playerEloEstimate.value = task.result
  }

  if (task.status === "completed") {
    playerEloError.value = ""
    playerEloLoading.value = false
    stopPlayerEloPolling()
    if (forceRefresh) {
      notify({
        kind: "success",
        title: "角色 ELO 已刷新",
        message: "后台重算已完成，结果已更新",
      })
    }
    return
  }

  if (task.status === "failed") {
    playerEloLoading.value = false
    playerEloError.value = task.error || "角色 ELO 计算失败"
    if (!task.result) {
      playerEloEstimate.value = null
    }
    stopPlayerEloPolling()
    return
  }

  playerEloLoading.value = true
  playerEloError.value = ""
  schedulePlayerEloPoll(task.jobId, requestToken, forceRefresh)
}

function schedulePlayerEloPoll(jobId: string, requestToken: number, forceRefresh: boolean) {
  stopPlayerEloPolling()
  playerEloPollTimer = window.setTimeout(async () => {
    try {
      const task = await getPlayerEloEstimateTask(jobId)
      await handlePlayerEloTaskStatus(task, requestToken, forceRefresh)
    } catch (error) {
      if (requestToken !== playerEloRequestToken) return
      playerEloLoading.value = false
      playerEloError.value = errorMessage(error)
      stopPlayerEloPolling()
    }
  }, PLAYER_ELO_POLL_INTERVAL_MS)
}

function stopPlayerEloPolling() {
  if (playerEloPollTimer !== null) {
    window.clearTimeout(playerEloPollTimer)
    playerEloPollTimer = null
  }
}

onBeforeUnmount(() => {
  stopPlayerEloPolling()
  detailLoadObserver?.disconnect()
  detailLoadObserver = null
})

async function toggleFunStats() {
  if (dataFilterHintVisible.value) {
    dataFilterHintVisible.value = false
    localStorage.setItem(DATA_FILTER_HINT_SEEN_KEY, "1")
  }

  if (funStatsVisible.value) {
    funStatsVisible.value = false
    return
  }

  funStatsVisible.value = true
  if (funStatsResult.value) return
  await loadFunStats(false)
}

async function loadFunStats(force: boolean) {
  const games = props.stats?.recentGames || []
  if (!games.length || funStatsLoading.value) return

  funStatsLoading.value = true
  try {
    const key = funStatsCacheKey(
      games,
      props.ownerPuuid,
      props.ownerLabel,
      props.sgpServerId,
    )
    if (!force) {
      const cached = loadCachedFunStats(key)
      if (cached) {
        funStatsResult.value = cached
        return
      }
    }

    await nextTick()
    await new Promise((resolve) => window.setTimeout(resolve, 0))
    const result = calculateFunStats(
      games,
      ratingContext.value,
      funStatsResult.value?.equipment.filter,
      funStatsResult.value?.penetration.goldRange,
      funStatsResult.value?.augment,
    )
    funStatsResult.value = result
    persistCurrentFunStats(key, result)
  } catch (error) {
    funStatsVisible.value = false
    notify({
      kind: "error",
      title: "数据筛选统计失败",
      message: errorMessage(error),
      duration: 7000,
    })
  } finally {
    funStatsLoading.value = false
  }
}

function updateFunEquipment(filter: FunEquipmentFilter) {
  const games = props.stats?.recentGames || []
  if (!games.length || !funStatsResult.value) return

  const result: FunStatsResult = {
    ...funStatsResult.value,
    generatedAt: Date.now(),
    equipment: calculateEquipmentStats(games, ratingContext.value, filter),
  }
  funStatsResult.value = result
  persistCurrentFunStats(
    funStatsCacheKey(games, props.ownerPuuid, props.ownerLabel, props.sgpServerId),
    result,
  )
}

function updateFunPenetration(goldRange: FunGoldRange) {
  const games = props.stats?.recentGames || []
  if (!games.length || !funStatsResult.value) return

  const result: FunStatsResult = {
    ...funStatsResult.value,
    generatedAt: Date.now(),
    penetration: calculatePenetrationStats(games, ratingContext.value, goldRange),
  }
  funStatsResult.value = result
  persistCurrentFunStats(
    funStatsCacheKey(games, props.ownerPuuid, props.ownerLabel, props.sgpServerId),
    result,
  )
}

function updateFunAugment(filter: FunAugmentFilter) {
  const games = props.stats?.recentGames || []
  if (!games.length || !funStatsResult.value) return

  const result: FunStatsResult = {
    ...funStatsResult.value,
    generatedAt: Date.now(),
    augment: calculateAugmentStats(games, ratingContext.value, filter),
  }
  funStatsResult.value = result
  persistCurrentFunStats(
    funStatsCacheKey(games, props.ownerPuuid, props.ownerLabel, props.sgpServerId),
    result,
  )
}

function persistCurrentFunStats(key: string, result: FunStatsResult) {
  try {
    persistFunStats(key, result)
  } catch (error) {
    notify({
      kind: "warning",
      title: "数据筛选已生成",
      message: `本地缓存写入失败：${errorMessage(error)}`,
    })
  }
}

function openChampionGames(championId: number) {
  openStatsGames({
    kind: "champion",
    id: championId,
    label: championName(props.champions, championId),
  })
}

function openUsageGames(kind: "augment" | "item", entry: UsageStatsRow) {
  openStatsGames({ kind, id: entry.id, label: entry.name })
}

function openEncounterGames(entry: EncounterStatsRow) {
  const kind = activeStatsTable.value
  if (
    kind !== "allySummoner" &&
    kind !== "enemySummoner" &&
    kind !== "allyChampion" &&
    kind !== "enemyChampion"
  ) {
    return
  }

  openStatsGames({
    kind,
    id: entry.entityType === "champion" ? Number(entry.championId || 0) : entry.key,
    label: entry.name,
  })
}

function openStatsGames(selection: StatsGameSelection) {
  savedTableScrollTop = tableWrapRef.value?.scrollTop ?? 0
  savedPageScrollTop = scrollParent(statsRootRef.value)?.scrollTop ?? 0
  selectedStatsVisibleCount.value = DETAIL_GAME_BATCH_SIZE
  selectedStatsGamesFilter.value = selection
}

async function closeStatsGames() {
  selectedStatsGamesFilter.value = null
  selectedStatsVisibleCount.value = DETAIL_GAME_BATCH_SIZE
  await nextTick()
  if (tableWrapRef.value) tableWrapRef.value.scrollTop = savedTableScrollTop

  const parent = scrollParent(statsRootRef.value)
  if (parent) parent.scrollTop = savedPageScrollTop
}

function gameMatchesStatsSelection(game: RecentGame, selection: StatsGameSelection) {
  const numericId = Number(selection.id)
  switch (selection.kind) {
    case "champion":
      return game.championId === numericId
    case "augment":
      return (game.augmentIds || []).some((augmentId) => augmentId === numericId)
    case "item":
      return (game.itemIds || []).some((itemId) => itemId === numericId)
    case "allySummoner":
      return relatedPlayers(game, "ally").some(
        (player) => normalizePuuidKey(player.puuid) === normalizePuuidKey(String(selection.id)),
      )
    case "enemySummoner":
      return relatedPlayers(game, "enemy").some(
        (player) => normalizePuuidKey(player.puuid) === normalizePuuidKey(String(selection.id)),
      )
    case "allyChampion":
      return relatedPlayers(game, "ally").some((player) => Number(player.championId) === numericId)
    case "enemyChampion":
      return relatedPlayers(game, "enemy").some((player) => Number(player.championId) === numericId)
  }
}

function statsSelectionTitle(kind: StatsTableTab) {
  switch (kind) {
    case "champion":
      return "单英雄战绩"
    case "augment":
      return "海克斯战绩"
    case "item":
      return "装备战绩"
    case "allySummoner":
      return "同队战绩"
    case "enemySummoner":
      return "对手战绩"
    case "allyChampion":
      return "友方英雄战绩"
    case "enemyChampion":
      return "敌方英雄战绩"
  }
}

function loadMoreSelectedStatsGames() {
  if (!hasMoreSelectedStatsGames.value) return
  selectedStatsVisibleCount.value = Math.min(
    selectedStatsVisibleCount.value + DETAIL_GAME_BATCH_SIZE,
    selectedStatsGames.value.length,
  )
}

function handleDetailLoadIntersection(entries: IntersectionObserverEntry[]) {
  if (entries.some((entry) => entry.isIntersecting)) loadMoreSelectedStatsGames()
}

function observeDetailLoadSentinel(current: HTMLElement | null, previous: HTMLElement | null) {
  if (previous) detailLoadObserver?.unobserve(previous)
  if (!current) return

  if (!detailLoadObserver) {
    detailLoadObserver = new IntersectionObserver(handleDetailLoadIntersection, {
      root: null,
      rootMargin: "160px 0px 220px 0px",
      threshold: 0,
    })
  }
  detailLoadObserver.observe(current)
}

function requestMoreSelectedGamesIfVisible() {
  if (!hasMoreSelectedStatsGames.value) return
  const sentinel = detailLoadSentinelRef.value
  if (!sentinel) return
  const viewportHeight = window.innerHeight || document.documentElement.clientHeight
  if (sentinel.getBoundingClientRect().top <= viewportHeight + 180) loadMoreSelectedStatsGames()
}

watch(detailLoadSentinelRef, observeDetailLoadSentinel)

watch(
  () => [visibleSelectedStatsGames.value.length, selectedStatsGames.value.length] as const,
  async () => {
    await nextTick()
    requestMoreSelectedGamesIfVisible()
  },
)

function scrollParent(element: HTMLElement | null) {
  let current = element?.parentElement || null
  while (current) {
    const style = window.getComputedStyle(current)
    const overflowY = style.overflowY
    if (
      (overflowY === "auto" || overflowY === "scroll" || overflowY === "overlay") &&
      current.scrollHeight > current.clientHeight
    ) {
      return current
    }
    current = current.parentElement
  }

  return document.scrollingElement as HTMLElement | null
}

function queueName(game: RecentGame) {
  const map: Record<number, string> = {
    400: "匹配",
    420: "单双排",
    430: "匹配",
    440: "灵活排",
    450: "大乱斗",
    480: "极限闪击",
    490: "快速匹配",
    900: "无限火力",
    1700: "斗魂竞技场",
    1710: "斗魂竞技场",
    1711: "斗魂竞技场",
    1712: "斗魂竞技场",
    1900: "无限火力",
    2400: "海克斯大乱斗",
  }

  return map[game.queueId] || game.gameMode || `队列 ${game.queueId}`
}

function kNumber(value: number) {
  return `${(value / 1000).toFixed(1)}k`
}

function shareText(part: number, total: number) {
  return `${Math.round(ratio(part, total) * 100)}%`
}

function gameDamageConversion(game: RecentGame) {
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  if (goldShare === 0) return "0.00"
  return fixed(ratio(game.damageToChampions, game.teamDamageToChampions) / goldShare)
}

function shortAugmentName(augmentId: number) {
  const name = combinedAugmentMap.value[augmentId]?.name || `强化${augmentId}`
  return Array.from(name).slice(0, 4).join("")
}

function augmentRarityClass(augmentId: number) {
  switch (combinedAugmentMap.value[augmentId]?.rarity) {
    case "kPrismatic":
      return "augment-prismatic"
    case "kGold":
      return "augment-gold"
    case "kSilver":
      return "augment-silver"
    case "kBronze":
      return "augment-bronze"
    default:
      return ""
  }
}

function ratio(part: number, total: number) {
  return total > 0 ? part / total : 0
}

function scoreText(score: number, games: number) {
  return games > 0 ? `${Math.round(score)}分` : "样本不足"
}

function profileLevelClass(score: number, games: number) {
  return games > 0 ? `profile-${profileScoreLevel(score)}` : "profile-empty"
}

function profileEvaluationText(score: number, games: number) {
  return games > 0 ? profileTierLabel(score) : "样本不足"
}

function abilityRate(games: number) {
  return ratio(games, playerProfile.value.games)
}

function mainRoleText(role: string) {
  return role && role !== "样本不足" ? `主玩${role}` : "样本不足"
}

function tagToneClass(label: string) {
  if (/小坑比|大坑比/.test(label)) return "tag-danger"
  if (/小有实力/.test(label)) return "tag-team"
  if (
    /通天|小代|全场火力|无解|输出机器|爆炸核弹|大魔王|恐怖利刃|无情收割|吃草挤奶|核心大C|chovy|faker|城墙|叹息之墙|半肉战神|最强前锋|高光|稳定|核心|控杀|控制/.test(
      label,
    )
  ) {
    return "tag-strong"
  }
  if (/战犯|低能|混子|K头|k头|开游戏|纸糊|发软|隐身|拉胯|毫无存在|拿钱不干事|自爆/.test(label)) {
    return "tag-danger"
  }
  if (/承伤|团队|辅助|前排|功能|治疗/.test(label)) {
    return "tag-team"
  }
  if (/冲锋|冲阵|上下限|浴血|顶级前锋|刀尖舔血/.test(label)) {
    return "tag-warn"
  }
  return "tag-neutral"
}

function championTagToneClass(band: string) {
  switch (band) {
    case "excellent":
      return "tag-band-excellent"
    case "normal":
      return "tag-band-normal"
    case "low":
      return "tag-band-low"
    case "disaster":
      return "tag-band-disaster"
    default:
      return "tag-neutral"
  }
}

function championProfile(championId: number) {
  return (
    championProfileMap.value[championId] || {
      championId,
      games: 0,
      averageScore: 0,
      mainRoleLabel: "样本不足",
      label: "样本不足",
      labelBand: "empty",
      highlightRate: 0,
      disasterRate: 0,
      averageDamageShare: 0,
      averageDamageConversion: 0,
      averageMitigationShare: 0,
      averageHealingShare: 0,
    }
  )
}

function buildUsageStats(
  games: RecentGame[],
  pickIds: (game: RecentGame) => number[],
  assetMap: Record<number, GameAssetBundle["items"][number]>,
  kind: "augment" | "item",
) {
  const rows = new Map<
    number,
    {
      games: number
      wins: number
      lastSeenAt: number
      championMap: Map<number, { games: number; wins: number }>
    }
  >()

  for (const game of games) {
    const ids = pickIds(game)
      .map((id) => Number(id))
      .filter((id) => id > 0 && shouldTrackUsageAsset(assetMap[id], kind))
    if (!ids.length) continue

    for (const id of ids) {
      const current = rows.get(id) || {
        games: 0,
        wins: 0,
        lastSeenAt: 0,
        championMap: new Map<number, { games: number; wins: number }>(),
      }
      current.games += 1
      current.wins += Number(game.win)
      current.lastSeenAt = Math.max(current.lastSeenAt, game.gameCreation)
      const champion = current.championMap.get(game.championId) || { games: 0, wins: 0 }
      champion.games += 1
      champion.wins += Number(game.win)
      current.championMap.set(game.championId, champion)
      rows.set(id, current)
    }
  }

  return Array.from(rows.entries())
    .map(([id, value]) => {
      const topChampions = Array.from(value.championMap.entries())
        .map(([championId, champion]) => ({
          championId,
          games: champion.games,
          wins: champion.wins,
          winRate: ratio(champion.wins, champion.games),
        }))
        .sort((a, b) => b.games - a.games || b.winRate - a.winRate || a.championId - b.championId)
        .slice(0, 3)

      return {
        id,
        name: assetMap[id]?.name || `${kind === "augment" ? "海克斯" : "装备"} ${id}`,
        iconPath: assetMap[id]?.iconPath,
        games: value.games,
        wins: value.wins,
        losses: value.games - value.wins,
        pickRate: ratio(value.games, games.length),
        winRate: ratio(value.wins, value.games),
        label: usageLabel(kind, value.games, ratio(value.wins, value.games), ratio(value.games, games.length)),
        topChampions,
        lastSeenAt: value.lastSeenAt,
        rarityClass: kind === "augment" ? augmentRarityClass(id) : "",
        rarity: kind === "augment" ? normalizeAugmentRarity(assetMap[id]?.rarity) : null,
      } satisfies UsageStatsRow
    })
    .sort((a, b) => b.games - a.games || b.winRate - a.winRate || a.id - b.id)
}

function buildEncounterSummonerStats(
  games: RecentGame[],
  side: "ally" | "enemy",
) {
  const entries = new Map<
    string,
    {
      name: string
      games: number
      wins: number
      lastSeenAt: number
      championMap: Map<number, { games: number; wins: number }>
    }
  >()

  for (const game of games) {
    const seen = new Set<string>()
    for (const player of relatedPlayers(game, side)) {
      const puuid = normalizePuuidKey(player.puuid)
      if (!puuid || seen.has(puuid)) continue
      seen.add(puuid)
      let current = entries.get(puuid)
      if (!current) {
        current = {
          name: player.summonerName || "未知召唤师",
          games: 0,
          wins: 0,
          lastSeenAt: 0,
          championMap: new Map(),
        }
        entries.set(puuid, current)
      }
      if (player.summonerName) current.name = player.summonerName
      current.games += 1
      current.wins += Number(game.win)
      current.lastSeenAt = Math.max(current.lastSeenAt, game.gameCreation)

      const championId = Number(player.championId || 0)
      if (championId > 0) {
        const championEntry = current.championMap.get(championId) || { games: 0, wins: 0 }
        championEntry.games += 1
        championEntry.wins += Number(game.win)
        current.championMap.set(championId, championEntry)
      }
    }
  }

  return Array.from(entries.entries())
    .map(([puuid, value]) => {
      const topChampions = topChampionLinksFromMap(value.championMap)
      return {
        key: puuid,
        name: value.name,
        entityType: "summoner",
        games: value.games,
        wins: value.wins,
        losses: value.games - value.wins,
        pickRate: ratio(value.games, games.length),
        winRate: ratio(value.wins, value.games),
        label: encounterLabel(side === "ally" ? "allySummoner" : "enemySummoner", value.games, ratio(value.wins, value.games), ratio(value.games, games.length)),
        topChampions,
        lastSeenAt: value.lastSeenAt,
      } satisfies EncounterStatsRow
    })
    .filter((entry) => entry.games >= 3)
    .sort((a, b) => b.games - a.games || b.winRate - a.winRate || a.name.localeCompare(b.name, "zh-CN"))
}

function buildEncounterChampionStats(
  games: RecentGame[],
  side: "ally" | "enemy",
) {
  const entries = new Map<
    number,
    {
      games: number
      wins: number
      lastSeenAt: number
      championMap: Map<number, { games: number; wins: number }>
    }
  >()

  for (const game of games) {
    const seen = new Set<number>()
    for (const player of relatedPlayers(game, side)) {
      const championId = Number(player.championId || 0)
      if (championId <= 0 || seen.has(championId)) continue
      seen.add(championId)
      let current = entries.get(championId)
      if (!current) {
        current = {
          games: 0,
          wins: 0,
          lastSeenAt: 0,
          championMap: new Map(),
        }
        entries.set(championId, current)
      }

      current.games += 1
      current.wins += Number(game.win)
      current.lastSeenAt = Math.max(current.lastSeenAt, game.gameCreation)

      if (game.championId > 0) {
        const ownerChampion = current.championMap.get(game.championId) || { games: 0, wins: 0 }
        ownerChampion.games += 1
        ownerChampion.wins += Number(game.win)
        current.championMap.set(game.championId, ownerChampion)
      }
    }
  }

  return Array.from(entries.entries())
    .map(([championId, value]) => ({
      key: `champion:${championId}`,
      name: championName(props.champions, championId),
      entityType: "champion",
      championId,
      games: value.games,
      wins: value.wins,
      losses: value.games - value.wins,
      pickRate: ratio(value.games, games.length),
      winRate: ratio(value.wins, value.games),
      label: encounterLabel(side === "ally" ? "allyChampion" : "enemyChampion", value.games, ratio(value.wins, value.games), ratio(value.games, games.length)),
      topChampions: topChampionLinksFromMap(value.championMap),
      lastSeenAt: value.lastSeenAt,
    } satisfies EncounterStatsRow))
    .sort((a, b) => b.games - a.games || b.winRate - a.winRate || a.name.localeCompare(b.name, "zh-CN"))
}

function relatedPlayers(game: RecentGame, side: "ally" | "enemy"): RecentGameParticipantSummary[] {
  return side === "ally" ? game.allyPlayers || [] : game.enemyPlayers || []
}

function topChampionLinksFromMap(championMap: Map<number, { games: number; wins: number }>) {
  return Array.from(championMap.entries())
    .map(([championId, value]) => ({
      championId,
      games: value.games,
      wins: value.wins,
      winRate: ratio(value.wins, value.games),
    }))
    .sort((a, b) => b.games - a.games || b.winRate - a.winRate || a.championId - b.championId)
    .slice(0, 3)
}

function normalizePuuidKey(value: string | undefined) {
  return (value || "").trim().toLowerCase()
}

function shouldTrackUsageAsset(
  asset: GameAssetBundle["items"][number] | undefined,
  kind: "augment" | "item",
) {
  if (!asset?.name) return false
  if (kind === "augment") return true
  return isCompletedTrackedItem(asset)
}

function isCompletedTrackedItem(item: GameAssetBundle["items"][number]) {
  const name = (item.name || "").toLowerCase()
  const categories = new Set((item.categories || []).map((category) => category.toLowerCase()))
  const priceTotal = Number(item.priceTotal || item.price || 0)

  if (/魄罗佳肴|poro[- ]?snax|poro snack/.test(item.name) || /poro[- ]?snax/.test(name)) {
    return false
  }

  if (
    categories.has("consumable") ||
    categories.has("trinket") ||
    categories.has("vision") ||
    categories.has("lane")
  ) {
    return false
  }

  if (priceTotal <= 0) return false
  if (categories.has("boots")) return true
  if (priceTotal >= 1800) return true

  return false
}

function usageLabel(kind: "augment" | "item", games: number, winRate: number, pickRate: number) {
  const hotLabel = kind === "augment" ? "版本答案" : "常驻大件"
  const stableLabel = kind === "augment" ? "稳定海克斯" : "稳定出装"
  const coldLabel = kind === "augment" ? "高风险赌狗" : "高风险出装"

  if (games >= 20 && winRate >= 0.62) return hotLabel
  if (games >= 12 && winRate >= 0.56) return stableLabel
  if (games >= 12 && pickRate >= 0.18) return "高频选择"
  if (games >= 8 && winRate <= 0.4) return coldLabel
  return games >= 5 ? "样本观察" : "冷门样本"
}

function encounterLabel(
  kind: "allySummoner" | "enemySummoner" | "allyChampion" | "enemyChampion",
  games: number,
  winRate: number,
  pickRate: number,
) {
  switch (kind) {
    case "allySummoner":
      if (games >= 10 && winRate >= 0.6) return "上分搭子"
      if (games >= 6 && winRate >= 0.55) return "稳定队友"
      if (games >= 6 && winRate <= 0.4) return "高危搭子"
      return pickRate >= 0.08 ? "常见队友" : "样本观察"
    case "enemySummoner":
      if (games >= 8 && winRate >= 0.58) return "稳定拿分"
      if (games >= 6 && winRate <= 0.42) return "苦手对手"
      return pickRate >= 0.08 ? "常见对手" : "样本观察"
    case "allyChampion":
      if (games >= 10 && winRate >= 0.6) return "强势组合"
      if (games >= 6 && winRate >= 0.55) return "常用搭配"
      if (games >= 6 && winRate <= 0.42) return "磨合一般"
      return pickRate >= 0.08 ? "高频搭档" : "样本观察"
    case "enemyChampion":
      if (games >= 8 && winRate >= 0.58) return "稳定拿分"
      if (games >= 6 && winRate <= 0.42) return "苦手英雄"
      return pickRate >= 0.08 ? "高频对位" : "样本观察"
  }
}

function tableSearchPlaceholder() {
  switch (activeStatsTable.value) {
    case "augment":
      return "搜索海克斯 / 英雄 / 称号"
    case "item":
      return "搜索装备 / 英雄 / 称号"
    case "allySummoner":
    case "enemySummoner":
      return "搜索召唤师 / 英雄 / 称号"
    case "allyChampion":
    case "enemyChampion":
      return "搜索英雄 / 我的英雄 / 称号"
    default:
      return "搜索英雄 / 称号"
  }
}

function augmentRarityLabel(rarity: FunAugmentRarity | null | undefined) {
  switch (rarity) {
    case "silver":
      return "银色"
    case "gold":
      return "黄金"
    case "prismatic":
      return "棱彩"
    case "bronze":
      return "青铜"
    default:
      return ""
  }
}

function toggleAugmentRarityFilter(rarity: FunAugmentRarity | "all") {
  augmentRarityFilter.value = rarity
}

function activateStatsTable(tab: StatsTableTab) {
  activeStatsTable.value = tab
}

function toggleChampionSort(key: ChampionSortKey) {
  championSort.value = nextSortState(championSort.value, key, key === "name" || key === "label" ? "asc" : "desc")
}

function toggleAugmentSort(key: UsageSortKey) {
  augmentSort.value = nextSortState(augmentSort.value, key, key === "name" || key === "label" ? "asc" : "desc")
}

function toggleItemSort(key: UsageSortKey) {
  itemSort.value = nextSortState(itemSort.value, key, key === "name" || key === "label" ? "asc" : "desc")
}

function toggleEncounterSort(key: UsageSortKey) {
  encounterSort.value = nextSortState(encounterSort.value, key, key === "name" || key === "label" ? "asc" : "desc")
}

function nextSortState<K extends string>(
  state: { key: K; direction: SortDirection },
  key: K,
  initialDirection: SortDirection,
) : { key: K; direction: SortDirection } {
  if (state.key !== key) {
    return { key, direction: initialDirection }
  }

  return { key, direction: state.direction === "desc" ? "asc" : "desc" }
}

function compareChampionStats(
  a: PlayerStatsResponse["championStats"][number],
  b: PlayerStatsResponse["championStats"][number],
) {
  const direction = championSort.value.direction === "asc" ? 1 : -1
  const profileA = championProfile(a.championId)
  const profileB = championProfile(b.championId)
  let result = 0

  switch (championSort.value.key) {
    case "name":
      result = championName(props.champions, a.championId).localeCompare(championName(props.champions, b.championId), "zh-CN")
      break
    case "games":
      result = a.games - b.games
      break
    case "winRate":
      result = a.winRate - b.winRate
      break
    case "averageScore":
      result = profileA.averageScore - profileB.averageScore
      break
    case "label":
      result = profileA.label.localeCompare(profileB.label, "zh-CN")
      break
    case "averageKda":
      result = a.averageKda - b.averageKda
      break
    case "damageShare":
      result = a.damageShare - b.damageShare
      break
    case "damageConversionRate":
      result = a.damageConversionRate - b.damageConversionRate
      break
    case "mitigationShare":
      result = a.mitigationShare - b.mitigationShare
      break
    case "healingShare":
      result = a.healingShare - b.healingShare
      break
  }

  if (result !== 0) return result * direction

  return (
    (b.games - a.games) ||
    (b.winRate - a.winRate) ||
    (profileB.averageScore - profileA.averageScore) ||
    (b.averageKda - a.averageKda) ||
    (b.damageShare - a.damageShare) ||
    (b.damageConversionRate - a.damageConversionRate) ||
    (b.mitigationShare - a.mitigationShare) ||
    (b.healingShare - a.healingShare) ||
    championName(props.champions, a.championId).localeCompare(
      championName(props.champions, b.championId),
      "zh-CN",
    ) ||
    profileA.label.localeCompare(profileB.label, "zh-CN") ||
    a.championId - b.championId
  )
}

function compareUsageStats(
  a: UsageStatsRow,
  b: UsageStatsRow,
  sort: { key: UsageSortKey; direction: SortDirection },
) {
  const direction = sort.direction === "asc" ? 1 : -1
  let result = 0

  switch (sort.key) {
    case "name":
      result = a.name.localeCompare(b.name, "zh-CN")
      break
    case "games":
      result = a.games - b.games
      break
    case "pickRate":
      result = a.pickRate - b.pickRate
      break
    case "winRate":
      result = a.winRate - b.winRate
      break
    case "label":
      result = a.label.localeCompare(b.label, "zh-CN")
      break
    case "topChampion":
      result = (a.topChampions[0]?.games || 0) - (b.topChampions[0]?.games || 0)
      break
    case "lastSeenAt":
      result = a.lastSeenAt - b.lastSeenAt
      break
  }

  if (result !== 0) return result * direction

  return (
    (b.games - a.games) ||
    (b.pickRate - a.pickRate) ||
    (b.winRate - a.winRate) ||
    ((b.topChampions[0]?.games || 0) - (a.topChampions[0]?.games || 0)) ||
    (b.lastSeenAt - a.lastSeenAt) ||
    a.name.localeCompare(b.name, "zh-CN") ||
    a.label.localeCompare(b.label, "zh-CN") ||
    a.id - b.id
  )
}

function compareEncounterStats(
  a: EncounterStatsRow,
  b: EncounterStatsRow,
  sort: { key: UsageSortKey; direction: SortDirection },
) {
  const direction = sort.direction === "asc" ? 1 : -1
  let result = 0

  switch (sort.key) {
    case "name":
      result = a.name.localeCompare(b.name, "zh-CN")
      break
    case "games":
      result = a.games - b.games
      break
    case "pickRate":
      result = a.pickRate - b.pickRate
      break
    case "winRate":
      result = a.winRate - b.winRate
      break
    case "label":
      result = a.label.localeCompare(b.label, "zh-CN")
      break
    case "topChampion":
      result = (a.topChampions[0]?.games || 0) - (b.topChampions[0]?.games || 0)
      break
    case "lastSeenAt":
      result = a.lastSeenAt - b.lastSeenAt
      break
  }

  if (result !== 0) return result * direction

  return (
    (b.games - a.games) ||
    (b.pickRate - a.pickRate) ||
    (b.winRate - a.winRate) ||
    ((b.topChampions[0]?.games || 0) - (a.topChampions[0]?.games || 0)) ||
    (b.lastSeenAt - a.lastSeenAt) ||
    a.name.localeCompare(b.name, "zh-CN") ||
    a.label.localeCompare(b.label, "zh-CN") ||
    a.key.localeCompare(b.key, "en")
  )
}

function sortIconState<K extends string>(state: { key: K; direction: SortDirection }, key: K) {
  if (state.key !== key) return "none"
  return state.direction
}

function statsTableTitle() {
  switch (activeStatsTable.value) {
    case "augment":
      return "海克斯战绩"
    case "item":
      return "装备战绩"
    case "allySummoner":
      return "队友召唤师战绩"
    case "enemySummoner":
      return "对手召唤师战绩"
    case "allyChampion":
      return "友方英雄战绩"
    case "enemyChampion":
      return "敌方英雄战绩"
    default:
      return "单英雄战绩"
  }
}

function encounterEntityColumnLabel() {
  switch (activeStatsTable.value) {
    case "allySummoner":
      return "队友召唤师"
    case "enemySummoner":
      return "对手召唤师"
    case "allyChampion":
      return "友方英雄"
    case "enemyChampion":
      return "敌方英雄"
    default:
      return "对象"
  }
}

function encounterPairingColumnLabel() {
  switch (activeStatsTable.value) {
    case "allyChampion":
    case "enemyChampion":
      return "我的英雄"
    default:
      return "常用英雄"
  }
}

function topChampionText(entry: UsageStatsRow) {
  if (!entry.topChampions.length) return "暂无"
  return entry.topChampions
    .map((champion) => `${championName(props.champions, champion.championId)} ${champion.games}场`)
    .join(" / ")
}

function topEncounterChampionText(entry: EncounterStatsRow) {
  if (!entry.topChampions.length) return "暂无"
  return entry.topChampions
    .map((champion) => `${championName(props.champions, champion.championId)} ${champion.games}场`)
    .join(" / ")
}

function monthDayText(timestamp: number) {
  if (!timestamp) return "-"
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
  }).format(new Date(timestamp))
}

function hourMinuteText(timestamp: number) {
  if (!timestamp) return "-"
  return new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp))
}

function durationText(seconds: number) {
  if (!seconds) return "-"
  const minutes = Math.floor(seconds / 60)
  const restSeconds = Math.floor(seconds % 60)
  return `${minutes}:${String(restSeconds).padStart(2, "0")}`
}

function playerEloSummaryText() {
  if (playerEloLoading.value) return "后台计算中，结果会自动刷新"
  if (playerEloEstimate.value) {
    return `样本 ${playerEloEstimate.value.sampleGames} 局，去极值后 ${playerEloEstimate.value.trimmedGames} 局`
  }
  if (playerEloError.value) return playerEloError.value
  return "等待可用样本"
}

function playerEloDetailText() {
  if (!playerEloEstimate.value) return "最近最多 10 局单人开局，每局取单人玩家最高 ELO"
  return `均值 ${fixed(playerEloEstimate.value.averageElo, 1)} · 中位 ${fixed(playerEloEstimate.value.medianElo, 1)}`
}

function isGameLeader(
  game: RecentGame,
  kind: "damage" | "gold" | "mitigation" | "healing" | "conversion",
) {
  switch (kind) {
    case "damage":
      return game.gameDamageLeader || game.teamDamageLeader
    case "gold":
      return game.teamGoldLeader
    case "mitigation":
      return game.teamMitigationLeader
    case "healing":
      return game.teamHealingLeader
    case "conversion":
      return game.teamDamageConversionLeader
  }
}

async function copyChampionStatsImage() {
  if (!props.stats || shareBusy.value) return
  shareBusy.value = "champions"
  try {
    await waitForCaptureRender()
    const target = championStatsCaptureRef.value
    if (!target) throw new Error("截图区域未就绪")
    await copyElementAsPng(target)
    notify({
      kind: "success",
      title: "单英雄战绩图片已复制",
      message: `已截取前 ${shareChampionStats.value.length} 个英雄`,
    })
  } catch (error) {
    notify({
      kind: "error",
      title: "单英雄战绩图片生成失败",
      message: errorMessage(error),
      duration: 7000,
    })
  } finally {
    shareBusy.value = null
  }
}

async function copyChampionGamesImage() {
  if (!selectedChampionId.value || shareBusy.value) return
  shareBusy.value = "games"
  try {
    await waitForCaptureRender()
    const target = championGamesCaptureRef.value
    if (!target) throw new Error("截图区域未就绪")
    await copyElementAsPng(target)
    notify({
      kind: "success",
      title: "单英雄具体战绩图片已复制",
      message: `${selectedChampionLabel.value} · ${shareSelectedChampionGames.value.length} 局`,
    })
  } catch (error) {
    notify({
      kind: "error",
      title: "单英雄具体战绩图片生成失败",
      message: errorMessage(error),
      duration: 7000,
    })
  } finally {
    shareBusy.value = null
  }
}

async function waitForCaptureRender() {
  await nextTick()
  await new Promise((resolve) => window.setTimeout(resolve, 600))
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

async function openHiddenRating() {
  try {
    await openUrl(HIDDEN_RATING_URL)
  } catch (error) {
    notify({
      kind: "error",
      title: "隐藏分页面打开失败",
      message: errorMessage(error),
      duration: 7000,
    })
  }
}
</script>

<template>
  <section class="empty-panel" v-if="!stats">
    <div class="empty-title">暂无数据</div>
    <div class="empty-subtitle">统计面板空闲</div>
  </section>

  <section class="stats" v-else ref="statsRootRef">
    <div class="stats-overview">
      <div
        v-if="ELO_FEATURE_ENABLED"
        class="metric metric-elo metric-elo-wide"
        :title="playerEloEstimate ? playerEloEstimate.rawGameMaxElos.join(' / ') : ''"
      >
        <div class="metric-head">
          <span>角色 ELO</span>
          <button
            class="metric-action"
            type="button"
            :disabled="playerEloLoading"
            @click="refreshPlayerEloEstimateFromSource"
          >
            {{ playerEloLoading ? "刷新中..." : "清缓存刷新" }}
          </button>
        </div>
        <div class="metric-elo-body">
          <strong>{{ playerEloLoading ? "计算中..." : playerEloEstimate ? fixed(playerEloEstimate.finalElo, 1) : "--" }}</strong>
          <div class="metric-elo-meta">
            <small>{{ playerEloSummaryText() }}</small>
            <small>{{ playerEloDetailText() }}</small>
            <small class="metric-note">成功结果缓存 1 天</small>
          </div>
        </div>
      </div>

      <div class="overview-main">
        <div class="stats-owner-heading">
          <span>当前角色</span>
          <strong>{{ statsOwnerLabel }}</strong>
          <button
            class="hidden-rating-button"
            type="button"
            title="在默认浏览器中打开 ARAMKit 隐藏分查询"
            @click="openHiddenRating"
          >
            <ExternalLink :size="14" />
            隐藏分查询
          </button>
        </div>
        <div class="metric-grid">
          <div class="metric">
            <span>胜率</span>
            <strong>{{ stats.summary.games }}场 {{ percent(stats.summary.winRate) }}胜率</strong>
            <small>
              {{ stats.summary.wins }}胜 {{ stats.summary.losses }}负
            </small>
          </div>
          <div class="metric">
            <span>KDA</span>
            <strong>{{ fixed(stats.summary.averageKda) }}</strong>
            <small>
              {{ fixed(stats.summary.averageKills, 1) }} /
              {{ fixed(stats.summary.averageDeaths, 1) }} /
              {{ fixed(stats.summary.averageAssists, 1) }}
            </small>
          </div>
          <div class="metric">
            <span>英雄池</span>
            <strong>{{ stats.summary.uniqueChampions }}</strong>
            <small class="metric-avatar metric-avatar-stack">
              <ChampionAvatar
                v-for="championId in topChampionIds"
                :key="championId"
                :champion-id="championId"
                :champions="champions"
                :size="30"
              />
            </small>
          </div>
        </div>

        <section class="panel profile-panel">
          <div class="profile-heading">
            <div class="section-title">玩家画像</div>
          </div>

          <div class="profile-layout">
            <div
              :class="[
                'profile-score',
                profileLevelClass(playerProfile.overallScore, playerProfile.games),
              ]"
            >
              <div class="profile-score-line">
                <span>综合分</span>
                <strong>{{ scoreText(playerProfile.overallScore, playerProfile.games) }}</strong>
              </div>
              <em>{{ mainRoleText(playerProfile.mainRoleLabel) }}</em>
            </div>

            <div class="profile-tags">
              <span
                v-for="tag in playerProfile.tags"
                :key="tag"
                :class="tagToneClass(tag)"
              >
                {{ tag }}
              </span>
            </div>
          </div>

          <div class="ability-grid">
            <article
              v-for="ability in abilityCards"
              :key="ability.key"
              :class="['ability-item', profileLevelClass(ability.averageScore, ability.games)]"
            >
              <header>
                <span>{{ ability.label }}</span>
                <strong>{{ scoreText(ability.averageScore, ability.games) }}</strong>
              </header>
              <div>
                <span>场数 <b>{{ ability.games }}</b></span>
                <span>占比 <b>{{ percent(abilityRate(ability.games)) }}</b></span>
              </div>
              <footer>
                <span>高光局 <b>{{ percent(ability.highlightRate) }}</b></span>
                <span>战犯局 <b>{{ percent(ability.disasterRate) }}</b></span>
              </footer>
            </article>
          </div>
        </section>
      </div>

      <aside class="panel role-bar-panel">
        <div class="role-bar-scroll">
          <div
            v-for="role in roleBars"
            :key="role.label"
            class="role-bar-row"
            :style="{ '--role-color': role.color }"
          >
            <div class="role-bar-head">
              <strong>{{ role.label }}</strong>
              <span>{{ percent(role.rate) }}</span>
            </div>
            <div class="role-bar-track">
              <div class="role-bar-fill" :style="{ width: role.width }"></div>
            </div>
            <div class="role-bar-meta">
              <span>{{ role.games }} 场</span>
              <span>胜率 {{ percent(role.winRate) }}</span>
            </div>
          </div>
        </div>
      </aside>
    </div>

    <div class="fun-stats-entry" v-if="!selectedStatsGamesFilter">
      <button
        class="data-filter-button"
        type="button"
        :disabled="funStatsLoading || stats.recentGames.length === 0"
        @click="toggleFunStats"
      >
        <span v-if="dataFilterHintVisible" class="data-filter-hint" aria-hidden="true"></span>
        <Sparkles :size="16" />
        {{ funStatsLoading ? "统计中" : funStatsVisible ? "收起数据筛选" : "数据筛选" }}
      </button>
    </div>

    <FunStatsPanel
      v-if="!selectedStatsGamesFilter && funStatsVisible && funStatsResult"
      :result="funStatsResult"
      :loading="funStatsLoading"
      :games="stats.recentGames"
      :items="gameAssets.items"
      :augments="funAugmentAssets"
      :champions="champions"
      @refresh="loadFunStats(true)"
      @equipment-change="updateFunEquipment"
      @penetration-change="updateFunPenetration"
      @augment-change="updateFunAugment"
    />

    <div class="content-grid">
      <section class="panel table-panel" v-if="!selectedStatsGamesFilter">
        <div class="panel-heading">
          <div class="table-heading-block">
            <div class="section-title">{{ statsTableTitle() }}</div>
            <div class="stats-table-tabs">
              <button :class="{ active: activeStatsTable === 'champion' }" @click="activateStatsTable('champion')">
                英雄
              </button>
              <button :class="{ active: activeStatsTable === 'augment' }" @click="activateStatsTable('augment')">
                海克斯
              </button>
              <button :class="{ active: activeStatsTable === 'item' }" @click="activateStatsTable('item')">
                装备
              </button>
              <button :class="{ active: activeStatsTable === 'allySummoner' }" @click="activateStatsTable('allySummoner')">
                队友召唤师
              </button>
              <button :class="{ active: activeStatsTable === 'enemySummoner' }" @click="activateStatsTable('enemySummoner')">
                对手召唤师
              </button>
              <button :class="{ active: activeStatsTable === 'allyChampion' }" @click="activateStatsTable('allyChampion')">
                友方英雄
              </button>
              <button :class="{ active: activeStatsTable === 'enemyChampion' }" @click="activateStatsTable('enemyChampion')">
                敌方英雄
              </button>
            </div>
          </div>
          <div class="panel-heading-actions">
            <div v-if="activeStatsTable === 'augment'" class="augment-rarity-filters">
              <button
                :class="{ active: augmentRarityFilter === 'all' }"
                @click="toggleAugmentRarityFilter('all')"
              >
                全部
              </button>
              <button
                class="silver"
                :class="{ active: augmentRarityFilter === 'silver' }"
                @click="toggleAugmentRarityFilter('silver')"
              >
                银色
              </button>
              <button
                class="gold"
                :class="{ active: augmentRarityFilter === 'gold' }"
                @click="toggleAugmentRarityFilter('gold')"
              >
                黄金
              </button>
              <button
                class="prismatic"
                :class="{ active: augmentRarityFilter === 'prismatic' }"
                @click="toggleAugmentRarityFilter('prismatic')"
              >
                棱彩
              </button>
            </div>
            <label class="champion-search">
              <input
                v-model="championSearch"
                type="search"
                :placeholder="tableSearchPlaceholder()"
                autocomplete="off"
                spellcheck="false"
              />
            </label>
            <button
              class="share-action"
              v-if="activeStatsTable === 'champion'"
              :disabled="shareBusy !== null || shareChampionStats.length === 0"
              @click="copyChampionStatsImage"
            >
              <ClipboardCopy :size="14" />
              {{ shareBusy === "champions" ? "生成中" : `分享前 ${shareChampionStats.length} 个` }}
            </button>
          </div>
        </div>
        <div class="table-wrap" ref="tableWrapRef">
          <table v-if="activeStatsTable === 'champion'" class="champion-stats-table">
            <thead>
              <tr>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('name')">
                    英雄
                    <ArrowUp v-if="sortIconState(championSort, 'name') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'name') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('games')">
                    场次
                    <ArrowUp v-if="sortIconState(championSort, 'games') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'games') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('winRate')">
                    胜率
                    <ArrowUp v-if="sortIconState(championSort, 'winRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'winRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('averageScore')">
                    评分
                    <ArrowUp v-if="sortIconState(championSort, 'averageScore') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'averageScore') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>评价</th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('label')">
                    标签
                    <ArrowUp v-if="sortIconState(championSort, 'label') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'label') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('averageKda')">
                    K / D / A
                    <ArrowUp v-if="sortIconState(championSort, 'averageKda') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'averageKda') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('damageShare')">
                    伤害占比
                    <ArrowUp v-if="sortIconState(championSort, 'damageShare') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'damageShare') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('damageConversionRate')">
                    伤害转化率
                    <ArrowUp v-if="sortIconState(championSort, 'damageConversionRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'damageConversionRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('mitigationShare')">
                    承担占比
                    <ArrowUp v-if="sortIconState(championSort, 'mitigationShare') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'mitigationShare') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleChampionSort('healingShare')">
                    治疗占比
                    <ArrowUp v-if="sortIconState(championSort, 'healingShare') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(championSort, 'healingShare') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="champ in filteredChampionStats"
                :key="champ.championId"
                class="champion-row"
                tabindex="0"
                @click="openChampionGames(champ.championId)"
                @keydown.enter="openChampionGames(champ.championId)"
                @keydown.space.prevent="openChampionGames(champ.championId)"
              >
                <td>
                  <ChampionAvatar :champion-id="champ.championId" :champions="champions" :size="30" />
                </td>
                <td>{{ champ.games }}</td>
                <td>{{ percent(champ.winRate) }}</td>
                <td>
                  <span
                    :class="[
                      'champion-score',
                      profileLevelClass(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      ),
                    ]"
                  >
                    {{
                      scoreText(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      )
                    }}
                  </span>
                  <small class="champion-role">
                    {{ championProfile(champ.championId).mainRoleLabel }}
                  </small>
                </td>
                <td>
                  <span
                    :class="[
                      'champion-label-tag',
                      tagToneClass(
                        profileEvaluationText(
                          championProfile(champ.championId).averageScore,
                          championProfile(champ.championId).games,
                        ),
                      ),
                    ]"
                  >
                    {{
                      profileEvaluationText(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      )
                    }}
                  </span>
                </td>
                <td>
                  <span
                    :class="[
                      'champion-label-tag',
                      championTagToneClass(championProfile(champ.championId).labelBand),
                    ]"
                  >
                    {{ championProfile(champ.championId).label }}
                  </span>
                </td>
                <td>
                  <span class="kda-total">
                    {{ fixed(champ.averageKills, 1) }} /
                    {{ fixed(champ.averageDeaths, 1) }} /
                    {{ fixed(champ.averageAssists, 1) }}
                  </span>
                  <small class="kda-detail">
                    KDA {{ fixed(champ.averageKda) }}
                  </small>
                </td>
                <td>{{ percent(champ.damageShare) }}</td>
                <td>{{ fixed(champ.damageConversionRate) }}</td>
                <td>{{ percent(champ.mitigationShare) }}</td>
                <td>{{ percent(champ.healingShare) }}</td>
              </tr>
            </tbody>
          </table>

          <table v-else-if="activeStatsTable === 'augment'">
            <thead>
              <tr>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('name')">
                    海克斯
                    <ArrowUp v-if="sortIconState(augmentSort, 'name') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'name') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('games')">
                    出场
                    <ArrowUp v-if="sortIconState(augmentSort, 'games') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'games') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('pickRate')">
                    出场率
                    <ArrowUp v-if="sortIconState(augmentSort, 'pickRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'pickRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('winRate')">
                    胜率
                    <ArrowUp v-if="sortIconState(augmentSort, 'winRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'winRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('label')">
                    称号
                    <ArrowUp v-if="sortIconState(augmentSort, 'label') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'label') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('topChampion')">
                    常用英雄
                    <ArrowUp v-if="sortIconState(augmentSort, 'topChampion') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'topChampion') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleAugmentSort('lastSeenAt')">
                    最近出现
                    <ArrowUp v-if="sortIconState(augmentSort, 'lastSeenAt') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(augmentSort, 'lastSeenAt') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="entry in filteredAugmentStats"
                :key="entry.id"
                class="stats-entry-row"
                tabindex="0"
                @click="openUsageGames('augment', entry)"
                @keydown.enter="openUsageGames('augment', entry)"
                @keydown.space.prevent="openUsageGames('augment', entry)"
              >
                <td>
                  <div class="asset-cell">
                    <AssetIcon
                      :class="['augment-table-icon', entry.rarityClass]"
                      :path="entry.iconPath"
                      :label="entry.name"
                      :fallback="String(entry.id)"
                      :size="28"
                    />
                    <div class="asset-meta">
                      <strong>{{ entry.name }}</strong>
                      <small :class="['asset-tag', entry.rarityClass]">{{ entry.id }}</small>
                    </div>
                  </div>
                </td>
                <td>{{ entry.games }} 场</td>
                <td>{{ percent(entry.pickRate) }}</td>
                <td>{{ percent(entry.winRate) }}</td>
                <td><span class="champion-label-tag tag-neutral">{{ entry.label }}</span></td>
                <td>
                  <div class="usage-champions" :title="topChampionText(entry)">
                    <span
                      v-for="champion in entry.topChampions"
                      :key="champion.championId"
                      class="usage-champion-chip"
                    >
                      {{ championName(champions, champion.championId) }} {{ champion.games }}
                    </span>
                  </div>
                </td>
                <td>{{ monthDayText(entry.lastSeenAt) }}</td>
              </tr>
            </tbody>
          </table>

          <table v-else-if="activeStatsTable === 'item'">
            <thead>
              <tr>
                <th>
                  <button class="table-sort" @click="toggleItemSort('name')">
                    装备
                    <ArrowUp v-if="sortIconState(itemSort, 'name') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'name') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleItemSort('games')">
                    出场
                    <ArrowUp v-if="sortIconState(itemSort, 'games') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'games') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleItemSort('pickRate')">
                    出场率
                    <ArrowUp v-if="sortIconState(itemSort, 'pickRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'pickRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleItemSort('winRate')">
                    胜率
                    <ArrowUp v-if="sortIconState(itemSort, 'winRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'winRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleItemSort('label')">
                    称号
                    <ArrowUp v-if="sortIconState(itemSort, 'label') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'label') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleItemSort('topChampion')">
                    常用英雄
                    <ArrowUp v-if="sortIconState(itemSort, 'topChampion') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'topChampion') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleItemSort('lastSeenAt')">
                    最近出现
                    <ArrowUp v-if="sortIconState(itemSort, 'lastSeenAt') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(itemSort, 'lastSeenAt') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="entry in filteredItemStats"
                :key="entry.id"
                class="stats-entry-row"
                tabindex="0"
                @click="openUsageGames('item', entry)"
                @keydown.enter="openUsageGames('item', entry)"
                @keydown.space.prevent="openUsageGames('item', entry)"
              >
                <td>
                  <div class="asset-cell">
                    <AssetIcon
                      :path="entry.iconPath"
                      :label="entry.name"
                      :fallback="String(entry.id)"
                      :size="28"
                    />
                    <div class="asset-meta">
                      <strong>{{ entry.name }}</strong>
                      <small>{{ entry.id }}</small>
                    </div>
                  </div>
                </td>
                <td>{{ entry.games }} 场</td>
                <td>{{ percent(entry.pickRate) }}</td>
                <td>{{ percent(entry.winRate) }}</td>
                <td><span class="champion-label-tag tag-neutral">{{ entry.label }}</span></td>
                <td>
                  <div class="usage-champions" :title="topChampionText(entry)">
                    <span
                      v-for="champion in entry.topChampions"
                      :key="champion.championId"
                      class="usage-champion-chip"
                    >
                      {{ championName(champions, champion.championId) }} {{ champion.games }}
                    </span>
                  </div>
                </td>
                <td>{{ monthDayText(entry.lastSeenAt) }}</td>
              </tr>
            </tbody>
          </table>

          <table v-else>
            <thead>
              <tr>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('name')">
                    {{ encounterEntityColumnLabel() }}
                    <ArrowUp v-if="sortIconState(encounterSort, 'name') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'name') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('games')">
                    出场
                    <ArrowUp v-if="sortIconState(encounterSort, 'games') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'games') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('pickRate')">
                    出场率
                    <ArrowUp v-if="sortIconState(encounterSort, 'pickRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'pickRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('winRate')">
                    胜率
                    <ArrowUp v-if="sortIconState(encounterSort, 'winRate') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'winRate') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('label')">
                    称号
                    <ArrowUp v-if="sortIconState(encounterSort, 'label') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'label') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('topChampion')">
                    {{ encounterPairingColumnLabel() }}
                    <ArrowUp v-if="sortIconState(encounterSort, 'topChampion') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'topChampion') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
                <th>
                  <button class="table-sort" @click="toggleEncounterSort('lastSeenAt')">
                    最近出现
                    <ArrowUp v-if="sortIconState(encounterSort, 'lastSeenAt') === 'asc'" :size="14" />
                    <ArrowDown v-else-if="sortIconState(encounterSort, 'lastSeenAt') === 'desc'" :size="14" />
                    <ChevronsUpDown v-else :size="14" />
                  </button>
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="entry in filteredEncounterStats"
                :key="entry.key"
                class="stats-entry-row"
                tabindex="0"
                @click="openEncounterGames(entry)"
                @keydown.enter="openEncounterGames(entry)"
                @keydown.space.prevent="openEncounterGames(entry)"
              >
                <td>
                  <div class="asset-cell">
                    <ChampionAvatar
                      v-if="entry.entityType === 'champion' && entry.championId"
                      :champion-id="entry.championId"
                      :champions="champions"
                      :size="30"
                    />
                    <div class="asset-meta">
                      <strong>{{ entry.name }}</strong>
                      <small>{{ entry.entityType === "champion" ? "英雄" : "召唤师" }}</small>
                    </div>
                  </div>
                </td>
                <td>{{ entry.games }} 场</td>
                <td>{{ percent(entry.pickRate) }}</td>
                <td>{{ percent(entry.winRate) }}</td>
                <td><span class="champion-label-tag tag-neutral">{{ entry.label }}</span></td>
                <td>
                  <div class="usage-champions" :title="topEncounterChampionText(entry)">
                    <span
                      v-for="champion in entry.topChampions"
                      :key="champion.championId"
                      class="usage-champion-chip"
                    >
                      {{ championName(champions, champion.championId) }} {{ champion.games }}
                    </span>
                  </div>
                </td>
                <td>{{ monthDayText(entry.lastSeenAt) }}</td>
              </tr>
            </tbody>
          </table>

          <div class="table-empty" v-if="activeStatsTable === 'champion' && filteredChampionStats.length === 0">
            没有匹配到英雄
          </div>
          <div class="table-empty" v-else-if="activeStatsTable === 'augment' && filteredAugmentStats.length === 0">
            没有匹配到海克斯
          </div>
          <div class="table-empty" v-else-if="activeStatsTable === 'item' && filteredItemStats.length === 0">
            没有匹配到装备
          </div>
          <div v-else-if="filteredEncounterStats.length === 0" class="table-empty">
            没有匹配到对象
          </div>
        </div>
      </section>

      <section class="panel stats-games-panel" v-else>
        <div class="detail-header">
          <div class="detail-title-group">
            <button @click="closeStatsGames">返回</button>
            <div>
              <div class="section-title">{{ selectedStatsGamesTitle }}</div>
              <span>已显示 {{ visibleSelectedStatsGames.length }} / {{ selectedStatsGames.length }} 局</span>
            </div>
          </div>
          <button
            v-if="selectedChampionId"
            class="share-action"
            :disabled="shareBusy !== null || shareSelectedChampionGames.length === 0"
            @click="copyChampionGamesImage"
          >
            <ClipboardCopy :size="14" />
            {{ shareBusy === "games" ? "生成中" : `分享前 ${shareSelectedChampionGames.length} 场` }}
          </button>
        </div>

        <GameRecordList
          :games="visibleSelectedStatsGames"
          :champions="champions"
          :game-assets="gameAssets"
          :sgp-server-id="sgpServerId"
          :owner-label="ownerLabel"
          :owner-puuid="ownerPuuid"
          external-detail
          @open-match="emit('openMatch', $event)"
        />
        <button
          v-if="hasMoreSelectedStatsGames"
          ref="detailLoadSentinelRef"
          class="detail-load-more"
          type="button"
          @click="loadMoreSelectedStatsGames"
        >
          继续下滑加载更多
        </button>
        <div v-else-if="selectedStatsGames.length" class="detail-load-complete">
          已加载全部 {{ selectedStatsGames.length }} 场
        </div>
      </section>
    </div>

    <div
      class="share-capture-root"
      :class="{ mobile: shareSettings.mobileShareLayout }"
      aria-hidden="true"
    >
      <section class="share-card" ref="championStatsCaptureRef">
        <header>
          <strong>单英雄战绩</strong>
          <span>前 {{ shareChampionStats.length }} 个英雄</span>
        </header>
        <div class="mobile-champion-list" v-if="shareSettings.mobileShareLayout">
          <article v-for="champ in shareChampionStats" :key="champ.championId">
            <div class="mobile-champion-head">
              <ChampionAvatar
                class="mobile-avatar"
                :champion-id="champ.championId"
                :champions="champions"
                :size="28"
              />
              <strong class="mobile-champion-name">{{ championName(champions, champ.championId) }}</strong>
              <span class="mobile-chip">{{ champ.games }} 场</span>
              <span class="mobile-chip">胜率 {{ percent(champ.winRate) }}</span>
              <span class="mobile-chip score">
                {{
                  scoreText(
                    championProfile(champ.championId).averageScore,
                    championProfile(champ.championId).games,
                  )
                }}
              </span>
              <span class="mobile-chip">
                {{
                  profileEvaluationText(
                    championProfile(champ.championId).averageScore,
                    championProfile(champ.championId).games,
                  )
                }}
              </span>
              <span
                :class="[
                  'mobile-chip',
                  championTagToneClass(championProfile(champ.championId).labelBand),
                ]"
              >
                {{ championProfile(champ.championId).label }}
              </span>
            </div>
            <div class="mobile-champion-stats">
              <div>
                <span>KDA</span>
                <strong>{{ fixed(champ.averageKda) }}</strong>
                <em>
                  {{ fixed(champ.averageKills, 1) }}/{{ fixed(champ.averageDeaths, 1) }}/{{ fixed(champ.averageAssists, 1) }}
                </em>
              </div>
              <div>
                <span>伤害</span>
                <strong>{{ percent(champ.damageShare) }}</strong>
              </div>
              <div>
                <span>伤转</span>
                <strong>{{ fixed(champ.damageConversionRate) }}</strong>
              </div>
              <div>
                <span>承担</span>
                <strong>{{ percent(champ.mitigationShare) }}</strong>
              </div>
              <div>
                <span>治疗</span>
                <strong>{{ percent(champ.healingShare) }}</strong>
              </div>
            </div>
          </article>
        </div>
        <table class="share-table" v-else>
          <thead>
            <tr>
              <th>英雄</th>
              <th>场次</th>
              <th>胜率</th>
              <th>评分</th>
              <th>评价</th>
              <th>标签</th>
              <th>K / D / A</th>
              <th>伤害占比</th>
              <th>伤害转化率</th>
              <th>承担占比</th>
              <th>治疗占比</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="champ in shareChampionStats" :key="champ.championId">
              <td>
                <span class="share-champion">
                  <ChampionAvatar :champion-id="champ.championId" :champions="champions" :size="30" />
                  <b>{{ championName(champions, champ.championId) }}</b>
                </span>
              </td>
              <td>{{ champ.games }}</td>
              <td>{{ percent(champ.winRate) }}</td>
              <td>
                {{
                  scoreText(
                    championProfile(champ.championId).averageScore,
                    championProfile(champ.championId).games,
                  )
                }}
                <small>{{ championProfile(champ.championId).mainRoleLabel }}</small>
              </td>
              <td>
                <span
                  :class="[
                    'champion-label-tag',
                    tagToneClass(
                      profileEvaluationText(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      ),
                    ),
                  ]"
                >
                  {{
                    profileEvaluationText(
                      championProfile(champ.championId).averageScore,
                      championProfile(champ.championId).games,
                    )
                  }}
                </span>
              </td>
              <td>
                <span
                  :class="[
                    'champion-label-tag',
                    championTagToneClass(championProfile(champ.championId).labelBand),
                  ]"
                >
                  {{ championProfile(champ.championId).label }}
                </span>
              </td>
              <td>
                {{ fixed(champ.averageKills, 1) }} /
                {{ fixed(champ.averageDeaths, 1) }} /
                {{ fixed(champ.averageAssists, 1) }}
                <small>KDA {{ fixed(champ.averageKda) }}</small>
              </td>
              <td>{{ percent(champ.damageShare) }}</td>
              <td>{{ fixed(champ.damageConversionRate) }}</td>
              <td>{{ percent(champ.mitigationShare) }}</td>
              <td>{{ percent(champ.healingShare) }}</td>
            </tr>
          </tbody>
        </table>
      </section>

      <section class="share-card share-games-card" ref="championGamesCaptureRef">
        <header>
          <strong>{{ selectedChampionLabel }} · 单英雄战绩</strong>
          <span>前 {{ shareSelectedChampionGames.length }} 场</span>
        </header>
        <div class="mobile-games-list" v-if="shareSettings.mobileShareLayout">
          <article
            v-for="game in shareSelectedChampionGames"
            :key="game.gameId"
            :class="{ win: game.win, lose: !game.win }"
          >
            <div class="mobile-game-loadout">
              <div class="mobile-game-time">
                <strong>{{ monthDayText(game.gameCreation) }}</strong>
                <span>{{ hourMinuteText(game.gameCreation) }}</span>
                <em>{{ durationText(game.gameDuration) }}</em>
              </div>
              <div class="mobile-game-hero">
                <ChampionAvatar
                  class="mobile-avatar"
                  :champion-id="game.championId"
                  :champions="champions"
                  :size="30"
                />
                <span>{{ queueName(game) }}</span>
              </div>
              <div class="mobile-spells">
                <AssetIcon
                  v-if="game.spell1Id"
                  :path="spellMap[game.spell1Id]?.iconPath"
                  :label="spellMap[game.spell1Id]?.name"
                  :fallback="String(game.spell1Id)"
                  :size="16"
                />
                <AssetIcon
                  v-if="game.spell2Id"
                  :path="spellMap[game.spell2Id]?.iconPath"
                  :label="spellMap[game.spell2Id]?.name"
                  :fallback="String(game.spell2Id)"
                  :size="16"
                />
              </div>
              <div class="mobile-items-kda">
                <div class="mobile-items">
                  <AssetIcon
                    v-for="itemId in game.itemIds"
                    :key="itemId"
                    :path="itemMap[itemId]?.iconPath"
                    :label="itemMap[itemId]?.name"
                    :fallback="String(itemId)"
                    :size="17"
                  />
                </div>
                <strong>{{ game.kills }}/{{ game.deaths }}/{{ game.assists }}</strong>
              </div>
              <div class="mobile-runes text-runes" v-if="game.augmentIds.length">
                <span
                  v-for="augmentId in game.augmentIds.slice(0, 4)"
                  :key="augmentId"
                  :class="augmentRarityClass(augmentId)"
                >
                  {{ shortAugmentName(augmentId) }}
                </span>
              </div>
              <div class="mobile-runes" v-else>
                <AssetIcon
                  v-for="perkId in game.perkIds.slice(0, 4)"
                  :key="perkId"
                  :path="perkMap[perkId]?.iconPath"
                  :label="perkMap[perkId]?.name"
                  :fallback="String(perkId)"
                  :size="16"
                />
              </div>
            </div>
            <div class="mobile-game-stats">
              <div :class="{ leader: isGameLeader(game, 'damage') }">
                <span>伤害</span>
                <strong>{{ kNumber(game.damageToChampions) }}</strong>
                <em>{{ shareText(game.damageToChampions, game.teamDamageToChampions) }}</em>
              </div>
              <div :class="{ leader: isGameLeader(game, 'gold') }">
                <span>经济</span>
                <strong>{{ kNumber(game.goldEarned) }}</strong>
                <em>{{ shareText(game.goldEarned, game.teamGoldEarned) }}</em>
              </div>
              <div :class="{ leader: isGameLeader(game, 'mitigation') }">
                <span>承伤</span>
                <strong>{{ kNumber(mitigationValue(game)) }}</strong>
                <em>{{ shareText(mitigationValue(game), teamMitigationValue(game)) }}</em>
              </div>
              <div :class="{ leader: isGameLeader(game, 'healing') }">
                <span>治疗</span>
                <strong>{{ kNumber(game.totalHeal) }}</strong>
                <em>{{ shareText(game.totalHeal, game.teamTotalHeal) }}</em>
              </div>
              <div :class="{ leader: isGameLeader(game, 'conversion') }">
                <span>伤转</span>
                <strong>{{ gameDamageConversion(game) }}</strong>
              </div>
            </div>
          </article>
        </div>
        <GameRecordList
          v-else
          :games="shareSelectedChampionGames"
          :champions="champions"
          :game-assets="gameAssets"
          :sgp-server-id="sgpServerId"
          disable-detail
        />
      </section>
    </div>

  </section>
</template>

<style scoped>
.empty-panel,
.stats {
  min-height: 100%;
}

.empty-panel {
  display: grid;
  place-content: center;
  gap: 8px;
  color: #657179;
  text-align: center;
}

.empty-title {
  color: #263238;
  font-size: 22px;
  font-weight: 700;
}

.empty-subtitle {
  font-size: 14px;
}

.stats {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.stats-overview {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 390px;
  gap: 14px;
  align-items: start;
}

.overview-main {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 14px;
}

.metric-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.metric,
.panel {
  background: #ffffff;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  box-shadow: 0 12px 28px rgba(32, 67, 73, 0.08);
}

.metric {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 16px;
}

.metric-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.metric span,
.metric small {
  color: #6c7a80;
  font-size: 13px;
}

.metric-action {
  border: 1px solid rgba(75, 63, 191, 0.18);
  border-radius: 999px;
  background: rgba(75, 63, 191, 0.08);
  color: #4b3fbf;
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
  padding: 6px 10px;
  transition: background 0.18s ease, border-color 0.18s ease, color 0.18s ease;
}

.metric-action:hover:not(:disabled) {
  background: rgba(75, 63, 191, 0.14);
  border-color: rgba(75, 63, 191, 0.28);
}

.metric-action:disabled {
  cursor: default;
  opacity: 0.6;
}

.metric strong {
  color: #20333a;
  font-size: 28px;
  line-height: 1.1;
}

.metric-elo {
  box-sizing: border-box;
  width: 100%;
  min-height: 148px;
  border-color: #d7d3ff;
  background: linear-gradient(180deg, #faf8ff 0%, #ffffff 100%);
}

.metric-elo-wide {
  grid-column: 1 / -1;
  gap: 12px;
  padding: 18px 20px;
}

.metric-elo .metric-head {
  align-items: flex-start;
}

.metric-elo strong {
  color: #4b3fbf;
}

.metric-elo-body {
  display: grid;
  grid-template-columns: minmax(170px, 240px) minmax(0, 1fr);
  gap: 16px;
  align-items: end;
}

.metric-elo-wide strong {
  font-size: 58px;
  line-height: 0.95;
}

.metric-elo-meta {
  display: flex;
  min-width: 0;
  flex-direction: column;
  justify-content: flex-end;
  gap: 4px;
}

.metric-elo small {
  font-size: 14px;
  line-height: 1.35;
  word-break: break-word;
}

.metric-note {
  margin-top: 2px;
}

.metric-avatar {
  display: flex;
  align-items: center;
  min-height: 32px;
}

.metric-avatar-stack {
  gap: 4px;
}

.metric-avatar-stack :deep(.champion-avatar) {
  flex: 0 0 auto;
}

.profile-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.profile-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.profile-heading .section-title {
  margin-bottom: 0;
}

.profile-heading span {
  color: #718087;
  font-size: 12px;
  font-weight: 800;
}

.profile-layout {
  display: grid;
  grid-template-columns: 250px minmax(0, 1fr);
  gap: 10px;
  align-items: stretch;
}

.profile-score {
  display: flex;
  min-height: 92px;
  flex-direction: column;
  justify-content: center;
  gap: 5px;
  border-radius: 8px;
  padding: 12px;
}

.profile-score-line {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
}

.profile-score-line span,
.profile-score-line strong {
  color: inherit;
  font-size: 30px;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.profile-score em {
  font-size: 16px;
  font-style: normal;
  font-weight: 900;
}

.profile-tags {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-content: center;
  gap: 7px;
  border-radius: 8px;
  background: #f7fbfa;
  padding: 12px;
}

.profile-tags span {
  border-radius: 999px;
  font-size: 13px;
  font-weight: 900;
  line-height: 1;
  padding: 7px 10px;
  white-space: nowrap;
}

.ability-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}

.ability-item {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 8px;
  border-radius: 8px;
  background: #f7fbfa;
  padding: 11px 12px;
}

.ability-item header,
.ability-item div,
.ability-item footer {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.ability-item header span {
  color: inherit;
  font-size: 20px;
  font-weight: 950;
  line-height: 1;
}

.ability-item header strong {
  font-size: 23px;
  font-weight: 950;
  line-height: 1;
}

.ability-item div span,
.ability-item footer span {
  color: #5f7076;
  font-size: 14px;
  font-weight: 900;
  white-space: nowrap;
}

.ability-item div b,
.ability-item footer b {
  color: #20333a;
  font-weight: 950;
}

.role-bar-panel {
  contain: size;
  align-self: start;
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
  padding: 0;
}

.role-bar-scroll {
  display: flex;
  width: 100%;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  gap: 12px;
  overflow-x: hidden;
  overflow-y: auto;
  padding: 12px;
  scrollbar-gutter: stable;
  scrollbar-width: thin;
}

.role-bar-row {
  --role-color: #2f78d6;
  display: flex;
  flex: 0 0 auto;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
  border: 1px solid #e0ebe8;
  border-radius: 8px;
  background: #fbfdfc;
  padding: 10px;
}

.role-bar-head,
.role-bar-meta {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.role-bar-head strong {
  overflow: hidden;
  color: #20333a;
  font-size: 17px;
  font-weight: 950;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.role-bar-head span {
  color: var(--role-color);
  font-size: 18px;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.role-bar-track {
  position: relative;
  overflow: hidden;
  height: 13px;
  border-radius: 999px;
  background: #eaf1ef;
}

.role-bar-fill {
  height: 100%;
  min-width: 6px;
  border-radius: inherit;
  background: linear-gradient(90deg, var(--role-color), color-mix(in srgb, var(--role-color) 72%, #ffffff));
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.42) inset;
}

.role-bar-meta span {
  color: #4f6066;
  font-size: 13px;
  font-weight: 900;
  white-space: nowrap;
}

.tag-strong {
  color: #5d3300;
  background: #ffe09b;
}

.tag-danger {
  color: #922f2f;
  background: #f9d1d0;
}

.tag-team {
  color: #135c42;
  background: #d0f0dd;
}

.tag-warn {
  color: #7b4d02;
  background: #ffe2b8;
}

.tag-neutral {
  color: #1f514a;
  background: #dceee9;
}

.tag-band-excellent,
.mobile-chip.tag-band-excellent {
  color: #5d3300;
  background: #ffe09b;
}

.tag-band-normal,
.mobile-chip.tag-band-normal {
  color: #135c42;
  background: #d0f0dd;
}

.tag-band-low,
.mobile-chip.tag-band-low {
  color: #174d83;
  background: #dcecff;
}

.tag-band-disaster,
.mobile-chip.tag-band-disaster {
  color: #922f2f;
  background: #f9d1d0;
}

.profile-excellent {
  color: #5d3300;
  background:
    linear-gradient(135deg, rgba(255, 244, 184, 0.96), rgba(255, 195, 64, 0.9) 45%, rgba(255, 236, 150, 0.96)),
    #ffd36a;
}

.profile-good {
  color: #145b3e;
  background: #d9f1df;
}

.profile-average {
  color: #174d83;
  background: #dcecff;
}

.profile-poor {
  color: #8f3434;
  background: #f8dedc;
}

.profile-empty {
  color: #657179;
  background: #edf4f2;
}

.content-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 14px;
}

.fun-stats-entry {
  display: flex;
  justify-content: flex-start;
}

.fun-stats-entry button {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  min-height: 34px;
  padding: 7px 13px;
  border: 1px solid #b9c9d9;
  border-radius: 7px;
  color: #253d5a;
  background: #eef4fa;
  cursor: pointer;
  font-size: 13px;
  font-weight: 800;
}

.data-filter-hint {
  position: absolute;
  top: -4px;
  right: -4px;
  width: 9px;
  height: 9px;
  border: 2px solid #fff;
  border-radius: 50%;
  background: #e33c4e;
  box-shadow: 0 1px 5px rgb(205 37 55 / 35%);
}

.fun-stats-entry button:hover:not(:disabled) {
  border-color: #7693b3;
  color: #17395e;
  background: #e2edf8;
}

.fun-stats-entry button:disabled {
  cursor: default;
  opacity: 0.55;
}

.panel {
  min-width: 0;
  padding: 16px;
}

.section-title {
  color: #263238;
  font-size: 16px;
  font-weight: 700;
  margin-bottom: 14px;
}

.stats-owner-heading {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 10px;
  margin-bottom: 14px;
}

.stats-owner-heading span {
  color: #6a7d83;
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.04em;
}

.stats-owner-heading strong {
  min-width: 0;
  overflow: hidden;
  color: #1d2f34;
  font-size: 22px;
  font-weight: 800;
  line-height: 1.1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hidden-rating-button {
  display: inline-flex;
  min-height: 32px;
  align-items: center;
  gap: 6px;
  margin-left: auto;
  flex: 0 0 auto;
  border: 1px solid #74449a;
  border-radius: 7px;
  padding: 6px 10px;
  color: #fff8ff;
  background: #4a205f;
  cursor: pointer;
  font-size: 13px;
  font-weight: 800;
  line-height: 1;
  white-space: nowrap;
  transition: border-color 0.18s ease, background 0.18s ease, color 0.18s ease;
}

.hidden-rating-button:hover {
  border-color: #9b6abd;
  color: #ffffff;
  background: #602a7d;
}

.hidden-rating-button:focus-visible {
  outline: 2px solid #9b6abd;
  outline-offset: 2px;
}

.panel-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.table-heading-block {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
}

.stats-table-tabs {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 6px;
}

.stats-table-tabs button,
.table-sort {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: 0;
  color: inherit;
  background: transparent;
  cursor: pointer;
}

.stats-table-tabs button {
  border: 1px solid #d7e4e1;
  border-radius: 999px;
  color: #53666c;
  background: #f5faf8;
  font-size: 12px;
  font-weight: 900;
  line-height: 1;
  padding: 6px 10px;
}

.stats-table-tabs button.active {
  border-color: #1f5f56;
  color: #ffffff;
  background: #1f5f56;
}

.panel-heading .section-title {
  margin-bottom: 0;
}

.panel-heading-actions {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.augment-rarity-filters {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 6px;
}

.augment-rarity-filters button {
  border: 1px solid #d7e4e1;
  border-radius: 999px;
  color: #53666c;
  background: #f5faf8;
  cursor: pointer;
  font-size: 12px;
  font-weight: 900;
  line-height: 1;
  padding: 6px 10px;
}

.augment-rarity-filters button.active {
  color: #ffffff;
}

.augment-rarity-filters button.active.silver {
  border-color: #6b8390;
  background: #6b8390;
}

.augment-rarity-filters button.active.gold {
  border-color: #b67b17;
  background: #b67b17;
}

.augment-rarity-filters button.active.prismatic {
  border-color: #8b4fd6;
  background: #8b4fd6;
}

.augment-rarity-filters button.active:not(.silver):not(.gold):not(.prismatic) {
  border-color: #1f5f56;
  background: #1f5f56;
}

.champion-search {
  display: inline-flex;
  min-width: 190px;
  align-items: center;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  background: #f8fbfa;
  padding: 7px 10px;
}

.champion-search input {
  width: 100%;
  border: 0;
  outline: 0;
  color: #20333a;
  background: transparent;
  font-size: 13px;
  font-weight: 800;
}

.champion-search input::placeholder {
  color: #8a9aa0;
}

.share-action {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  gap: 5px;
  border-radius: 8px;
  color: #ffffff;
  background: #1f5f56;
  cursor: pointer;
  font-size: 12px;
  font-weight: 800;
  padding: 8px 10px;
}

.share-action:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.table-wrap {
  position: relative;
  min-height: 420px;
  max-height: 560px;
  overflow: auto;
}

.table-empty {
  display: grid;
  min-height: 160px;
  place-items: center;
  color: #718087;
  font-size: 13px;
  font-weight: 800;
}

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

th,
td {
  border-bottom: 1px solid #edf2f1;
  padding: 10px 8px;
  text-align: left;
  white-space: nowrap;
}

th {
  color: #6c7a80;
  font-weight: 600;
  position: sticky;
  top: 0;
  background: #ffffff;
}

td {
  color: #263238;
}

.table-sort {
  width: 100%;
  justify-content: flex-start;
  color: #6c7a80;
  font-size: 12px;
  font-weight: 800;
  line-height: 1;
  padding: 0;
}

.champion-stats-table td:first-child {
  width: 54px;
}

.champion-row,
.stats-entry-row {
  cursor: pointer;
}

.champion-row:hover td,
.stats-entry-row:hover td {
  background: #f3f8f6;
}

.champion-row:focus-visible,
.stats-entry-row:focus-visible {
  outline: 2px solid #2f78d6;
  outline-offset: -2px;
}

.kda-total,
.kda-detail {
  display: block;
}

.kda-total {
  color: #20333a;
  font-weight: 800;
}

.champion-score,
.champion-role {
  display: block;
}

.champion-score {
  width: fit-content;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 950;
  line-height: 1;
  padding: 5px 7px;
}

.champion-role {
  color: #718087;
  font-size: 11px;
  margin-top: 3px;
}

.champion-label-tag {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 950;
  line-height: 1;
  padding: 6px 8px;
  white-space: nowrap;
}

.asset-cell {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
}

.asset-meta {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 3px;
}

.asset-meta strong,
.asset-meta small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.asset-meta strong {
  color: #20333a;
  font-size: 13px;
  font-weight: 800;
}

:deep(.augment-table-icon) {
  border-width: 2px;
  background: #e7eeec;
}

:deep(.augment-table-icon img) {
  object-fit: contain;
}

:deep(.augment-table-icon.augment-prismatic) {
  border-color: #a77cd0;
  background: #613b89;
}

:deep(.augment-table-icon.augment-gold) {
  border-color: #d6ad4c;
  background: #8f640f;
}

:deep(.augment-table-icon.augment-silver) {
  border-color: #98a7b5;
  background: #657789;
}

:deep(.augment-table-icon.augment-bronze) {
  border-color: #b78669;
  background: #734631;
}

.asset-meta small {
  color: #718087;
  font-size: 11px;
  font-weight: 700;
}

.asset-tag.augment-prismatic {
  color: #6d2c91;
}

.asset-tag.augment-gold {
  color: #7b4d02;
}

.asset-tag.augment-silver {
  color: #49606f;
}

.asset-tag.augment-bronze {
  color: #7a4323;
}

.usage-champions {
  display: flex;
  max-width: 280px;
  flex-wrap: wrap;
  gap: 5px;
}

.usage-champion-chip {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  color: #315f58;
  background: #edf5f3;
  font-size: 11px;
  font-weight: 800;
  line-height: 1;
  padding: 5px 7px;
  white-space: nowrap;
}

.kda-detail {
  color: #718087;
  font-size: 11px;
  margin-top: 2px;
}

.detail-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.detail-title-group {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
}

.detail-header button {
  border-radius: 8px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  padding: 8px 12px;
}

.detail-header .section-title {
  margin-bottom: 2px;
}

.detail-header span {
  color: #718087;
  font-size: 12px;
}

.detail-load-more,
.detail-load-complete {
  display: flex;
  width: 100%;
  min-height: 42px;
  align-items: center;
  justify-content: center;
  border: 0;
  color: #60757b;
  background: transparent;
  font-size: 12px;
  font-weight: 800;
}

.detail-load-more {
  cursor: pointer;
}

.detail-load-more:hover {
  color: #1f5f56;
  background: #f3f8f6;
}

.detail-load-more:focus-visible {
  outline: 2px solid #2f78d6;
  outline-offset: -2px;
}

.share-capture-root {
  position: fixed;
  top: 0;
  left: -12000px;
  z-index: -1;
  width: 1240px;
  pointer-events: none;
}

.share-capture-root.mobile {
  width: 430px;
}

.share-card {
  width: 1240px;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  color: #263238;
  background: #f6faf9;
  padding: 16px;
}

.share-capture-root.mobile .share-card {
  width: 430px;
  padding: 12px;
}

.share-card + .share-card {
  margin-top: 18px;
}

.share-card header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 18px;
  margin-bottom: 12px;
}

.share-card header strong {
  color: #1f2a2e;
  font-size: 20px;
}

.share-capture-root.mobile .share-card header {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}

.share-capture-root.mobile .share-card header strong {
  font-size: 18px;
}

.share-card header span {
  color: #718087;
  font-size: 13px;
  font-weight: 800;
}

.share-table {
  border: 1px solid #dce7e4;
  border-radius: 8px;
  overflow: hidden;
  background: #ffffff;
}

.share-table th {
  position: static;
  color: #63747a;
  background: #edf5f3;
}

.share-table td,
.share-table th {
  padding: 11px 10px;
}

.share-table small {
  display: block;
  color: #718087;
  font-size: 11px;
  margin-top: 2px;
}

.share-champion {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.share-champion b {
  font-weight: 800;
}

.share-games-card {
  padding: 14px;
}

.mobile-champion-list,
.mobile-games-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.mobile-champion-list article,
.mobile-games-list article {
  border: 1px solid #dce7e4;
  border-left-width: 4px;
  border-radius: 8px;
  background: #ffffff;
  padding: 10px;
}

.mobile-champion-list article {
  border-left-color: #1f5f56;
}

.mobile-games-list article.win {
  border-left-color: #2f78d6;
  background: #eef6ff;
}

.mobile-games-list article.lose {
  border-left-color: #ca4b4b;
  background: #fff1f1;
}

.mobile-champion-head {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr) auto auto auto;
  min-width: 0;
  align-items: center;
  gap: 7px;
}

.mobile-avatar {
  flex: 0 0 auto;
}

.mobile-champion-name {
  overflow: hidden;
  color: #1f2a2e;
  font-size: 13px;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-chip {
  border-radius: 999px;
  color: #315f58;
  background: #edf5f3;
  font-size: 11px;
  font-weight: 800;
  line-height: 1;
  padding: 5px 7px;
  white-space: nowrap;
}

.mobile-chip.score {
  color: #5d3300;
  background: #ffe6a6;
}

.mobile-champion-stats,
.mobile-game-stats {
  display: grid;
  grid-template-columns: repeat(5, minmax(0, 1fr));
  gap: 6px;
  margin-top: 8px;
}

.mobile-game-stats {
  grid-template-columns: repeat(5, minmax(0, 1fr));
}

.mobile-champion-stats div,
.mobile-game-stats div {
  min-width: 0;
  border-radius: 7px;
  background: rgba(255, 255, 255, 0.74);
  padding: 6px;
}

.mobile-champion-stats span,
.mobile-game-stats span {
  display: block;
  color: #718087;
  font-size: 10px;
  font-weight: 800;
}

.mobile-champion-stats strong,
.mobile-game-stats strong {
  display: block;
  overflow: hidden;
  color: #20333a;
  font-size: 13px;
  line-height: 1.15;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-champion-stats em,
.mobile-game-stats em {
  display: block;
  color: #25845f;
  font-size: 10px;
  font-style: normal;
  font-weight: 800;
  margin-top: 2px;
}

.mobile-game-stats div.leader strong,
.mobile-game-stats div.leader em {
  color: #d22f2f;
  font-weight: 900;
}

.mobile-game-loadout {
  display: grid;
  grid-template-columns: 42px 44px 18px minmax(124px, 1fr) 108px;
  align-items: center;
  gap: 6px;
}

.mobile-game-time {
  display: flex;
  min-width: 0;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  text-align: center;
}

.mobile-game-time strong {
  overflow: hidden;
  max-width: 100%;
  color: #3f555b;
  font-size: 9px;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-game-time span,
.mobile-game-time em {
  color: #718087;
  font-size: 9px;
  font-style: normal;
  font-weight: 900;
  line-height: 1;
}

.mobile-game-time em {
  color: #53666c;
}

.mobile-game-hero {
  display: flex;
  min-width: 0;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.mobile-game-hero span {
  overflow: hidden;
  max-width: 44px;
  color: #315f58;
  font-size: 9px;
  font-weight: 800;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-spells {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.mobile-items-kda {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 4px;
}

.mobile-items {
  display: flex;
  min-width: 0;
  flex-wrap: nowrap;
  gap: 2px;
}

.mobile-items-kda > strong {
  overflow: hidden;
  color: #20333a;
  font-size: 12px;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-runes {
  display: grid;
  grid-template-columns: repeat(2, max-content);
  justify-content: center;
  gap: 2px;
  overflow: hidden;
}

.mobile-runes.text-runes {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  width: 100%;
  justify-content: stretch;
}

.mobile-runes.text-runes span {
  box-sizing: border-box;
  width: 100%;
  overflow: hidden;
  border: 1px solid rgba(31, 55, 59, 0.08);
  border-radius: 5px;
  color: #34534d;
  background: rgba(255, 255, 255, 0.78);
  font-size: 9px;
  font-weight: 800;
  line-height: 1;
  padding: 3px 4px;
  text-align: center;
  white-space: nowrap;
}

.mobile-runes.text-runes .augment-prismatic {
  border-color: rgba(170, 72, 215, 0.42);
  color: #6d2c91;
  background: linear-gradient(135deg, rgba(249, 226, 255, 0.95), rgba(218, 183, 255, 0.95));
}

.mobile-runes.text-runes .augment-gold {
  border-color: rgba(199, 144, 36, 0.48);
  color: #7b4d02;
  background: rgba(255, 230, 161, 0.96);
}

.mobile-runes.text-runes .augment-silver {
  border-color: rgba(134, 151, 166, 0.48);
  color: #49606f;
  background: rgba(229, 237, 243, 0.96);
}

.mobile-runes.text-runes .augment-bronze {
  border-color: rgba(167, 105, 60, 0.46);
  color: #7a4323;
  background: rgba(236, 201, 174, 0.96);
}

@media (max-width: 1050px) {
  .stats-overview {
    grid-template-columns: 1fr;
  }

  .metric-elo-body {
    grid-template-columns: 1fr;
    align-items: start;
  }

  .metric-elo-wide strong {
    font-size: 48px;
  }

  .metric-grid {
    grid-template-columns: 1fr 1fr;
  }

  .profile-layout,
  .ability-grid {
    grid-template-columns: 1fr;
  }

  .panel-heading {
    align-items: flex-start;
    flex-direction: column;
  }

  .table-heading-block,
  .panel-heading-actions {
    width: 100%;
  }

  .table-heading-block {
    align-items: flex-start;
    flex-direction: column;
  }

  .champion-search {
    min-width: 0;
    width: 100%;
  }

  .augment-rarity-filters {
    width: 100%;
  }

  .usage-champions {
    max-width: none;
  }
}
</style>
