<script setup lang="ts">
import { ref, watch } from "vue"
import { loadRankedStats, searchPlayer } from "../api"
import { refreshGameTitleSelection } from "../gamePresentation"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  OpenMatchPayload,
  PlayerStatsResponse,
  RankedStatsResponse,
  ShareSettings,
} from "../types"
import RecordView from "./RecordView.vue"

const RECENT_PAGE_SIZE = 20
const MAX_RECENT_DEPTH = 1000
const DEFAULT_STATS_DEPTH = 100

const props = defineProps<{
  query: string
  sgpServerId?: string
  champions: Record<number, ChampionSummaryItem>
  gameAssets: GameAssetBundle
  shareSettings: ShareSettings
}>()

const emit = defineEmits<{
  openMatch: [payload: OpenMatchPayload]
}>()

const recentStats = ref<PlayerStatsResponse | null>(null)
const fullStats = ref<PlayerStatsResponse | null>(null)
const rankedStats = ref<RankedStatsResponse | null>(null)
const recentDepth = ref(RECENT_PAGE_SIZE)
const recentHasMore = ref(true)
const statsDepth = ref(DEFAULT_STATS_DEPTH)
const statsFetchDepth = ref(DEFAULT_STATS_DEPTH)
const recentLoading = ref(false)
const statsLoading = ref(false)
const rankedLoading = ref(false)
const error = ref("")
let rankedRequestKey = ""

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

async function loadRecent(forceRefresh = false, previousLoaded?: number) {
  if (recentLoading.value) return false
  if (forceRefresh) refreshGameTitleSelection()
  recentLoading.value = true
  error.value = ""

  try {
    const stats = await searchPlayer(
      props.query,
      recentDepth.value,
      props.sgpServerId,
      forceRefresh,
      false,
      "transient",
    )
    recentStats.value = stats
    // 使用战绩响应确认过的 PUUID 再读段位，避免刚进入详情页时两类 SGP 请求并发而丢失段位结果。
    void loadRanked(stats.summoner.puuid, forceRefresh)

    if (typeof previousLoaded === "number" && stats.depthLoaded <= previousLoaded) {
      recentHasMore.value = false
    } else if (stats.depthLoaded > previousLoaded! || forceRefresh) {
      recentHasMore.value = recentDepth.value < MAX_RECENT_DEPTH
    }

    return true
  } catch (loadError) {
    error.value = errorMessage(loadError)
    return false
  } finally {
    recentLoading.value = false
  }
}

async function loadRecentMore() {
  if (recentLoading.value || !recentHasMore.value) return

  const previousDepth = recentDepth.value
  const previousLoaded = recentStats.value?.depthLoaded ?? 0
  recentDepth.value = Math.min(MAX_RECENT_DEPTH, recentDepth.value + RECENT_PAGE_SIZE)

  if (recentDepth.value === previousDepth) {
    recentHasMore.value = false
    return
  }

  const loaded = await loadRecent(false, previousLoaded)
  if (!loaded) recentDepth.value = previousDepth
}

async function loadStats(forceRefresh = false) {
  if (
    fullStats.value &&
    !forceRefresh &&
    fullStats.value.depthRequested >= statsFetchDepth.value
  ) {
    return
  }
  if (statsLoading.value) return
  if (forceRefresh) refreshGameTitleSelection()
  statsLoading.value = true
  error.value = ""

  try {
    const stats = await searchPlayer(
      props.query,
      statsFetchDepth.value,
      props.sgpServerId,
      forceRefresh,
      false,
      "transient",
    )
    fullStats.value = stats
    void loadRanked(stats.summoner.puuid, forceRefresh)
  } catch (loadError) {
    error.value = errorMessage(loadError)
  } finally {
    statsLoading.value = false
  }
}

async function loadRanked(puuid: string, forceRefresh = false) {
  const normalizedPuuid = puuid.trim()
  if (!normalizedPuuid) return
  if (rankedStats.value && !forceRefresh) return
  if (rankedLoading.value) return
  const requestKey = `${props.sgpServerId || ""}:${normalizedPuuid}`

  if (forceRefresh) rankedStats.value = null
  rankedRequestKey = requestKey
  rankedLoading.value = true

  try {
    const ranked = await loadRankedStats(normalizedPuuid, props.sgpServerId)
    if (rankedRequestKey === requestKey) rankedStats.value = ranked
  } catch {
    if (rankedRequestKey === requestKey) rankedStats.value = null
  } finally {
    if (rankedRequestKey === requestKey) rankedLoading.value = false
  }
}

function changeStatsDepth(depth: number) {
  const nextDepth = Math.min(1000, Math.max(50, Math.round(Number(depth) || DEFAULT_STATS_DEPTH)))
  if (nextDepth === statsDepth.value && fullStats.value) return

  statsDepth.value = nextDepth
  statsFetchDepth.value = Math.max(statsFetchDepth.value, nextDepth)
  if (fullStats.value && fullStats.value.depthRequested < statsFetchDepth.value) {
    fullStats.value = null
  }
  void loadStats()
}

function ensureStatsDepth(depth: number) {
  const nextDepth = Math.min(1000, Math.max(50, Math.round(Number(depth) || DEFAULT_STATS_DEPTH)))
  if (nextDepth <= statsFetchDepth.value) return

  statsFetchDepth.value = nextDepth
  void loadStats()
}

watch(
  () => [props.query, props.sgpServerId],
  () => {
    rankedStats.value = null
    rankedRequestKey = ""
  },
  { immediate: true },
)
</script>

<template>
  <RecordView
    :recent-stats="recentStats"
    :full-stats="fullStats"
    :ranked-stats="rankedStats"
    :ranked-loading="rankedLoading"
    :share-settings="shareSettings"
    :champions="champions"
    :game-assets="gameAssets"
    :loading="recentLoading || statsLoading"
    :recent-loading="recentLoading"
    :recent-has-more="recentHasMore"
    :stats-depth="statsDepth"
    :error="error"
    :sgp-server-id="sgpServerId"
    @load-recent="loadRecent"
    @load-stats="loadStats"
    @refresh-recent="loadRecent(true)"
    @refresh-stats="loadStats(true)"
    @load-recent-more="loadRecentMore"
    @change-stats-depth="changeStatsDepth"
    @ensure-stats-depth="ensureStatsDepth"
    @open-match="emit('openMatch', $event)"
  />
</template>
