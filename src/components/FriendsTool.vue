<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue"
import {
  ArrowDown,
  ArrowUp,
  ChevronsUpDown,
  GripVertical,
  LoaderCircle,
  RefreshCw,
  Search,
  Star,
} from "lucide-vue-next"
import { loadFriends, searchPlayer } from "../api"
import type { FriendToolEntry, PlayerStatsResponse } from "../types"
import AssetIcon from "./AssetIcon.vue"

type FriendStatsState = "idle" | "loading" | "ready" | "error"
type FriendSortMode = "default" | "friendDays" | "winRate" | "lastGame"
type SortDirection = "asc" | "desc"

type PersistedFriendStats = {
  games: number
  wins: number
  winRate: number
  lastGameTime: number
  statsOutdated?: boolean
  updatedAt: number
}

type FriendRow = FriendToolEntry & {
  statsState: FriendStatsState
  games: number
  wins: number
  winRate: number
  lastGameTime: number
  statsOutdated: boolean
  statsError: string
}

const props = defineProps<{ active: boolean }>()
const emit = defineEmits<{
  openFriend: [payload: { puuid: string; label: string }]
}>()

const FRIEND_ORDER_KEY = "lol-stats.tools.friend-order"
const FRIEND_CUSTOM_ORDER_ENABLED_KEY = "lol-stats.tools.friend-order.customized"
const FRIEND_FAVORITES_KEY = "lol-stats.tools.friend-favorites"
const FRIEND_STATS_CACHE_KEY = "lol-stats.tools.friend-stats"
const FRIEND_STATS_CONCURRENCY = 2
const FRIEND_STATUS_POLL_MS = 5_000
const FRIEND_STATS_MAX_IDLE_MS = 30 * 86_400_000

const friends = ref<FriendRow[]>([])
const friendOrder = ref<string[]>(loadFriendOrder())
const customOrderEnabled = ref(localStorage.getItem(FRIEND_CUSTOM_ORDER_ENABLED_KEY) === "1")
const favoriteFriendKeys = ref<Set<string>>(loadFavoriteFriendKeys())
const searchQuery = ref("")
const sortMode = ref<FriendSortMode>("default")
const sortDirection = ref<SortDirection>("desc")
const listLoading = ref(false)
const statusPolling = ref(false)
const statsRefreshActive = ref(false)
const statsProgress = ref({ completed: 0, total: 0 })
const initialized = ref(false)
const listError = ref("")
const draggingPuuid = ref("")
const dragOverPuuid = ref("")
const dragPointer = ref({ x: 0, y: 0 })
let statsRunId = 0
let statusPollTimer: number | null = null
const persistedFriendStats = loadPersistedFriendStats()

const statsLoading = computed(
  () => statsRefreshActive.value || friends.value.some((friend) => friend.statsState === "loading"),
)
const visibleFriends = computed(() => {
  const keyword = searchQuery.value.trim().toLocaleLowerCase("zh-CN")
  const filtered = keyword
    ? friends.value.filter((friend) =>
      [friend.displayName, friend.gameName, friend.tagLine, friend.statusMessage]
        .some((value) => value.toLocaleLowerCase("zh-CN").includes(keyword)),
    )
    : [...friends.value]
  const customOrder = new Map(friends.value.map((friend, index) => [friendKey(friend), index]))

  return filtered.sort((left, right) => {
    if (sortMode.value !== "default") {
      const leftMetric = friendMetric(left, sortMode.value)
      const rightMetric = friendMetric(right, sortMode.value)
      const leftMissing = metricMissing(leftMetric, sortMode.value)
      const rightMissing = metricMissing(rightMetric, sortMode.value)
      if (leftMissing !== rightMissing) return leftMissing ? 1 : -1
      if (leftMetric !== rightMetric) {
        return sortDirection.value === "asc"
          ? leftMetric - rightMetric
          : rightMetric - leftMetric
      }
      return left.displayName.localeCompare(right.displayName, "zh-CN")
    }

    const favoriteDiff = Number(isFavorite(right)) - Number(isFavorite(left))
    if (favoriteDiff) return favoriteDiff

    if (customOrderEnabled.value) {
      const customDiff = (customOrder.get(friendKey(left)) || 0) - (customOrder.get(friendKey(right)) || 0)
      if (customDiff) return customDiff
    }

    const onlineDiff = Number(isOnlineFriend(right)) - Number(isOnlineFriend(left))
    if (onlineDiff) return onlineDiff

    return left.displayName.localeCompare(right.displayName, "zh-CN")
  })
})

watch(
  () => props.active,
  (active) => {
    if (active) {
      if (!initialized.value) void refreshFriends()
      startFriendStatusPolling()
    } else {
      stopFriendStatusPolling()
    }
  },
  { immediate: true },
)

onUnmounted(() => {
  statsRunId += 1
  stopFriendStatusPolling()
  cleanupFriendDrag()
})

function loadFriendOrder() {
  try {
    const value = JSON.parse(localStorage.getItem(FRIEND_ORDER_KEY) || "[]")
    return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : []
  } catch {
    return []
  }
}

function loadFavoriteFriendKeys() {
  try {
    const value = JSON.parse(localStorage.getItem(FRIEND_FAVORITES_KEY) || "[]")
    return new Set(Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [])
  } catch {
    return new Set<string>()
  }
}

function loadPersistedFriendStats() {
  try {
    const value = JSON.parse(localStorage.getItem(FRIEND_STATS_CACHE_KEY) || "{}")
    return value && typeof value === "object"
      ? value as Record<string, PersistedFriendStats>
      : {}
  } catch {
    return {} as Record<string, PersistedFriendStats>
  }
}

function persistFriendOrder() {
  friendOrder.value = friends.value.map((friend) => friend.puuid || friend.id)
  localStorage.setItem(FRIEND_ORDER_KEY, JSON.stringify(friendOrder.value))
}

function persistFavoriteFriendKeys() {
  localStorage.setItem(FRIEND_FAVORITES_KEY, JSON.stringify([...favoriteFriendKeys.value]))
}

function persistFriendStats(key: string, friend: FriendRow) {
  persistedFriendStats[key] = {
    games: friend.games,
    wins: friend.wins,
    winRate: friend.winRate,
    lastGameTime: friend.lastGameTime,
    statsOutdated: friend.statsOutdated,
    updatedAt: Date.now(),
  }
  localStorage.setItem(FRIEND_STATS_CACHE_KEY, JSON.stringify(persistedFriendStats))
}

function persistReadyFriendStats() {
  let changed = false
  for (const friend of friends.value) {
    if (friend.statsState !== "ready") continue
    persistedFriendStats[friendKey(friend)] = {
      games: friend.games,
      wins: friend.wins,
      winRate: friend.winRate,
      lastGameTime: friend.lastGameTime,
      statsOutdated: friend.statsOutdated,
      updatedAt: Date.now(),
    }
    changed = true
  }
  if (changed) localStorage.setItem(FRIEND_STATS_CACHE_KEY, JSON.stringify(persistedFriendStats))
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function friendKey(friend: Pick<FriendToolEntry, "puuid" | "id">) {
  return friend.puuid || friend.id
}

function applySavedOrder(rows: FriendRow[]) {
  const orderIndex = new Map(friendOrder.value.map((key, index) => [key, index]))
  return rows.sort((left, right) => {
    const leftIndex = orderIndex.get(friendKey(left))
    const rightIndex = orderIndex.get(friendKey(right))
    if (leftIndex !== undefined && rightIndex !== undefined) return leftIndex - rightIndex
    if (leftIndex !== undefined) return -1
    if (rightIndex !== undefined) return 1
    return left.displayName.localeCompare(right.displayName, "zh-CN")
  })
}

async function refreshFriends() {
  if (listLoading.value) return
  listLoading.value = true
  listError.value = ""
  const runId = ++statsRunId

  try {
    const entries = await loadFriends(true)
    const previous = new Map(friends.value.map((friend) => [friendKey(friend), friend]))
    friends.value = applySavedOrder(
      entries.map((entry) => friendRowFromEntry(entry, previous.get(friendKey(entry)))),
    )
    persistFriendOrder()
    persistReadyFriendStats()
    initialized.value = true
    if (props.active) startFriendStatusPolling()
    void hydrateFriendStats(runId)
  } catch (error) {
    listError.value = errorMessage(error)
  } finally {
    listLoading.value = false
  }
}

function refreshFriendData() {
  void refreshFriends()
}

function friendRowFromEntry(entry: FriendToolEntry, old?: FriendRow): FriendRow {
  const cached = old?.statsState === "ready"
    ? old
    : persistedFriendStats[friendKey(entry)]
  return {
    ...entry,
    statsState: cached ? "ready" : "idle",
    games: cached?.games || 0,
    wins: cached?.wins || 0,
    winRate: cached?.winRate || 0,
    lastGameTime: cached?.lastGameTime || 0,
    statsOutdated: Boolean(cached?.statsOutdated),
    statsError: "",
  }
}

function toggleSort(mode: Exclude<FriendSortMode, "default">) {
  if (sortMode.value === mode) {
    sortDirection.value = sortDirection.value === "desc" ? "asc" : "desc"
    return
  }
  sortMode.value = mode
  sortDirection.value = "desc"
}

function restoreDefaultSort() {
  sortMode.value = "default"
  sortDirection.value = "desc"
}

function startFriendStatusPolling() {
  if (!props.active || statusPollTimer !== null) return
  statusPollTimer = window.setInterval(() => {
    void pollFriendStatuses()
  }, FRIEND_STATUS_POLL_MS)
}

function stopFriendStatusPolling() {
  if (statusPollTimer === null) return
  window.clearInterval(statusPollTimer)
  statusPollTimer = null
}

async function pollFriendStatuses() {
  if (!props.active || listLoading.value || statusPolling.value) return
  statusPolling.value = true
  let needsFullRefresh = false
  try {
    const entries = await loadFriends(false)
    const entryMap = new Map(entries.map((entry) => [friendKey(entry), entry]))
    const currentKeys = new Set(friends.value.map((friend) => friendKey(friend)))
    needsFullRefresh = entries.length !== friends.value.length || entries.some((entry) => !currentKeys.has(friendKey(entry)))

    if (!needsFullRefresh) {
      friends.value = friends.value.map((friend) => {
        const entry = entryMap.get(friendKey(friend))
        if (!entry) return friend
        return {
          ...friend,
          id: entry.id,
          puuid: entry.puuid,
          gameName: entry.gameName,
          tagLine: entry.tagLine,
          displayName: entry.displayName,
          iconId: entry.iconId,
          summonerId: entry.summonerId,
          availability: entry.availability,
          statusMessage: entry.statusMessage,
        }
      })
    }
  } catch {
    // 状态轮询失败时保留现有列表，下一轮自动重试。
  } finally {
    statusPolling.value = false
  }

  if (needsFullRefresh) void refreshFriends()
}

async function hydrateFriendStats(runId: number) {
  const queue = friendStatsRefreshQueue()
  let cursor = 0
  statsRefreshActive.value = true
  statsProgress.value = { completed: 0, total: queue.length }

  const worker = async () => {
    while (runId === statsRunId) {
      const friend = queue[cursor]
      cursor += 1
      if (!friend) return

      const key = friendKey(friend)
      const hasCache = friend.statsState === "ready" && friend.lastGameTime > 0
      if (!hasCache) patchFriend(key, { statsState: "loading", statsError: "" })
      try {
        const latest = await searchPlayer(friend.puuid, 1, undefined, true, true)
        if (runId !== statsRunId) return
        const latestGameTime = latestGameTimestamp(latest)

        if (latestGameTime <= 0) {
          patchFriend(key, {
            statsState: "ready",
            games: 0,
            wins: 0,
            winRate: 0,
            lastGameTime: 0,
            statsOutdated: false,
            statsError: "",
          })
          persistPatchedFriendStats(key)
          continue
        }

        if (friendStatsOutdated(latestGameTime)) {
          patchFriend(key, {
            statsState: "ready",
            lastGameTime: latestGameTime,
            statsOutdated: true,
            statsError: "",
          })
          persistPatchedFriendStats(key)
          continue
        }

        const shouldLoadFullStats =
          !hasCache ||
          friend.statsOutdated ||
          latestGameTime !== friend.lastGameTime
        if (!shouldLoadFullStats) {
          patchFriend(key, { statsState: "ready", statsOutdated: false, statsError: "" })
          continue
        }

        const stats = await searchPlayer(friend.puuid, 20, undefined, true, true)
        if (runId !== statsRunId) return
        patchFriend(key, friendStatsPatch(stats))
        persistPatchedFriendStats(key)
      } catch (error) {
        if (runId !== statsRunId) return
        patchFriend(key, {
          statsState: hasCache ? "ready" : "error",
          statsError: errorMessage(error),
        })
      } finally {
        if (runId === statsRunId) {
          statsProgress.value = {
            ...statsProgress.value,
            completed: statsProgress.value.completed + 1,
          }
        }
      }
    }
  }

  await Promise.all(Array.from({ length: FRIEND_STATS_CONCURRENCY }, () => worker()))
  if (runId === statsRunId) statsRefreshActive.value = false
}

function friendStatsRefreshQueue() {
  const customOrder = new Map(friendOrder.value.map((key, index) => [key, index]))

  return friends.value
    .filter((friend) => friend.puuid)
    .sort((left, right) => {
      const favoriteDiff = Number(isFavorite(right)) - Number(isFavorite(left))
      if (favoriteDiff) return favoriteDiff

      if (customOrderEnabled.value) {
        const leftIndex = customOrder.get(friendKey(left)) ?? Number.MAX_SAFE_INTEGER
        const rightIndex = customOrder.get(friendKey(right)) ?? Number.MAX_SAFE_INTEGER
        if (leftIndex !== rightIndex) return leftIndex - rightIndex
      }

      const onlineDiff = Number(isOnlineFriend(right)) - Number(isOnlineFriend(left))
      if (onlineDiff) return onlineDiff

      const lastGameDiff = right.lastGameTime - left.lastGameTime
      if (lastGameDiff) return lastGameDiff

      return left.displayName.localeCompare(right.displayName, "zh-CN")
    })
}

function friendStatsOutdated(lastGameTime: number) {
  return lastGameTime > 0 && Date.now() - lastGameTime > FRIEND_STATS_MAX_IDLE_MS
}

function persistPatchedFriendStats(key: string) {
  const updated = friends.value.find((item) => friendKey(item) === key)
  if (updated) persistFriendStats(key, updated)
}

function latestGameTimestamp(stats: PlayerStatsResponse) {
  return stats.recentGames.reduce(
    (latest, game) => Math.max(latest, Number(game.gameCreation || 0)),
    0,
  )
}

function friendStatsPatch(stats: PlayerStatsResponse): Partial<FriendRow> {
  return {
    statsState: "ready",
    games: stats.summary.games,
    wins: stats.summary.wins,
    winRate: stats.summary.winRate,
    lastGameTime: latestGameTimestamp(stats),
    statsOutdated: false,
    statsError: "",
  }
}

function patchFriend(key: string, patch: Partial<FriendRow>) {
  const index = friends.value.findIndex((friend) => friendKey(friend) === key)
  if (index < 0) return
  friends.value[index] = { ...friends.value[index], ...patch }
}

function friendDays(friend: FriendRow) {
  const days = friendDaysValue(friend)
  if (days < 0) return "未知"
  return `${days} 天`
}

function friendDaysValue(friend: FriendRow) {
  const timestamp = Date.parse(friend.friendsSince || "")
  if (!Number.isFinite(timestamp)) return -1
  return Math.max(0, Math.floor((Date.now() - timestamp) / 86_400_000))
}

function friendMetric(friend: FriendRow, mode: FriendSortMode) {
  switch (mode) {
    case "friendDays":
      return friendDaysValue(friend)
    case "winRate":
      return friend.statsState === "ready" && !friend.statsOutdated ? friend.winRate : -1
    case "lastGame":
      return friend.statsState === "ready" ? friend.lastGameTime : -1
    default:
      return 0
  }
}

function metricMissing(value: number, mode: FriendSortMode) {
  return mode === "lastGame" ? value <= 0 : value < 0
}

function isOnlineFriend(friend: FriendRow) {
  return friend.availability !== "offline"
}

function lastGameText(friend: FriendRow) {
  if (friend.statsState === "loading" || friend.statsState === "idle") return "读取中"
  if (friend.statsState === "error") return "读取失败"
  if (!friend.lastGameTime) return "暂无记录"
  return new Date(friend.lastGameTime).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  })
}

function winRateText(friend: FriendRow) {
  if (friend.statsState === "loading" || friend.statsState === "idle") return "读取中"
  if (friend.statsState === "error") return "读取失败"
  if (friend.statsOutdated) return "年代久远"
  if (!friend.games) return "暂无记录"
  return `${Math.round(friend.winRate * 100)}%`
}

function availabilityLabel(availability: string) {
  switch (availability) {
    case "chat":
      return "聊天"
    case "mobile":
      return "在线分组"
    case "away":
      return "离开"
    case "dnd":
      return "游戏中"
    case "offline":
      return "离线"
    case "spectating":
      return "观战中"
    case "online":
      return "在线"
    default:
      return availability || "未知"
  }
}

function isFavorite(friend: FriendRow) {
  return favoriteFriendKeys.value.has(friendKey(friend))
}

function toggleFavorite(friend: FriendRow) {
  const key = friendKey(friend)
  const next = new Set(favoriteFriendKeys.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  favoriteFriendKeys.value = next
  persistFavoriteFriendKeys()
}

function profileIconPath(iconId: number) {
  return iconId > 0 ? `/lol-game-data/assets/v1/profile-icons/${iconId}.jpg` : undefined
}

function openFriend(friend: FriendRow) {
  emit("openFriend", { puuid: friend.puuid, label: friend.displayName })
}

function startFriendDrag(event: PointerEvent, friend: FriendRow) {
  if (event.button !== 0) return
  event.preventDefault()
  restoreDefaultSort()
  draggingPuuid.value = friendKey(friend)
  dragPointer.value = { x: event.clientX, y: event.clientY }
  updateFriendDragTarget(event.clientX, event.clientY)
  window.addEventListener("pointermove", handleFriendPointerMove)
  window.addEventListener("pointerup", handleFriendPointerUp)
}

function handleFriendPointerMove(event: PointerEvent) {
  if (!draggingPuuid.value) return
  dragPointer.value = { x: event.clientX, y: event.clientY }
  updateFriendDragTarget(event.clientX, event.clientY)
}

function updateFriendDragTarget(x: number, y: number) {
  const row = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>("[data-friend-key]")
  dragOverPuuid.value = row?.dataset.friendKey || ""
}

function handleFriendPointerUp(event: PointerEvent) {
  if (!draggingPuuid.value) return
  const row = (document.elementFromPoint(event.clientX, event.clientY) as HTMLElement | null)?.closest<HTMLElement>(
    "[data-friend-key]",
  )
  const targetKey = row?.dataset.friendKey || ""
  if (targetKey && targetKey !== draggingPuuid.value) {
    const rect = row?.getBoundingClientRect()
    moveFriend(draggingPuuid.value, targetKey, Boolean(rect && event.clientY > rect.top + rect.height / 2))
  }
  endFriendDrag()
}

function moveFriend(sourceKey: string, targetKey: string, after: boolean) {
  const sourceIndex = friends.value.findIndex((friend) => friendKey(friend) === sourceKey)
  const targetIndex = friends.value.findIndex((friend) => friendKey(friend) === targetKey)
  if (sourceIndex < 0 || targetIndex < 0) return

  const next = [...friends.value]
  const [source] = next.splice(sourceIndex, 1)
  let insertIndex = next.findIndex((friend) => friendKey(friend) === targetKey)
  if (after) insertIndex += 1
  next.splice(Math.max(0, insertIndex), 0, source)
  friends.value = next
  customOrderEnabled.value = true
  localStorage.setItem(FRIEND_CUSTOM_ORDER_ENABLED_KEY, "1")
  persistFriendOrder()
}

function endFriendDrag() {
  draggingPuuid.value = ""
  dragOverPuuid.value = ""
  cleanupFriendDrag()
}

function cleanupFriendDrag() {
  window.removeEventListener("pointermove", handleFriendPointerMove)
  window.removeEventListener("pointerup", handleFriendPointerUp)
}
</script>

<template>
  <section class="friends-tool">
    <header class="friends-heading">
      <div>
        <strong>好友</strong>
        <span>{{ visibleFriends.length === friends.length ? friends.length : `${visibleFriends.length}/${friends.length}` }} 人</span>
      </div>
      <div class="friends-controls">
        <label class="friend-search">
          <Search :size="15" />
          <input v-model="searchQuery" placeholder="搜索好友" />
        </label>
      </div>
      <div class="friends-actions">
        <span v-if="statsLoading">战绩 {{ statsProgress.completed }}/{{ statsProgress.total }}</span>
        <button :disabled="listLoading || statsLoading" title="刷新好友和战绩" @click="refreshFriendData">
          <RefreshCw :class="{ spin: listLoading || statsLoading }" :size="17" />
          刷新
        </button>
      </div>
    </header>

    <div class="friend-columns" v-if="visibleFriends.length">
      <button
        class="column-sort"
        :class="{ active: sortMode === 'default' }"
        title="恢复默认排序"
        @click="restoreDefaultSort"
      >
        好友
        <ChevronsUpDown :size="14" />
      </button>
      <button class="column-sort" @click="toggleSort('friendDays')">
        成为好友
        <ArrowUp v-if="sortMode === 'friendDays' && sortDirection === 'asc'" :size="14" />
        <ArrowDown v-else-if="sortMode === 'friendDays'" :size="14" />
        <ChevronsUpDown v-else :size="14" />
      </button>
      <button class="column-sort" @click="toggleSort('winRate')">
        近20局胜率
        <ArrowUp v-if="sortMode === 'winRate' && sortDirection === 'asc'" :size="14" />
        <ArrowDown v-else-if="sortMode === 'winRate'" :size="14" />
        <ChevronsUpDown v-else :size="14" />
      </button>
      <button class="column-sort" @click="toggleSort('lastGame')">
        上次游戏
        <ArrowUp v-if="sortMode === 'lastGame' && sortDirection === 'asc'" :size="14" />
        <ArrowDown v-else-if="sortMode === 'lastGame'" :size="14" />
        <ChevronsUpDown v-else :size="14" />
      </button>
    </div>

    <div class="friend-list" v-if="visibleFriends.length">
      <article
        v-for="friend in visibleFriends"
        :key="friendKey(friend)"
        class="friend-row"
        :class="{
          dragging: draggingPuuid === friendKey(friend),
          'drag-over': dragOverPuuid === friendKey(friend),
        }"
        :data-friend-key="friendKey(friend)"
      >
        <div class="friend-identity">
          <button class="drag-handle" title="拖动排序" @pointerdown.stop="startFriendDrag($event, friend)">
            <GripVertical :size="17" />
          </button>
          <button
            class="favorite-button"
            :class="{ active: isFavorite(friend) }"
            :title="isFavorite(friend) ? '取消收藏' : '收藏置顶'"
            @click="toggleFavorite(friend)"
          >
            <Star :size="17" :fill="isFavorite(friend) ? 'currentColor' : 'none'" />
          </button>
          <AssetIcon :path="profileIconPath(friend.iconId)" :label="friend.displayName" fallback="?" :size="34" />
          <div class="friend-name-status">
            <button class="friend-name" :disabled="!friend.puuid" @click="openFriend(friend)">
              {{ friend.displayName }}
            </button>
            <span :class="['friend-presence', friend.availability]">
              <i :class="friend.availability"></i>{{ availabilityLabel(friend.availability) }}
            </span>
          </div>
        </div>
        <strong>{{ friendDays(friend) }}</strong>
        <strong :class="{ muted: friend.statsState !== 'ready' || friend.statsOutdated }">{{ winRateText(friend) }}</strong>
        <span class="last-game" :title="friend.statsError">{{ lastGameText(friend) }}</span>
      </article>
    </div>

    <div class="friends-state" v-else-if="listLoading && !friends.length">
      <LoaderCircle class="spin" :size="28" />
      <strong>正在读取好友</strong>
    </div>
    <div class="friends-state error" v-else-if="listError">
      <strong>好友读取失败</strong>
      <span>{{ listError }}</span>
    </div>
    <div class="friends-state" v-else-if="friends.length && searchQuery.trim()">
      <strong>没有匹配的好友</strong>
    </div>
    <div class="friends-state" v-else>
      <strong>暂无好友</strong>
    </div>

    <div
      v-if="draggingPuuid"
      class="friend-drag-ghost"
      :style="{ left: `${dragPointer.x}px`, top: `${dragPointer.y}px` }"
    >
      {{ friends.find((friend) => friendKey(friend) === draggingPuuid)?.displayName }}
    </div>
  </section>
</template>

<style scoped>
.friends-tool {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  border-top: 1px solid #dce7e4;
}

.friends-heading,
.friends-heading > div,
.friends-actions,
.friends-controls,
.friend-search,
.friend-identity,
.friend-name-status,
.friend-presence {
  display: flex;
  align-items: center;
}

.friends-heading {
  min-height: 54px;
  display: grid;
  grid-template-columns: auto minmax(320px, 1fr) auto;
  gap: 16px;
  padding: 8px 12px;
}

.friends-heading > div {
  gap: 9px;
}

.friends-heading strong {
  color: #20333a;
  font-size: 18px;
  font-weight: 950;
}

.friends-heading span,
.friends-actions span {
  color: #697b80;
  font-size: 12px;
  font-weight: 850;
}

.friends-actions {
  gap: 10px;
}

.friends-controls {
  min-width: 0;
  justify-content: flex-end;
  gap: 8px;
}

.friend-search {
  width: min(280px, 100%);
  height: 34px;
  min-width: 160px;
  gap: 7px;
  border: 1px solid #ccdcd8;
  border-radius: 7px;
  color: #728287;
  background: #ffffff;
  padding: 0 10px;
}

.friend-search input {
  min-width: 0;
  flex: 1;
  border: 0;
  outline: 0;
  color: #20333a;
  background: transparent;
  font-size: 13px;
  font-weight: 850;
}

.friends-actions button {
  display: inline-flex;
  height: 34px;
  align-items: center;
  gap: 6px;
  border: 1px solid #bdd4cf;
  border-radius: 7px;
  color: #1f6256;
  background: #f5fbf9;
  font-size: 13px;
  font-weight: 900;
  padding: 0 12px;
  cursor: pointer;
}

.friends-actions button:disabled {
  opacity: 0.55;
  cursor: wait;
}

.friend-columns,
.friend-row {
  display: grid;
  grid-template-columns: minmax(320px, 1.5fr) minmax(100px, 0.55fr) minmax(110px, 0.55fr) minmax(130px, 0.7fr);
  align-items: center;
  gap: 12px;
}

.friend-columns {
  min-height: 34px;
  border-block: 1px solid #e0e9e7;
  color: #687a7f;
  background: #f4f8f7;
  font-size: 12px;
  font-weight: 900;
  padding: 0 14px;
}

.column-sort {
  display: inline-flex;
  width: max-content;
  align-items: center;
  gap: 4px;
  border: 0;
  color: inherit;
  background: transparent;
  font: inherit;
  padding: 4px 0;
  cursor: pointer;
}

.column-sort:hover {
  color: #1f6f62;
}

.column-sort.active {
  color: #176f5e;
}

.friend-list {
  min-height: 0;
  flex: 1;
  overflow: auto;
}

.friend-row {
  min-height: 58px;
  border-bottom: 1px solid #e5ecea;
  background: #ffffff;
  padding: 7px 14px;
  transition: background 0.12s ease, box-shadow 0.12s ease;
}

.friend-row:hover,
.friend-row.drag-over {
  background: #f3faf8;
}

.friend-row.drag-over {
  box-shadow: inset 0 2px 0 #27806f;
}

.friend-row.dragging {
  opacity: 0.38;
}

.friend-identity {
  min-width: 0;
  gap: 9px;
}

.friend-name-status {
  min-width: 0;
  gap: 9px;
}

.drag-handle,
.favorite-button,
.friend-name {
  border: 0;
  background: transparent;
  padding: 0;
}

.drag-handle {
  display: grid;
  width: 22px;
  height: 34px;
  flex: 0 0 auto;
  place-items: center;
  color: #8a999c;
  cursor: grab;
  touch-action: none;
}

.favorite-button {
  display: grid;
  width: 22px;
  height: 34px;
  flex: 0 0 auto;
  place-items: center;
  color: #9aa7aa;
  cursor: pointer;
}

.favorite-button:hover,
.favorite-button.active {
  color: #d89a16;
}

.drag-handle:active {
  cursor: grabbing;
}

.friend-name {
  overflow: hidden;
  max-width: 100%;
  color: #176aa0;
  font-size: 14px;
  font-weight: 950;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}

.friend-name:disabled {
  color: #6b7c80;
  cursor: default;
}

.friend-presence {
  flex: 0 0 auto;
  gap: 5px;
  color: #7d8b8f;
  font-size: 13px;
  font-weight: 900;
  white-space: nowrap;
}

.friend-presence i {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: currentColor;
  box-shadow: 0 0 6px color-mix(in srgb, currentColor 58%, transparent);
}

.friend-presence.chat,
.friend-presence.online,
.friend-presence.mobile {
  color: #08ad61;
}

.friend-presence.dnd,
.friend-presence.spectating {
  color: #1687e8;
}

.friend-presence.away {
  color: #e44b55;
}

.friend-presence.offline {
  color: #98a5a8;
}

.friend-row > strong,
.last-game {
  color: #263a40;
  font-size: 13px;
  font-weight: 900;
  white-space: nowrap;
}

.friend-row > strong.muted,
.last-game {
  color: #6c7d82;
}

.friends-state {
  display: flex;
  min-height: 260px;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: #66797e;
}

.friends-state strong {
  font-size: 16px;
  font-weight: 950;
}

.friends-state span {
  max-width: 620px;
  font-size: 13px;
  font-weight: 800;
  text-align: center;
}

.friends-state.error {
  color: #a74343;
}

.friend-drag-ghost {
  position: fixed;
  z-index: 1900;
  max-width: 260px;
  overflow: hidden;
  border: 1px solid #a8c9c1;
  border-radius: 7px;
  color: #183c35;
  background: #eff9f6;
  box-shadow: 0 12px 28px rgba(24, 45, 49, 0.24);
  font-size: 13px;
  font-weight: 950;
  padding: 8px 12px;
  pointer-events: none;
  text-overflow: ellipsis;
  transform: translate(12px, 12px);
  white-space: nowrap;
}

.spin {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@media (max-width: 920px) {
  .friends-heading {
    grid-template-columns: 1fr auto;
  }

  .friends-controls {
    grid-column: 1 / -1;
    grid-row: 2;
    justify-content: flex-start;
  }

  .friend-columns,
  .friend-row {
    grid-template-columns: minmax(280px, 1fr) 90px 100px 112px;
    gap: 8px;
  }
}
</style>
