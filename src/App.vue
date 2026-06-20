<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, provide, ref, watch } from "vue"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { openUrl } from "@tauri-apps/plugin-opener"
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  FileText,
  LoaderCircle,
  RefreshCw,
  Search,
  Settings,
  Swords,
  UserRound,
  X,
} from "lucide-vue-next"
import {
  cancelStatsLoad,
  checkAppUpdate,
  connectionStatus,
  loadCurrentRankedStats,
  loadGameflowPhase,
  loadRankedStats,
  loadChampions,
  loadGameAssets,
  loadLiveGame,
  loadMatchDetail,
  loadSelfStats,
  loadSelfStatsWithProgress,
  searchPlayer,
  searchPlayerWithProgress,
} from "./api"
import LiveGamePanel from "./components/LiveGamePanel.vue"
import MatchOverviewPanel from "./components/MatchOverviewPanel.vue"
import PlayerRecordTab from "./components/PlayerRecordTab.vue"
import RecordView from "./components/RecordView.vue"
import ToastStack from "./components/ToastStack.vue"
import { notifyKey, type AppToast, type ToastPayload } from "./notifications"
import { championName } from "./utils"
import type {
  AppUpdateInfo,
  ChampionSummaryItem,
  ConnectionStatus,
  GameAssetBundle,
  LiveGameResponse,
  MatchDetailPlayer,
  MatchDetailResponse,
  OpenMatchPayload,
  PlayerStatsResponse,
  RankedStatsResponse,
  RecentGame,
  ShareSettings,
  StatsLoadProgress,
} from "./types"

type PageKey = "current" | "live" | "search" | "details"
type SearchServerOption = { id: string; label: string }
type SearchHistoryItem = { query: string; sgpServerId: string }
type OverviewDrillTab = {
  id: string
  type: "overview"
  title: string
  ownerLabel: string
  game: RecentGame
  sgpServerId?: string
  detail: MatchDetailResponse | null
  loading: boolean
  error: string
}
type PlayerDrillTab = {
  id: string
  type: "player"
  title: string
  playerLabel: string
  query: string
  sgpServerId?: string
}
type DrillTab = OverviewDrillTab | PlayerDrillTab
type DrillHistoryPayload =
  | {
      id: string
      type: "overview"
      title: string
      ownerLabel: string
      game: RecentGame
      sgpServerId?: string
    }
  | {
      id: string
      type: "player"
      title: string
      playerLabel: string
      query: string
      sgpServerId?: string
    }
type NavigationSnapshot = {
  page: PageKey
  activeDrillTabId?: string
  drillTab?: DrillHistoryPayload
}
type HiddenDrillCacheEntry = {
  tab: DrillTab
  hiddenAt: number
}

const SEARCH_HISTORY_KEY = "lol-stats.search-history"
const SHARE_SETTINGS_KEY = "lol-stats.share-settings"
const MIN_PROGRESS_VISIBLE_MS = 700
const HIDDEN_DRILL_CACHE_MS = 10 * 60 * 1000
const HIDDEN_DRILL_CACHE_LIMIT = 20
const DEFAULT_SEARCH_SERVER_ID = "TENCENT_HN1"
const RECENT_PAGE_SIZE = 20
const MAX_RECENT_DEPTH = 1000
const DEFAULT_STATS_DEPTH = 500
const MIN_STATS_DEPTH = 50
const MAX_STATS_DEPTH = 500
const LIVE_STATS_DEPTH = 50
const LIVE_AUTO_REFRESH_MS = 15000
const GAMEFLOW_WATCH_MS = 1000
const AUTO_LIVE_PHASES = new Set(["GameStart", "InProgress", "Reconnect"])
const DEFAULT_SHARE_SETTINGS: ShareSettings = {
  championAnalysisLimit: 10,
  championGamesAnalysisLimit: 20,
  mobileShareLayout: true,
}
const emptyGameAssets = (): GameAssetBundle => ({
  summonerSpells: [],
  items: [],
  perks: [],
  augments: [],
})

const searchServerOptions: SearchServerOption[] = [
  { id: "TENCENT_HN1", label: "艾欧尼亚" },
  { id: "TENCENT_HN10", label: "黑色玫瑰" },
  { id: "TENCENT_NJ100", label: "联盟一区" },
  { id: "TENCENT_GZ100", label: "联盟二区" },
  { id: "TENCENT_CQ100", label: "联盟三区" },
  { id: "TENCENT_TJ100", label: "联盟四区" },
  { id: "TENCENT_TJ101", label: "联盟五区" },
  { id: "TENCENT_BGP2", label: "峡谷之巅" },
  { id: "TENCENT_PBE", label: "体验服" },
  { id: "TENCENT_PREPBE", label: "PREPBE" },
]

const status = ref<ConnectionStatus | null>(null)
const champions = ref<ChampionSummaryItem[]>([])
const gameAssets = ref<GameAssetBundle>(emptyGameAssets())
const activePage = ref<PageKey>("current")
const workspaceRef = ref<HTMLElement | null>(null)
const settingsOpen = ref(false)
const shareSettings = ref<ShareSettings>(loadShareSettings())
const updateDialog = ref<AppUpdateInfo | null>(null)

const currentRecentStats = ref<PlayerStatsResponse | null>(null)
const currentFullStats = ref<PlayerStatsResponse | null>(null)
const currentRankedStats = ref<RankedStatsResponse | null>(null)
const currentRecentDepth = ref(RECENT_PAGE_SIZE)
const currentRecentHasMore = ref(true)
const currentStatsDepth = ref(DEFAULT_STATS_DEPTH)
const currentRecentLoading = ref(false)
const currentStatsLoading = ref(false)
const currentRankedLoading = ref(false)
const currentError = ref("")

const searchInput = ref("")
const activeSearchQuery = ref("")
const selectedSearchServerId = ref(DEFAULT_SEARCH_SERVER_ID)
const activeSearchServerId = ref(DEFAULT_SEARCH_SERVER_ID)
const searchHistory = ref<SearchHistoryItem[]>(loadSearchHistory())
const searchRecentStats = ref<PlayerStatsResponse | null>(null)
const searchFullStats = ref<PlayerStatsResponse | null>(null)
const searchRankedStats = ref<RankedStatsResponse | null>(null)
const searchRecentDepth = ref(RECENT_PAGE_SIZE)
const searchRecentHasMore = ref(true)
const searchStatsDepth = ref(DEFAULT_STATS_DEPTH)
const searchRecentLoading = ref(false)
const searchStatsLoading = ref(false)
const searchRankedLoading = ref(false)
const searchError = ref("")

const liveGame = ref<LiveGameResponse | null>(null)
const liveLoading = ref(false)
const liveRefreshing = ref(false)
const liveError = ref("")
const drillTabs = ref<DrillTab[]>([])
const activeDrillTabId = ref("")
const toasts = ref<AppToast[]>([])
const navigationHistory = ref<NavigationSnapshot[]>([{ page: "current" }])
const navigationHistoryIndex = ref(0)
const progressOverlay = ref({
  visible: false,
  requestId: "",
  loaded: 0,
  total: 0,
  cancelled: false,
})

let unlistenProgress: UnlistenFn | null = null
let liveRefreshTimer: number | null = null
let gameflowWatchTimer: number | null = null
let autoLiveSessionActive = false
let searchServerTouched = false
let searchServerInitializedFromClient = false
let searchRankedRequestKey = ""
let nextToastId = 0
let drillCachePruneTimer: number | null = null
let applyingNavigationHistory = false
let lastDetailsSnapshot: NavigationSnapshot | null = null
const hiddenDrillTabCache = new Map<string, HiddenDrillCacheEntry>()
const pageScrollTop: Record<PageKey, number> = {
  current: 0,
  live: 0,
  search: 0,
  details: 0,
}

const championMap = computed(() => {
  return champions.value.reduce<Record<number, ChampionSummaryItem>>((acc, champion) => {
    if (champion.id > 0) acc[champion.id] = champion
    return acc
  }, {})
})

const progressRatio = computed(() => {
  if (!progressOverlay.value.total) return 0
  return Math.min(progressOverlay.value.loaded / progressOverlay.value.total, 1)
})

const progressRingStyle = computed(() => ({
  "--progress": `${progressRatio.value * 360}deg`,
}))

const activeDrillTab = computed(
  () => drillTabs.value.find((tab) => tab.id === activeDrillTabId.value) || null,
)
const canGoBack = computed(() => navigationHistoryIndex.value > 0)
const canGoForward = computed(() => navigationHistoryIndex.value < navigationHistory.value.length - 1)

const navItems: Array<{ key: PageKey; label: string; icon: typeof UserRound }> = [
  { key: "current", label: "当前角色", icon: UserRound },
  { key: "live", label: "实时战绩", icon: Swords },
  { key: "search", label: "查战绩", icon: Search },
  { key: "details", label: "详细战绩", icon: FileText },
]

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function savePageScroll(page: PageKey) {
  pageScrollTop[page] = workspaceRef.value?.scrollTop ?? 0
}

async function restorePageScroll(page: PageKey) {
  await nextTick()
  if (!workspaceRef.value) return
  workspaceRef.value.scrollTop = pageScrollTop[page] ?? 0
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string) {
  return Promise.race<T>([
    promise,
    new Promise<T>((_, reject) => {
      window.setTimeout(() => reject(new Error(message)), timeoutMs)
    }),
  ])
}

function clampInteger(value: unknown, fallback: number, min: number, max: number) {
  const numberValue = Number(value)
  if (!Number.isFinite(numberValue)) return fallback
  return Math.min(max, Math.max(min, Math.round(numberValue)))
}

function loadShareSettings(): ShareSettings {
  try {
    const raw = localStorage.getItem(SHARE_SETTINGS_KEY)
    const parsed = raw ? JSON.parse(raw) : {}
    return {
      championAnalysisLimit: clampInteger(
        parsed.championAnalysisLimit,
        DEFAULT_SHARE_SETTINGS.championAnalysisLimit,
        1,
        50,
      ),
      championGamesAnalysisLimit: clampInteger(
        parsed.championGamesAnalysisLimit,
        DEFAULT_SHARE_SETTINGS.championGamesAnalysisLimit,
        1,
        100,
      ),
      mobileShareLayout:
        typeof parsed.mobileShareLayout === "boolean"
          ? parsed.mobileShareLayout
          : DEFAULT_SHARE_SETTINGS.mobileShareLayout,
    }
  } catch {
    return { ...DEFAULT_SHARE_SETTINGS }
  }
}

function persistShareSettings() {
  shareSettings.value = {
    championAnalysisLimit: clampInteger(
      shareSettings.value.championAnalysisLimit,
      DEFAULT_SHARE_SETTINGS.championAnalysisLimit,
      1,
      50,
    ),
    championGamesAnalysisLimit: clampInteger(
      shareSettings.value.championGamesAnalysisLimit,
      DEFAULT_SHARE_SETTINGS.championGamesAnalysisLimit,
      1,
      100,
    ),
    mobileShareLayout: Boolean(shareSettings.value.mobileShareLayout),
  }
  localStorage.setItem(SHARE_SETTINGS_KEY, JSON.stringify(shareSettings.value))
}

function showToast(toast: ToastPayload) {
  const id = ++nextToastId
  toasts.value = [{ ...toast, id }, ...toasts.value].slice(0, 4)
  return id
}

function dismissToast(id: number) {
  toasts.value = toasts.value.filter((toast) => toast.id !== id)
}

function runToastAction(id: number) {
  const toast = toasts.value.find((item) => item.id === id)
  toast?.onAction?.()
  dismissToast(id)
}

function notifyError(title: string, error: unknown) {
  showToast({
    kind: "error",
    title,
    message: errorMessage(error),
    duration: 7000,
  })
}

async function checkForAppUpdate() {
  try {
    const update = await checkAppUpdate()
    if (!update.hasUpdate) return

    updateDialog.value = update
  } catch {
    // 更新提示不能影响主流程；网络失败或更新源不可达时静默跳过。
  }
}

function closeUpdateDialog() {
  updateDialog.value = null
}

function updateDownloadPassword() {
  const message = updateDialog.value?.releaseName || ""
  return message.match(/(?:密码|提取码|访问码)[:：]\s*([A-Za-z0-9_-]+)/)?.[1] || ""
}

async function copyTextToClipboard(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text)
    return
  }

  const textarea = document.createElement("textarea")
  textarea.value = text
  textarea.style.position = "fixed"
  textarea.style.left = "-9999px"
  textarea.style.opacity = "0"
  document.body.appendChild(textarea)
  textarea.focus()
  textarea.select()
  const copied = document.execCommand("copy")
  textarea.remove()
  if (!copied) throw new Error("剪切板不可用")
}

async function copyUpdatePassword() {
  const password = updateDownloadPassword()
  if (!password) {
    showToast({
      kind: "warning",
      title: "没有找到下载密码",
      message: updateDialog.value?.releaseName || "更新提示里没有可复制的密码",
      duration: 6000,
    })
    return
  }

  try {
    await copyTextToClipboard(password)
    showToast({
      kind: "success",
      title: "下载密码已复制",
      message: `下载密码：${password}`,
      duration: 7000,
    })
  } catch (error) {
    notifyError("下载密码复制失败", error)
  }
}

function openUpdateDownload() {
  const update = updateDialog.value
  if (!update) return

  void (async () => {
    const password = updateDownloadPassword()

    if (password) {
      try {
        await copyTextToClipboard(password)
      } catch (error) {
        notifyError("下载密码复制失败", error)
      }
    }

    try {
      await openUrl(update.releasePageUrl)
      showToast({
        kind: password ? "success" : "info",
        title: password ? "下载页已打开，密码已复制" : "下载页已打开",
        message: password ? `下载密码：${password}` : update.releaseName || "请在下载页获取新版安装包",
        duration: 7000,
      })
    } catch (error) {
      notifyError("无法打开下载页", error)
    }
  })()
}

provide(notifyKey, showToast)

function drillTabKey(prefix: string, ...parts: Array<string | number | undefined>) {
  return [prefix, ...parts.map((part) => String(part || ""))].join(":")
}

function detailPlayerLabel(player: MatchDetailPlayer) {
  if (player.gameName && player.tagLine) return `${player.gameName}#${player.tagLine}`
  return player.summonerName || player.puuid || "未知玩家"
}

function copyRecentGame(game: RecentGame): RecentGame {
  return {
    ...game,
    itemIds: [...(game.itemIds || [])],
    perkIds: [...(game.perkIds || [])],
    augmentIds: [...(game.augmentIds || [])],
  }
}

function overviewHistoryPayload(payload: OpenMatchPayload): Extract<DrillHistoryPayload, { type: "overview" }> {
  return {
    id: drillTabKey("overview", payload.sgpServerId, payload.game.gameId, payload.ownerPuuid),
    type: "overview",
    title: `${payload.ownerLabel} ${championName(championMap.value, payload.game.championId)}`,
    ownerLabel: payload.ownerLabel,
    game: copyRecentGame(payload.game),
    sgpServerId: payload.sgpServerId,
  }
}

function playerHistoryPayload(
  player: MatchDetailPlayer,
  sgpServerId?: string,
): Extract<DrillHistoryPayload, { type: "player" }> {
  const label = detailPlayerLabel(player)
  return {
    id: drillTabKey("player", sgpServerId, player.puuid),
    type: "player",
    title: `${label} 总览`,
    playerLabel: label,
    query: player.puuid,
    sgpServerId,
  }
}

function drillTabToHistoryPayload(tab: DrillTab): DrillHistoryPayload {
  if (tab.type === "overview") {
    return {
      id: tab.id,
      type: "overview",
      title: tab.title,
      ownerLabel: tab.ownerLabel,
      game: copyRecentGame(tab.game),
      sgpServerId: tab.sgpServerId,
    }
  }

  return {
    id: tab.id,
    type: "player",
    title: tab.title,
    playerLabel: tab.playerLabel,
    query: tab.query,
    sgpServerId: tab.sgpServerId,
  }
}

function visibleDrillTab(id: string) {
  return drillTabs.value.find((tab) => tab.id === id) || null
}

function cachedDrillTab(id: string) {
  return hiddenDrillTabCache.get(id)?.tab || null
}

function detailsSnapshot(tab: DrillTab | null): NavigationSnapshot {
  if (!tab) return { page: "details" }
  return {
    page: "details",
    activeDrillTabId: tab.id,
    drillTab: drillTabToHistoryPayload(tab),
  }
}

function currentNavigationSnapshot(): NavigationSnapshot {
  if (activePage.value !== "details") return { page: activePage.value }

  const activeTab = activeDrillTab.value || drillTabs.value[0] || null
  return activeTab ? detailsSnapshot(activeTab) : lastDetailsSnapshot || { page: "details" }
}

function navigationSnapshotForPage(page: PageKey): NavigationSnapshot {
  if (page !== "details") return { page }

  const activeTab = activeDrillTab.value || drillTabs.value[0] || null
  return activeTab ? detailsSnapshot(activeTab) : lastDetailsSnapshot || { page: "details" }
}

function normalizeNavigationSnapshot(snapshot: NavigationSnapshot): NavigationSnapshot {
  if (snapshot.page !== "details") return { page: snapshot.page }
  if (snapshot.drillTab) {
    return {
      page: "details",
      activeDrillTabId: snapshot.drillTab.id,
      drillTab: snapshot.drillTab,
    }
  }

  const tab = snapshot.activeDrillTabId
    ? visibleDrillTab(snapshot.activeDrillTabId) || cachedDrillTab(snapshot.activeDrillTabId)
    : activeDrillTab.value || drillTabs.value[0] || null
  return detailsSnapshot(tab)
}

function sameNavigationSnapshot(left: NavigationSnapshot, right: NavigationSnapshot) {
  return left.page === right.page && (left.activeDrillTabId || "") === (right.activeDrillTabId || "")
}

function pushNavigationSnapshot(snapshot: NavigationSnapshot) {
  if (applyingNavigationHistory) return

  const normalized = normalizeNavigationSnapshot(snapshot)
  const current = navigationHistory.value[navigationHistoryIndex.value]
  if (current && sameNavigationSnapshot(current, normalized)) return

  navigationHistory.value = [
    ...navigationHistory.value.slice(0, navigationHistoryIndex.value + 1),
    normalized,
  ]
  navigationHistoryIndex.value = navigationHistory.value.length - 1
}

function navigateToPage(page: PageKey) {
  const snapshot = navigationSnapshotForPage(page)
  pushNavigationSnapshot(snapshot)

  if (page === "details" && snapshot.drillTab) {
    const tab = ensureDrillTab(snapshot.drillTab)
    activeDrillTabId.value = tab.id
    lastDetailsSnapshot = detailsSnapshot(tab)
  } else if (page === "details" && !activeDrillTabId.value) {
    const tab = drillTabs.value[0] || null
    activeDrillTabId.value = tab?.id || ""
    lastDetailsSnapshot = tab ? detailsSnapshot(tab) : lastDetailsSnapshot
  }

  activePage.value = page
}

function pruneHiddenDrillCache() {
  const now = Date.now()
  for (const [id, entry] of hiddenDrillTabCache.entries()) {
    if (now - entry.hiddenAt > HIDDEN_DRILL_CACHE_MS) hiddenDrillTabCache.delete(id)
  }

  if (hiddenDrillTabCache.size <= HIDDEN_DRILL_CACHE_LIMIT) return

  const entries = [...hiddenDrillTabCache.entries()].sort((left, right) => left[1].hiddenAt - right[1].hiddenAt)
  for (const [id] of entries.slice(0, hiddenDrillTabCache.size - HIDDEN_DRILL_CACHE_LIMIT)) {
    hiddenDrillTabCache.delete(id)
  }
}

function restoreHiddenDrillTab(id: string) {
  const visible = visibleDrillTab(id)
  if (visible) return visible

  const entry = hiddenDrillTabCache.get(id)
  if (!entry) return null

  if (Date.now() - entry.hiddenAt > HIDDEN_DRILL_CACHE_MS) {
    hiddenDrillTabCache.delete(id)
    return null
  }

  hiddenDrillTabCache.delete(id)
  drillTabs.value.push(entry.tab)
  return entry.tab
}

function hideDrillTabForHistory(id: string) {
  const index = drillTabs.value.findIndex((tab) => tab.id === id)
  if (index < 0) return

  const [tab] = drillTabs.value.splice(index, 1)
  lastDetailsSnapshot = detailsSnapshot(tab)
  hiddenDrillTabCache.set(id, { tab, hiddenAt: Date.now() })
  pruneHiddenDrillCache()

  if (activeDrillTabId.value === id) {
    const nextTab = drillTabs.value[Math.max(0, index - 1)] || drillTabs.value[0]
    activeDrillTabId.value = nextTab?.id || ""
  }
}

function hideCurrentDrillForHistory(target: NavigationSnapshot) {
  const current = currentNavigationSnapshot()
  if (current.page !== "details" || !current.activeDrillTabId) return
  if (target.page === "details" && target.activeDrillTabId === current.activeDrillTabId) return

  hideDrillTabForHistory(current.activeDrillTabId)
}

function createOverviewTab(payload: Extract<DrillHistoryPayload, { type: "overview" }>) {
  const tab: OverviewDrillTab = {
    id: payload.id,
    type: "overview",
    title: payload.title,
    ownerLabel: payload.ownerLabel,
    game: copyRecentGame(payload.game),
    sgpServerId: payload.sgpServerId,
    detail: null,
    loading: true,
    error: "",
  }

  drillTabs.value.push(tab)
  void loadOverviewTabDetail(tab.id, tab.game.gameId, tab.sgpServerId)
  return tab
}

function createPlayerTab(payload: Extract<DrillHistoryPayload, { type: "player" }>) {
  const tab: PlayerDrillTab = {
    id: payload.id,
    type: "player",
    title: payload.title,
    playerLabel: payload.playerLabel,
    query: payload.query,
    sgpServerId: payload.sgpServerId,
  }

  drillTabs.value.push(tab)
  return tab
}

function ensureDrillTab(payload: DrillHistoryPayload) {
  const existing = visibleDrillTab(payload.id) || restoreHiddenDrillTab(payload.id)
  if (existing) return existing

  return payload.type === "overview" ? createOverviewTab(payload) : createPlayerTab(payload)
}

function activateDrillTab(id: string, options: { recordHistory?: boolean } = {}) {
  const tab = visibleDrillTab(id) || restoreHiddenDrillTab(id)
  if (!tab) return

  activeDrillTabId.value = id
  activePage.value = "details"
  lastDetailsSnapshot = detailsSnapshot(tab)
  if (options.recordHistory !== false) pushNavigationSnapshot(lastDetailsSnapshot)
}

function closeDrillTab(id: string) {
  hiddenDrillTabCache.delete(id)
  const index = drillTabs.value.findIndex((tab) => tab.id === id)
  if (index < 0) return

  drillTabs.value.splice(index, 1)
  if (activeDrillTabId.value !== id) {
    if (lastDetailsSnapshot?.activeDrillTabId === id) {
      const activeTab = activeDrillTab.value || drillTabs.value[0] || null
      lastDetailsSnapshot = activeTab ? detailsSnapshot(activeTab) : null
    }
    return
  }

  const nextTab = drillTabs.value[Math.max(0, index - 1)] || drillTabs.value[0]
  activeDrillTabId.value = nextTab?.id || ""
  lastDetailsSnapshot = nextTab ? detailsSnapshot(nextTab) : null
}

async function openOverviewTab(payload: OpenMatchPayload) {
  const tab = ensureDrillTab(overviewHistoryPayload(payload))
  activateDrillTab(tab.id)
}

async function loadOverviewTabDetail(id: string, gameId: number, sgpServerId?: string) {
  updateOverviewTab(id, { detail: null, loading: true, error: "" })

  try {
    const detail = await withTimeout(
      loadMatchDetail(gameId, sgpServerId),
      20_000,
      "对局详情读取超时，请稍后重试",
    )
    updateOverviewTab(id, { detail, loading: false, error: "" })
  } catch (error) {
    updateOverviewTab(id, { error: errorMessage(error), loading: false })
  }
}

function updateOverviewTab(id: string, patch: Partial<OverviewDrillTab>) {
  const index = drillTabs.value.findIndex((tab) => tab.id === id)
  if (index >= 0 && drillTabs.value[index].type === "overview") {
    drillTabs.value[index] = {
      ...drillTabs.value[index],
      ...patch,
    } as OverviewDrillTab
    return
  }

  const cached = hiddenDrillTabCache.get(id)
  if (!cached || cached.tab.type !== "overview") return

  cached.tab = {
    ...cached.tab,
    ...patch,
  } as OverviewDrillTab
}

function openPlayerDrillTab(player: MatchDetailPlayer, sgpServerId?: string) {
  if (!player.puuid) {
    showToast({ kind: "error", title: "无法打开玩家战绩", message: "该玩家缺少 PUUID" })
    return
  }

  const tab = ensureDrillTab(playerHistoryPayload(player, sgpServerId))
  activateDrillTab(tab.id)
}

function applyNavigationSnapshot(snapshot: NavigationSnapshot) {
  applyingNavigationHistory = true
  try {
    if (snapshot.page === "details" && snapshot.drillTab) {
      const tab = ensureDrillTab(snapshot.drillTab)
      activeDrillTabId.value = tab.id
      lastDetailsSnapshot = detailsSnapshot(tab)
    } else if (snapshot.page === "details") {
      const tab = drillTabs.value[0] || null
      activeDrillTabId.value = tab?.id || ""
      lastDetailsSnapshot = tab ? detailsSnapshot(tab) : lastDetailsSnapshot
    }

    activePage.value = snapshot.page
  } finally {
    applyingNavigationHistory = false
  }
}

function navigateBack() {
  if (!canGoBack.value) return

  const targetIndex = navigationHistoryIndex.value - 1
  const target = navigationHistory.value[targetIndex]
  savePageScroll(activePage.value)
  hideCurrentDrillForHistory(target)
  navigationHistoryIndex.value = targetIndex
  applyNavigationSnapshot(target)
}

function navigateForward() {
  if (!canGoForward.value) return

  const targetIndex = navigationHistoryIndex.value + 1
  const target = navigationHistory.value[targetIndex]
  savePageScroll(activePage.value)
  hideCurrentDrillForHistory(target)
  navigationHistoryIndex.value = targetIndex
  applyNavigationSnapshot(target)
}

function isKnownSearchServer(sgpServerId: string) {
  return searchServerOptions.some((option) => option.id === sgpServerId)
}

function normalizeSearchServerId(sgpServerId?: string) {
  const normalized = sgpServerId?.trim().toUpperCase() || DEFAULT_SEARCH_SERVER_ID
  return isKnownSearchServer(normalized) ? normalized : DEFAULT_SEARCH_SERVER_ID
}

function searchServerLabel(sgpServerId: string) {
  return searchServerOptions.find((option) => option.id === sgpServerId)?.label || sgpServerId
}

function currentClientSearchServerId() {
  const client = status.value?.client
  if (!client) return ""

  if (client.region.toUpperCase() === "TENCENT" && client.rsoPlatformId) {
    return `TENCENT_${client.rsoPlatformId.toUpperCase()}`
  }

  return client.region.toUpperCase()
}

function syncSearchServerFromClient() {
  if (searchServerTouched || searchServerInitializedFromClient) return

  const currentServerId = currentClientSearchServerId()
  if (!isKnownSearchServer(currentServerId)) return

  selectedSearchServerId.value = currentServerId
  if (!activeSearchQuery.value) activeSearchServerId.value = currentServerId
  searchServerInitializedFromClient = true
}

function markSearchServerTouched() {
  searchServerTouched = true
}

function loadSearchHistory(): SearchHistoryItem[] {
  try {
    const raw = localStorage.getItem(SEARCH_HISTORY_KEY)
    const parsed = raw ? JSON.parse(raw) : []

    if (!Array.isArray(parsed)) return []

    return parsed
      .map((item): SearchHistoryItem | null => {
        if (typeof item === "string") {
          const query = item.trim()
          return query ? { query, sgpServerId: DEFAULT_SEARCH_SERVER_ID } : null
        }

        if (item && typeof item.query === "string") {
          const query = item.query.trim()
          return query
            ? { query, sgpServerId: normalizeSearchServerId(item.sgpServerId) }
            : null
        }

        return null
      })
      .filter((item): item is SearchHistoryItem => item !== null)
      .slice(0, 8)
  } catch {
    return []
  }
}

function saveSearchHistory(query: string, sgpServerId: string) {
  const normalizedServerId = normalizeSearchServerId(sgpServerId)
  searchHistory.value = [
    { query, sgpServerId: normalizedServerId },
    ...searchHistory.value.filter(
      (item) => item.query !== query || item.sgpServerId !== normalizedServerId,
    ),
  ].slice(0, 8)
  persistSearchHistory()
}

function persistSearchHistory() {
  localStorage.setItem(SEARCH_HISTORY_KEY, JSON.stringify(searchHistory.value))
}

function removeSearchHistory(item: SearchHistoryItem) {
  showToast({
    kind: "warning",
    title: "删除这条历史查询？",
    message: `${searchServerLabel(item.sgpServerId)} · ${item.query}`,
    actionLabel: "删除",
    duration: 9000,
    onAction: () => {
      searchHistory.value = searchHistory.value.filter(
        (history) => history.query !== item.query || history.sgpServerId !== item.sgpServerId,
      )
      persistSearchHistory()
      showToast({ kind: "success", title: "已删除历史查询" })
    },
  })
}

function createRequestId(prefix: string) {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

function beginProgress(requestId: string, total: number) {
  progressOverlay.value = {
    visible: true,
    requestId,
    loaded: 0,
    total,
    cancelled: false,
  }
}

function endProgress(requestId: string) {
  if (progressOverlay.value.requestId === requestId) {
    progressOverlay.value.visible = false
  }
}

async function keepProgressVisible(startedAt: number) {
  const elapsed = Date.now() - startedAt
  if (elapsed >= MIN_PROGRESS_VISIBLE_MS) return
  await new Promise((resolve) => window.setTimeout(resolve, MIN_PROGRESS_VISIBLE_MS - elapsed))
}

async function cancelProgress() {
  const requestId = progressOverlay.value.requestId
  if (!requestId) return
  progressOverlay.value.cancelled = true
  progressOverlay.value.visible = false
  await cancelStatsLoad(requestId).catch(() => {})
}

async function ensureClientReady() {
  status.value = await connectionStatus()
  if (!status.value.connected) {
    throw new Error(status.value.message)
  }

  syncSearchServerFromClient()

  if (champions.value.length === 0) {
    try {
      champions.value = await loadChampions()
    } catch (error) {
      notifyError("英雄资源读取失败", error)
    }
  }

  if (
    gameAssets.value.summonerSpells.length === 0 &&
    gameAssets.value.items.length === 0 &&
    gameAssets.value.perks.length === 0 &&
    gameAssets.value.augments.length === 0
  ) {
    try {
      gameAssets.value = await loadGameAssets()
    } catch (error) {
      notifyError("对局资源读取失败", error)
    }
  }
}

async function loadCurrentRecent(forceRefresh = false, previousLoaded?: number) {
  if (currentRecentLoading.value) return false
  currentRecentLoading.value = true
  currentError.value = ""
  try {
    await ensureClientReady()
    if (forceRefresh) currentRankedStats.value = null
    void loadCurrentRanked()
    const stats = await loadSelfStats(currentRecentDepth.value, forceRefresh)
    currentRecentStats.value = stats
    if (typeof previousLoaded === "number" && stats.depthLoaded <= previousLoaded) {
      currentRecentHasMore.value = false
    } else if (stats.depthLoaded > previousLoaded! || forceRefresh) {
      currentRecentHasMore.value = currentRecentDepth.value < MAX_RECENT_DEPTH
    }
    return true
  } catch (error) {
    currentError.value = errorMessage(error)
    notifyError("当前战绩读取失败", error)
    return false
  } finally {
    currentRecentLoading.value = false
  }
}

async function loadCurrentRecentMore() {
  if (currentRecentLoading.value || !currentRecentHasMore.value) return

  const previousDepth = currentRecentDepth.value
  const previousLoaded = currentRecentStats.value?.depthLoaded ?? 0
  currentRecentDepth.value = Math.min(MAX_RECENT_DEPTH, currentRecentDepth.value + RECENT_PAGE_SIZE)

  if (currentRecentDepth.value === previousDepth) {
    currentRecentHasMore.value = false
    return
  }

  const loaded = await loadCurrentRecent(false, previousLoaded)
  if (!loaded) {
    currentRecentDepth.value = previousDepth
  }
}

async function loadCurrentRanked() {
  if (currentRankedStats.value || currentRankedLoading.value) return
  currentRankedLoading.value = true
  try {
    await ensureClientReady()
    currentRankedStats.value = await loadCurrentRankedStats()
  } catch (error) {
    notifyError("段位读取失败", error)
  } finally {
    currentRankedLoading.value = false
  }
}

async function loadCurrentStats(forceRefresh = false) {
  if (currentFullStats.value && !forceRefresh) return
  if (currentStatsLoading.value) return
  const requestId = createRequestId("current-stats")
  const startedAt = Date.now()
  currentStatsLoading.value = true
  currentError.value = ""
  beginProgress(requestId, currentStatsDepth.value)
  try {
    await ensureClientReady()
    void loadCurrentRanked()
    const stats = await loadSelfStatsWithProgress(currentStatsDepth.value, requestId, forceRefresh)
    if (!progressOverlay.value.cancelled) currentFullStats.value = stats
  } catch (error) {
    if (!progressOverlay.value.cancelled) {
      currentError.value = errorMessage(error)
      notifyError("数据统计读取失败", error)
    }
  } finally {
    if (!progressOverlay.value.cancelled) await keepProgressVisible(startedAt)
    endProgress(requestId)
    currentStatsLoading.value = false
  }
}

function changeCurrentStatsDepth(depth: number) {
  const nextDepth = clampInteger(depth, DEFAULT_STATS_DEPTH, MIN_STATS_DEPTH, MAX_STATS_DEPTH)
  if (nextDepth === currentStatsDepth.value && currentFullStats.value) return

  currentStatsDepth.value = nextDepth
  currentFullStats.value = null
  void loadCurrentStats()
}

function runSearch(query = searchInput.value, sgpServerId = selectedSearchServerId.value) {
  const text = query.trim()
  if (!text) return
  const normalizedServerId = normalizeSearchServerId(sgpServerId)

  navigateToPage("search")
  activeSearchQuery.value = text
  activeSearchServerId.value = normalizedServerId
  selectedSearchServerId.value = normalizedServerId
  searchInput.value = text
  searchRecentStats.value = null
  searchFullStats.value = null
  searchRankedStats.value = null
  searchRankedLoading.value = false
  searchRankedRequestKey = ""
  searchRecentDepth.value = RECENT_PAGE_SIZE
  searchRecentHasMore.value = true
  searchError.value = ""
  saveSearchHistory(text, normalizedServerId)
}

function returnToSearchHome() {
  activeSearchQuery.value = ""
  searchRecentStats.value = null
  searchFullStats.value = null
  searchRankedStats.value = null
  searchRankedLoading.value = false
  searchRankedRequestKey = ""
  searchRecentDepth.value = RECENT_PAGE_SIZE
  searchRecentHasMore.value = true
  searchError.value = ""
}

async function loadSearchRanked(puuid: string, forceRefresh = false) {
  const normalizedPuuid = puuid.trim()
  if (!normalizedPuuid) return
  if (searchRankedLoading.value) return
  if (searchRankedStats.value && !forceRefresh) return

  if (forceRefresh) searchRankedStats.value = null
  const requestKey = `${activeSearchServerId.value}:${activeSearchQuery.value}:${normalizedPuuid}`
  searchRankedRequestKey = requestKey
  searchRankedLoading.value = true

  try {
    const ranked = await loadRankedStats(normalizedPuuid, activeSearchServerId.value)
    if (searchRankedRequestKey === requestKey) {
      searchRankedStats.value = ranked
    }
  } catch {
    if (searchRankedRequestKey === requestKey) {
      searchRankedStats.value = null
    }
  } finally {
    if (searchRankedRequestKey === requestKey) {
      searchRankedLoading.value = false
    }
  }
}

async function loadSearchRecent(forceRefresh = false, previousLoaded?: number) {
  if (!activeSearchQuery.value || searchRecentLoading.value) return false
  searchRecentLoading.value = true
  searchError.value = ""
  try {
    await ensureClientReady()
    const stats = await searchPlayer(
      activeSearchQuery.value,
      searchRecentDepth.value,
      activeSearchServerId.value,
      forceRefresh,
    )
    searchRecentStats.value = stats
    void loadSearchRanked(stats.summoner.puuid, forceRefresh)
    if (typeof previousLoaded === "number" && stats.depthLoaded <= previousLoaded) {
      searchRecentHasMore.value = false
    } else if (stats.depthLoaded > previousLoaded! || forceRefresh) {
      searchRecentHasMore.value = searchRecentDepth.value < MAX_RECENT_DEPTH
    }
    return true
  } catch (error) {
    searchError.value = errorMessage(error)
    notifyError("查询战绩读取失败", error)
    return false
  } finally {
    searchRecentLoading.value = false
  }
}

async function loadSearchRecentMore() {
  if (!activeSearchQuery.value || searchRecentLoading.value || !searchRecentHasMore.value) return

  const previousDepth = searchRecentDepth.value
  const previousLoaded = searchRecentStats.value?.depthLoaded ?? 0
  searchRecentDepth.value = Math.min(MAX_RECENT_DEPTH, searchRecentDepth.value + RECENT_PAGE_SIZE)

  if (searchRecentDepth.value === previousDepth) {
    searchRecentHasMore.value = false
    return
  }

  const loaded = await loadSearchRecent(false, previousLoaded)
  if (!loaded) {
    searchRecentDepth.value = previousDepth
  }
}

async function loadSearchStats(forceRefresh = false) {
  if (searchFullStats.value && !forceRefresh) return
  if (!activeSearchQuery.value || searchStatsLoading.value) return
  const requestId = createRequestId("search-stats")
  const startedAt = Date.now()
  searchStatsLoading.value = true
  searchError.value = ""
  beginProgress(requestId, searchStatsDepth.value)
  try {
    await ensureClientReady()
    const stats = await searchPlayerWithProgress(
      activeSearchQuery.value,
      searchStatsDepth.value,
      requestId,
      activeSearchServerId.value,
      forceRefresh,
    )
    if (!progressOverlay.value.cancelled) {
      searchFullStats.value = stats
      void loadSearchRanked(stats.summoner.puuid, forceRefresh)
    }
  } catch (error) {
    if (!progressOverlay.value.cancelled) {
      searchError.value = errorMessage(error)
      notifyError("查询统计读取失败", error)
    }
  } finally {
    if (!progressOverlay.value.cancelled) await keepProgressVisible(startedAt)
    endProgress(requestId)
    searchStatsLoading.value = false
  }
}

function changeSearchStatsDepth(depth: number) {
  const nextDepth = clampInteger(depth, DEFAULT_STATS_DEPTH, MIN_STATS_DEPTH, MAX_STATS_DEPTH)
  if (nextDepth === searchStatsDepth.value && searchFullStats.value) return

  searchStatsDepth.value = nextDepth
  searchFullStats.value = null
  void loadSearchStats()
}

async function refreshLiveGame(options: { silent?: boolean; switchPage?: boolean } = {}) {
  const silent = Boolean(options.silent)
  const switchPage = options.switchPage ?? !silent
  if (liveLoading.value || liveRefreshing.value) return

  if (silent && liveGame.value) {
    liveRefreshing.value = true
  } else {
    liveLoading.value = true
  }

  liveError.value = ""
  if (switchPage) navigateToPage("live")

  try {
    await ensureClientReady()
    liveGame.value = await loadLiveGame(LIVE_STATS_DEPTH)
  } catch (error) {
    liveError.value = errorMessage(error)
    if (!silent) {
      notifyError("实时战绩读取失败", error)
      liveGame.value = null
    }
  } finally {
    liveLoading.value = false
    liveRefreshing.value = false
  }
}

function startLiveAutoRefresh() {
  if (liveRefreshTimer !== null) return
  void refreshLiveGame({ silent: Boolean(liveGame.value), switchPage: false })
  liveRefreshTimer = window.setInterval(() => {
    if (activePage.value === "live") {
      void refreshLiveGame({ silent: true, switchPage: false })
    }
  }, LIVE_AUTO_REFRESH_MS)
}

function stopLiveAutoRefresh() {
  if (liveRefreshTimer === null) return
  window.clearInterval(liveRefreshTimer)
  liveRefreshTimer = null
}

function shouldAutoOpenLive(phase?: string | null) {
  return phase ? AUTO_LIVE_PHASES.has(phase) : false
}

async function checkGameflowForAutoLive() {
  const phase = await loadGameflowPhase().catch(() => null)
  const shouldOpen = shouldAutoOpenLive(phase)

  if (!shouldOpen) {
    autoLiveSessionActive = false
    return
  }

  if (autoLiveSessionActive) return
  autoLiveSessionActive = true

  navigateToPage("live")
  void refreshLiveGame({ silent: true, switchPage: false })
}

function startGameflowWatcher() {
  if (gameflowWatchTimer !== null) return
  void checkGameflowForAutoLive()
  gameflowWatchTimer = window.setInterval(() => {
    void checkGameflowForAutoLive()
  }, GAMEFLOW_WATCH_MS)
}

function stopGameflowWatcher() {
  if (gameflowWatchTimer === null) return
  window.clearInterval(gameflowWatchTimer)
  gameflowWatchTimer = null
}

watch(activePage, (page, previousPage) => {
  if (previousPage) savePageScroll(previousPage)
  void restorePageScroll(page)

  if (page === "live") {
    startLiveAutoRefresh()
  } else {
    stopLiveAutoRefresh()
  }
})

onMounted(async () => {
  startGameflowWatcher()
  void checkForAppUpdate()
  drillCachePruneTimer = window.setInterval(pruneHiddenDrillCache, 60_000)
  unlistenProgress = await listen<StatsLoadProgress>("stats-load-progress", (event) => {
    const progress = event.payload
    if (progress.requestId !== progressOverlay.value.requestId) return
    progressOverlay.value.loaded = progress.loaded
    progressOverlay.value.total = progress.total
  })
})

onUnmounted(() => {
  unlistenProgress?.()
  stopGameflowWatcher()
  stopLiveAutoRefresh()
  if (drillCachePruneTimer !== null) {
    window.clearInterval(drillCachePruneTimer)
    drillCachePruneTimer = null
  }
})
</script>

<template>
  <main class="app-shell">
    <aside class="sidebar">
      <nav class="side-nav">
        <div class="nav-history">
          <button aria-label="后退" :disabled="!canGoBack" @click="navigateBack">
            <ChevronLeft :size="18" />
          </button>
          <button aria-label="前进" :disabled="!canGoForward" @click="navigateForward">
            <ChevronRight :size="18" />
          </button>
        </div>
        <button
          v-for="item in navItems"
          :key="item.key"
          :class="{ active: activePage === item.key }"
          @click="navigateToPage(item.key)"
        >
          <component :is="item.icon" :size="18" />
          {{ item.label }}
        </button>
      </nav>
      <button class="settings-entry" @click="settingsOpen = true">
        <Settings :size="18" />
        设置
      </button>
    </aside>

    <section class="workspace" ref="workspaceRef">
      <section class="drill-workspace" v-show="activePage === 'details'">
        <nav class="drill-tabs" v-if="drillTabs.length" aria-label="战绩标签页">
          <button
            v-for="tab in drillTabs"
            :key="tab.id"
            :class="{ active: activeDrillTabId === tab.id }"
            @click="activateDrillTab(tab.id)"
          >
            <span>{{ tab.title }}</span>
            <X :size="13" @click.stop="closeDrillTab(tab.id)" />
          </button>
        </nav>

        <section class="details-empty" v-if="!activeDrillTab">
          <strong>暂无详细战绩</strong>
          <span>从当前角色或查战绩页面点击任意具体对局后，会在这里新增标签页</span>
        </section>

        <MatchOverviewPanel
          v-else-if="activeDrillTab.type === 'overview'"
          :game="activeDrillTab.game"
          :match-detail="activeDrillTab.detail"
          :loading="activeDrillTab.loading"
          :error="activeDrillTab.error"
          :champions="championMap"
          :game-assets="gameAssets"
          :sgp-server-id="activeDrillTab.sgpServerId"
          @open-player="openPlayerDrillTab($event, activeDrillTab.sgpServerId)"
        />

        <PlayerRecordTab
          v-else-if="activeDrillTab.type === 'player'"
          :key="activeDrillTab.id"
          :query="activeDrillTab.query"
          :sgp-server-id="activeDrillTab.sgpServerId"
          :champions="championMap"
          :game-assets="gameAssets"
          :share-settings="shareSettings"
          @open-match="openOverviewTab"
        />
      </section>

      <RecordView
        v-show="activePage === 'current'"
        :recent-stats="currentRecentStats"
        :full-stats="currentFullStats"
        :ranked-stats="currentRankedStats"
        :ranked-loading="currentRankedLoading"
        :share-settings="shareSettings"
        :champions="championMap"
        :game-assets="gameAssets"
        :loading="currentRecentLoading || currentStatsLoading"
        :recent-loading="currentRecentLoading"
        :recent-has-more="currentRecentHasMore"
        :stats-depth="currentStatsDepth"
        :error="currentError"
        @load-recent="loadCurrentRecent"
        @load-stats="loadCurrentStats"
        @refresh-recent="loadCurrentRecent(true)"
        @refresh-stats="loadCurrentStats(true)"
        @load-recent-more="loadCurrentRecentMore"
        @change-stats-depth="changeCurrentStatsDepth"
        @open-match="openOverviewTab"
      />

      <section class="live-page" v-show="activePage === 'live'">
        <header class="page-header">
          <div>
            <div class="eyebrow">Live Game</div>
            <h2>实时战绩</h2>
          </div>
          <button class="primary-action" @click="refreshLiveGame()" :disabled="liveLoading || liveRefreshing">
            <LoaderCircle v-if="liveLoading || liveRefreshing" class="spin" :size="16" />
            <RefreshCw v-else :size="16" />
            {{ liveRefreshing ? "同步中" : "刷新" }}
          </button>
        </header>

        <LiveGamePanel
          :live-game="liveGame"
          :champions="championMap"
          :game-assets="gameAssets"
          :loading="liveLoading"
          :error="liveError"
        />
      </section>

      <section class="search-page" v-show="activePage === 'search'">
        <section class="search-landing" v-if="!activeSearchQuery">
          <div class="search-center">
            <h2>查战绩</h2>
            <div class="search-box large">
              <select
                v-model="selectedSearchServerId"
                class="server-select"
                aria-label="区服"
                @change="markSearchServerTouched"
              >
                <option v-for="server in searchServerOptions" :key="server.id" :value="server.id">
                  {{ server.label }}
                </option>
              </select>
              <Search :size="18" />
              <input v-model="searchInput" placeholder="GameName#Tag 或 PUUID" @keyup.enter="runSearch()" />
              <button @click="runSearch()" :disabled="!searchInput.trim()">搜索</button>
            </div>

            <div class="history" v-if="searchHistory.length">
              <div class="history-title">历史查询记录</div>
              <div
                v-for="item in searchHistory"
                :key="`${item.sgpServerId}:${item.query}`"
                class="history-item"
              >
                <button class="history-open" @click="runSearch(item.query, item.sgpServerId)">
                  <span>{{ searchServerLabel(item.sgpServerId) }}</span>
                  <strong>{{ item.query }}</strong>
                </button>
                <button
                  class="history-delete"
                  aria-label="删除历史查询"
                  @click="removeSearchHistory(item)"
                >
                  <X :size="14" />
                </button>
              </div>
            </div>
          </div>
        </section>

        <RecordView
          v-else
          :recent-stats="searchRecentStats"
          :full-stats="searchFullStats"
          :ranked-stats="searchRankedStats"
          :ranked-loading="searchRankedLoading"
          :champions="championMap"
          :game-assets="gameAssets"
          :loading="searchRecentLoading || searchStatsLoading"
          :recent-loading="searchRecentLoading"
          :recent-has-more="searchRecentHasMore"
          :stats-depth="searchStatsDepth"
          :error="searchError"
          :sgp-server-id="activeSearchServerId"
          :share-settings="shareSettings"
          @load-recent="loadSearchRecent"
          @load-stats="loadSearchStats"
          @refresh-recent="loadSearchRecent(true)"
          @refresh-stats="loadSearchStats(true)"
          @load-recent-more="loadSearchRecentMore"
          @change-stats-depth="changeSearchStatsDepth"
          @open-match="openOverviewTab"
        >
          <template #toolbar>
            <div class="search-toolbar">
              <button class="return-search" @click="returnToSearchHome">
                <ArrowLeft :size="16" />
                返回
              </button>
              <div class="search-box compact">
                <select
                  v-model="selectedSearchServerId"
                  class="server-select"
                  aria-label="区服"
                  @change="markSearchServerTouched"
                >
                  <option v-for="server in searchServerOptions" :key="server.id" :value="server.id">
                    {{ server.label }}
                  </option>
                </select>
                <Search :size="16" />
                <input v-model="searchInput" placeholder="GameName#Tag 或 PUUID" @keyup.enter="runSearch()" />
                <button @click="runSearch()" :disabled="!searchInput.trim()">搜索</button>
              </div>
            </div>
          </template>
        </RecordView>
      </section>
    </section>

    <div class="progress-overlay" v-if="progressOverlay.visible">
      <div class="progress-modal">
        <div class="progress-ring" :style="progressRingStyle">
          <div>
            <strong>{{ progressOverlay.loaded }}</strong>
            <span>/{{ progressOverlay.total }}</span>
          </div>
        </div>
        <button @click="cancelProgress">取消</button>
      </div>
    </div>

    <div class="settings-overlay" v-if="settingsOpen" @click.self="settingsOpen = false">
      <section class="settings-modal">
        <header>
          <div>
            <span>截图设置</span>
            <strong>分享图片范围</strong>
          </div>
          <button @click="settingsOpen = false">关闭</button>
        </header>

        <label>
          <span>单英雄战绩截图英雄数</span>
          <input
            v-model.number="shareSettings.championAnalysisLimit"
            type="number"
            min="1"
            max="50"
            @change="persistShareSettings"
          />
        </label>

        <label>
          <span>单英雄具体战绩截图场数</span>
          <input
            v-model.number="shareSettings.championGamesAnalysisLimit"
            type="number"
            min="1"
            max="100"
            @change="persistShareSettings"
          />
        </label>

        <label class="settings-toggle">
          <span>生成手机版截图</span>
          <input
            v-model="shareSettings.mobileShareLayout"
            type="checkbox"
            @change="persistShareSettings"
          />
        </label>
      </section>
    </div>

    <div class="update-overlay" v-if="updateDialog">
      <section class="update-modal" role="dialog" aria-modal="true" aria-labelledby="update-title">
        <button class="update-close" aria-label="关闭更新提示" @click="closeUpdateDialog">
          <X :size="16" />
        </button>
        <div class="update-badge">New Version</div>
        <header>
          <span>发现新版本</span>
          <strong id="update-title">{{ updateDialog.latestVersion }}</strong>
        </header>

        <p>
          {{ updateDialog.releaseName || "新版安装包已经准备好，点击下载后请选择覆盖安装以保留任务栏图标。" }}
        </p>

        <dl>
          <div>
            <dt>当前版本</dt>
            <dd>{{ updateDialog.currentVersion }}</dd>
          </div>
          <div>
            <dt>最新版本</dt>
            <dd>{{ updateDialog.latestVersion }}</dd>
          </div>
        </dl>

        <footer>
          <button class="update-secondary" @click="closeUpdateDialog">稍后</button>
          <button class="update-copy" @click="copyUpdatePassword">复制密码</button>
          <button class="update-primary" @click="openUpdateDownload">立即下载</button>
        </footer>
      </section>
    </div>

    <ToastStack :toasts="toasts" @dismiss="dismissToast" @action="runToastAction" />
  </main>
</template>

<style>
:root {
  font-family:
    Inter, "Microsoft YaHei UI", "Microsoft YaHei", system-ui, -apple-system, BlinkMacSystemFont,
    "Segoe UI", sans-serif;
  color: #263238;
  background: #e9efed;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-width: 1040px;
  min-height: 680px;
  overflow: hidden;
}

button,
input,
select {
  font: inherit;
}

button {
  border: 0;
}

button:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.spin {
  animation: spin 0.9s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>

<style scoped>
.app-shell {
  display: grid;
  grid-template-columns: 124px minmax(0, 1fr);
  height: 100vh;
  background:
    linear-gradient(135deg, rgba(46, 125, 107, 0.13), transparent 34%),
    linear-gradient(315deg, rgba(229, 184, 75, 0.18), transparent 30%),
    #eef4f2;
}

.sidebar {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 12px;
  border-right: 1px solid rgba(38, 50, 56, 0.1);
  background: rgba(255, 255, 255, 0.76);
  backdrop-filter: blur(14px);
  padding: 14px 10px;
}

.side-nav {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.nav-history {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
  margin-bottom: 2px;
}

.side-nav .nav-history button {
  justify-content: center;
  padding: 8px 0;
  color: #1f4f48;
  background: #d9e9e5;
}

.side-nav .nav-history button:not(:disabled):hover {
  background: #c8ddd8;
}

.side-nav button {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 7px;
  border-radius: 8px;
  color: #54666c;
  background: transparent;
  cursor: pointer;
  padding: 10px 8px;
  font-size: 13px;
}

.settings-entry {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  gap: 7px;
  border-radius: 8px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  padding: 10px 8px;
  font-size: 13px;
}

.side-nav button.active {
  color: #12312b;
  background: #dfecea;
}

.workspace {
  position: relative;
  min-width: 0;
  overflow: auto;
  padding: 26px;
}

.drill-workspace {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 12px;
}

.drill-tabs {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  gap: 8px;
}

.drill-tabs button {
  display: inline-flex;
  max-width: 280px;
  align-items: center;
  gap: 8px;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  color: #315f58;
  background: #ffffff;
  cursor: pointer;
  font-size: 13px;
  font-weight: 800;
  padding: 8px 10px;
}

.drill-tabs button.active {
  color: #ffffff;
  border-color: #1f5f56;
  background: #1f5f56;
}

.drill-tabs span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.drill-tabs svg {
  flex: 0 0 auto;
}

.details-empty {
  display: grid;
  min-height: calc(100vh - 96px);
  place-content: center;
  gap: 8px;
  border: 1px dashed #cbdcd8;
  border-radius: 8px;
  color: #657179;
  background: rgba(255, 255, 255, 0.62);
  text-align: center;
}

.details-empty strong {
  color: #263238;
  font-size: 22px;
  font-weight: 800;
}

.details-empty span {
  font-size: 13px;
}

.live-page,
.search-page {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.page-header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 18px;
}

.eyebrow {
  color: #6f7e84;
  font-size: 12px;
  font-weight: 800;
  letter-spacing: 0;
  text-transform: uppercase;
}

.page-header h2,
.search-center h2 {
  margin: 4px 0 0;
  color: #1f2a2e;
  font-size: 30px;
  line-height: 1.15;
}

.primary-action,
.search-box button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border-radius: 8px;
  color: #ffffff;
  background: #1f5f56;
  cursor: pointer;
  padding: 10px 14px;
}

.search-landing {
  display: grid;
  min-height: calc(100vh - 52px);
  place-items: center;
}

.search-center {
  display: flex;
  width: min(680px, 100%);
  flex-direction: column;
  align-items: stretch;
  gap: 16px;
}

.search-center h2 {
  text-align: center;
}

.search-box {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid #cddfdc;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 12px 28px rgba(32, 67, 73, 0.08);
  padding: 6px;
}

.server-select {
  flex: 0 0 116px;
  min-width: 0;
  border: 0;
  border-right: 1px solid #dce8e5;
  outline: 0;
  color: #264d47;
  background: transparent;
  cursor: pointer;
  font-weight: 800;
  padding: 10px 10px 10px 6px;
}

.search-box input {
  width: 100%;
  min-width: 0;
  border: 0;
  outline: 0;
  color: #263238;
  background: transparent;
}

.search-box.large {
  padding-left: 14px;
}

.search-box.large input {
  padding: 12px 0;
}

.search-box.compact {
  width: min(620px, 54vw);
  box-shadow: none;
}

.search-toolbar {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
}

.return-search {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border-radius: 8px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  padding: 10px 12px;
}

.history {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: center;
}

.history-title {
  flex-basis: 100%;
  color: #718087;
  font-size: 13px;
  text-align: center;
}

.history-item {
  position: relative;
  display: inline-flex;
  align-items: center;
}

.history-open {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  max-width: 260px;
  border-radius: 999px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  padding: 7px 34px 7px 12px;
}

.history-open span {
  flex: 0 0 auto;
  color: #718087;
  font-size: 12px;
}

.history-open strong {
  min-width: 0;
  overflow: hidden;
  color: #315f58;
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-delete {
  position: absolute;
  right: 6px;
  display: grid;
  width: 22px;
  height: 22px;
  place-items: center;
  border-radius: 50%;
  color: #8b5b5a;
  background: #f7dedc;
  cursor: pointer;
  opacity: 0;
  pointer-events: none;
  transition:
    opacity 0.15s ease,
    background 0.15s ease;
}

.history-item:hover .history-delete,
.history-delete:focus-visible {
  opacity: 1;
  pointer-events: auto;
}

.history-delete:hover {
  background: #efc7c4;
}

.progress-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: grid;
  place-items: center;
  background: rgba(238, 244, 242, 0.72);
  backdrop-filter: blur(8px);
}

.progress-modal {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 18px;
}

.progress-ring {
  display: grid;
  width: 168px;
  height: 168px;
  place-items: center;
  border-radius: 50%;
  background:
    radial-gradient(circle at center, #ffffff 0 58%, transparent 59%),
    conic-gradient(#1f5f56 var(--progress), #dce7e4 0deg);
  box-shadow: 0 20px 50px rgba(32, 67, 73, 0.18);
}

.progress-ring div {
  display: flex;
  align-items: baseline;
  gap: 2px;
  color: #53656b;
}

.progress-ring strong {
  color: #1f2a2e;
  font-size: 30px;
}

.progress-ring span {
  font-size: 15px;
}

.progress-modal button {
  border-radius: 8px;
  color: #ffffff;
  background: #be4b49;
  cursor: pointer;
  padding: 10px 18px;
}

.settings-overlay {
  position: fixed;
  inset: 0;
  z-index: 1300;
  display: grid;
  place-items: center;
  background: rgba(20, 35, 38, 0.32);
  backdrop-filter: blur(8px);
}

.settings-modal {
  display: flex;
  width: 360px;
  flex-direction: column;
  gap: 14px;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 24px 70px rgba(18, 42, 46, 0.24);
  padding: 16px;
}

.settings-modal header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
}

.settings-modal header div {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.settings-modal header span,
.settings-modal label span {
  color: #718087;
  font-size: 12px;
  font-weight: 800;
}

.settings-modal header strong {
  color: #1f2a2e;
  font-size: 18px;
}

.settings-modal header button {
  border-radius: 8px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  padding: 8px 10px;
  font-weight: 800;
}

.settings-modal label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #f7fbfa;
  padding: 10px 12px;
}

.settings-modal input {
  width: 92px;
  border: 1px solid #cddfdc;
  border-radius: 7px;
  outline: 0;
  color: #263238;
  background: #ffffff;
  padding: 8px 9px;
  font-weight: 800;
}

.settings-modal input[type="checkbox"] {
  width: 18px;
  height: 18px;
  accent-color: #1f5f56;
  cursor: pointer;
}

.update-overlay {
  position: fixed;
  inset: 0;
  z-index: 1500;
  display: grid;
  place-items: center;
  background:
    linear-gradient(135deg, rgba(31, 95, 86, 0.24), rgba(20, 35, 38, 0.18)),
    rgba(20, 35, 38, 0.42);
  backdrop-filter: blur(10px);
  padding: 24px;
}

.update-modal {
  position: relative;
  display: flex;
  width: min(520px, calc(100vw - 48px));
  flex-direction: column;
  gap: 18px;
  overflow: hidden;
  border: 1px solid rgba(214, 229, 225, 0.92);
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 30px 90px rgba(12, 32, 35, 0.34);
  padding: 26px;
}

.update-modal::before {
  position: absolute;
  inset: 0 0 auto;
  height: 5px;
  background: linear-gradient(90deg, #1f5f56, #d5a835, #2f78d6);
  content: "";
}

.update-close {
  position: absolute;
  top: 14px;
  right: 14px;
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  border-radius: 50%;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
}

.update-close:hover {
  background: #dfecea;
}

.update-badge {
  width: fit-content;
  border-radius: 999px;
  color: #1f5f56;
  background: #e6f2ef;
  font-size: 12px;
  font-weight: 950;
  letter-spacing: 0;
  padding: 6px 10px;
}

.update-modal header {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.update-modal header span {
  color: #63757a;
  font-size: 13px;
  font-weight: 900;
}

.update-modal header strong {
  color: #18282c;
  font-size: 42px;
  font-weight: 950;
  line-height: 1;
}

.update-modal p {
  max-width: 440px;
  margin: 0;
  color: #4f6368;
  font-size: 15px;
  font-weight: 750;
  line-height: 1.7;
}

.update-modal dl {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
  margin: 0;
}

.update-modal dl div {
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #f6faf9;
  padding: 12px;
}

.update-modal dt,
.update-modal dd {
  margin: 0;
}

.update-modal dt {
  color: #718087;
  font-size: 12px;
  font-weight: 850;
}

.update-modal dd {
  margin-top: 5px;
  color: #20333a;
  font-size: 18px;
  font-weight: 950;
}

.update-modal footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 4px;
}

.update-modal footer button {
  border-radius: 8px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 900;
  padding: 10px 16px;
}

.update-secondary {
  color: #315f58;
  background: #edf5f3;
}

.update-copy {
  color: #8b6410;
  background: #fff4ce;
}

.update-primary {
  color: #ffffff;
  background: #1f5f56;
  box-shadow: 0 12px 24px rgba(31, 95, 86, 0.22);
}

.settings-toggle {
  cursor: pointer;
}
</style>
