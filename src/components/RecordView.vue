<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { LoaderCircle, RefreshCw } from "lucide-vue-next"
import type {
  ChampionStat,
  ChampionSummaryItem,
  GameAssetBundle,
  OpenMatchPayload,
  PlayerStatsResponse,
  RankedQueueEntry,
  RankedStatsResponse,
  RecentGame,
  ShareSettings,
} from "../types"
import { rankIconLarge } from "../rankIcons"
import { mitigationValue, riotId, teamMitigationValue } from "../utils"
import MatchHistoryPanel from "./MatchHistoryPanel.vue"
import StatsPanel from "./StatsPanel.vue"

type RecordTab = "recent" | "stats"
type QueueFilterKey = "ranked" | "normal" | "arena" | "aram"
type QueueOption = {
  key: QueueFilterKey
  label: string
  matches: (game: RecentGame) => boolean
}

const props = defineProps<{
  recentStats: PlayerStatsResponse | null
  fullStats: PlayerStatsResponse | null
  rankedStats?: RankedStatsResponse | null
  rankedLoading?: boolean
  shareSettings: ShareSettings
  champions: Record<number, ChampionSummaryItem>
  gameAssets: GameAssetBundle
  loading: boolean
  recentLoading?: boolean
  recentHasMore?: boolean
  statsDepth: number
  error: string
  sgpServerId?: string
}>()

const emit = defineEmits<{
  loadRecent: []
  loadStats: []
  refreshRecent: []
  refreshStats: []
  loadRecentMore: []
  changeStatsDepth: [depth: number]
  openMatch: [payload: OpenMatchPayload]
}>()

const activeTab = ref<RecordTab>("recent")
const selectedQueue = ref<QueueFilterKey | null>(null)
const statsDepthInput = ref(String(props.statsDepth))
const statsDepthPickerOpen = ref(false)
const statsDateStart = ref("")
const statsDateEnd = ref("")
const statsDatePickerOpen = ref(false)
let statsDepthCloseTimer = 0
let statsDateCloseTimer = 0

const rankedQueueIds = new Set([420, 440])
const normalQueueIds = new Set([400, 430, 480, 490])
const hexAramQueueIds = new Set([2400])
const aramQueueIds = new Set([450])
const apexTiers = new Set(["MASTER", "GRANDMASTER", "CHALLENGER"])
const statsDepthOptions = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000]
const tierLabels: Record<string, string> = {
  IRON: "坚韧黑铁",
  BRONZE: "英勇黄铜",
  SILVER: "不屈白银",
  GOLD: "荣耀黄金",
  PLATINUM: "华贵铂金",
  EMERALD: "流光翡翠",
  DIAMOND: "璀璨钻石",
  MASTER: "超凡大师",
  GRANDMASTER: "傲世宗师",
  CHALLENGER: "最强王者",
}

const queueOptions: QueueOption[] = [
  {
    key: "ranked",
    label: "排位",
    matches: (game) => rankedQueueIds.has(game.queueId),
  },
  {
    key: "normal",
    label: "匹配",
    matches: (game) =>
      normalQueueIds.has(game.queueId) ||
      (game.gameMode === "CLASSIC" && !rankedQueueIds.has(game.queueId)),
  },
  {
    key: "arena",
    label: "海克斯大乱斗",
    matches: (game) => hexAramQueueIds.has(game.queueId),
  },
  {
    key: "aram",
    label: "大乱斗",
    matches: (game) => aramQueueIds.has(game.queueId) || game.gameMode === "ARAM",
  },
]

const currentSummoner = computed(
  () => props.recentStats?.summoner || props.fullStats?.summoner,
)
const ownerLabel = computed(() => (currentSummoner.value ? riotId(currentSummoner.value) : "玩家"))
const ownerPuuid = computed(() => currentSummoner.value?.puuid)

const activeQueueOption = computed(() =>
  queueOptions.find((option) => option.key === selectedQueue.value),
)

const filteredStats = computed(() => {
  if (!props.fullStats || !activeQueueOption.value) return null
  const queueOption = activeQueueOption.value
  return buildFilteredStats(
    props.fullStats,
    (game) => queueOption.matches(game) && dateFilterMatches(game),
  )
})

const statsDateLabel = computed(() => {
  if (!statsDateStart.value && !statsDateEnd.value) return "不限"
  if (statsDateStart.value && statsDateEnd.value) {
    return `${formatDateInputLabel(statsDateStart.value)} - ${formatDateInputLabel(statsDateEnd.value)}`
  }
  if (statsDateStart.value) return `${formatDateInputLabel(statsDateStart.value)} 起`
  return `至 ${formatDateInputLabel(statsDateEnd.value)}`
})

const rankCards = computed(() => {
  const ranked = props.rankedStats
  if (!ranked) return []

  const flex = ranked.queueMap.RANKED_FLEX_SR
  const solo = ranked.queueMap.RANKED_SOLO_5x5
  return [
    { label: "当前灵活组排", value: formatCurrentRank(flex), tier: flex.tier },
    { label: "当前单双排", value: formatCurrentRank(solo), tier: solo.tier },
    { label: "历史灵活组排最高", value: formatHighestRank(flex), tier: bestTier(flex) },
    { label: "历史单双排最高", value: formatHighestRank(solo), tier: bestTier(solo) },
  ]
})

watch(
  activeTab,
  (tab) => {
    if (tab === "recent" && !props.recentStats) emit("loadRecent")
    if (tab === "stats") requestStatsIfNeeded()
  },
  { immediate: true },
)

watch(
  () => props.statsDepth,
  (depth) => {
    statsDepthInput.value = String(depth)
  },
)

function selectQueue(key: QueueFilterKey) {
  selectedQueue.value = key
  activeTab.value = "stats"
  requestStatsIfNeeded()
}

function openStatsTab() {
  const wasStatsTab = activeTab.value === "stats"
  activeTab.value = "stats"
  if (wasStatsTab) requestStatsIfNeeded()
}

function requestStatsIfNeeded() {
  if (!props.fullStats) emit("loadStats")
}

function commitStatsDepth() {
  const nextDepth = clampStatsDepth(statsDepthInput.value)
  statsDepthInput.value = String(nextDepth)
  statsDepthPickerOpen.value = false
  if (nextDepth !== props.statsDepth) {
    emit("changeStatsDepth", nextDepth)
  }
}

function openStatsDepthPicker() {
  window.clearTimeout(statsDepthCloseTimer)
  statsDepthPickerOpen.value = true
}

function scheduleStatsDepthPickerClose() {
  window.clearTimeout(statsDepthCloseTimer)
  statsDepthCloseTimer = window.setTimeout(() => {
    statsDepthPickerOpen.value = false
    commitStatsDepth()
  }, 120)
}

function selectStatsDepth(depth: number) {
  window.clearTimeout(statsDepthCloseTimer)
  statsDepthInput.value = String(depth)
  statsDepthPickerOpen.value = false
  if (depth !== props.statsDepth) {
    emit("changeStatsDepth", depth)
  }
}

function normalizeStatsDepthInput(event: Event) {
  const input = event.target as HTMLInputElement
  statsDepthInput.value = input.value.replace(/\D/g, "").slice(0, 4)
}

function toggleStatsDatePicker() {
  window.clearTimeout(statsDateCloseTimer)
  statsDatePickerOpen.value = !statsDatePickerOpen.value
}

function openStatsDatePicker() {
  window.clearTimeout(statsDateCloseTimer)
  statsDatePickerOpen.value = true
}

function scheduleStatsDatePickerClose() {
  window.clearTimeout(statsDateCloseTimer)
  statsDateCloseTimer = window.setTimeout(() => {
    statsDatePickerOpen.value = false
  }, 120)
}

function closeStatsDatePicker() {
  window.clearTimeout(statsDateCloseTimer)
  statsDatePickerOpen.value = false
}

function clearStatsDateFilter() {
  statsDateStart.value = ""
  statsDateEnd.value = ""
  closeStatsDatePicker()
}

function dateFilterMatches(game: RecentGame) {
  const timestamp = game.gameCreation
  const start = dateStartTimestamp(statsDateStart.value)
  const end = dateEndTimestamp(statsDateEnd.value)
  if (start !== null && timestamp < start) return false
  if (end !== null && timestamp > end) return false
  return true
}

function dateStartTimestamp(value: string) {
  if (!value) return null
  const date = new Date(`${value}T00:00:00`)
  return Number.isNaN(date.getTime()) ? null : date.getTime()
}

function dateEndTimestamp(value: string) {
  if (!value) return null
  const date = new Date(`${value}T23:59:59.999`)
  return Number.isNaN(date.getTime()) ? null : date.getTime()
}

function formatDateInputLabel(value: string) {
  const [, month = "", day = ""] = value.split("-")
  return month && day ? `${Number(month)}月${Number(day)}日` : value
}

function refreshActiveTab() {
  if (activeTab.value === "recent") {
    emit("refreshRecent")
  } else {
    emit("refreshStats")
  }
}

function queueCount(option: QueueOption) {
  if (!props.fullStats) return null
  return props.fullStats.recentGames.filter(option.matches).length
}

function clampStatsDepth(value: unknown) {
  const numberValue = Number(value)
  if (!Number.isFinite(numberValue)) return props.statsDepth
  return Math.min(1000, Math.max(50, Math.round(numberValue)))
}

function formatCurrentRank(entry: RankedQueueEntry) {
  return formatRank(entry.tier, entry.division, entry.leaguePoints)
}

function formatHighestRank(entry: RankedQueueEntry) {
  return formatRank(bestTier(entry), bestDivision(entry))
}

function bestTier(entry: RankedQueueEntry) {
  return (
    entry.highestTier ||
    entry.previousSeasonHighestTier ||
    entry.previousSeasonEndTier ||
    ""
  )
}

function bestDivision(entry: RankedQueueEntry) {
  return (
    entry.highestDivision ||
    entry.previousSeasonHighestDivision ||
    entry.previousSeasonEndDivision ||
    ""
  )
}

function formatRank(tier: string, division = "", leaguePoints?: number) {
  const normalizedTier = tier.trim().toUpperCase()
  if (!normalizedTier || normalizedTier === "NONE" || normalizedTier === "NA") return "未定级"

  const tierLabel = tierLabels[normalizedTier] || normalizedTier
  const divisionText = apexTiers.has(normalizedTier) ? "" : normalizeDivision(division)
  const lpText = typeof leaguePoints === "number" && leaguePoints > 0 ? ` ${leaguePoints}LP` : ""
  return `${tierLabel}${divisionText ? ` ${divisionText}` : ""}${lpText}`
}

function normalizeDivision(division: string) {
  const normalized = division.trim().toUpperCase()
  if (!normalized || normalized === "NA" || normalized === "NONE") return ""
  return normalized
}

function rankTierClass(tier: string) {
  const normalized = tier.trim().toLowerCase()
  return normalized ? `tier-${normalized}` : "tier-unranked"
}

function buildFilteredStats(
  source: PlayerStatsResponse,
  matches: (game: RecentGame) => boolean,
): PlayerStatsResponse {
  const games = source.recentGames.filter(matches)
  const championStats = buildChampionStats(games)
  const wins = games.filter((game) => game.win).length
  const kills = games.reduce((sum, game) => sum + game.kills, 0)
  const deaths = games.reduce((sum, game) => sum + game.deaths, 0)
  const assists = games.reduce((sum, game) => sum + game.assists, 0)

  return {
    ...source,
    depthLoaded: games.length,
    summary: {
      games: games.length,
      wins,
      losses: games.length - wins,
      winRate: ratio(wins, games.length),
      averageKda: calcKda(kills, deaths, assists),
      averageKills: average(kills, games.length),
      averageDeaths: average(deaths, games.length),
      averageAssists: average(assists, games.length),
      uniqueChampions: championStats.length,
      mostPlayedChampionId: championStats[0]?.championId,
    },
    championStats,
    recentGames: games,
  }
}

function buildChampionStats(games: RecentGame[]): ChampionStat[] {
  const byChampion = new Map<number, RecentGame[]>()
  for (const game of games) {
    const list = byChampion.get(game.championId) || []
    list.push(game)
    byChampion.set(game.championId, list)
  }

  return Array.from(byChampion.entries())
    .map(([championId, championGames]) => {
      const wins = championGames.filter((game) => game.win).length
      const kills = championGames.reduce((sum, game) => sum + game.kills, 0)
      const deaths = championGames.reduce((sum, game) => sum + game.deaths, 0)
      const assists = championGames.reduce((sum, game) => sum + game.assists, 0)
      const damage = championGames.reduce((sum, game) => sum + game.damageToChampions, 0)
      const cs = championGames.reduce((sum, game) => sum + game.cs, 0)
      const teamDamage = championGames.reduce((sum, game) => sum + game.teamDamageToChampions, 0)
      const gold = championGames.reduce((sum, game) => sum + game.goldEarned, 0)
      const teamGold = championGames.reduce((sum, game) => sum + game.teamGoldEarned, 0)
      const mitigated = championGames.reduce((sum, game) => sum + mitigationValue(game), 0)
      const teamMitigated = championGames.reduce(
        (sum, game) => sum + teamMitigationValue(game),
        0,
      )
      const healing = championGames.reduce((sum, game) => sum + game.totalHeal, 0)
      const teamHealing = championGames.reduce((sum, game) => sum + game.teamTotalHeal, 0)
      const damageShare = ratio(damage, teamDamage)
      const goldShare = ratio(gold, teamGold)

      return {
        championId,
        games: championGames.length,
        wins,
        losses: championGames.length - wins,
        winRate: ratio(wins, championGames.length),
        pickRate: ratio(championGames.length, games.length),
        averageKda: calcKda(kills, deaths, assists),
        averageKills: average(kills, championGames.length),
        averageDeaths: average(deaths, championGames.length),
        averageAssists: average(assists, championGames.length),
        averageDamageToChampions: average(damage, championGames.length),
        averageCs: average(cs, championGames.length),
        damageShare,
        damageConversionRate: goldShare === 0 ? 0 : round2(damageShare / goldShare),
        mitigationShare: ratio(mitigated, teamMitigated),
        healingShare: ratio(healing, teamHealing),
        goldShare,
        lastPlayedAt: Math.max(...championGames.map((game) => game.gameCreation)),
      }
    })
    .sort((a, b) => b.games - a.games || b.lastPlayedAt - a.lastPlayedAt)
}

function calcKda(kills: number, deaths: number, assists: number) {
  return round2((kills + assists) / Math.max(deaths, 1))
}

function average(total: number, count: number) {
  return count === 0 ? 0 : round2(total / count)
}

function ratio(part: number, total: number) {
  return total === 0 ? 0 : round2(part / total)
}

function round2(value: number) {
  return Math.round(value * 100) / 100
}
</script>

<template>
  <section class="record-view">
    <header class="record-topbar">
      <nav class="record-tabs">
        <button :class="{ active: activeTab === 'recent' }" @click="activeTab = 'recent'">
          当前战绩
        </button>
        <button :class="{ active: activeTab === 'stats' }" @click="openStatsTab">
          数据统计
        </button>
      </nav>

      <div class="topbar-meta">
        <slot name="toolbar" />
        <button class="refresh-tab" :disabled="loading" @click="refreshActiveTab">
          <LoaderCircle v-if="loading" class="spin" :size="15" />
          <RefreshCw v-else :size="15" />
          刷新
        </button>
        <div class="player-chip" v-if="activeTab === 'recent' && currentSummoner">
          {{ riotId(currentSummoner) }}
        </div>
      </div>
    </header>

    <section class="rank-strip" v-if="activeTab === 'recent' && (rankedLoading || rankCards.length)">
      <div class="rank-card loading" v-if="rankedLoading && !rankCards.length">
        <span>段位</span>
        <strong>读取中</strong>
      </div>
      <article
        v-for="card in rankCards"
        :key="card.label"
        :class="['rank-card', rankTierClass(card.tier)]"
      >
        <img class="rank-card-icon" :src="rankIconLarge(card.tier)" :alt="card.value" />
        <div class="rank-card-text">
          <span>{{ card.label }}</span>
          <strong>{{ card.value }}</strong>
        </div>
      </article>
    </section>

    <div class="loading-pill" v-if="loading">正在读取数据</div>

    <MatchHistoryPanel
      v-if="activeTab === 'recent'"
      :stats="recentStats"
      :champions="champions"
      :game-assets="gameAssets"
      :sgp-server-id="sgpServerId"
      :loading-more="recentLoading"
      :has-more="recentHasMore"
      :owner-label="ownerLabel"
      :owner-puuid="ownerPuuid"
      @load-more="emit('loadRecentMore')"
      @open-match="emit('openMatch', $event)"
    />

    <section class="stats-stage" v-else>
      <div class="queue-picker centered" v-if="!selectedQueue">
        <button
          v-for="option in queueOptions"
          :key="option.key"
          @click="selectQueue(option.key)"
        >
          <strong>{{ option.label }}</strong>
          <span v-if="queueCount(option) === null">加载后查看统计</span>
          <span v-else>{{ queueCount(option) }} 局</span>
        </button>
      </div>

      <template v-else>
        <div class="queue-tabs">
          <button
            v-for="option in queueOptions"
            :key="option.key"
            :class="{ active: selectedQueue === option.key }"
            @click="selectQueue(option.key)"
          >
            {{ option.label }}
          </button>
          <div class="queue-tools">
            <div
              class="stats-date-control"
              @focusin="openStatsDatePicker"
              @focusout="scheduleStatsDatePickerClose"
            >
              <button type="button" class="stats-date-trigger" @click="toggleStatsDatePicker">
                <span>日期</span>
                <strong>{{ statsDateLabel }}</strong>
              </button>
              <div class="stats-date-menu" v-if="statsDatePickerOpen">
                <label>
                  <span>起始日期</span>
                  <input v-model="statsDateStart" type="date" :max="statsDateEnd || undefined" />
                </label>
                <label>
                  <span>结束日期</span>
                  <input v-model="statsDateEnd" type="date" :min="statsDateStart || undefined" />
                </label>
                <div class="stats-date-actions">
                  <button type="button" @mousedown.prevent="clearStatsDateFilter">不限</button>
                  <button type="button" @mousedown.prevent="closeStatsDatePicker">完成</button>
                </div>
              </div>
            </div>
            <label
              class="stats-depth-control"
              @focusin="openStatsDepthPicker"
              @focusout="scheduleStatsDepthPickerClose"
            >
              <span>最大场次</span>
              <input
                v-model="statsDepthInput"
                type="text"
                inputmode="numeric"
                pattern="[0-9]*"
                autocomplete="off"
                spellcheck="false"
                :disabled="loading"
                @click="openStatsDepthPicker"
                @input="normalizeStatsDepthInput"
                @keyup.enter="commitStatsDepth"
              />
              <div class="stats-depth-menu" v-if="statsDepthPickerOpen">
                <button
                  v-for="depth in statsDepthOptions"
                  :key="depth"
                  type="button"
                  @mousedown.prevent="selectStatsDepth(depth)"
                >
                  {{ depth }}
                </button>
              </div>
            </label>
          </div>
        </div>

        <StatsPanel
          :stats="filteredStats"
          :champions="champions"
          :game-assets="gameAssets"
          :sgp-server-id="sgpServerId"
          :share-settings="shareSettings"
          :owner-label="ownerLabel"
          :owner-puuid="ownerPuuid"
          @open-match="emit('openMatch', $event)"
        />

        <div class="empty-filter" v-if="filteredStats && filteredStats.depthLoaded === 0">
          <strong>{{ activeQueueOption?.label }}暂无数据</strong>
          <span>当前样本里没有匹配到这个模式的对局</span>
        </div>
      </template>
    </section>
  </section>
</template>

<style scoped>
.record-view {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.record-topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
}

.record-tabs,
.queue-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.queue-tools {
  display: inline-flex;
  flex: 0 0 auto;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  margin-left: auto;
}

.record-tabs button,
.queue-tabs button,
.queue-picker button {
  border: 0;
  border-radius: 8px;
  cursor: pointer;
}

.record-tabs button,
.queue-tabs button {
  color: #53656b;
  background: #edf4f2;
  padding: 10px 14px;
}

.record-tabs button.active,
.queue-tabs button.active {
  color: #ffffff;
  background: #1f5f56;
}

.stats-depth-control {
  position: relative;
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 7px;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  color: #53656b;
  background: #ffffff;
  padding: 6px 8px;
}

.stats-date-control {
  position: relative;
  display: inline-flex;
  flex: 0 0 auto;
}

.stats-date-trigger {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  color: #53656b;
  background: #ffffff;
  cursor: pointer;
  padding: 6px 9px;
}

.stats-date-trigger span,
.stats-depth-control span {
  color: #63747a;
  font-size: 12px;
  font-weight: 800;
  white-space: nowrap;
}

.stats-date-trigger strong {
  max-width: 150px;
  overflow: hidden;
  color: #1f3f39;
  font-size: 13px;
  font-weight: 950;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stats-date-menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 22;
  display: grid;
  min-width: 220px;
  gap: 8px;
  border: 1px solid #d6e5e1;
  border-radius: 10px;
  background: #ffffff;
  box-shadow: 0 14px 30px rgba(32, 67, 73, 0.16);
  padding: 10px;
}

.stats-date-menu label {
  display: grid;
  gap: 5px;
}

.stats-date-menu label span {
  color: #63747a;
  font-size: 12px;
  font-weight: 900;
}

.stats-date-menu input {
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  color: #1f3f39;
  background: #f8fbfa;
  font-size: 13px;
  font-weight: 850;
  padding: 7px 8px;
}

.stats-date-actions {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 6px;
}

.stats-date-actions button {
  border-radius: 7px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  font-size: 12px;
  font-weight: 900;
  padding: 8px 0;
}

.stats-date-actions button:hover {
  color: #ffffff;
  background: #1f5f56;
}

.stats-depth-control input {
  width: 74px;
  border: 0;
  outline: 0;
  color: #1f3f39;
  background: transparent;
  font-size: 13px;
  font-weight: 900;
}

.stats-depth-menu {
  position: absolute;
  top: calc(100% + 6px);
  right: 0;
  z-index: 20;
  display: grid;
  width: 100%;
  min-width: 112px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 14px 30px rgba(32, 67, 73, 0.16);
  padding: 6px;
}

.stats-depth-menu button {
  border-radius: 6px;
  color: #315f58;
  background: #edf5f3;
  cursor: pointer;
  font-size: 12px;
  font-weight: 900;
  padding: 7px 0;
}

.stats-depth-menu button:hover {
  color: #ffffff;
  background: #1f5f56;
}

.topbar-meta {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
}

.player-chip {
  max-width: 280px;
  overflow: hidden;
  border: 1px solid #d6e5e1;
  border-radius: 999px;
  color: #304f4a;
  background: #ffffff;
  font-size: 13px;
  font-weight: 700;
  padding: 7px 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.refresh-tab {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  gap: 5px;
  border: 1px solid #d6e5e1;
  border-radius: 8px;
  color: #315f58;
  background: #ffffff;
  cursor: pointer;
  font-size: 13px;
  font-weight: 800;
  padding: 7px 10px;
}

.refresh-tab:hover:not(:disabled) {
  color: #ffffff;
  background: #1f5f56;
}

.rank-strip {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
}

.rank-card {
  --rank-accent: #7d8b91;
  display: flex;
  min-width: 0;
  min-height: 54px;
  align-items: center;
  gap: 9px;
  border: 1px solid #dce7e4;
  border-left: 4px solid var(--rank-accent);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.86);
  box-shadow: 0 10px 24px rgba(32, 67, 73, 0.06);
  padding: 8px 10px;
}

.rank-card-icon {
  width: 42px;
  height: 42px;
  flex: 0 0 auto;
  object-fit: contain;
  filter: drop-shadow(0 4px 8px rgba(32, 67, 73, 0.16));
}

.rank-card-text {
  display: flex;
  min-width: 0;
  flex-direction: column;
  justify-content: center;
  gap: 5px;
}

.rank-card span {
  overflow: hidden;
  color: #6d7b80;
  font-size: 11px;
  font-weight: 800;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rank-card strong {
  overflow: hidden;
  color: #1f2a2e;
  font-size: 14px;
  line-height: 1.15;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rank-card.loading strong {
  color: #657179;
}

.rank-card.tier-iron {
  --rank-accent: #68727a;
}

.rank-card.tier-bronze {
  --rank-accent: #a56638;
}

.rank-card.tier-silver {
  --rank-accent: #8c9faa;
}

.rank-card.tier-gold {
  --rank-accent: #d0a02f;
}

.rank-card.tier-platinum {
  --rank-accent: #39a18c;
}

.rank-card.tier-emerald {
  --rank-accent: #28a85b;
}

.rank-card.tier-diamond {
  --rank-accent: #4d8de8;
}

.rank-card.tier-master {
  --rank-accent: #9a66d6;
}

.rank-card.tier-grandmaster {
  --rank-accent: #d24d55;
}

.rank-card.tier-challenger {
  --rank-accent: #d89f34;
}

.loading-pill {
  align-self: flex-start;
  border: 1px solid #d9e6e2;
  border-radius: 999px;
  color: #2f635c;
  background: #ffffff;
  padding: 8px 12px;
}

.stats-stage {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.queue-picker.centered {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 180px));
  justify-content: center;
  gap: 12px;
  min-height: 420px;
  align-content: center;
}

.queue-picker button {
  display: flex;
  flex-direction: column;
  gap: 8px;
  border: 1px solid #dce7e4;
  color: #263238;
  background: #ffffff;
  box-shadow: 0 12px 28px rgba(32, 67, 73, 0.07);
  padding: 20px;
  text-align: left;
}

.queue-picker strong {
  font-size: 18px;
}

.queue-picker span {
  color: #728087;
  font-size: 12px;
}

.empty-filter {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  border: 1px dashed #cbdcd8;
  border-radius: 8px;
  color: #657179;
  padding: 24px;
}

.empty-filter strong {
  color: #263238;
}

@media (max-width: 1100px) {
  .rank-strip {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .queue-picker.centered {
    grid-template-columns: repeat(2, minmax(0, 180px));
  }
}
</style>
