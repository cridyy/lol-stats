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
import {
  buildChampionProfiles,
  buildPlayerProfile,
  profileScoreLevel,
  profileTierLabel,
} from "../playerProfile"
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
const statsRootRef = ref<HTMLElement | null>(null)
const tableWrapRef = ref<HTMLElement | null>(null)
const championStatsCaptureRef = ref<HTMLElement | null>(null)
const championGamesCaptureRef = ref<HTMLElement | null>(null)
const notify = inject(notifyKey, () => 0)
let savedTableScrollTop = 0
let savedPageScrollTop = 0
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
const ratingContext = computed(() => ({
  items: itemMap.value,
  champions: props.champions,
}))
const playerProfile = computed(() =>
  buildPlayerProfile(props.stats?.recentGames || [], ratingContext.value),
)
const championProfileMap = computed(() =>
  buildChampionProfiles(props.stats?.recentGames || [], ratingContext.value).reduce<
    Record<number, ReturnType<typeof buildChampionProfiles>[number]>
  >((acc, profile) => {
    acc[profile.championId] = profile
    return acc
  }, {}),
)
const abilityCards = computed(() => [
  playerProfile.value.abilities.carry,
  playerProfile.value.abilities.frontline,
  playerProfile.value.abilities.support,
])
const roleBarColors = ["#2f78d6", "#d08a20", "#d44b7a", "#8b5cf6", "#d34f4f", "#5c6f7a"]
const roleBars = computed(() =>
  playerProfile.value.roleDistribution.slice(0, roleBarColors.length).map((role, index) => ({
    ...role,
    color: roleBarColors[index],
    width: `${Math.max(role.rate * 100, 4)}%`,
  })),
)

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
  savedTableScrollTop = tableWrapRef.value?.scrollTop ?? 0
  savedPageScrollTop = scrollParent(statsRootRef.value)?.scrollTop ?? 0
  selectedChampionId.value = championId
}

async function closeChampionGames() {
  selectedChampionId.value = null
  await nextTick()
  if (tableWrapRef.value) tableWrapRef.value.scrollTop = savedTableScrollTop

  const parent = scrollParent(statsRootRef.value)
  if (parent) parent.scrollTop = savedPageScrollTop
}

function scrollParent(element: HTMLElement | null) {
  let current = element?.parentElement || null
  while (current) {
    const style = window.getComputedStyle(current)
    const overflowY = style.overflowY
    if (
      (overflowY === "auto" || overflowY === "scroll" || overflowY === "overlay") &&
      current.scrollHeight > current.clientHeight
    ) {
      return current
    }
    current = current.parentElement
  }

  return document.scrollingElement as HTMLElement | null
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

function scoreText(score: number, games: number) {
  return games > 0 ? `${Math.round(score)}分` : "样本不足"
}

function profileLevelClass(score: number, games: number) {
  return games > 0 ? `profile-${profileScoreLevel(score)}` : "profile-empty"
}

function profileEvaluationText(score: number, games: number) {
  return games > 0 ? profileTierLabel(score) : "样本不足"
}

function abilityRate(games: number) {
  return ratio(games, playerProfile.value.games)
}

function mainRoleText(role: string) {
  return role && role !== "样本不足" ? `主玩${role}` : "样本不足"
}

function tagToneClass(label: string) {
  if (/小坑比|大坑比/.test(label)) return "tag-danger"
  if (/小有实力/.test(label)) return "tag-team"
  if (
    /通天|小代|全场火力|无解|输出机器|爆炸核弹|大魔王|恐怖利刃|无情收割|吃草挤奶|核心大C|chovy|faker|城墙|叹息之墙|半肉战神|最强前锋|高光|稳定|核心|控杀|控制/.test(
      label,
    )
  ) {
    return "tag-strong"
  }
  if (/战犯|低能|混子|K头|k头|开游戏|纸糊|发软|隐身|拉胯|毫无存在|拿钱不干事|自爆/.test(label)) {
    return "tag-danger"
  }
  if (/承伤|团队|辅助|前排|功能|治疗/.test(label)) {
    return "tag-team"
  }
  if (/冲锋|冲阵|上下限|浴血|顶级前锋|刀尖舔血/.test(label)) {
    return "tag-warn"
  }
  return "tag-neutral"
}

function championTagToneClass(band: string) {
  switch (band) {
    case "excellent":
      return "tag-band-excellent"
    case "normal":
      return "tag-band-normal"
    case "low":
      return "tag-band-low"
    case "disaster":
      return "tag-band-disaster"
    default:
      return "tag-neutral"
  }
}

function championProfile(championId: number) {
  return (
    championProfileMap.value[championId] || {
      championId,
      games: 0,
      averageScore: 0,
      mainRoleLabel: "样本不足",
      label: "样本不足",
      labelBand: "empty",
      highlightRate: 0,
      disasterRate: 0,
      averageDamageShare: 0,
      averageDamageConversion: 0,
      averageMitigationShare: 0,
      averageHealingShare: 0,
    }
  )
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

  <section class="stats" v-else ref="statsRootRef">
    <div class="stats-overview">
      <div class="overview-main">
        <div class="metric-grid">
          <div class="metric">
            <span>胜率</span>
            <strong>{{ stats.summary.games }}场 {{ percent(stats.summary.winRate) }}胜率</strong>
            <small>
              {{ stats.summary.wins }}胜 {{ stats.summary.losses }}负
            </small>
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
        </div>

        <section class="panel profile-panel">
          <div class="profile-heading">
            <div class="section-title">玩家画像</div>
          </div>

          <div class="profile-layout">
            <div
              :class="[
                'profile-score',
                profileLevelClass(playerProfile.overallScore, playerProfile.games),
              ]"
            >
              <div class="profile-score-line">
                <span>综合分</span>
                <strong>{{ scoreText(playerProfile.overallScore, playerProfile.games) }}</strong>
              </div>
              <em>{{ mainRoleText(playerProfile.mainRoleLabel) }}</em>
            </div>

            <div class="profile-tags">
              <span
                v-for="tag in playerProfile.tags"
                :key="tag"
                :class="tagToneClass(tag)"
              >
                {{ tag }}
              </span>
            </div>
          </div>

          <div class="ability-grid">
            <article
              v-for="ability in abilityCards"
              :key="ability.key"
              :class="['ability-item', profileLevelClass(ability.averageScore, ability.games)]"
            >
              <header>
                <span>{{ ability.label }}</span>
                <strong>{{ scoreText(ability.averageScore, ability.games) }}</strong>
              </header>
              <div>
                <span>场数 <b>{{ ability.games }}</b></span>
                <span>占比 <b>{{ percent(abilityRate(ability.games)) }}</b></span>
              </div>
              <footer>
                <span>高光局 <b>{{ percent(ability.highlightRate) }}</b></span>
                <span>战犯局 <b>{{ percent(ability.disasterRate) }}</b></span>
              </footer>
            </article>
          </div>
        </section>
      </div>

      <aside class="panel role-bar-panel">
        <div class="role-bar-scroll">
          <div
            v-for="role in roleBars"
            :key="role.label"
            class="role-bar-row"
            :style="{ '--role-color': role.color }"
          >
            <div class="role-bar-head">
              <strong>{{ role.label }}</strong>
              <span>{{ percent(role.rate) }}</span>
            </div>
            <div class="role-bar-track">
              <div class="role-bar-fill" :style="{ width: role.width }"></div>
            </div>
            <div class="role-bar-meta">
              <span>{{ role.games }} 场</span>
              <span>胜率 {{ percent(role.winRate) }}</span>
            </div>
          </div>
        </div>
      </aside>
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
        <div class="table-wrap" ref="tableWrapRef">
          <table>
            <thead>
              <tr>
                <th>英雄</th>
                <th>场次</th>
                <th>胜率</th>
                <th>评分</th>
                <th>评价</th>
                <th>标签</th>
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
                  <span
                    :class="[
                      'champion-score',
                      profileLevelClass(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      ),
                    ]"
                  >
                    {{
                      scoreText(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      )
                    }}
                  </span>
                  <small class="champion-role">
                    {{ championProfile(champ.championId).mainRoleLabel }}
                  </small>
                </td>
                <td>
                  <span
                    :class="[
                      'champion-label-tag',
                      tagToneClass(
                        profileEvaluationText(
                          championProfile(champ.championId).averageScore,
                          championProfile(champ.championId).games,
                        ),
                      ),
                    ]"
                  >
                    {{
                      profileEvaluationText(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      )
                    }}
                  </span>
                </td>
                <td>
                  <span
                    :class="[
                      'champion-label-tag',
                      championTagToneClass(championProfile(champ.championId).labelBand),
                    ]"
                  >
                    {{ championProfile(champ.championId).label }}
                  </span>
                </td>
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
            <button @click="closeChampionGames">返回</button>
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
              <span class="mobile-chip score">
                {{
                  scoreText(
                    championProfile(champ.championId).averageScore,
                    championProfile(champ.championId).games,
                  )
                }}
              </span>
              <span class="mobile-chip">
                {{
                  profileEvaluationText(
                    championProfile(champ.championId).averageScore,
                    championProfile(champ.championId).games,
                  )
                }}
              </span>
              <span
                :class="[
                  'mobile-chip',
                  championTagToneClass(championProfile(champ.championId).labelBand),
                ]"
              >
                {{ championProfile(champ.championId).label }}
              </span>
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
              <th>评分</th>
              <th>评价</th>
              <th>标签</th>
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
                {{
                  scoreText(
                    championProfile(champ.championId).averageScore,
                    championProfile(champ.championId).games,
                  )
                }}
                <small>{{ championProfile(champ.championId).mainRoleLabel }}</small>
              </td>
              <td>
                <span
                  :class="[
                    'champion-label-tag',
                    tagToneClass(
                      profileEvaluationText(
                        championProfile(champ.championId).averageScore,
                        championProfile(champ.championId).games,
                      ),
                    ),
                  ]"
                >
                  {{
                    profileEvaluationText(
                      championProfile(champ.championId).averageScore,
                      championProfile(champ.championId).games,
                    )
                  }}
                </span>
              </td>
              <td>
                <span
                  :class="[
                    'champion-label-tag',
                    championTagToneClass(championProfile(champ.championId).labelBand),
                  ]"
                >
                  {{ championProfile(champ.championId).label }}
                </span>
              </td>
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

.stats-overview {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 390px;
  gap: 14px;
  align-items: stretch;
}

.overview-main {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 14px;
}

.metric-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
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

.profile-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.profile-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.profile-heading .section-title {
  margin-bottom: 0;
}

.profile-heading span {
  color: #718087;
  font-size: 12px;
  font-weight: 800;
}

.profile-layout {
  display: grid;
  grid-template-columns: 250px minmax(0, 1fr);
  gap: 10px;
  align-items: stretch;
}

.profile-score {
  display: flex;
  min-height: 92px;
  flex-direction: column;
  justify-content: center;
  gap: 5px;
  border-radius: 8px;
  padding: 12px;
}

.profile-score-line {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
}

.profile-score-line span,
.profile-score-line strong {
  color: inherit;
  font-size: 30px;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.profile-score em {
  font-size: 16px;
  font-style: normal;
  font-weight: 900;
}

.profile-tags {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  align-content: center;
  gap: 7px;
  border-radius: 8px;
  background: #f7fbfa;
  padding: 12px;
}

.profile-tags span {
  border-radius: 999px;
  font-size: 13px;
  font-weight: 900;
  line-height: 1;
  padding: 7px 10px;
  white-space: nowrap;
}

.ability-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}

.ability-item {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 8px;
  border-radius: 8px;
  background: #f7fbfa;
  padding: 11px 12px;
}

.ability-item header,
.ability-item div,
.ability-item footer {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.ability-item header span {
  color: inherit;
  font-size: 20px;
  font-weight: 950;
  line-height: 1;
}

.ability-item header strong {
  font-size: 23px;
  font-weight: 950;
  line-height: 1;
}

.ability-item div span,
.ability-item footer span {
  color: #5f7076;
  font-size: 14px;
  font-weight: 900;
  white-space: nowrap;
}

.ability-item div b,
.ability-item footer b {
  color: #20333a;
  font-weight: 950;
}

.role-bar-panel {
  contain: size;
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
  padding: 0;
}

.role-bar-scroll {
  display: flex;
  width: 100%;
  height: 100%;
  min-height: 0;
  flex-direction: column;
  gap: 12px;
  overflow-x: hidden;
  overflow-y: auto;
  padding: 16px 12px 16px 16px;
  scrollbar-gutter: stable;
  scrollbar-width: thin;
}

.role-bar-row {
  --role-color: #2f78d6;
  display: flex;
  flex: 0 0 auto;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
  border: 1px solid #e0ebe8;
  border-radius: 8px;
  background: #fbfdfc;
  padding: 10px;
}

.role-bar-head,
.role-bar-meta {
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.role-bar-head strong {
  overflow: hidden;
  color: #20333a;
  font-size: 17px;
  font-weight: 950;
  line-height: 1;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.role-bar-head span {
  color: var(--role-color);
  font-size: 18px;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.role-bar-track {
  position: relative;
  overflow: hidden;
  height: 13px;
  border-radius: 999px;
  background: #eaf1ef;
}

.role-bar-fill {
  height: 100%;
  min-width: 6px;
  border-radius: inherit;
  background: linear-gradient(90deg, var(--role-color), color-mix(in srgb, var(--role-color) 72%, #ffffff));
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.42) inset;
}

.role-bar-meta span {
  color: #4f6066;
  font-size: 13px;
  font-weight: 900;
  white-space: nowrap;
}

.tag-strong {
  color: #5d3300;
  background: #ffe09b;
}

.tag-danger {
  color: #922f2f;
  background: #f9d1d0;
}

.tag-team {
  color: #135c42;
  background: #d0f0dd;
}

.tag-warn {
  color: #7b4d02;
  background: #ffe2b8;
}

.tag-neutral {
  color: #1f514a;
  background: #dceee9;
}

.tag-band-excellent,
.mobile-chip.tag-band-excellent {
  color: #5d3300;
  background: #ffe09b;
}

.tag-band-normal,
.mobile-chip.tag-band-normal {
  color: #135c42;
  background: #d0f0dd;
}

.tag-band-low,
.mobile-chip.tag-band-low {
  color: #174d83;
  background: #dcecff;
}

.tag-band-disaster,
.mobile-chip.tag-band-disaster {
  color: #922f2f;
  background: #f9d1d0;
}

.profile-excellent {
  color: #5d3300;
  background:
    linear-gradient(135deg, rgba(255, 244, 184, 0.96), rgba(255, 195, 64, 0.9) 45%, rgba(255, 236, 150, 0.96)),
    #ffd36a;
}

.profile-good {
  color: #145b3e;
  background: #d9f1df;
}

.profile-average {
  color: #174d83;
  background: #dcecff;
}

.profile-poor {
  color: #8f3434;
  background: #f8dedc;
}

.profile-empty {
  color: #657179;
  background: #edf4f2;
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

.champion-score,
.champion-role {
  display: block;
}

.champion-score {
  width: fit-content;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 950;
  line-height: 1;
  padding: 5px 7px;
}

.champion-role {
  color: #718087;
  font-size: 11px;
  margin-top: 3px;
}

.champion-label-tag {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 950;
  line-height: 1;
  padding: 6px 8px;
  white-space: nowrap;
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
  width: 1240px;
  pointer-events: none;
}

.share-capture-root.mobile {
  width: 430px;
}

.share-card {
  width: 1240px;
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
  grid-template-columns: 28px minmax(0, 1fr) auto auto auto;
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

.mobile-chip.score {
  color: #5d3300;
  background: #ffe6a6;
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
  .stats-overview {
    grid-template-columns: 1fr;
  }

  .metric-grid {
    grid-template-columns: 1fr 1fr;
  }

  .profile-layout,
  .ability-grid {
    grid-template-columns: 1fr;
  }
}
</style>
