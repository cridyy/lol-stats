<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { loadRankedStats } from "../api"
import { globalBaselinesByChampionId } from "../globalBaselines"
import {
  buildChampionProfiles,
  buildPlayerProfile,
  profileTierClass,
  profileTierLabel,
  type ChampionProfile,
  type PlayerProfile,
} from "../playerProfile"
import { calculateOfficialRating, type OutputRating } from "../scoring"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  GameAssetEntry,
  LiveGameResponse,
  LivePlayer,
  LivePremadeMarker,
  LiveTeam,
  RankedQueueEntry,
  RankedStatsResponse,
  RecentGame,
} from "../types"
import ChampionAvatar from "./ChampionAvatar.vue"
import { championName, fixed, mitigationValue, percent, phaseName, riotId, teamMitigationValue } from "../utils"

const LIVE_DISPLAY_DEPTH = 50
const MIN_VALID_GAME_DURATION_SECONDS = 8 * 60

const props = withDefaults(
  defineProps<{
    liveGame: LiveGameResponse | null
    champions: Record<number, ChampionSummaryItem>
    gameAssets: GameAssetBundle
    loading: boolean
    error: string
    cardExtraHeight?: number
  }>(),
  {
    cardExtraHeight: 0,
  },
)

const emit = defineEmits<{
  openPlayer: [player: LivePlayer]
}>()

const RANK_CACHE_TTL_MS = 5 * 60 * 1000
const rankedData = ref<Record<string, RankedStatsResponse | null>>({})
const rankedLoading = ref<Record<string, boolean>>({})
const rankedCache = new Map<string, { data: RankedStatsResponse | null; loadedAt: number }>()
const rankedRequests = new Map<string, Promise<void>>()

const rankTierLabels: Record<string, string> = {
  IRON: "黑铁",
  BRONZE: "黄铜",
  SILVER: "白银",
  GOLD: "黄金",
  PLATINUM: "铂金",
  EMERALD: "翡翠",
  DIAMOND: "钻石",
  MASTER: "大师",
  GRANDMASTER: "宗师",
  CHALLENGER: "王者",
}
const apexRankTiers = new Set(["MASTER", "GRANDMASTER", "CHALLENGER"])

interface RecentSummary {
  winRate: number
  averageKda: number
  damageShare: number
  damageConversion: number
  mitigationShare: number
}

interface LivePlayerView {
  player: LivePlayer
  games: RecentGame[]
  ratingsByGameId: Map<number, OutputRating>
  summary: RecentSummary
  profile: PlayerProfile | null
  championProfile: ChampionProfile | null
}

interface LiveTeamSummary extends RecentSummary {
  players: number
  averageScore: number
}

interface LiveTeamView {
  team: LiveTeam
  players: LivePlayerView[]
  summary: LiveTeamSummary
}

const itemMap = computed(() => indexAssets(props.gameAssets.items))
const livePanelStyle = computed(() => ({
  "--live-card-extra-height": `${Math.min(480, Math.max(0, props.cardExtraHeight))}px`,
}))
const ratingContext = computed(() => ({
  items: itemMap.value,
  champions: props.champions,
}))
const liveTeamViews = computed<LiveTeamView[]>(() => {
  const liveGame = props.liveGame
  if (!liveGame) return []

  return liveGame.teams.map((team) => {
    const players = team.players.map((player) => buildLivePlayerView(player))
    return {
      team,
      players,
      summary: summarizeTeamViews(players),
    }
  })
})

watch(
  () =>
    props.liveGame?.teams
      .flatMap((team) => team.players)
      .map((player) => player.puuid)
      .join("|") || "",
  () => {
    const players = props.liveGame?.teams.flatMap((team) => team.players) || []
    for (const player of players) void ensureRanked(player)
  },
  { immediate: true },
)

function indexAssets(entries: GameAssetEntry[]) {
  return entries.reduce<Record<number, GameAssetEntry>>((acc, entry) => {
    acc[entry.id] = entry
    return acc
  }, {})
}

function queryStageName(stage?: string) {
  const map: Record<string, string> = {
    "champ-select": "选人阶段",
    "in-game": "游戏中",
  }
  return stage ? map[stage] || stage : "-"
}

function queueName(liveGame: LiveGameResponse) {
  const map: Record<number, string> = {
    400: "匹配",
    420: "单双排",
    430: "匹配",
    440: "灵活排",
    450: "大乱斗",
    490: "快速匹配",
    1700: "斗魂竞技场",
    1710: "斗魂竞技场",
    1711: "斗魂竞技场",
    1712: "斗魂竞技场",
    2400: "海克斯大乱斗",
  }

  if (liveGame.queueId && map[liveGame.queueId]) return map[liveGame.queueId]
  return liveGame.gameMode || liveGame.queueType || (liveGame.queueId ? `队列 ${liveGame.queueId}` : "-")
}

function filterRecentGames(player: LivePlayer) {
  return (
    player.stats?.recentGames
      .filter((game) => game.gameDuration >= MIN_VALID_GAME_DURATION_SECONDS)
      .slice(0, LIVE_DISPLAY_DEPTH) || []
  )
}

function rankedKey(player: LivePlayer) {
  return player.puuid.trim().toLowerCase()
}

function setRankedData(key: string, data: RankedStatsResponse | null) {
  rankedData.value = { ...rankedData.value, [key]: data }
}

function setRankedLoading(key: string, loading: boolean) {
  const next = { ...rankedLoading.value }
  if (loading) next[key] = true
  else delete next[key]
  rankedLoading.value = next
}

async function ensureRanked(player: LivePlayer) {
  const key = rankedKey(player)
  if (!key || player.isPlaceholder) return

  const cached = rankedCache.get(key)
  if (cached && Date.now() - cached.loadedAt < RANK_CACHE_TTL_MS) {
    setRankedData(key, cached.data)
    return
  }

  const existing = rankedRequests.get(key)
  if (existing) return existing

  setRankedLoading(key, true)
  const request = loadRankedStats(player.puuid)
    .then((data) => {
      rankedCache.set(key, { data, loadedAt: Date.now() })
      setRankedData(key, data)
    })
    .catch(() => {
      rankedCache.set(key, { data: null, loadedAt: Date.now() })
      setRankedData(key, null)
    })
    .finally(() => {
      setRankedLoading(key, false)
      rankedRequests.delete(key)
    })

  rankedRequests.set(key, request)
  return request
}

function playerLevel(player: LivePlayer) {
  const level = player.summoner?.summonerLevel || 0
  return level > 0 ? String(level) : "-"
}

function normalizedRankTier(tier?: string) {
  const value = tier?.trim().toUpperCase() || ""
  return value && value !== "NA" && value !== "NONE" ? value : "UNRANKED"
}

function hasRankTier(entry?: RankedQueueEntry | null) {
  return Boolean(entry && normalizedRankTier(entry.tier) !== "UNRANKED")
}

function rankedResponseFor(player: LivePlayer) {
  const key = rankedKey(player)
  if (!key) return undefined
  return Object.prototype.hasOwnProperty.call(rankedData.value, key)
    ? rankedData.value[key]
    : undefined
}

function rankedEntryFor(player: LivePlayer) {
  const ranked = rankedResponseFor(player)
  if (!ranked || ranked.unavailableReason) return null

  const solo = ranked.queueMap?.RANKED_SOLO_5x5
  const flex = ranked.queueMap?.RANKED_FLEX_SR
  const preferred = props.liveGame?.queueId === 440 ? flex : solo
  const fallback = preferred === flex ? solo : flex
  return hasRankTier(preferred) ? preferred : hasRankTier(fallback) ? fallback : preferred || fallback || null
}

function rankTextFor(player: LivePlayer) {
  if (player.isPlaceholder || !rankedKey(player)) return "未定级"
  if (rankedLoading.value[rankedKey(player)]) return "读取中"

  const ranked = rankedResponseFor(player)
  if (ranked === undefined) return "读取中"
  if (!ranked || ranked.unavailableReason) return "暂不可读"

  const entry = rankedEntryFor(player)
  if (!entry) return "未定级"

  const tier = normalizedRankTier(entry.tier)
  if (tier === "UNRANKED") return "未定级"
  const tierLabel = rankTierLabels[tier] || tier
  const division = apexRankTiers.has(tier) ? "" : entry.division?.trim().toUpperCase() || ""
  return `${tierLabel}${division ? ` ${division}` : ""}`
}

function rankTitleFor(player: LivePlayer) {
  const ranked = rankedResponseFor(player)
  const entry = rankedEntryFor(player)
  if (ranked?.unavailableReason) return ranked.unavailableReason
  if (!entry || !hasRankTier(entry)) return rankTextFor(player)
  const lp = entry.leaguePoints > 0 ? ` · ${entry.leaguePoints} LP` : ""
  return `${rankTextFor(player)}${lp}`
}

function gameKda(game: RecentGame) {
  return `${game.kills}/${game.deaths}/${game.assists}`
}

function statPercent(value?: number) {
  return typeof value === "number" && Number.isFinite(value) ? percent(value) : "-"
}

function statFixed(value?: number, digits = 2) {
  return typeof value === "number" && Number.isFinite(value) ? fixed(value, digits) : "-"
}

function ratio(part: number, total: number) {
  return total > 0 ? part / total : 0
}

function averageKda(games: RecentGame[]) {
  const kills = games.reduce((sum, game) => sum + game.kills, 0)
  const deaths = games.reduce((sum, game) => sum + game.deaths, 0)
  const assists = games.reduce((sum, game) => sum + game.assists, 0)
  return (kills + assists) / Math.max(deaths, 1)
}

function summarizeRecentGames(games: RecentGame[]): RecentSummary {
  const wins = games.filter((game) => game.win).length
  const damage = games.reduce((sum, game) => sum + game.damageToChampions, 0)
  const teamDamage = games.reduce((sum, game) => sum + game.teamDamageToChampions, 0)
  const gold = games.reduce((sum, game) => sum + game.goldEarned, 0)
  const teamGold = games.reduce((sum, game) => sum + game.teamGoldEarned, 0)
  const mitigation = games.reduce((sum, game) => sum + mitigationValue(game), 0)
  const teamMitigation = games.reduce((sum, game) => sum + teamMitigationValue(game), 0)
  const aggregateDamageShare = ratio(damage, teamDamage)
  const aggregateGoldShare = ratio(gold, teamGold)

  return {
    winRate: games.length ? wins / games.length : 0,
    averageKda: averageKda(games),
    damageShare: aggregateDamageShare,
    damageConversion: aggregateGoldShare > 0 ? aggregateDamageShare / aggregateGoldShare : 0,
    mitigationShare: ratio(mitigation, teamMitigation),
  }
}

function buildLivePlayerView(player: LivePlayer): LivePlayerView {
  const games = filterRecentGames(player)
  const ratingsByGameId = new Map<number, OutputRating>()
  for (const game of games) {
    ratingsByGameId.set(
      game.gameId,
      calculateOfficialRating(
        game,
        ratingContext.value,
        globalBaselinesByChampionId.value,
      ),
    )
  }
  // 实时战绩只看短样本，不使用数据统计页的大样本时间衰减。
  const profile = player.stats
    ? buildPlayerProfile(games, ratingContext.value, {
      timeWeighted: false,
      globalBaselines: globalBaselinesByChampionId.value,
    })
    : null
  const selectedChampionId = currentChampionId(player)
  const championGames = selectedChampionId
    ? games.filter((game) => game.championId === selectedChampionId)
    : []
  const championProfile = championGames.length
    ? buildChampionProfiles(championGames, ratingContext.value, {
      timeWeighted: false,
      globalBaselines: globalBaselinesByChampionId.value,
    })[0] || null
    : null

  return {
    player,
    games,
    ratingsByGameId,
    summary: summarizeRecentGames(games),
    profile,
    championProfile,
  }
}

function summarizeTeamViews(players: LivePlayerView[]): LiveTeamSummary {
  const sampled = players.filter((player) => player.games.length)
  const average = (selector: (player: LivePlayerView) => number) => {
    if (!sampled.length) return 0
    return sampled.reduce((sum, player) => sum + selector(player), 0) / sampled.length
  }

  return {
    players: sampled.length,
    winRate: average((player) => player.summary.winRate),
    damageShare: average((player) => player.summary.damageShare),
    averageKda: average((player) => player.summary.averageKda),
    damageConversion: average((player) => player.summary.damageConversion),
    mitigationShare: average((player) => player.summary.mitigationShare),
    averageScore: average((player) => player.profile?.overallScore || 0),
  }
}

function teamAverageName(team: LiveTeam, index: number, teams: LiveTeam[]) {
  if (teams.length === 2) return index === 0 ? "我方" : "敌方"
  return team.name
}

function winRateTone(value: number) {
  if (value > 0.5) return "tone-good"
  if (value < 0.5) return "tone-bad"
  return "tone-warn"
}

function kdaTone(value: number) {
  if (value > 2.5) return "tone-good"
  if (value < 2.5) return "tone-bad"
  return "tone-warn"
}

function selectedChampionScoreText(profile: ChampionProfile | null) {
  return profile?.games ? `${Math.round(profile.averageScore)}分` : "-"
}

function selectedChampionScoreTitle(player: LivePlayer, profile: ChampionProfile | null) {
  const champion = championName(props.champions, currentChampionId(player))
  return profile?.games
    ? `${champion} · 历史均分 ${selectedChampionScoreText(profile)}`
    : `${champion} · 暂无历史评分`
}

function selectedChampionClass(profile: ChampionProfile | null) {
  return profile?.games ? profileTierClass(profile.averageScore) : "profile-empty"
}

function currentChampionId(player: LivePlayer) {
  return player.championId || player.championPickIntent || 0
}

function profileScoreText(profile: PlayerProfile | null) {
  return profile?.games ? `${Math.round(profile.overallScore)}分` : "-"
}

function playerProfileTierLabel(profile: PlayerProfile | null) {
  if (!profile?.games) return "样本不足"
  return profileTierLabel(profile.overallScore)
}

function profileClass(profile: PlayerProfile | null) {
  return profile?.games ? profileTierClass(profile.overallScore) : "profile-empty"
}

function livePlayerRiotId(player: LivePlayer) {
  return player.summoner ? riotId(player.summoner) : player.puuid || "未知玩家"
}

function premadeClass(marker: LivePremadeMarker) {
  return `premade-${marker.groupId.toLowerCase()}`
}

function premadeMembers(marker: LivePremadeMarker) {
  const players = props.liveGame?.teams.flatMap((team) => team.players) || []
  return marker.memberPuuids
    .map((puuid) => players.find((player) => player.puuid.toLowerCase() === puuid.toLowerCase()))
    .filter((player): player is LivePlayer => Boolean(player))
    .map((player) => riotId(player.summoner))
    .join("、")
}

function premadeTitle(marker: LivePremadeMarker) {
  const members = premadeMembers(marker)
  const source =
    marker.source === "历史同队" && marker.togetherTimes
      ? `${marker.source} ${marker.togetherTimes} 次以上`
      : marker.source
  return members ? `${marker.label} · ${source} · ${members}` : `${marker.label} · ${source}`
}

function openLivePlayer(player: LivePlayer) {
  if (!player.puuid || player.isPlaceholder) return
  emit("openPlayer", player)
}

function gameRating(playerView: LivePlayerView, game: RecentGame) {
  return playerView.ratingsByGameId.get(game.gameId) || calculateOfficialRating(
    game,
    ratingContext.value,
    globalBaselinesByChampionId.value,
  )
}

function gameRatingClass(playerView: LivePlayerView, game: RecentGame) {
  return `score-${gameRating(playerView, game).level}`
}

</script>

<template>
  <section class="live" :style="livePanelStyle">
    <header class="live-header">
      <div>
        <div class="label">实战读取</div>
        <h2>{{ liveGame ? phaseName(liveGame.phase) : "未读取" }}</h2>
      </div>
      <div class="game-meta" v-if="liveGame">
        <span>{{ queryStageName(liveGame.queryStage) }}</span>
        <span>{{ queueName(liveGame) }}</span>
        <span v-if="liveGame.gameId">对局 {{ liveGame.gameId }}</span>
      </div>
    </header>

    <div class="live-empty" v-if="loading && !liveGame">正在读取双方数据</div>
    <div class="live-empty error" v-else-if="!liveGame && error">{{ error }}</div>
    <div class="live-empty" v-else-if="!liveGame">当前没有可读取的对局</div>

    <template v-else-if="liveGame.teams.some((team) => team.players.length)">
      <div class="live-stale-notice" v-if="error">
        {{ error }}，已保留上一局实时战绩
      </div>

      <div class="team-averages">
        <section
          class="team-average"
          v-for="(teamView, index) in liveTeamViews"
          :key="`average-${teamView.team.name}`"
        >
          <header>
            <strong>{{ teamAverageName(teamView.team, index, liveGame.teams) }}</strong>
            <span>{{ teamView.summary.players }} 人样本</span>
          </header>
          <div class="average-metrics">
            <span>
              <i>平均胜率</i>
              <b>{{ statPercent(teamView.summary.winRate) }}</b>
            </span>
            <span>
              <i>平均伤害</i>
              <b>{{ statPercent(teamView.summary.damageShare) }}</b>
            </span>
            <span>
              <i>平均 KDA</i>
              <b>{{ statFixed(teamView.summary.averageKda) }}</b>
            </span>
            <span>
              <i>平均伤转</i>
              <b>{{ statFixed(teamView.summary.damageConversion) }}</b>
            </span>
            <span>
              <i>平均承伤</i>
              <b>{{ statPercent(teamView.summary.mitigationShare) }}</b>
            </span>
            <span>
              <i>平均评分</i>
              <b>{{ statFixed(teamView.summary.averageScore, 0) }}分</b>
            </span>
          </div>
        </section>
      </div>

      <div class="teams">
        <section class="team" v-for="teamView in liveTeamViews" :key="teamView.team.name">
          <div class="team-title">
            <strong>{{ teamView.team.name }}</strong>
            <span>{{ teamView.players.length }} 人</span>
          </div>

          <div class="player-grid">
            <article
              class="player-card"
              v-for="playerView in teamView.players"
              :key="`${teamView.team.name}-${playerView.player.puuid}`"
            >
              <div
                v-if="currentChampionId(playerView.player)"
                :class="['locked-hero-line', selectedChampionClass(playerView.championProfile)]"
                :title="selectedChampionScoreTitle(playerView.player, playerView.championProfile)"
              >
                <ChampionAvatar
                  :champion-id="currentChampionId(playerView.player)"
                  :champions="champions"
                  :size="34"
                />
                <strong>{{ selectedChampionScoreText(playerView.championProfile) }}</strong>
                <button
                  class="live-summoner-link"
                  type="button"
                  :title="`查看 ${livePlayerRiotId(playerView.player)} 的总览`"
                  :disabled="!playerView.player.puuid || playerView.player.isPlaceholder"
                  @click="openLivePlayer(playerView.player)"
                >
                  {{ livePlayerRiotId(playerView.player) }}
                </button>
                <span
                  v-if="playerView.player.premade"
                  :class="['premade-tag', premadeClass(playerView.player.premade)]"
                  :title="premadeTitle(playerView.player.premade)"
                >
                  {{ playerView.player.premade.label }}
                </span>
              </div>

              <div class="rank-line" :title="rankTitleFor(playerView.player)">
                <span class="rank-level">
                  <i>等级</i>
                  <b>{{ playerLevel(playerView.player) }}</b>
                </span>
                <span class="rank-divider" aria-hidden="true"></span>
                <span class="rank-entry">
                  <strong>{{ rankTextFor(playerView.player) }}</strong>
                </span>
                <span class="rank-divider" aria-hidden="true"></span>
                <span class="rank-win-rate">
                  <i>胜率</i>
                  <b :class="winRateTone(playerView.summary.winRate)">
                    {{ statPercent(playerView.summary.winRate) }}
                  </b>
                </span>
                <span class="rank-divider" aria-hidden="true"></span>
                <span class="rank-kda">
                  <i>KDA</i>
                  <b :class="kdaTone(playerView.summary.averageKda)">
                    {{ statFixed(playerView.summary.averageKda, 1) }}
                  </b>
                </span>
              </div>

              <div class="profile-panel-line" v-if="playerView.player.stats">
                <div
                  :class="['profile-main-row', profileClass(playerView.profile)]"
                  :title="`${playerProfileTierLabel(playerView.profile)} · ${profileScoreText(playerView.profile)}`"
                >
                  <strong>{{ playerProfileTierLabel(playerView.profile) }}</strong>
                  <b>{{ profileScoreText(playerView.profile) }}</b>
                </div>
                <div :class="['profile-stat-row', 'profile-overall-row', profileClass(playerView.profile)]">
                  <span>主玩 <b>{{ playerView.profile?.mainRoleLabel || "-" }}</b></span>
                  <span>carry率 <b>{{ statPercent(playerView.profile?.highlightRate) }}</b></span>
                  <span>战犯率 <b>{{ statPercent(playerView.profile?.disasterRate) }}</b></span>
                </div>
              </div>

              <div class="player-error" v-if="!playerView.player.stats">
                <strong>{{ playerView.player.isPlaceholder ? "占位玩家" : "读取失败" }}</strong>
                <span>{{ playerView.player.error || "暂无战绩数据" }}</span>
              </div>

              <div class="recent-strip" v-if="playerView.games.length">
                <div
                  class="recent-game"
                  v-for="game in playerView.games"
                  :key="game.gameId"
                  :class="{ win: game.win, loss: !game.win }"
                >
                  <ChampionAvatar :champion-id="game.championId" :champions="champions" :size="22" />
                  <div class="recent-kda">
                    <strong>{{ gameKda(game) }}</strong>
                  </div>
                  <div :class="['score-cell', 'recent-score-cell', gameRatingClass(playerView, game)]">
                    <strong>{{ gameRating(playerView, game).score }}分</strong>
                  </div>
                </div>
              </div>
              <div class="recent-empty" v-else-if="playerView.player.stats">暂无数据</div>
            </article>
          </div>
        </section>
      </div>
    </template>

    <div class="live-empty" v-else>当前阶段没有可读取的真实玩家</div>
  </section>
</template>

<style scoped>
.live {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.live-header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
}

.label {
  color: #75838a;
  font-size: 13px;
}

h2 {
  margin: 4px 0 0;
  color: #1f2a2e;
  font-size: 26px;
}

.game-meta {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  color: #53656b;
  font-size: 13px;
}

.game-meta span {
  border: 1px solid #d6e5e1;
  border-radius: 999px;
  padding: 6px 10px;
  background: rgba(255, 255, 255, 0.72);
}

.live-empty {
  display: grid;
  min-height: 360px;
  place-items: center;
  border: 1px dashed #cbdcd8;
  border-radius: 8px;
  color: #657179;
  background: rgba(255, 255, 255, 0.55);
}

.live-empty.error {
  color: #a94745;
  border-color: #efc4c2;
}

.live-stale-notice {
  border: 1px solid #efd6a3;
  border-radius: 8px;
  background: #fff6df;
  color: #8a5a12;
  font-size: 13px;
  font-weight: 700;
  padding: 9px 12px;
}

.team-averages {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(0, 1fr));
  gap: 12px;
}

.team-average {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 8px;
  border: 1px solid #d7e5e2;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.78);
  box-shadow: 0 10px 24px rgba(32, 67, 73, 0.06);
  padding: 10px 12px;
}

.team-average header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.team-average header strong {
  color: #1f2a2e;
  font-size: 16px;
  font-weight: 950;
}

.team-average header span {
  color: #6d7d83;
  font-size: 12px;
  font-weight: 800;
}

.average-metrics {
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 8px;
}

.average-metrics span {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}

.average-metrics i {
  overflow: hidden;
  color: #52636a;
  font-size: 11px;
  font-style: normal;
  font-weight: 800;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.average-metrics b {
  color: #1f2a2e;
  font-size: 17px;
  font-weight: 950;
}

.teams {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.team {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 10px;
}

.team-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: #263238;
}

.team-title strong {
  font-size: 16px;
  font-weight: 900;
}

.team-title span {
  color: #6f7f84;
  font-size: 12px;
}

.player-grid {
  --live-player-card-min: 270px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(var(--live-player-card-min), 1fr));
  justify-content: start;
  gap: 8px;
  min-width: 0;
}

.player-card {
  display: flex;
  min-width: var(--live-player-card-min);
  min-height: calc(447px + var(--live-card-extra-height, 0px));
  flex-direction: column;
  gap: 8px;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 10px 24px rgba(32, 67, 73, 0.06);
  padding: 9px;
}

.locked-hero-line {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
  border-radius: 7px;
  padding: 7px 9px;
}

.locked-hero-line strong {
  flex: 0 0 auto;
  color: inherit;
  font-size: 18px;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.live-summoner-link {
  appearance: none;
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  border: 0;
  padding: 0;
  color: inherit;
  background: transparent;
  cursor: pointer;
  font: inherit;
  font-size: 16px;
  font-weight: 950;
  line-height: 1;
  text-align: left;
  text-decoration: underline;
  text-decoration-thickness: 1px;
  text-underline-offset: 3px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.live-summoner-link:disabled {
  cursor: default;
  text-decoration: none;
}

.live-summoner-link:focus-visible {
  outline: 2px solid currentColor;
  outline-offset: 3px;
}

.premade-tag {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  border: 1px solid rgba(255, 255, 255, 0.36);
  border-radius: 999px;
  padding: 2px 7px;
  color: #1d2730;
  font-size: 12px;
  font-weight: 950;
  line-height: 1.1;
  white-space: nowrap;
  box-shadow: 0 6px 14px rgba(20, 35, 45, 0.1);
}

.premade-a {
  color: #24103d;
  background: linear-gradient(135deg, #d8b4fe, #7e22ce);
}

.premade-b {
  background: linear-gradient(135deg, #bfdbfe, #3b82f6);
  color: #071827;
}

.premade-c {
  background: linear-gradient(135deg, #bbf7d0, #22c55e);
}

.premade-d {
  background: linear-gradient(135deg, #fecdd3, #fb7185);
}

.premade-e {
  background: linear-gradient(135deg, #ddd6fe, #8b5cf6);
  color: #120b24;
}

.premade-f,
.premade-g,
.premade-h,
.premade-i,
.premade-j,
.premade-z {
  background: linear-gradient(135deg, #e2e8f0, #94a3b8);
}

.rank-line {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 7px;
  border-top: 1px solid #edf2f0;
  border-bottom: 1px solid #edf2f0;
  padding: 4px 1px;
  color: #53666c;
}

.rank-level,
.rank-entry,
.rank-win-rate,
.rank-kda {
  display: inline-flex;
  min-width: 0;
  align-items: baseline;
  gap: 4px;
  white-space: nowrap;
}

.rank-level {
  flex: 0 0 auto;
}

.rank-entry {
  flex: 1 1 auto;
  overflow: hidden;
}

.rank-win-rate {
  flex: 0 0 auto;
}

.rank-kda {
  flex: 0 0 auto;
}

.rank-level i,
.rank-entry i,
.rank-win-rate i,
.rank-kda i {
  color: #829095;
  font-size: 11px;
  font-style: normal;
  font-weight: 800;
}

.rank-level b,
.rank-entry strong,
.rank-win-rate b,
.rank-kda b {
  color: #263c42;
  font-size: 13px;
  font-weight: 950;
}

.rank-win-rate b.tone-good,
.rank-kda b.tone-good {
  color: #16844f;
}

.rank-win-rate b.tone-warn,
.rank-kda b.tone-warn {
  color: #c67900;
}

.rank-win-rate b.tone-bad,
.rank-kda b.tone-bad {
  color: #c9403d;
}

.rank-level b {
  color: #c9403d;
}

.rank-entry strong {
  color: #16844f;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rank-divider {
  width: 1px;
  height: 18px;
  flex: 0 0 1px;
  background: #dbe7e4;
}

.profile-panel-line {
  display: flex;
  width: 100%;
  flex-direction: column;
  gap: 6px;
}

.profile-main-row,
.profile-stat-row {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 8px;
}

.profile-main-row {
  justify-content: space-between;
  border-radius: 7px;
  padding: 9px 10px;
}

.profile-main-row strong,
.profile-main-row b {
  color: inherit;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.profile-main-row strong {
  font-size: 25px;
}

.profile-main-row b {
  font-size: 21px;
}

.profile-stat-row {
  flex-wrap: nowrap;
  overflow: hidden;
  border-radius: 7px;
  padding: 8px 10px;
}

.profile-overall-row {
  color: #174d83;
  background: #dcecff;
}

.profile-stat-row span {
  display: inline-flex;
  flex: 1 1 0;
  min-width: 0;
  align-items: baseline;
  gap: 3px;
  color: rgba(31, 42, 46, 0.78);
  font-size: 12.5px;
  font-weight: 900;
  white-space: nowrap;
}

.profile-stat-row b {
  overflow: visible;
  max-width: none;
  color: #d22f2f;
  font-size: 13.5px;
  font-weight: 950;
  white-space: nowrap;
}

.profile-tier-apex {
  color: #5d3300;
  background:
    linear-gradient(135deg, rgba(255, 244, 184, 0.96), rgba(255, 195, 64, 0.9) 45%, rgba(255, 236, 150, 0.96)),
    #ffd36a;
}

.profile-tier-steady {
  color: #145b3e;
  background: #d9f1df;
}

.profile-tier-normal {
  color: #174d83;
  background: #dcecff;
}

.profile-tier-small-pit {
  color: #8a5200;
  background: #fff1c8;
}

.profile-tier-big-pit {
  color: #8f3434;
  background: #f8dedc;
}

.profile-empty {
  color: #657179;
  background: #edf4f2;
}

.score-cell.score-excellent::after {
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
  animation: live-score-shine 2.8s ease-in-out infinite;
}

/* 实时页会同时展示多名玩家的历史小条，避免为每条记录创建无限循环动画。 */
.recent-score-cell.score-excellent::after {
  display: none;
}

@keyframes live-score-shine {
  0% {
    left: -90%;
  }

  52%,
  100% {
    left: 132%;
  }
}

.tone-good {
  color: #16844f;
}

.tone-warn {
  color: #c67900;
}

.tone-bad {
  color: #c9403d;
}

.player-error {
  min-width: 0;
  border-radius: 7px;
  background: #fff3f2;
  color: #a94745;
  padding: 8px;
}

.player-error strong,
.player-error span {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.player-error strong {
  font-size: 12px;
  font-weight: 900;
}

.player-error span {
  margin-top: 3px;
  font-size: 12px;
}

.recent-strip {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  max-height: calc(271px + var(--live-card-extra-height, 0px));
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior-y: contain;
  margin-top: auto;
  padding: 1px 3px 1px 1px;
  scrollbar-width: thin;
}

.recent-game {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) 42px;
  align-items: center;
  gap: 6px;
  flex: 0 0 auto;
  width: 100%;
  min-height: 28px;
  border: 1px solid transparent;
  border-left-width: 3px;
  border-radius: 5px;
  padding: 1px 3px;
  color: #263238;
  font: inherit;
  text-align: left;
}

.recent-game.win {
  border-color: #b9d9f1;
  border-left-color: #3e89cf;
  background: #edf7ff;
}

.recent-game.loss {
  border-color: #efc9c8;
  border-left-color: #ca4b4b;
  background: #fff1f1;
}

.recent-kda {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 0;
}

.recent-kda strong {
  overflow: hidden;
  color: #202b31;
  font-size: 12px;
  font-weight: 950;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.score-cell {
  position: relative;
  display: flex;
  min-width: 0;
  height: 22px;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border: 1px solid transparent;
  border-radius: 4px;
  padding: 1px 2px;
  text-align: center;
}

.score-cell strong,
.score-cell span {
  position: relative;
  z-index: 1;
  max-width: 100%;
  color: inherit;
  line-height: 1;
  white-space: nowrap;
}

.score-cell strong {
  font-size: 14px;
  font-weight: 950;
}

.score-cell span {
  overflow: hidden;
  font-size: 9px;
  font-weight: 900;
  text-overflow: ellipsis;
}

.score-excellent {
  color: #5d3300;
  border-color: rgba(245, 185, 52, 0.72);
  background:
    linear-gradient(135deg, rgba(255, 244, 184, 0.96), rgba(255, 195, 64, 0.9) 45%, rgba(255, 236, 150, 0.96)),
    #ffd36a;
}

.score-good {
  color: #145b3e;
  border-color: rgba(77, 168, 111, 0.46);
  background: rgba(204, 239, 218, 0.88);
}

.score-average {
  color: #174d83;
  border-color: rgba(77, 133, 201, 0.4);
  background: rgba(205, 229, 255, 0.92);
}

.score-poor {
  color: #8f3434;
  border-color: rgba(202, 75, 75, 0.42);
  background: rgba(248, 214, 213, 0.92);
}

.recent-game:hover {
  border-color: #7dbdb2;
}

.recent-empty {
  display: grid;
  min-height: 56px;
  place-items: center;
  border-radius: 7px;
  color: #718087;
  background: #f6faf9;
  font-size: 12px;
}

@media (max-width: 760px) {
  .live-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .game-meta {
    justify-content: flex-start;
  }

  .player-grid {
    grid-template-columns: minmax(var(--live-player-card-min), 1fr);
  }
}
</style>
