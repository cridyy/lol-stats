<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import { computed, inject, nextTick, onUnmounted, ref, watch } from "vue"
import { ArrowLeft, ClipboardCopy, Eraser, ExternalLink, Save } from "lucide-vue-next"
import { copyElementAsPng } from "../imageShare"
import { notifyKey } from "../notifications"
import type { ChampionSummaryItem } from "../types"
import { championName } from "../utils"
import ChampionAvatar from "./ChampionAvatar.vue"

type ToolTab = "liveChampion" | "aramkit" | "tier" | "reserved"
type TierId = "hang" | "top" | "elite" | "npc" | "bad"

type TierState = Record<TierId, number[]>

type SavedRanking = {
  name: string
  tiers: TierState
  updatedAt: number
}

type RankingDraft = {
  activeSaveName: string
  tiers: TierState
}

type DropTarget =
  | { kind: "tier"; tierId: TierId; index?: number }
  | { kind: "pool" }

const props = defineProps<{
  champions: Record<number, ChampionSummaryItem>
  liveChampionId?: number | null
}>()

const RANKING_DRAFT_KEY = "lol-stats.tools.tier-ranking.draft"
const RANKING_SAVED_KEY = "lol-stats.tools.tier-ranking.saved"
const ARAMKIT_NOTICE_KEY = "lol-stats.tools.aramkit.notice.dismissed"
const ARAMKIT_URL = "https://aramkit.com/zh-CN"
const DEFAULT_SAVE_NAME = "控"

const tierRows: Array<{ id: TierId; label: string; className: string }> = [
  { id: "hang", label: "夯", className: "tier-hang" },
  { id: "top", label: "顶级", className: "tier-top" },
  { id: "elite", label: "人上人", className: "tier-elite" },
  { id: "npc", label: "NPC", className: "tier-npc" },
  { id: "bad", label: "拉完了", className: "tier-bad" },
]

const activeTab = ref<ToolTab>("aramkit")
const activeSaveName = ref(DEFAULT_SAVE_NAME)
const tiers = ref<TierState>(emptyTiers())
const savedRankings = ref<Record<string, SavedRanking>>(loadSavedRankings())
const draggingChampionId = ref<number | null>(null)
const dragOverTarget = ref<TierId | "pool" | null>(null)
const dragPointer = ref({ x: 0, y: 0 })
const initialized = ref(false)
const saveDialogOpen = ref(false)
const saveNameInput = ref(DEFAULT_SAVE_NAME)
const clearDialogOpen = ref(false)
const aramkitNoticeOpen = ref(!localStorage.getItem(ARAMKIT_NOTICE_KEY))
const aramkitDontRemind = ref(false)
const aramkitFrameRef = ref<HTMLIFrameElement | null>(null)
const aramkitLiveFrameRef = ref<HTMLIFrameElement | null>(null)
const aramkitFrameSrc = ref(ARAMKIT_URL)
const exporting = ref(false)
const captureRef = ref<HTMLElement | null>(null)
const notify = inject(notifyKey, () => 0)

const championList = computed(() =>
  Object.values(props.champions)
    .filter((champion) => champion.id > 0)
    .sort((left, right) => championName(props.champions, left.id).localeCompare(championName(props.champions, right.id), "zh-CN")),
)

const placedChampionIds = computed(() => new Set(Object.values(tiers.value).flat()))
const poolChampions = computed(() =>
  championList.value.filter((champion) => !placedChampionIds.value.has(champion.id)),
)
const savedNames = computed(() => {
  const names = new Set([DEFAULT_SAVE_NAME, ...Object.keys(savedRankings.value)])
  return Array.from(names).sort((left, right) => {
    if (left === DEFAULT_SAVE_NAME) return -1
    if (right === DEFAULT_SAVE_NAME) return 1
    return left.localeCompare(right, "zh-CN")
  })
})
const liveChampion = computed(() =>
  props.liveChampionId ? props.champions[props.liveChampionId] || null : null,
)
const liveChampionUrl = computed(() => {
  if (!liveChampion.value) return ""
  return aramkitChampionUrl(liveChampion.value)
})
const isAramkitActive = computed(() => activeTab.value === "aramkit" || activeTab.value === "liveChampion")
const activeAramkitUrl = computed(() =>
  activeTab.value === "liveChampion" && liveChampionUrl.value ? liveChampionUrl.value : ARAMKIT_URL,
)

watch(
  [tiers, activeSaveName],
  () => {
    if (!initialized.value) return
    persistDraft()
  },
  { deep: true },
)

watch(
  () => championList.value.length,
  (count) => {
    if (initialized.value || count <= 0) return
    loadInitialDraft()
    initialized.value = true
  },
  { immediate: true },
)

watch(
  () => liveChampionUrl.value,
  (url, previousUrl) => {
    if (!url) {
      if (activeTab.value === "liveChampion") activeTab.value = "aramkit"
      return
    }

    if (!previousUrl && activeTab.value === "aramkit") {
      activeTab.value = "liveChampion"
    }
  },
)

onUnmounted(() => {
  cleanupPointerDrag()
})

function emptyTiers(): TierState {
  return {
    hang: [],
    top: [],
    elite: [],
    npc: [],
    bad: [],
  }
}

function cloneTiers(value: TierState): TierState {
  return {
    hang: [...(value.hang || [])],
    top: [...(value.top || [])],
    elite: [...(value.elite || [])],
    npc: [...(value.npc || [])],
    bad: [...(value.bad || [])],
  }
}

function sanitizeTiers(value: unknown): TierState {
  const source = (value || {}) as Partial<Record<TierId, unknown>>
  const knownChampionIds = new Set(championList.value.map((champion) => champion.id))
  const seen = new Set<number>()
  const result = emptyTiers()

  for (const tier of tierRows) {
    const ids = Array.isArray(source[tier.id]) ? (source[tier.id] as unknown[]) : []
    result[tier.id] = ids
      .map((id) => Number(id))
      .filter((id) => Number.isFinite(id) && knownChampionIds.has(id) && !seen.has(id))
    for (const id of result[tier.id]) seen.add(id)
  }

  return result
}

function loadInitialDraft() {
  const draft = readJson<RankingDraft>(RANKING_DRAFT_KEY)
  if (draft) {
    activeSaveName.value = draft.activeSaveName || DEFAULT_SAVE_NAME
    tiers.value = sanitizeTiers(draft.tiers)
    return
  }

  const saved = savedRankings.value[DEFAULT_SAVE_NAME]
  activeSaveName.value = DEFAULT_SAVE_NAME
  tiers.value = saved ? sanitizeTiers(saved.tiers) : emptyTiers()
  persistDraft()
}

function loadSavedRankings() {
  const stored = readJson<Record<string, SavedRanking>>(RANKING_SAVED_KEY) || {}
  if (!stored[DEFAULT_SAVE_NAME]) {
    stored[DEFAULT_SAVE_NAME] = {
      name: DEFAULT_SAVE_NAME,
      tiers: emptyTiers(),
      updatedAt: Date.now(),
    }
  }
  return stored
}

function persistDraft() {
  writeJson(RANKING_DRAFT_KEY, {
    activeSaveName: activeSaveName.value || DEFAULT_SAVE_NAME,
    tiers: cloneTiers(tiers.value),
  })
}

function persistSavedRankings() {
  writeJson(RANKING_SAVED_KEY, savedRankings.value)
}

function readJson<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : null
  } catch {
    return null
  }
}

function writeJson(key: string, value: unknown) {
  localStorage.setItem(key, JSON.stringify(value))
}

function activateTab(tab: ToolTab) {
  if (tab === "liveChampion" && !liveChampionUrl.value) return
  activeTab.value = tab
  if ((tab === "aramkit" || tab === "liveChampion") && !localStorage.getItem(ARAMKIT_NOTICE_KEY)) {
    aramkitNoticeOpen.value = true
  }
}

function closeAramKitNotice() {
  if (aramkitDontRemind.value) {
    localStorage.setItem(ARAMKIT_NOTICE_KEY, "1")
  }
  aramkitNoticeOpen.value = false
}

async function openAramKitInBrowser() {
  try {
    await openUrl(activeAramkitUrl.value)
  } catch (error) {
    notify({
      kind: "error",
      title: "打开浏览器失败",
      message: error instanceof Error ? error.message : String(error),
      duration: 7000,
    })
  }
}

function wait(ms: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, ms))
}

async function goBackAramKitFrame() {
  const frame = activeTab.value === "liveChampion" ? aramkitLiveFrameRef.value : aramkitFrameRef.value
  if (!frame) {
    notify({
      kind: "info",
      title: "内嵌页面未就绪",
      message: "请稍后再试，或在页面内使用 Alt + 左方向键。",
      duration: 5000,
    })
    return
  }

  try {
    frame.focus()
    frame.contentWindow?.focus()
  } catch {
    frame.focus()
  }

  try {
    await nextTick()
    await wait(80)
    await invoke("send_alt_left_shortcut")
  } catch (error) {
    notify({
      kind: "info",
      title: "后退快捷键发送失败",
      message: error instanceof Error ? error.message : String(error),
      duration: 7000,
    })
  }
}

function aramkitChampionUrl(champion: ChampionSummaryItem) {
  const slug = aramkitChampionSlug(champion)
  return `${ARAMKIT_URL}/champions/${slug}`
}

function aramkitChampionSlug(champion: ChampionSummaryItem) {
  return (champion.alias || String(champion.id)).toLowerCase()
}

function championsInTier(tierId: TierId) {
  return tiers.value[tierId]
    .map((id) => props.champions[id])
    .filter(Boolean)
}

function startDrag(event: PointerEvent, championId: number) {
  if (event.button !== 0) return

  event.preventDefault()
  draggingChampionId.value = championId
  dragPointer.value = { x: event.clientX, y: event.clientY }
  const target = dropTargetFromPoint(event.clientX, event.clientY)
  dragOverTarget.value = target?.kind === "tier" ? target.tierId : target?.kind === "pool" ? "pool" : null

  window.addEventListener("pointermove", handlePointerMove)
  window.addEventListener("pointerup", handlePointerUp)
}

function endDrag() {
  draggingChampionId.value = null
  dragOverTarget.value = null
  cleanupPointerDrag()
}

function cleanupPointerDrag() {
  window.removeEventListener("pointermove", handlePointerMove)
  window.removeEventListener("pointerup", handlePointerUp)
}

function handlePointerMove(event: PointerEvent) {
  if (!draggingChampionId.value) return

  dragPointer.value = { x: event.clientX, y: event.clientY }
  const target = dropTargetFromPoint(event.clientX, event.clientY)
  dragOverTarget.value = target?.kind === "tier" ? target.tierId : target?.kind === "pool" ? "pool" : null
}

function handlePointerUp(event: PointerEvent) {
  const championId = draggingChampionId.value
  if (!championId) return

  const target = dropTargetFromPoint(event.clientX, event.clientY)
  if (target?.kind === "tier") {
    moveChampionToTier(championId, target.tierId, target.index)
  } else if (target?.kind === "pool") {
    removeChampionFromTiers(championId)
  }

  endDrag()
}

function dropTargetFromPoint(x: number, y: number): DropTarget | null {
  const element = document.elementFromPoint(x, y) as HTMLElement | null
  const target = element?.closest<HTMLElement>("[data-drop-target]")
  if (!target) return null

  if (target.dataset.dropTarget === "pool") return { kind: "pool" }
  if (target.dataset.dropTarget !== "tier") return null

  const tierId = target.dataset.tierId as TierId | undefined
  if (!tierId || !tierRows.some((tier) => tier.id === tierId)) return null

  const championElement = element?.closest<HTMLElement>("[data-tier-champion]")
  const rawIndex = championElement?.dataset.championIndex
  const championTierId = championElement?.dataset.tierId
  if (championElement && championTierId === tierId && rawIndex !== undefined) {
    const rect = championElement.getBoundingClientRect()
    const index = Number(rawIndex) + (x > rect.left + rect.width / 2 ? 1 : 0)
    return { kind: "tier", tierId, index }
  }

  return { kind: "tier", tierId }
}

function moveChampionToTier(championId: number, tierId: TierId, index?: number) {
  const next = cloneTiers(tiers.value)
  let sourceTierId: TierId | null = null
  let sourceIndex = -1

  for (const tier of tierRows) {
    const currentIndex = next[tier.id].indexOf(championId)
    if (currentIndex >= 0) {
      sourceTierId = tier.id
      sourceIndex = currentIndex
    }
    next[tier.id] = next[tier.id].filter((id) => id !== championId)
  }

  const target = next[tierId]
  let safeIndex = typeof index === "number" ? Math.max(0, Math.min(index, target.length)) : target.length
  if (sourceTierId === tierId && sourceIndex >= 0 && sourceIndex < safeIndex) {
    safeIndex -= 1
  }
  target.splice(safeIndex, 0, championId)
  tiers.value = next
}

function removeChampionFromTiers(championId: number) {
  const next = cloneTiers(tiers.value)
  for (const tier of tierRows) {
    next[tier.id] = next[tier.id].filter((id) => id !== championId)
  }
  tiers.value = next
}

function openSaveDialog() {
  saveNameInput.value = activeSaveName.value || DEFAULT_SAVE_NAME
  saveDialogOpen.value = true
  void nextTick(() => document.querySelector<HTMLInputElement>(".tier-save-dialog input")?.focus())
}

function confirmSaveRanking() {
  const name = saveNameInput.value.trim() || DEFAULT_SAVE_NAME
  activeSaveName.value = name
  savedRankings.value = {
    ...savedRankings.value,
    [name]: {
      name,
      tiers: cloneTiers(tiers.value),
      updatedAt: Date.now(),
    },
  }
  persistSavedRankings()
  persistDraft()
  saveDialogOpen.value = false
  notify({ kind: "success", title: "排行已保存", message: name })
}

function switchSavedRanking(event: Event) {
  const name = (event.target as HTMLSelectElement).value || DEFAULT_SAVE_NAME
  activeSaveName.value = name
  tiers.value = sanitizeTiers(savedRankings.value[name]?.tiers || emptyTiers())
}

function clearCurrentRanking() {
  tiers.value = emptyTiers()
  clearDialogOpen.value = false
}

async function exportRankingImage() {
  if (!captureRef.value || exporting.value) return
  exporting.value = true

  try {
    await copyElementAsPng(captureRef.value, {
      backgroundColor: "#f6faf9",
      pixelRatio: 2,
    })
    notify({ kind: "success", title: "排行截图已复制", message: activeSaveName.value })
  } catch (error) {
    notify({
      kind: "error",
      title: "排行截图生成失败",
      message: error instanceof Error ? error.message : String(error),
      duration: 7000,
    })
  } finally {
    exporting.value = false
  }
}
</script>

<template>
  <section class="tools-panel">
    <header class="tools-header">
      <div class="tool-tabs" aria-label="工具标签页">
        <button
          v-if="liveChampion"
          class="live-champion-tab"
          :class="{ active: activeTab === 'liveChampion' }"
          :title="`打开 ${championName(champions, liveChampion.id)} 的 AramKit 页面`"
          @click="activateTab('liveChampion')"
        >
          <ChampionAvatar :champion-id="liveChampion.id" :champions="champions" :size="20" />
          {{ championName(champions, liveChampion.id) }}
        </button>
        <button :class="{ active: activeTab === 'aramkit' }" @click="activateTab('aramkit')">AramKit</button>
        <button :class="{ active: activeTab === 'tier' }" @click="activateTab('tier')">由夯到拉</button>
        <button :class="{ active: activeTab === 'reserved' }" @click="activateTab('reserved')">备用工具</button>
      </div>

      <label class="ranking-switch" v-show="activeTab === 'tier'">
        <span>切换</span>
        <select :value="activeSaveName" @change="switchSavedRanking">
          <option v-for="name in savedNames" :key="name" :value="name">{{ name }}</option>
        </select>
      </label>

      <div class="tier-actions" v-show="activeTab === 'tier'">
        <button @click="openSaveDialog">
          <Save :size="15" />
          保存
        </button>
        <button @click="clearDialogOpen = true">
          <Eraser :size="15" />
          清空
        </button>
        <button :disabled="exporting" @click="exportRankingImage">
          <ClipboardCopy :size="15" />
          {{ exporting ? "生成中" : "导出" }}
        </button>
      </div>

      <div class="tier-actions external-actions" v-show="isAramkitActive">
        <span class="external-url">{{ activeAramkitUrl }}</span>
        <button @click="openAramKitInBrowser">
          <ExternalLink :size="15" />
          默认浏览器打开
        </button>
        <button @click="goBackAramKitFrame">
          <ArrowLeft :size="15" />
          返回
        </button>
      </div>
    </header>

    <section class="aramkit-tool" v-show="activeTab === 'aramkit'">
      <iframe
        ref="aramkitFrameRef"
        class="aramkit-frame"
        :src="aramkitFrameSrc"
        title="ARAMKit 第三方页面"
        referrerpolicy="no-referrer-when-downgrade"
      ></iframe>
    </section>

    <section class="aramkit-tool" v-show="activeTab === 'liveChampion' && liveChampionUrl">
      <iframe
        ref="aramkitLiveFrameRef"
        class="aramkit-frame"
        :src="liveChampionUrl"
        title="ARAMKit 实时英雄页面"
        referrerpolicy="no-referrer-when-downgrade"
      ></iframe>
    </section>

    <section class="tier-tool" v-show="activeTab === 'tier'">
      <div class="tier-capture" ref="captureRef">
        <div class="tier-board">
          <section
            v-for="tier in tierRows"
            :key="tier.id"
            :class="['tier-row', tier.className, { 'drag-over': dragOverTarget === tier.id }]"
            data-drop-target="tier"
            :data-tier-id="tier.id"
          >
            <div class="tier-label">{{ tier.label }}</div>
            <div class="tier-slots">
              <div
                v-for="(champion, index) in championsInTier(tier.id)"
                :key="champion.id"
                :class="['tier-champion', { dragging: draggingChampionId === champion.id }]"
                :title="championName(champions, champion.id)"
                data-tier-champion="true"
                :data-tier-id="tier.id"
                :data-champion-index="index"
                @pointerdown.stop="startDrag($event, champion.id)"
                @dragstart.prevent
              >
                <ChampionAvatar :champion-id="champion.id" :champions="champions" :size="58" />
              </div>
            </div>
          </section>
        </div>
      </div>

      <section
        :class="['champion-pool', { 'drag-over': dragOverTarget === 'pool' }]"
        data-drop-target="pool"
      >
        <header>
          <strong>英雄池</strong>
          <span>{{ poolChampions.length }} 个</span>
        </header>
        <div class="pool-grid">
          <div
            v-for="champion in poolChampions"
            :key="champion.id"
            :class="['pool-champion', { dragging: draggingChampionId === champion.id }]"
            :title="championName(champions, champion.id)"
            @pointerdown.stop="startDrag($event, champion.id)"
            @dragstart.prevent
          >
            <ChampionAvatar :champion-id="champion.id" :champions="champions" :size="48" />
          </div>
        </div>
      </section>
    </section>

    <section class="tool-placeholder" v-show="activeTab === 'reserved'">
      <strong>备用工具</strong>
    </section>

    <div class="center-overlay" v-if="aramkitNoticeOpen" @click.self="closeAramKitNotice">
      <section class="center-dialog aramkit-notice-dialog">
        <header>
          <strong>第三方页面提示</strong>
          <button @click="closeAramKitNotice">关闭</button>
        </header>
        <p>
          当前页面为第三方网站 ARAMKit，原网站地址为 {{ ARAMKIT_URL }}。本软件仅内嵌展示原网页，不修改页面、不注入脚本、不读取或缓存该网站数据。
        </p>
        <label class="dont-remind-row">
          <input v-model="aramkitDontRemind" type="checkbox" />
          <span>不再提醒</span>
        </label>
        <footer>
          <button @click="openAramKitInBrowser">
            <ExternalLink :size="15" />
            默认浏览器打开
          </button>
          <button class="primary" @click="closeAramKitNotice">关闭</button>
        </footer>
      </section>
    </div>

    <div class="center-overlay" v-if="saveDialogOpen" @click.self="saveDialogOpen = false">
      <section class="center-dialog tier-save-dialog">
        <header>
          <strong>保存排行</strong>
          <button @click="saveDialogOpen = false">关闭</button>
        </header>
        <label>
          <span>姓名</span>
          <input v-model="saveNameInput" @keyup.enter="confirmSaveRanking" />
        </label>
        <footer>
          <button @click="saveDialogOpen = false">取消</button>
          <button class="primary" @click="confirmSaveRanking">保存</button>
        </footer>
      </section>
    </div>

    <div class="center-overlay" v-if="clearDialogOpen" @click.self="clearDialogOpen = false">
      <section class="center-dialog">
        <header>
          <strong>确认清空</strong>
          <button @click="clearDialogOpen = false">关闭</button>
        </header>
        <p>当前排行会被清空，并立即成为新的本地草稿。</p>
        <footer>
          <button @click="clearDialogOpen = false">取消</button>
          <button class="danger" @click="clearCurrentRanking">确认清空</button>
        </footer>
      </section>
    </div>

    <div
      v-if="draggingChampionId"
      class="drag-ghost"
      :style="{ left: `${dragPointer.x}px`, top: `${dragPointer.y}px` }"
    >
      <ChampionAvatar :champion-id="draggingChampionId" :champions="champions" :size="58" />
    </div>
  </section>
</template>

<style scoped>
.tools-panel {
  display: flex;
  min-height: calc(100vh - 52px);
  flex-direction: column;
  gap: 8px;
  padding: 8px 10px 10px;
}

.tools-header,
.tool-tabs,
.tier-actions,
.ranking-switch,
.champion-pool header,
.center-dialog header,
.center-dialog footer {
  display: flex;
  align-items: center;
}

.tools-header {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
  gap: 8px;
}

.tool-tabs {
  grid-column: 1;
  gap: 8px;
}

.tool-tabs button,
.tier-actions button,
.center-dialog button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border: 1px solid #d9e5e2;
  border-radius: 8px;
  color: #30454a;
  background: #f8fbfa;
  font-size: 13px;
  font-weight: 900;
  line-height: 1;
  padding: 9px 12px;
  cursor: pointer;
}

.tool-tabs button.active,
.tier-actions button:hover,
.center-dialog .primary {
  border-color: #1f6f62;
  color: #f7fffc;
  background: #1f6f62;
}

.tool-tabs .live-champion-tab {
  border-color: #c7d8d4;
  background: linear-gradient(180deg, #ffffff 0%, #eef6f4 100%);
}

.tool-tabs .live-champion-tab.active {
  border-color: #1f6f62;
  background: linear-gradient(180deg, #2b8a78 0%, #1f6f62 100%);
}

.tool-tabs .live-champion-tab :deep(.champion-avatar) {
  flex: 0 0 auto;
}

.tier-tool {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 12px;
}

.aramkit-tool {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
}

.aramkit-frame {
  width: 100%;
  min-height: 0;
  flex: 1;
  border: 1px solid #dce8e5;
  border-radius: 8px;
  background: #ffffff;
}

.ranking-switch {
  grid-column: 2;
  justify-content: center;
  gap: 8px;
  color: #496066;
  font-size: 13px;
  font-weight: 900;
  white-space: nowrap;
}

.ranking-switch select,
.center-dialog input {
  height: 34px;
  min-width: 150px;
  border: 1px solid #d7e4e1;
  border-radius: 8px;
  color: #20333a;
  background: #ffffff;
  font-size: 14px;
  font-weight: 900;
  outline: none;
  padding: 0 10px;
}

.tier-actions {
  grid-column: 3;
  justify-content: flex-end;
  gap: 8px;
}

.external-actions {
  min-width: 0;
}

.external-url {
  overflow: hidden;
  min-width: 0;
  color: #52666b;
  font-size: 12px;
  font-weight: 900;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tier-actions button:disabled {
  opacity: 0.6;
  cursor: wait;
}

.tier-capture {
  background: transparent;
}

.tier-board {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tier-row {
  display: grid;
  min-height: 82px;
  grid-template-columns: 110px minmax(0, 1fr);
  overflow: hidden;
  border: 1px solid #dbe8e5;
  border-radius: 8px;
  background: #ffffff;
  transition:
    border-color 0.15s ease,
    box-shadow 0.15s ease,
    transform 0.15s ease;
}

.tier-row.drag-over,
.champion-pool.drag-over {
  border-color: #1f6f62;
  box-shadow: 0 0 0 3px rgba(31, 111, 98, 0.14);
}

.tier-label {
  display: grid;
  place-items: center;
  color: #17292f;
  font-size: 22px;
  font-weight: 950;
}

.tier-hang .tier-label {
  background: #ffd76e;
}

.tier-top .tier-label {
  background: #8ee1b4;
}

.tier-elite .tier-label {
  background: #9fc7ff;
}

.tier-npc .tier-label {
  background: #d7deea;
}

.tier-bad .tier-label {
  background: #f6a2a2;
}

.tier-slots {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-content: center;
  align-items: center;
  gap: 7px;
  padding: 10px;
}

.tier-champion,
.pool-champion {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  background: transparent;
  padding: 0;
  cursor: grab;
  touch-action: none;
  user-select: none;
  -webkit-user-drag: none;
  transition:
    opacity 0.15s ease,
    transform 0.15s ease;
}

.tier-champion :deep(img),
.pool-champion :deep(img) {
  pointer-events: none;
  user-select: none;
  -webkit-user-drag: none;
}

.tier-champion:hover,
.pool-champion:hover {
  transform: translateY(-1px);
}

.tier-champion.dragging,
.pool-champion.dragging {
  opacity: 0.42;
  transform: scale(0.94);
}

.tier-champion:active,
.pool-champion:active {
  cursor: grabbing;
}

.champion-pool {
  min-height: 160px;
  border: 1px solid #dce8e5;
  border-radius: 8px;
  background: #fbfdfc;
  padding: 12px;
  transition:
    border-color 0.15s ease,
    box-shadow 0.15s ease;
}

.champion-pool header {
  justify-content: space-between;
  margin-bottom: 10px;
}

.champion-pool strong {
  color: #20333a;
  font-size: 15px;
  font-weight: 950;
}

.champion-pool span {
  color: #60747a;
  font-size: 13px;
  font-weight: 900;
}

.pool-grid {
  display: flex;
  max-height: 230px;
  flex-wrap: wrap;
  gap: 7px;
  overflow: auto;
  padding-right: 4px;
}

.tool-placeholder {
  display: grid;
  min-height: 280px;
  place-items: center;
  border: 1px dashed #ccdbd8;
  border-radius: 8px;
  color: #66797e;
  background: #f8fbfa;
  font-size: 18px;
  font-weight: 950;
}

.drag-ghost {
  position: fixed;
  z-index: 1900;
  pointer-events: none;
  transform: translate(-50%, -50%) scale(1.08);
  filter: drop-shadow(0 12px 20px rgba(22, 39, 43, 0.28));
}

.center-overlay {
  position: fixed;
  z-index: 1700;
  inset: 0;
  display: grid;
  place-items: center;
  background: rgba(15, 25, 28, 0.42);
}

.center-dialog {
  width: min(420px, calc(100vw - 40px));
  border: 1px solid rgba(255, 255, 255, 0.7);
  border-radius: 8px;
  background: #f8fbfa;
  box-shadow: 0 24px 70px rgba(20, 40, 44, 0.28);
  padding: 16px;
}

.center-dialog header {
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.center-dialog header strong {
  color: #20333a;
  font-size: 18px;
  font-weight: 950;
}

.center-dialog header button {
  padding: 8px 10px;
}

.center-dialog label {
  display: flex;
  flex-direction: column;
  gap: 7px;
  color: #52666b;
  font-size: 13px;
  font-weight: 900;
}

.center-dialog input {
  width: 100%;
}

.center-dialog p {
  margin: 0;
  color: #52666b;
  font-size: 14px;
  font-weight: 800;
  line-height: 1.6;
}

.center-dialog .dont-remind-row {
  display: inline-flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  margin-top: 14px;
  color: #20333a;
  font-size: 13px;
  font-weight: 900;
  cursor: pointer;
}

.center-dialog .dont-remind-row input {
  width: 16px;
  height: 16px;
  min-width: 0;
  accent-color: #1f6f62;
}

.aramkit-notice-dialog footer button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.center-dialog footer {
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}

.center-dialog .danger {
  border-color: #b93b3b;
  color: #fff8f8;
  background: #b93b3b;
}

@media (max-width: 920px) {
  .tools-panel {
    padding: 8px;
  }

  .tier-row {
    grid-template-columns: 82px minmax(0, 1fr);
  }

  .tier-label {
    font-size: 18px;
  }

  .tools-header {
    grid-template-columns: 1fr;
  }

  .tool-tabs,
  .ranking-switch,
  .tier-actions {
    grid-column: auto;
  }

  .tier-actions,
  .ranking-switch {
    justify-content: flex-start;
  }

  .tier-actions {
    align-items: stretch;
    flex-wrap: wrap;
  }
}
</style>
