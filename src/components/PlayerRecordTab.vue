<script setup lang="ts">
import { ref } from "vue"
import { searchPlayer } from "../api"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  OpenMatchPayload,
  PlayerStatsResponse,
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
const recentDepth = ref(RECENT_PAGE_SIZE)
const recentHasMore = ref(true)
const statsDepth = ref(DEFAULT_STATS_DEPTH)
const recentLoading = ref(false)
const statsLoading = ref(false)
const error = ref("")

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

async function loadRecent(forceRefresh = false, previousLoaded?: number) {
  if (recentLoading.value) return false
  recentLoading.value = true
  error.value = ""

  try {
    const stats = await searchPlayer(
      props.query,
      recentDepth.value,
      props.sgpServerId,
      forceRefresh,
      false,
    )
    recentStats.value = stats

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
  if (fullStats.value && !forceRefresh) return
  if (statsLoading.value) return
  statsLoading.value = true
  error.value = ""

  try {
    fullStats.value = await searchPlayer(
      props.query,
      statsDepth.value,
      props.sgpServerId,
      forceRefresh,
      false,
    )
  } catch (loadError) {
    error.value = errorMessage(loadError)
  } finally {
    statsLoading.value = false
  }
}

function changeStatsDepth(depth: number) {
  const nextDepth = Math.min(1000, Math.max(50, Math.round(Number(depth) || DEFAULT_STATS_DEPTH)))
  if (nextDepth === statsDepth.value && fullStats.value) return

  statsDepth.value = nextDepth
  fullStats.value = null
  void loadStats()
}
</script>

<template>
  <RecordView
    :recent-stats="recentStats"
    :full-stats="fullStats"
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
    @open-match="emit('openMatch', $event)"
  />
</template>
