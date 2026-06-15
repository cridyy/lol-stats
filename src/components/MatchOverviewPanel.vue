<script setup lang="ts">
import { computed, inject, ref } from "vue"
import { ClipboardCopy, LoaderCircle } from "lucide-vue-next"
import { copyElementAsPng } from "../imageShare"
import { notifyKey } from "../notifications"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  GameAssetEntry,
  MatchDetailPlayer,
  MatchDetailResponse,
  RecentGame,
} from "../types"
import { fixed, formatDate } from "../utils"
import AssetIcon from "./AssetIcon.vue"
import ChampionAvatar from "./ChampionAvatar.vue"

const props = defineProps<{
  game: RecentGame
  matchDetail: MatchDetailResponse | null
  loading: boolean
  error: string
  champions: Record<number, ChampionSummaryItem>
  gameAssets: GameAssetBundle
}>()

const emit = defineEmits<{
  openPlayer: [player: MatchDetailPlayer]
}>()

const captureRef = ref<HTMLElement | null>(null)
const copying = ref(false)
const notify = inject(notifyKey, () => 0)

const spellMap = computed(() => indexAssets(props.gameAssets.summonerSpells))
const itemMap = computed(() => indexAssets(props.gameAssets.items))
const perkMap = computed(() => indexAssets(props.gameAssets.perks))
const augmentMap = computed(() => indexAssets(props.gameAssets.augments))

function indexAssets(entries: GameAssetEntry[]) {
  return entries.reduce<Record<number, GameAssetEntry>>((acc, entry) => {
    acc[entry.id] = entry
    return acc
  }, {})
}

function queueName(game: RecentGame | MatchDetailResponse) {
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

function ratio(part: number, total: number) {
  return total > 0 ? part / total : 0
}

function shareText(part: number, total: number) {
  return `${Math.round(ratio(part, total) * 100)}%`
}

function shareSuffix(part: number, total: number) {
  return `(${shareText(part, total)})`
}

function kNumber(value: number) {
  return `${(value / 1000).toFixed(1)}k`
}

function damageConversion(game: RecentGame) {
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  if (goldShare === 0) return "0.00"
  return fixed(ratio(game.damageToChampions, game.teamDamageToChampions) / goldShare)
}

function playerLabel(player: MatchDetailPlayer) {
  if (player.gameName && player.tagLine) return `${player.gameName}#${player.tagLine}`
  return player.summonerName || player.puuid || "未知玩家"
}

function augmentName(augmentId: number) {
  return augmentMap.value[augmentId]?.name || perkMap.value[augmentId]?.name || `强化 ${augmentId}`
}

function shortAugmentName(augmentId: number) {
  return Array.from(augmentName(augmentId)).slice(0, 5).join("")
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

function detailStatLeader(
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

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

async function copyImage() {
  const target = captureRef.value
  if (!target || !props.matchDetail || copying.value) return

  copying.value = true
  try {
    await copyElementAsPng(target, {
      backgroundColor: "#f6faf9",
      pixelRatio: 2,
      filter: (node) => !(node instanceof HTMLElement && node.classList.contains("overview-actions")),
    })

    notify({ kind: "success", title: "分享图片已复制", message: "可以直接粘贴到聊天窗口" })
  } catch (error) {
    notify({
      kind: "error",
      title: "分享图片生成失败",
      message: errorMessage(error),
      duration: 7000,
    })
  } finally {
    copying.value = false
  }
}
</script>

<template>
  <section class="overview-panel" ref="captureRef">
    <header class="overview-toolbar">
      <div class="overview-meta">
        <strong>总览</strong>
        <span>模式 {{ queueName(matchDetail || game) }}</span>
        <span>对局时间 {{ Math.floor(game.gameDuration / 60) }}:{{ String(game.gameDuration % 60).padStart(2, "0") }}</span>
        <span>开始时间 {{ formatDate(game.gameCreation) }}</span>
      </div>
      <div class="overview-actions">
        <button :disabled="copying || loading || !matchDetail" @click="copyImage">
          <ClipboardCopy :size="14" />
          {{ copying ? "生成中" : "分享" }}
        </button>
      </div>
    </header>

    <div class="overview-state" v-if="loading">
      <LoaderCircle class="spin" :size="18" />
      正在读取对局详情
    </div>
    <div class="overview-state error" v-else-if="error">{{ error }}</div>

    <div class="overview-body" v-else-if="matchDetail">
      <section v-for="team in matchDetail.teams" :key="team.teamId" class="team-block">
        <div class="team-header" :class="{ win: team.win, lose: !team.win }">
          <div class="team-result">
            <strong>{{ team.name }}</strong>
            <span>{{ team.win ? "胜利" : "失败" }}</span>
          </div>
          <span>技能</span>
          <span>装备</span>
          <span>符文</span>
          <span>K/D/A</span>
          <span>伤害</span>
          <span>经济</span>
          <span>承伤</span>
          <span>治疗</span>
          <span>伤转</span>
        </div>

        <div class="detail-list">
          <article
            v-for="player in team.players"
            :key="`${player.teamId}:${player.participantId}`"
            class="detail-row"
            :class="{ win: player.win, lose: !player.win }"
            :title="playerLabel(player)"
            tabindex="0"
            @click="emit('openPlayer', player)"
            @keydown.enter="emit('openPlayer', player)"
            @keydown.space.prevent="emit('openPlayer', player)"
          >
            <div class="champion-cell">
              <ChampionAvatar :champion-id="player.championId" :champions="champions" :size="42" />
              <span>{{ playerLabel(player) }}</span>
            </div>

            <div class="spell-column">
              <AssetIcon
                v-if="player.spell1Id"
                :path="spellMap[player.spell1Id]?.iconPath"
                :label="spellMap[player.spell1Id]?.name"
                :fallback="String(player.spell1Id)"
                :size="18"
              />
              <AssetIcon
                v-if="player.spell2Id"
                :path="spellMap[player.spell2Id]?.iconPath"
                :label="spellMap[player.spell2Id]?.name"
                :fallback="String(player.spell2Id)"
                :size="18"
              />
            </div>

            <div class="item-grid">
              <AssetIcon
                v-for="itemId in player.itemIds"
                :key="itemId"
                :path="itemMap[itemId]?.iconPath"
                :label="itemMap[itemId]?.name"
                :fallback="String(itemId)"
                :size="33"
              />
            </div>

            <div class="rune-grid text-grid" v-if="player.augmentIds.length">
              <span
                v-for="augmentId in player.augmentIds.slice(0, 4)"
                :key="augmentId"
                :class="['augment-tag', augmentRarityClass(augmentId)]"
              >
                {{ shortAugmentName(augmentId) }}
              </span>
            </div>
            <div class="rune-grid" v-else>
              <AssetIcon
                v-for="perkId in player.perkIds.slice(0, 4)"
                :key="perkId"
                :path="perkMap[perkId]?.iconPath"
                :label="perkMap[perkId]?.name"
                :fallback="String(perkId)"
                :size="20"
              />
            </div>

            <div class="kda-cell">
              <strong>{{ player.kills }}/{{ player.deaths }}/{{ player.assists }}</strong>
            </div>

            <div class="stat-cell">
              <strong :class="{ leader: detailStatLeader(player, 'damage') }">
                {{ kNumber(player.damageToChampions) }}<em>{{ shareSuffix(player.damageToChampions, player.teamDamageToChampions) }}</em>
              </strong>
            </div>

            <div class="stat-cell">
              <strong :class="{ leader: detailStatLeader(player, 'gold') }">
                {{ kNumber(player.goldEarned) }}<em>{{ shareSuffix(player.goldEarned, player.teamGoldEarned) }}</em>
              </strong>
            </div>

            <div class="stat-cell">
              <strong :class="{ leader: detailStatLeader(player, 'mitigation') }">
                {{ kNumber(player.damageSelfMitigated) }}<em>{{ shareSuffix(player.damageSelfMitigated, player.teamDamageSelfMitigated) }}</em>
              </strong>
            </div>

            <div class="stat-cell">
              <strong :class="{ leader: detailStatLeader(player, 'healing') }">
                {{ kNumber(player.totalHeal) }}<em>{{ shareSuffix(player.totalHeal, player.teamTotalHeal) }}</em>
              </strong>
            </div>

            <div class="stat-cell">
              <strong :class="{ leader: detailStatLeader(player, 'conversion') }">
                {{ damageConversion(player) }}
              </strong>
            </div>
          </article>
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.overview-panel {
  display: flex;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #f6faf9;
  padding: 12px;
}

.overview-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid #dce7e4;
  padding-bottom: 10px;
}

.overview-meta {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.overview-meta strong {
  border-radius: 8px;
  color: #ffffff;
  background: #1f5f56;
  font-size: 12px;
  padding: 7px 13px;
}

.overview-meta span {
  color: #718087;
  font-size: 12px;
  font-weight: 700;
}

.overview-actions button {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border-radius: 8px;
  color: #ffffff;
  background: #1f5f56;
  cursor: pointer;
  font-size: 12px;
  font-weight: 800;
  padding: 7px 10px;
}

.overview-state {
  display: inline-flex;
  min-height: 360px;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: #657179;
  font-weight: 800;
}

.overview-state.error {
  color: #a94745;
}

.overview-body,
.detail-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.team-block {
  overflow-x: auto;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #ffffff;
  padding: 8px;
}

.team-header,
.detail-row {
  display: grid;
  grid-template-columns: 160px 22px 252px 160px 59px repeat(5, 62px);
  min-width: 999px;
  align-items: center;
  gap: 4px;
}

.team-header {
  border-radius: 6px;
  margin-bottom: 6px;
  padding: 6px 7px;
}

.team-header.win {
  color: #1f5f9f;
  background: #e7f2ff;
}

.team-header.lose {
  color: #a23d3d;
  background: #ffe9e9;
}

.team-result {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

.team-result strong {
  font-size: 15.6px;
  line-height: 1;
}

.team-result span,
.team-header > span {
  font-size: 12px;
  font-weight: 900;
  line-height: 1;
  white-space: nowrap;
}

.team-header > span {
  text-align: center;
}

.detail-list {
  gap: 4px;
}

.detail-row {
  height: 48px;
  min-height: 48px;
  max-height: 48px;
  overflow: hidden;
  border: 1px solid #dce7e4;
  border-left-width: 5px;
  border-radius: 8px;
  cursor: pointer;
  padding: 2px 7px;
}

.detail-row:hover {
  filter: saturate(1.04) brightness(0.99);
}

.detail-row.win {
  border-color: #c9ddf8;
  border-left-color: #2f78d6;
  background: #eef6ff;
}

.detail-row.lose {
  border-color: #f1cdcd;
  border-left-color: #ca4b4b;
  background: #fff1f1;
}

.champion-cell {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

.champion-cell span {
  min-width: 0;
  overflow: hidden;
  color: #53666c;
  font-size: 13.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.spell-column {
  display: flex;
  height: 42px;
  flex-direction: column;
  justify-content: center;
  gap: 2px;
}

.item-grid {
  display: flex;
  min-width: 0;
  flex-wrap: nowrap;
  gap: 3px;
}

.rune-grid {
  display: grid;
  grid-template-columns: repeat(2, max-content);
  grid-auto-rows: 20px;
  align-content: center;
  justify-content: center;
  gap: 2px 4px;
  overflow: hidden;
}

.text-grid .augment-tag {
  display: inline-flex;
  box-sizing: border-box;
  width: calc(5em + 8px);
  height: 19px;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border: 1px solid rgba(31, 55, 59, 0.08);
  border-radius: 5px;
  color: #34534d;
  background: rgba(255, 255, 255, 0.62);
  font-size: 13.5px;
  font-weight: 700;
  line-height: 1;
  padding: 0 3px;
  text-align: center;
  white-space: nowrap;
}

.text-grid .augment-prismatic {
  border-color: rgba(170, 72, 215, 0.42);
  color: #6d2c91;
  background: linear-gradient(135deg, rgba(249, 226, 255, 0.9), rgba(218, 183, 255, 0.9));
}

.text-grid .augment-gold {
  border-color: rgba(199, 144, 36, 0.48);
  color: #7b4d02;
  background: rgba(255, 230, 161, 0.92);
}

.text-grid .augment-silver {
  border-color: rgba(134, 151, 166, 0.48);
  color: #49606f;
  background: rgba(229, 237, 243, 0.92);
}

.text-grid .augment-bronze {
  border-color: rgba(167, 105, 60, 0.46);
  color: #7a4323;
  background: rgba(236, 201, 174, 0.92);
}

.kda-cell,
.stat-cell {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: center;
}

.kda-cell strong {
  color: #20333a;
  font-size: 13px;
  line-height: 1;
  white-space: nowrap;
}

.stat-cell strong {
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  gap: 1px;
  color: #20333a;
  font-size: 16.5px;
  line-height: 1;
  white-space: nowrap;
}

.stat-cell strong.leader,
.stat-cell strong.leader em {
  color: #d22f2f;
  font-weight: 900;
}

.stat-cell em {
  color: #8a989c;
  font-size: 13.5px;
  font-style: normal;
  font-weight: 700;
}
</style>
