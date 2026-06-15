<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from "vue"
import { ClipboardCopy } from "lucide-vue-next"
import { copyElementAsPng } from "../imageShare"
import { notifyKey } from "../notifications"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  OpenMatchPayload,
  PlayerStatsResponse,
  RecentGame,
  ShareSettings,
} from "../types"
import AssetIcon from "./AssetIcon.vue"
import ChampionAvatar from "./ChampionAvatar.vue"
import GameRecordList from "./GameRecordList.vue"
import { championName, fixed, percent } from "../utils"

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

const selectedChampionId = ref<number | null>(null)
const shareBusy = ref<"champions" | "games" | null>(null)
const championStatsCaptureRef = ref<HTMLElement | null>(null)
const championGamesCaptureRef = ref<HTMLElement | null>(null)
const notify = inject(notifyKey, () => 0)
const selectedChampionGames = computed(() => {
  if (!selectedChampionId.value) return []
  return props.stats?.recentGames.filter((game) => game.championId === selectedChampionId.value) || []
})
const selectedChampionLabel = computed(() =>
  championName(props.champions, selectedChampionId.value || undefined),
)
const shareChampionStats = computed(
  () => props.stats?.championStats.slice(0, props.shareSettings.championAnalysisLimit) || [],
)
const shareSelectedChampionGames = computed(() =>
  selectedChampionGames.value.slice(0, props.shareSettings.championGamesAnalysisLimit),
)
const spellMap = computed(() => indexAssets(props.gameAssets.summonerSpells))
const itemMap = computed(() => indexAssets(props.gameAssets.items))
const perkMap = computed(() => indexAssets(props.gameAssets.perks))
const augmentMap = computed(() => indexAssets(props.gameAssets.augments))

function indexAssets(entries: GameAssetBundle["items"]) {
  return entries.reduce<Record<number, GameAssetBundle["items"][number]>>((acc, entry) => {
    acc[entry.id] = entry
    return acc
  }, {})
}

watch(
  () => props.stats,
  () => {
    selectedChampionId.value = null
  },
)

function openChampionGames(championId: number) {
  selectedChampionId.value = championId
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
  const name = augmentMap.value[augmentId]?.name || perkMap.value[augmentId]?.name || `强化${augmentId}`
  return Array.from(name).slice(0, 4).join("")
}

function augmentRarityClass(augmentId: number) {
  switch (augmentMap.value[augmentId]?.rarity || perkMap.value[augmentId]?.rarity) {
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
</script>

<template>
  <section class="empty-panel" v-if="!stats">
    <div class="empty-title">暂无数据</div>
    <div class="empty-subtitle">统计面板空闲</div>
  </section>

  <section class="stats" v-else>
    <div class="metric-grid">
      <div class="metric">
        <span>胜率</span>
        <strong>{{ percent(stats.summary.winRate) }}</strong>
        <small>{{ stats.summary.wins }}胜 {{ stats.summary.losses }}负</small>
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
        <small class="metric-avatar">
          <ChampionAvatar
            :champion-id="stats.summary.mostPlayedChampionId"
            :champions="champions"
            :size="30"
          />
        </small>
      </div>
      <div class="metric">
        <span>总场次</span>
        <strong>{{ stats.summary.games }}</strong>
        <small>已过滤人机/教程队列</small>
      </div>
    </div>

    <div class="content-grid">
      <section class="panel table-panel" v-if="!selectedChampionId">
        <div class="panel-heading">
          <div class="section-title">单英雄战绩</div>
          <button
            class="share-action"
            :disabled="shareBusy !== null || shareChampionStats.length === 0"
            @click="copyChampionStatsImage"
          >
            <ClipboardCopy :size="14" />
            {{ shareBusy === "champions" ? "生成中" : `分享前 ${shareChampionStats.length} 个` }}
          </button>
        </div>
        <div class="table-wrap">
          <table>
            <thead>
              <tr>
                <th>英雄</th>
                <th>场次</th>
                <th>胜率</th>
                <th>K / D / A</th>
                <th>伤害占比</th>
                <th>伤害转化率</th>
                <th>承担占比</th>
                <th>治疗占比</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="champ in stats.championStats"
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
        </div>
      </section>

      <section class="panel champion-games-panel" v-else>
        <div class="detail-header">
          <div class="detail-title-group">
            <button @click="selectedChampionId = null">返回</button>
            <div>
              <div class="section-title">{{ selectedChampionLabel }} · 单英雄战绩</div>
              <span>{{ selectedChampionGames.length }} 局</span>
            </div>
          </div>
          <button
            class="share-action"
            :disabled="shareBusy !== null || shareSelectedChampionGames.length === 0"
            @click="copyChampionGamesImage"
          >
            <ClipboardCopy :size="14" />
            {{ shareBusy === "games" ? "生成中" : `分享前 ${shareSelectedChampionGames.length} 场` }}
          </button>
        </div>

        <GameRecordList
          :games="selectedChampionGames"
          :champions="champions"
          :game-assets="gameAssets"
          :sgp-server-id="sgpServerId"
          :owner-label="ownerLabel"
          :owner-puuid="ownerPuuid"
          external-detail
          @open-match="emit('openMatch', $event)"
        />
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
                <strong>{{ kNumber(game.damageSelfMitigated) }}</strong>
                <em>{{ shareText(game.damageSelfMitigated, game.teamDamageSelfMitigated) }}</em>
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

.metric-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
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

.metric span,
.metric small {
  color: #6c7a80;
  font-size: 13px;
}

.metric strong {
  color: #20333a;
  font-size: 28px;
  line-height: 1.1;
}

.metric-avatar {
  display: flex;
  align-items: center;
  min-height: 32px;
}

.content-grid {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 14px;
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

.panel-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.panel-heading .section-title {
  margin-bottom: 0;
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
  min-height: 420px;
  max-height: 560px;
  overflow: auto;
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

td:first-child {
  width: 54px;
}

.champion-row {
  cursor: pointer;
}

.champion-row:hover td {
  background: #f3f8f6;
}

.champion-row:focus-visible {
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

.share-capture-root {
  position: fixed;
  top: 0;
  left: -12000px;
  z-index: -1;
  width: 1160px;
  pointer-events: none;
}

.share-capture-root.mobile {
  width: 430px;
}

.share-card {
  width: 1160px;
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
  grid-template-columns: 28px minmax(0, 1fr) auto auto;
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
  .metric-grid {
    grid-template-columns: 1fr 1fr;
  }
}
</style>
