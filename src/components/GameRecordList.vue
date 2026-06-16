<script setup lang="ts">
import { computed, inject, ref } from "vue"
import { ClipboardCopy } from "lucide-vue-next"
import { loadMatchDetail } from "../api"
import { copyElementAsPng } from "../imageShare"
import { notifyKey } from "../notifications"
import { calculateOutputRating, outputRatingTitle } from "../scoring"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  GameAssetEntry,
  MatchDetailPlayer,
  MatchDetailResponse,
  OpenMatchPayload,
  RecentGame,
} from "../types"
import { fixed, formatDate } from "../utils"
import AssetIcon from "./AssetIcon.vue"
import ChampionAvatar from "./ChampionAvatar.vue"

const props = defineProps<{
  games: RecentGame[]
  champions: Record<number, ChampionSummaryItem>
  gameAssets: GameAssetBundle
  sgpServerId?: string
  disableDetail?: boolean
  externalDetail?: boolean
  ownerLabel?: string
  ownerPuuid?: string
}>()

const emit = defineEmits<{
  openMatch: [payload: OpenMatchPayload]
}>()

const detailOpen = ref(false)
const detailLoading = ref(false)
const detailError = ref("")
const detailImageCopying = ref(false)
const selectedGame = ref<RecentGame | null>(null)
const matchDetail = ref<MatchDetailResponse | null>(null)
const matchDetailCaptureRef = ref<HTMLElement | null>(null)
const notify = inject(notifyKey, () => 0)

const spellMap = computed(() => indexAssets(props.gameAssets.summonerSpells))
const itemMap = computed(() => indexAssets(props.gameAssets.items))
const perkMap = computed(() => indexAssets(props.gameAssets.perks))
const augmentMap = computed(() => indexAssets(props.gameAssets.augments))
const ratingContext = computed(() => ({
  items: itemMap.value,
  champions: props.champions,
}))

function indexAssets(entries: GameAssetEntry[]) {
  return entries.reduce<Record<number, GameAssetEntry>>((acc, entry) => {
    acc[entry.id] = entry
    return acc
  }, {})
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

function ratio(part: number, total: number) {
  return total > 0 ? part / total : 0
}

function shareText(part: number, total: number) {
  return `${Math.round(ratio(part, total) * 100)}%`
}

function kNumber(value: number) {
  return `${(value / 1000).toFixed(1)}k`
}

function damageConversion(game: RecentGame) {
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  if (goldShare === 0) return "0.00"
  return fixed(ratio(game.damageToChampions, game.teamDamageToChampions) / goldShare)
}

function shareSuffix(part: number, total: number) {
  return `(${shareText(part, total)})`
}

function augmentName(augmentId: number) {
  return augmentAsset(augmentId)?.name || `强化 ${augmentId}`
}

function shortAugmentName(augmentId: number) {
  return Array.from(augmentName(augmentId)).slice(0, 5).join("")
}

function augmentAsset(augmentId: number) {
  return augmentMap.value[augmentId] || perkMap.value[augmentId]
}

function augmentRarityLabel(augmentId: number) {
  switch (augmentAsset(augmentId)?.rarity) {
    case "kPrismatic":
      return "棱彩"
    case "kGold":
      return "黄金"
    case "kSilver":
      return "白银"
    case "kBronze":
      return "青铜"
    default:
      return ""
  }
}

function augmentRarityClass(augmentId: number) {
  switch (augmentAsset(augmentId)?.rarity) {
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

function augmentTitle(augmentId: number) {
  const rarity = augmentRarityLabel(augmentId)
  return rarity ? `${augmentName(augmentId)} · ${rarity}` : augmentName(augmentId)
}

function hasAccolade(game: RecentGame) {
  return (
    game.teamDamageLeader ||
    game.gameDamageLeader ||
    game.teamMitigationLeader ||
    game.teamHealingLeader ||
    game.teamDamageConversionLeader ||
    game.teamGoldLeader
  )
}

function accoladeTags(game: RecentGame) {
  const tags: Array<{ text: string; className: string }> = []

  if (game.gameDamageLeader) {
    tags.push({ text: "伤害全场第一", className: "damage-global" })
  } else if (game.teamDamageLeader) {
    tags.push({ text: "伤害第一", className: "damage-team" })
  }

  if (game.teamMitigationLeader) {
    tags.push({ text: "承伤第一", className: "mitigation" })
  }

  if (game.teamHealingLeader) {
    tags.push({ text: "治疗第一", className: "healing" })
  }

  if (game.teamDamageConversionLeader) {
    tags.push({ text: "伤转第一", className: "conversion" })
  }

  if (game.teamGoldLeader) {
    tags.push({ text: "经济第一", className: "gold" })
  }

  return tags
}

function statTitle(game: RecentGame) {
  return `${queueName(game)} · ${formatDate(game.gameCreation)}`
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

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function playerLabel(player: MatchDetailPlayer) {
  if (player.gameName && player.tagLine) return `${player.gameName}#${player.tagLine}`
  return player.summonerName || player.puuid || "未知玩家"
}

function detailStatLeader(game: RecentGame, kind: "damage" | "gold" | "mitigation" | "healing" | "conversion") {
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

function outputRating(game: RecentGame) {
  return calculateOutputRating(game, ratingContext.value)
}

function outputRatingHint(game: RecentGame) {
  return outputRatingTitle(game, ratingContext.value)
}

async function copyMatchAnalysisImage() {
  const target = matchDetailCaptureRef.value
  if (!target || !matchDetail.value || detailImageCopying.value) return

  detailImageCopying.value = true
  try {
    await copyElementAsPng(target, {
      backgroundColor: "#f6faf9",
      pixelRatio: 2,
      filter: (node) => {
        return !(node instanceof HTMLElement && node.classList.contains("match-detail-actions"))
      },
    })

    notify({
      kind: "success",
      title: "分享图片已复制",
      message: "可以直接粘贴到聊天窗口",
    })
  } catch (error) {
    notify({
      kind: "error",
      title: "分享图片生成失败",
      message: errorMessage(error),
      duration: 7000,
    })
  } finally {
    detailImageCopying.value = false
  }
}

function closeMatchDetail() {
  detailOpen.value = false
  detailLoading.value = false
  detailError.value = ""
  selectedGame.value = null
  matchDetail.value = null
}

async function openMatchDetail(game: RecentGame) {
  if (props.disableDetail) return
  if (props.externalDetail) {
    emit("openMatch", {
      game,
      ownerLabel: props.ownerLabel || "玩家",
      ownerPuuid: props.ownerPuuid,
      sgpServerId: props.sgpServerId,
    })
    return
  }

  selectedGame.value = game
  detailOpen.value = true
  detailLoading.value = true
  detailError.value = ""
  matchDetail.value = null

  try {
    matchDetail.value = await loadMatchDetail(game.gameId, props.sgpServerId)
  } catch (error) {
    detailError.value = errorMessage(error)
  } finally {
    detailLoading.value = false
  }
}
</script>

<template>
  <section class="empty-records" v-if="games.length === 0">
    <strong>暂无战绩</strong>
    <span>当前筛选下没有可展示的对局</span>
  </section>

  <div class="record-list" v-else>
    <article
      v-for="game in games"
      :key="game.gameId"
      class="record-row interactive"
      :class="{ win: game.win, lose: !game.win }"
      :title="statTitle(game)"
      tabindex="0"
      @click="openMatchDetail(game)"
      @keydown.enter="openMatchDetail(game)"
      @keydown.space.prevent="openMatchDetail(game)"
    >
      <div class="time-cell">
        <strong>{{ monthDayText(game.gameCreation) }}</strong>
        <span>{{ hourMinuteText(game.gameCreation) }}</span>
        <em>{{ durationText(game.gameDuration) }}</em>
      </div>

      <div class="champion-cell">
        <ChampionAvatar :champion-id="game.championId" :champions="champions" :size="44" />
        <span class="queue-name">{{ queueName(game) }}</span>
      </div>

      <div class="spell-column">
        <AssetIcon
          v-if="game.spell1Id"
          :path="spellMap[game.spell1Id]?.iconPath"
          :label="spellMap[game.spell1Id]?.name"
          :fallback="String(game.spell1Id)"
          :size="22"
        />
        <AssetIcon
          v-if="game.spell2Id"
          :path="spellMap[game.spell2Id]?.iconPath"
          :label="spellMap[game.spell2Id]?.name"
          :fallback="String(game.spell2Id)"
          :size="22"
        />
      </div>

      <div class="build-cell">
        <div class="item-grid">
          <AssetIcon
            v-for="itemId in game.itemIds"
            :key="itemId"
            :path="itemMap[itemId]?.iconPath"
            :label="itemMap[itemId]?.name"
            :fallback="String(itemId)"
            :size="24"
          />
        </div>
      </div>

      <div class="rune-grid text-grid" v-if="game.augmentIds.length">
        <span
          v-for="augmentId in game.augmentIds"
          :key="augmentId"
          :class="['augment-tag', augmentRarityClass(augmentId)]"
          :title="augmentTitle(augmentId)"
        >
          {{ augmentName(augmentId) }}
        </span>
      </div>
      <div class="rune-grid" v-else>
        <AssetIcon
          v-for="perkId in game.perkIds"
          :key="perkId"
          :path="perkMap[perkId]?.iconPath"
          :label="perkMap[perkId]?.name"
          :fallback="String(perkId)"
          :size="22"
        />
      </div>

      <div class="kda-cell">
        <strong>{{ game.kills }}/{{ game.deaths }}/{{ game.assists }}</strong>
        <span>KDA {{ fixed(game.kda) }}</span>
      </div>

      <div class="stat-cell">
        <strong :class="{ leader: detailStatLeader(game, 'damage') }">{{ kNumber(game.damageToChampions) }}</strong>
        <span class="stat-share" :class="{ leader: detailStatLeader(game, 'damage') }">伤害 <b>{{ shareText(game.damageToChampions, game.teamDamageToChampions) }}</b></span>
      </div>

      <div class="stat-cell">
        <strong :class="{ leader: detailStatLeader(game, 'gold') }">{{ kNumber(game.goldEarned) }}</strong>
        <span class="stat-share" :class="{ leader: detailStatLeader(game, 'gold') }">经济 <b>{{ shareText(game.goldEarned, game.teamGoldEarned) }}</b></span>
      </div>

      <div class="stat-cell">
        <strong :class="{ leader: detailStatLeader(game, 'mitigation') }">{{ kNumber(game.damageSelfMitigated) }}</strong>
        <span class="stat-share" :class="{ leader: detailStatLeader(game, 'mitigation') }">承伤 <b>{{ shareText(game.damageSelfMitigated, game.teamDamageSelfMitigated) }}</b></span>
      </div>

      <div class="stat-cell">
        <strong :class="{ leader: detailStatLeader(game, 'healing') }">{{ kNumber(game.totalHeal) }}</strong>
        <span class="stat-share" :class="{ leader: detailStatLeader(game, 'healing') }">治疗 <b>{{ shareText(game.totalHeal, game.teamTotalHeal) }}</b></span>
      </div>

      <div class="stat-cell">
        <strong :class="{ leader: detailStatLeader(game, 'conversion') }">{{ damageConversion(game) }}</strong>
        <span>伤害转化率</span>
      </div>

      <div
        :class="['score-cell', `score-${outputRating(game).level}`]"
        :title="outputRatingHint(game)"
      >
        <strong>{{ outputRating(game).score }}分</strong>
        <span>{{ outputRating(game).role.label }} · {{ outputRating(game).label }}</span>
      </div>

      <div class="accolade-tags" v-if="hasAccolade(game)">
        <span
          v-for="tag in accoladeTags(game)"
          :key="tag.text"
          :class="['accolade-tag', tag.className]"
        >
          {{ tag.text }}
        </span>
      </div>
    </article>
  </div>

  <Teleport to="body">
    <div class="match-detail-overlay" v-if="detailOpen" @click.self="closeMatchDetail">
      <section class="match-detail-modal" ref="matchDetailCaptureRef">
        <header class="match-detail-toolbar">
          <div class="match-detail-toolbar-main">
            <button class="match-detail-tab active">总览</button>
            <template v-if="selectedGame">
              <span>模式 {{ queueName(selectedGame) }}</span>
              <span>对局时间 {{ durationText(selectedGame.gameDuration) }}</span>
              <span>开始时间 {{ formatDate(selectedGame.gameCreation) }}</span>
            </template>
          </div>
          <div class="match-detail-actions">
            <button
              class="match-detail-analyze"
              :disabled="detailImageCopying || detailLoading || !matchDetail"
              @click="copyMatchAnalysisImage"
            >
              <ClipboardCopy :size="14" />
              {{ detailImageCopying ? "生成中" : "分享" }}
            </button>
            <button class="match-detail-close" @click="closeMatchDetail">关闭</button>
          </div>
        </header>

        <div class="match-detail-state" v-if="detailLoading">正在读取对局详情</div>
        <div class="match-detail-state error" v-else-if="detailError">{{ detailError }}</div>

        <div class="match-detail-body" v-else-if="matchDetail">
          <section
            v-for="team in matchDetail.teams"
            :key="team.teamId"
            class="match-detail-team"
          >
            <div class="match-detail-team-header" :class="{ win: team.win, lose: !team.win }">
              <div class="match-detail-team-result">
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
              <span>评分</span>
            </div>

            <div class="record-list detail-record-list">
              <article
                v-for="player in team.players"
                :key="`${player.teamId}:${player.participantId}`"
                class="record-row detail-row"
                :class="{ win: player.win, lose: !player.win }"
                :title="playerLabel(player)"
              >
                <div class="champion-cell">
                  <ChampionAvatar :champion-id="player.championId" :champions="champions" :size="42" />
                  <span class="queue-name">{{ playerLabel(player) }}</span>
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

                <div class="build-cell">
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
                </div>

                <div class="rune-grid text-grid" v-if="player.augmentIds.length">
                  <span
                    v-for="augmentId in player.augmentIds.slice(0, 4)"
                    :key="augmentId"
                    :class="['augment-tag', augmentRarityClass(augmentId)]"
                    :title="augmentTitle(augmentId)"
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

                <div
                  :class="['score-cell detail-score-cell', `score-${outputRating(player).level}`]"
                  :title="outputRatingHint(player)"
                >
                  <strong>{{ outputRating(player).score }}分</strong>
                  <span>{{ outputRating(player).role.label }} · {{ outputRating(player).label }}</span>
                </div>
              </article>
            </div>
          </section>
        </div>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.empty-records {
  display: grid;
  min-height: 220px;
  place-content: center;
  gap: 8px;
  border: 1px dashed #cbdcd8;
  border-radius: 8px;
  color: #718087;
  text-align: center;
}

.empty-records strong {
  color: #263238;
}

.record-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-x: auto;
  padding-bottom: 2px;
}

.record-row {
  display: grid;
  grid-template-columns: 58px 64px 26px 112px 176px 86px repeat(5, 68px) 138px;
  min-width: 1097px;
  align-items: center;
  gap: 7px;
  border: 1px solid #dce7e4;
  border-left-width: 5px;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 10px 24px rgba(32, 67, 73, 0.06);
  padding: 8px;
}

.record-row.interactive {
  cursor: pointer;
}

.record-row.interactive:hover {
  filter: saturate(1.04) brightness(0.99);
}

.record-row.interactive:focus-visible {
  outline: 2px solid #2f78d6;
  outline-offset: -2px;
}

.record-row.win {
  border-color: #c9ddf8;
  border-left-color: #2f78d6;
  background: #eef6ff;
}

.record-row.lose {
  border-color: #f1cdcd;
  border-left-color: #ca4b4b;
  background: #fff1f1;
}

.time-cell,
.champion-cell,
.kda-cell,
.stat-cell,
.score-cell {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}

.time-cell {
  align-items: center;
  text-align: center;
}

.time-cell strong {
  overflow: hidden;
  max-width: 100%;
  color: #3f555b;
  font-size: 13px;
  font-weight: 900;
  line-height: 1.1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.time-cell span,
.time-cell em {
  color: #718087;
  font-size: 10px;
  font-style: normal;
  font-weight: 800;
  line-height: 1.1;
}

.time-cell em {
  color: #53666c;
  font-size: 13px;
  font-weight: 900;
}

.champion-cell {
  position: relative;
  align-items: center;
  gap: 4px;
  text-align: center;
}

.queue-name,
.kda-cell span,
.stat-cell span {
  overflow: hidden;
  color: #718087;
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.record-row.win .queue-name {
  color: #2f78d6;
}

.record-row.lose .queue-name {
  color: #ca4b4b;
}

.accolade-tags {
  display: flex;
  grid-column: 1 / -1;
  width: 100%;
  min-height: 20px;
  flex-wrap: nowrap;
  justify-content: flex-start;
  gap: 3px;
  padding-left: 2px;
}

.accolade-tag {
  border-radius: 4px;
  color: #ffffff;
  font-size: 10px;
  font-weight: 800;
  line-height: 1;
  padding: 4px 4px;
  white-space: nowrap;
}

.accolade-tag.damage-global {
  background: linear-gradient(135deg, #d9372d, #ff8a2a);
}

.accolade-tag.damage-team {
  background: #e57928;
}

.accolade-tag.mitigation {
  background: #25845f;
}

.accolade-tag.healing {
  background: #2fa772;
}

.accolade-tag.conversion {
  background: #c93d4d;
}

.accolade-tag.gold {
  color: #422900;
  background: #f1b739;
}

.spell-column {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.build-cell {
  display: flex;
  min-width: 0;
  flex-direction: column;
}

.item-grid,
.rune-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-content: center;
}

.item-grid {
  display: grid;
  grid-template-columns: repeat(4, 24px);
  grid-auto-rows: 24px;
  align-content: center;
  justify-content: start;
}

.rune-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, max-content));
}

.text-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.text-grid .augment-tag {
  overflow: hidden;
  border: 1px solid rgba(31, 55, 59, 0.08);
  border-radius: 5px;
  color: #34534d;
  background: rgba(255, 255, 255, 0.62);
  font-size: 11px;
  font-weight: 700;
  line-height: 1.25;
  padding: 4px 6px;
  text-overflow: ellipsis;
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

.kda-cell strong,
.stat-cell strong,
.score-cell strong {
  color: #20333a;
  font-size: 15.6px;
  line-height: 1;
  white-space: nowrap;
}

.stat-cell strong {
  font-size: 16.8px;
}

.score-cell {
  align-items: center;
  justify-content: center;
  gap: 3px;
  border-radius: 7px;
  background: rgba(255, 255, 255, 0.58);
  padding: 6px 4px;
  text-align: center;
}

.score-cell strong {
  position: relative;
  z-index: 1;
  font-size: 20px;
  font-weight: 950;
}

.score-cell span {
  position: relative;
  z-index: 1;
  max-width: 100%;
  color: inherit;
  font-size: 11.5px;
  font-weight: 900;
  line-height: 1;
  white-space: nowrap;
}

.score-excellent {
  position: relative;
  overflow: hidden;
  color: #5d3300;
  border: 1px solid rgba(245, 185, 52, 0.72);
  background:
    linear-gradient(135deg, rgba(255, 244, 184, 0.96), rgba(255, 195, 64, 0.9) 45%, rgba(255, 236, 150, 0.96)),
    #ffd36a;
  box-shadow:
    inset 0 0 0 1px rgba(255, 255, 255, 0.36),
    0 0 16px rgba(255, 191, 58, 0.34);
}

.score-excellent::after {
  position: absolute;
  inset: -60% auto -60% -80%;
  width: 58%;
  background: linear-gradient(
    90deg,
    transparent,
    rgba(255, 255, 255, 0.18),
    rgba(255, 255, 255, 0.74),
    rgba(255, 255, 255, 0.18),
    transparent
  );
  content: "";
  transform: rotate(18deg);
  animation: score-shine 2.8s ease-in-out infinite;
}

@keyframes score-shine {
  0% {
    left: -90%;
  }

  52%,
  100% {
    left: 132%;
  }
}

.score-good {
  color: #145b3e;
  background: rgba(204, 239, 218, 0.88);
}

.score-average {
  color: #174d83;
  background: rgba(205, 229, 255, 0.92);
}

.score-poor {
  color: #8f3434;
  background: rgba(248, 214, 213, 0.92);
}

.score-cell.score-excellent strong,
.score-cell.score-excellent span,
.score-cell.score-good strong,
.score-cell.score-good span,
.score-cell.score-average strong,
.score-cell.score-average span,
.score-cell.score-poor strong,
.score-cell.score-poor span {
  color: inherit;
}

.kda-cell span,
.stat-cell span {
  font-size: 13.2px;
  line-height: 1;
}

.stat-share b {
  color: #25845f;
  font-weight: 800;
}

.stat-cell strong.leader,
.stat-share.leader,
.stat-share.leader b {
  color: #d22f2f;
  font-weight: 900;
}

.match-detail-overlay {
  position: fixed;
  inset: 0;
  z-index: 1200;
  display: grid;
  place-items: center;
  background: rgba(20, 35, 38, 0.36);
  backdrop-filter: blur(8px);
  padding: 18px;
}

.match-detail-modal {
  display: flex;
  width: min(1360px, calc(100vw - 36px));
  max-height: calc(100vh - 36px);
  min-height: 500px;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid rgba(214, 229, 225, 0.95);
  border-radius: 8px;
  background: #f6faf9;
  box-shadow: 0 24px 80px rgba(18, 42, 46, 0.28);
}

.match-detail-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid #dce7e4;
  background: #ffffff;
  padding: 9px 12px;
}

.match-detail-toolbar-main {
  display: flex;
  min-width: 0;
  flex-wrap: nowrap;
  align-items: center;
  gap: 10px;
}

.match-detail-toolbar-main span {
  color: #718087;
  font-size: 12px;
  font-weight: 700;
  white-space: nowrap;
}

.match-detail-actions {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 8px;
}

.match-detail-close,
.match-detail-analyze,
.match-detail-tab {
  border-radius: 8px;
  cursor: pointer;
}

.match-detail-close,
.match-detail-analyze {
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 800;
  padding: 7px 10px;
}

.match-detail-close {
  color: #315f58;
  background: #edf5f3;
}

.match-detail-analyze {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  color: #ffffff;
  background: #1f5f56;
}

.match-detail-analyze:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.match-detail-tab {
  color: #ffffff;
  background: #1f5f56;
  font-weight: 700;
  padding: 7px 13px;
  font-size: 12px;
}

.match-detail-state {
  display: grid;
  flex: 1;
  place-items: center;
  color: #657179;
  font-weight: 700;
}

.match-detail-state.error {
  color: #a94745;
}

.match-detail-body {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
  padding: 10px 12px 12px;
}

.match-detail-team {
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #ffffff;
  padding: 8px;
}

.match-detail-team-header {
  display: grid;
  grid-template-columns: 160px 22px 252px 160px 74px repeat(5, 78px) 128px;
  min-width: 1226px;
  align-items: center;
  gap: 4px;
  border-radius: 6px;
  margin-bottom: 6px;
  padding: 6px 7px;
}

.match-detail-team-header.win {
  color: #1f5f9f;
  background: #e7f2ff;
}

.match-detail-team-header.lose {
  color: #a23d3d;
  background: #ffe9e9;
}

.match-detail-team-result {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
}

.match-detail-team-result strong {
  font-size: 15.6px;
  line-height: 1;
  white-space: nowrap;
}

.match-detail-team-result span {
  font-size: 13.2px;
  font-weight: 800;
  line-height: 1;
  white-space: nowrap;
}

.match-detail-team-header > span {
  font-size: 12px;
  font-weight: 900;
  line-height: 1;
  text-align: center;
  white-space: nowrap;
}

.detail-record-list {
  gap: 4px;
}

.detail-row {
  box-sizing: border-box;
  grid-template-columns: 160px 22px 252px 160px 74px repeat(5, 78px) 128px;
  min-width: 1226px;
  height: 48px;
  min-height: 48px;
  max-height: 48px;
  gap: 4px;
  overflow: hidden;
  padding: 2px 7px;
  box-shadow: none;
}

.detail-row .champion-cell {
  flex-direction: row;
  justify-content: flex-start;
  gap: 6px;
  text-align: left;
}

.detail-row .queue-name,
.detail-row .kda-cell span,
.detail-row .stat-cell span {
  font-size: 13.5px;
  line-height: 1;
}

.detail-row .queue-name {
  flex: 1 1 auto;
  min-width: 0;
}

.detail-row .kda-cell strong {
  font-size: 13px;
  line-height: 1;
  white-space: nowrap;
}

.detail-row .stat-cell strong {
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
  color: #20333a;
  font-size: 16.5px;
  line-height: 1;
  white-space: nowrap;
}

.detail-row .stat-cell strong.leader {
  color: #d22f2f;
  font-weight: 900;
}

.detail-row .stat-cell strong em {
  display: block;
  color: #8a989c;
  font-size: 13.5px;
  font-style: normal;
  font-weight: 700;
}

.detail-row .stat-cell strong.leader em {
  color: inherit;
  font-weight: 900;
}

.detail-score-cell {
  height: 42px;
  gap: 2px;
  padding: 3px;
}

.detail-score-cell strong {
  font-size: 18px;
  line-height: 1;
}

.detail-score-cell span {
  font-size: 10.5px;
  line-height: 1;
}

.detail-row .spell-column {
  height: 42px;
  justify-content: center;
  gap: 2px;
}

.detail-row .item-grid {
  display: flex;
  flex-wrap: nowrap;
  gap: 3px;
}

.detail-row .rune-grid {
  display: grid;
  grid-template-columns: repeat(2, max-content);
  grid-auto-rows: 20px;
  align-content: center;
  justify-content: center;
  gap: 2px 4px;
  overflow: hidden;
}

.detail-row .text-grid .augment-tag {
  box-sizing: border-box;
  flex: 0 0 calc(5em + 8px);
  width: calc(5em + 8px);
  max-width: calc(5em + 8px);
  height: 19px;
  font-size: 13.5px;
  line-height: 1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0 3px;
  text-align: center;
  text-overflow: clip;
}
</style>
