<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue"
import type { GlobalRating, OutputRating } from "../scoring"
import { gamePresentation } from "../gamePresentation"
import type { RatingContext } from "../roleClassifier"
import type { ChampionSummaryItem, GlobalChampionBaseline, RecentGame } from "../types"
import { championName, fixed } from "../utils"
import ChampionAvatar from "./ChampionAvatar.vue"

const props = defineProps<{
  game: RecentGame
  champions: Record<number, ChampionSummaryItem>
  ratingContext: RatingContext
  globalBaselines?: ReadonlyMap<number, GlobalChampionBaseline>
  compact?: boolean
}>()

const triggerRef = ref<HTMLElement | null>(null)
const cardRef = ref<HTMLElement | null>(null)
const open = ref(false)
const cardStyle = ref<Record<string, string>>({})
let closeTimer: number | undefined

const presentation = computed(() => gamePresentation(
  props.game,
  props.ratingContext,
  props.globalBaselines,
))
const rating = computed<OutputRating>(() => presentation.value.rating)
const globalRating = computed<GlobalRating>(() => presentation.value.globalRating)
const titleSet = computed(() => presentation.value.titleSet)
const championLabel = computed(() => championName(props.champions, props.game.championId))
const allTitles = computed(() => titleSet.value.titles)

function clearCloseTimer() {
  if (closeTimer !== undefined) {
    window.clearTimeout(closeTimer)
    closeTimer = undefined
  }
}

function positionCard() {
  const trigger = triggerRef.value
  const card = cardRef.value
  if (!trigger || !card) return

  const triggerRect = trigger.getBoundingClientRect()
  const cardRect = card.getBoundingClientRect()
  const margin = 10
  const viewportPadding = 8
  let left = triggerRect.left + triggerRect.width / 2 - cardRect.width / 2
  let top = triggerRect.top - cardRect.height - margin

  if (top < viewportPadding) top = triggerRect.bottom + margin
  left = Math.max(viewportPadding, Math.min(left, window.innerWidth - cardRect.width - viewportPadding))
  top = Math.max(viewportPadding, Math.min(top, window.innerHeight - cardRect.height - viewportPadding))
  cardStyle.value = { left: `${Math.round(left)}px`, top: `${Math.round(top)}px` }
}

function show() {
  clearCloseTimer()
  open.value = true
  void nextTick(positionCard)
}

function scheduleClose() {
  clearCloseTimer()
  closeTimer = window.setTimeout(() => {
    open.value = false
    closeTimer = undefined
  }, 120)
}

function keepOpen() {
  clearCloseTimer()
}

function reposition() {
  if (open.value) positionCard()
}

watch(open, (value) => {
  if (value) {
    window.addEventListener("resize", reposition)
    window.addEventListener("scroll", reposition, true)
  } else {
    window.removeEventListener("resize", reposition)
    window.removeEventListener("scroll", reposition, true)
  }
})

onBeforeUnmount(() => {
  clearCloseTimer()
  window.removeEventListener("resize", reposition)
  window.removeEventListener("scroll", reposition, true)
})
</script>

<template>
  <div
    ref="triggerRef"
    class="game-title-popover-trigger"
    @mouseenter="show"
    @mouseleave="scheduleClose"
    @focusin="show"
    @focusout="scheduleClose"
  >
    <slot />
  </div>

  <Teleport to="body">
    <section
      v-if="open"
      ref="cardRef"
      :class="['game-title-popover', { compact }]"
      :style="cardStyle"
      role="tooltip"
      @mouseenter="keepOpen"
      @mouseleave="scheduleClose"
    >
      <header class="game-title-popover-header">
        <ChampionAvatar :champion-id="game.championId" :champions="champions" :size="42" />
        <div class="game-title-popover-heading">
          <strong>{{ championLabel }}</strong>
          <span>{{ game.kills }}/{{ game.deaths }}/{{ game.assists }} · KDA {{ fixed(game.kda, 1) }}</span>
        </div>
        <b :class="['game-title-popover-score', `score-${rating.level}`]">{{ rating.score }}分</b>
      </header>

      <div class="game-title-popover-block game-title-popover-main-title">
        <span class="game-title-popover-label">当前称号</span>
        <strong>{{ titleSet.displayTitle }}</strong>
      </div>

      <div class="game-title-popover-block">
        <span class="game-title-popover-label">全部称号</span>
        <div v-if="allTitles.length" class="game-title-popover-chips">
          <span v-for="title in allTitles" :key="title" class="game-title-popover-chip">{{ title }}</span>
        </div>
        <span v-else class="game-title-popover-empty">本局暂无专属称号</span>
      </div>

      <div v-if="titleSet.factTags.length" class="game-title-popover-block">
        <span class="game-title-popover-label">事实标签</span>
        <div class="game-title-popover-chips">
          <span v-for="tag in titleSet.factTags" :key="tag" class="game-title-popover-chip fact">{{ tag }}</span>
        </div>
      </div>

      <div class="game-title-popover-footer">
        <span>{{ rating.role.label }} · {{ rating.label }}</span>
        <span v-if="globalRating.available">全服得分 {{ fixed(globalRating.globalScore, 1) }}/{{ globalRating.globalScoreMax }}分</span>
      </div>
    </section>
  </Teleport>
</template>

<style scoped>
.game-title-popover-trigger {
  display: flex;
  min-width: 0;
  align-self: stretch;
}

.game-title-popover-trigger :deep(.score-cell) {
  width: 100%;
}

.game-title-popover {
  position: fixed;
  z-index: 1200;
  display: flex;
  width: min(330px, calc(100vw - 16px));
  flex-direction: column;
  gap: 10px;
  border: 1px solid rgba(31, 95, 86, 0.28);
  border-radius: 12px;
  color: #263238;
  background: rgba(255, 255, 255, 0.98);
  box-shadow: 0 18px 48px rgba(20, 52, 55, 0.25);
  padding: 12px;
  pointer-events: auto;
}

.game-title-popover.compact {
  width: min(290px, calc(100vw - 16px));
}

.game-title-popover-header {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 9px;
  border-bottom: 1px solid #e3ece9;
  padding-bottom: 10px;
}

.game-title-popover-heading {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 3px;
}

.game-title-popover-heading strong {
  overflow: hidden;
  color: #1f5f56;
  font-size: 14px;
  font-weight: 900;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.game-title-popover-heading span,
.game-title-popover-footer {
  color: #718087;
  font-size: 11px;
  font-weight: 800;
}

.game-title-popover-score {
  flex: 0 0 auto;
  font-size: 18px;
  font-weight: 950;
}

.game-title-popover-score.score-excellent {
  color: #b06c00;
}

.game-title-popover-score.score-good {
  color: #1f765b;
}

.game-title-popover-score.score-average {
  color: #55728c;
}

.game-title-popover-score.score-poor {
  color: #b44242;
}

.game-title-popover-block {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.game-title-popover-label {
  color: #8a9a9e;
  font-size: 10px;
  font-weight: 900;
  letter-spacing: 0.06em;
}

.game-title-popover-main-title strong {
  color: #263238;
  font-size: 16px;
  font-weight: 950;
}

.game-title-popover-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.game-title-popover-empty {
  color: #98a5a8;
  font-size: 11px;
  font-weight: 700;
}

.game-title-popover-chip {
  border: 1px solid #d7e7e2;
  border-radius: 999px;
  color: #35665c;
  background: #f1f8f5;
  font-size: 11px;
  font-weight: 850;
  line-height: 1.2;
  padding: 4px 7px;
}

.game-title-popover-chip.fact {
  border-color: #f0d8a8;
  color: #8a5a16;
  background: #fff8e8;
}

.game-title-popover-footer {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 6px;
  border-top: 1px solid #e3ece9;
  padding-top: 8px;
}
</style>
