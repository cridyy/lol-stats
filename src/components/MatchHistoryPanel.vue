<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue"
import { LoaderCircle } from "lucide-vue-next"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  OpenMatchPayload,
  PlayerStatsResponse,
} from "../types"
import { fixed } from "../utils"
import GameRecordList from "./GameRecordList.vue"

const props = defineProps<{
  stats: PlayerStatsResponse | null
  champions: Record<number, ChampionSummaryItem>
  gameAssets: GameAssetBundle
  sgpServerId?: string
  loadingMore?: boolean
  hasMore?: boolean
  ownerLabel?: string
  ownerPuuid?: string
}>()

const emit = defineEmits<{
  loadMore: []
  openMatch: [payload: OpenMatchPayload]
}>()

const loadMoreSentinel = ref<HTMLElement | null>(null)
let observer: IntersectionObserver | null = null

function canLoadMore() {
  return Boolean(props.stats) && !props.loadingMore && props.hasMore !== false
}

function requestMoreIfVisible() {
  if (!canLoadMore()) return

  const sentinel = loadMoreSentinel.value
  if (!sentinel) return

  const rect = sentinel.getBoundingClientRect()
  const viewportHeight = window.innerHeight || document.documentElement.clientHeight
  if (rect.top <= viewportHeight + 180) {
    emit("loadMore")
  }
}

function handleIntersection(entries: IntersectionObserverEntry[]) {
  if (!canLoadMore()) return

  if (entries.some((entry) => entry.isIntersecting)) {
    emit("loadMore")
  }
}

onMounted(() => {
  observer = new IntersectionObserver(handleIntersection, {
    root: null,
    rootMargin: "160px 0px 220px 0px",
    threshold: 0,
  })

  if (loadMoreSentinel.value) observer.observe(loadMoreSentinel.value)
})

onUnmounted(() => {
  observer?.disconnect()
  observer = null
})

watch(loadMoreSentinel, (current, previous) => {
  if (previous) observer?.unobserve(previous)
  if (current) observer?.observe(current)
})

watch(
  () => [props.stats?.recentGames.length, props.loadingMore, props.hasMore] as const,
  async () => {
    await nextTick()
    requestMoreIfVisible()
  },
)
</script>

<template>
  <section class="empty-panel" v-if="!stats">
    <div class="empty-title">暂无战绩</div>
    <div class="empty-subtitle">等待读取客户端数据</div>
  </section>

  <section class="match-panel" v-else>
    <div class="match-summary">
      <div>
        <span>近 {{ stats.depthLoaded }} 局</span>
        <strong>{{ stats.summary.wins }}胜 {{ stats.summary.losses }}负</strong>
      </div>
      <div>
        <span>胜率</span>
        <strong>{{ Math.round(stats.summary.winRate * 100) }}%</strong>
      </div>
      <div>
        <span>KDA</span>
        <strong>{{ fixed(stats.summary.averageKda) }}</strong>
      </div>
    </div>

    <GameRecordList
      :games="stats.recentGames"
      :champions="champions"
      :game-assets="gameAssets"
      :sgp-server-id="sgpServerId"
      :owner-label="ownerLabel"
      :owner-puuid="ownerPuuid"
      external-detail
      @open-match="emit('openMatch', $event)"
    />

    <div class="load-more-sentinel" ref="loadMoreSentinel">
      <template v-if="loadingMore">
        <LoaderCircle class="spin" :size="16" />
        <span>正在加载下一组</span>
      </template>
      <span v-else-if="hasMore !== false">继续下滑加载更多</span>
      <span v-else>已加载到底</span>
    </div>
  </section>
</template>

<style scoped>
.empty-panel {
  display: grid;
  min-height: 420px;
  place-content: center;
  gap: 8px;
  color: #657179;
  text-align: center;
}

.empty-title {
  color: #263238;
  font-size: 22px;
  font-weight: 800;
}

.empty-subtitle {
  font-size: 14px;
}

.match-panel {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.match-summary {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.match-summary div,
.match-row {
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 12px 28px rgba(32, 67, 73, 0.07);
}

.match-summary div {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 16px;
}

.match-summary span {
  color: #718087;
  font-size: 12px;
}

.match-summary strong {
  color: #20333a;
  font-size: 24px;
}

.load-more-sentinel {
  display: inline-flex;
  align-self: center;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 36px;
  border: 1px solid #d9e6e2;
  border-radius: 999px;
  color: #53666c;
  background: rgba(255, 255, 255, 0.8);
  font-size: 12px;
  font-weight: 800;
  padding: 8px 13px;
}

@media (max-width: 1050px) {
  .match-summary {
    grid-template-columns: 1fr;
  }
}
</style>
