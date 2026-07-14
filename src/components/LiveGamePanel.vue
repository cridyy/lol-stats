<script setup lang="ts">
import { computed } from "vue"
import {
  buildChampionProfiles,
  buildPlayerProfile,
  profileTierClass,
  profileTierLabel,
  type ChampionProfile,
  type PlayerProfile,
} from "../playerProfile"
import type {
  ChampionSummaryItem,
  GameAssetBundle,
  GameAssetEntry,
  LiveGameResponse,
  LivePlayer,
  LivePremadeMarker,
  LiveTeam,
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
    const players = team.players.map((player) => buildLivePlayerView(player, liveGame.queryStage))
    return {
      team,
      players,
      summary: summarizeTeamViews(players),
    }
  })
})

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

function positionName(player: LivePlayer) {
  const raw = (player.position || player.selectedRole || "").toUpperCase()
  const map: Record<string, string> = {
    TOP: "上路",
    JUNGLE: "打野",
    MIDDLE: "中路",
    MID: "中路",
    BOTTOM: "下路",
    ADC: "下路",
    UTILITY: "辅助",
    SUPPORT: "辅助",
    NONE: "-",
    UNSELECTED: "-",
  }
  return map[raw] || raw || "-"
}

function filterRecentGames(player: LivePlayer) {
  return (
    player.stats?.recentGames
      .filter((game) => game.gameDuration >= MIN_VALID_GAME_DURATION_SECONDS)
      .slice(0, LIVE_DISPLAY_DEPTH) || []
  )
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

function damageShare(game: RecentGame) {
  return ratio(game.damageToChampions, game.teamDamageToChampions)
}

function mitigationShare(game: RecentGame) {
  return ratio(mitigationValue(game), teamMitigationValue(game))
}

function damageConversion(game: RecentGame) {
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  return goldShare > 0 ? damageShare(game) / goldShare : 0
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

function buildLivePlayerView(player: LivePlayer, queryStage: string): LivePlayerView {
  const games = filterRecentGames(player)
  const profile = player.stats ? buildPlayerProfile(games, ratingContext.value) : null
  const championGames =
    queryStage === "in-game" && player.championId
      ? games.filter((game) => game.championId === player.championId)
      : []
  const championProfile = championGames.length
    ? buildChampionProfiles(championGames, ratingContext.value)[0] || null
    : null

  return {
    player,
    games,
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

function kdaTone(value: number) {
  if (value > 3.5) return "tone-good"
  if (value >= 2) return "tone-warn"
  return "tone-bad"
}

function winRateTone(value: number) {
  if (value > 0.6) return "tone-good"
  if (value >= 0.35) return "tone-warn"
  return "tone-bad"
}

function damageConversionTone(value: number) {
  if (value > 1.2) return "tone-good"
  if (value >= 0.9) return "tone-warn"
  return "tone-bad"
}

function selectedChampionScoreText(profile: ChampionProfile | null) {
  return profile?.games ? `${Math.round(profile.averageScore)}分` : "样本不足"
}

function selectedChampionLabel(profile: ChampionProfile | null) {
  return profile?.label || "暂无英雄样本"
}

function selectedChampionClass(profile: ChampionProfile | null) {
  return profile?.games ? profileTierClass(profile.averageScore) : "profile-empty"
}

function profileScoreText(profile: PlayerProfile | null) {
  return profile?.games ? `${Math.round(profile.overallScore)}分` : "样本不足"
}

function abilityScoreText(score: number, games: number) {
  return games ? `${Math.round(score)}分` : "样本不足"
}

function playerProfileTierLabel(profile: PlayerProfile | null) {
  if (!profile?.games) return "样本不足"
  return profileTierLabel(profile.overallScore)
}

function profileClass(profile: PlayerProfile | null) {
  return profile?.games ? profileTierClass(profile.overallScore) : "profile-empty"
}

function carryProfileClass(profile: PlayerProfile | null) {
  const carry = profile?.abilities.carry
  if (!carry) return "profile-empty"
  return carry.games ? profileTierClass(carry.averageScore) : "profile-empty"
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

    <div class="live-empty" v-if="loading && !liveGame">正在读取双方近 50 局</div>
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
                v-if="liveGame.queryStage === 'in-game'"
                :class="['locked-hero-line', selectedChampionClass(playerView.championProfile)]"
                :title="`${championName(champions, playerView.player.championId)} · ${selectedChampionScoreText(playerView.championProfile)} · ${selectedChampionLabel(playerView.championProfile)}`"
              >
                <ChampionAvatar
                  v-if="playerView.player.championId"
                  :champion-id="playerView.player.championId"
                  :champions="champions"
                  :size="34"
                />
                <strong>{{ selectedChampionScoreText(playerView.championProfile) }}</strong>
                <span>{{ selectedChampionLabel(playerView.championProfile) }}</span>
              </div>

              <div class="identity">
                <ChampionAvatar
                  v-if="playerView.player.championId && liveGame.queryStage !== 'in-game'"
                  :champion-id="playerView.player.championId"
                  :champions="champions"
                  :size="42"
                />
                <div class="identity-text">
                  <div class="name-line">
                    <button class="player-name-button" type="button" @click="emit('openPlayer', playerView.player)">
                      {{ riotId(playerView.player.summoner) }}
                    </button>
                    <span
                      v-if="playerView.player.premade"
                      :class="['premade-tag', premadeClass(playerView.player.premade)]"
                      :title="premadeTitle(playerView.player.premade)"
                    >
                      {{ playerView.player.premade.label }}
                    </span>
                    <b v-if="playerView.player.stats" class="kda-badge">
                      <span>KDA</span>
                      <em :class="kdaTone(playerView.summary.averageKda)">
                        {{ statFixed(playerView.summary.averageKda) }}
                      </em>
                    </b>
                  </div>
                  <div class="summary-line" v-if="playerView.player.stats">
                    <span>
                      <i>胜率</i>
                      <b :class="winRateTone(playerView.summary.winRate)">
                        {{ statPercent(playerView.summary.winRate) }}
                      </b>
                    </span>
                    <span>
                      <i>伤转</i>
                      <b :class="damageConversionTone(playerView.summary.damageConversion)">
                        {{ statFixed(playerView.summary.damageConversion) }}
                      </b>
                    </span>
                    <span>
                      <i>承伤</i>
                      <b>{{ statPercent(playerView.summary.mitigationShare) }}</b>
                    </span>
                  </div>
                  <span v-else>
                    {{ championName(champions, playerView.player.championId) }} ·
                    {{ positionName(playerView.player) }}
                  </span>
                </div>
              </div>

              <div
                v-if="playerView.player.stats"
                class="profile-panel-line"
              >
                <div :class="['profile-main-row', profileClass(playerView.profile)]">
                  <strong>{{ playerProfileTierLabel(playerView.profile) }}</strong>
                  <b>{{ profileScoreText(playerView.profile) }}</b>
                </div>
                <div :class="['profile-stat-row', 'profile-overall-row', profileClass(playerView.profile)]">
                  <span>主玩 <b>{{ playerView.profile?.mainRoleLabel }}</b></span>
                  <span>carry率 <b>{{ statPercent(playerView.profile?.highlightRate) }}</b></span>
                  <span>战犯率 <b>{{ statPercent(playerView.profile?.disasterRate) }}</b></span>
                </div>
                <div :class="['profile-stat-row', 'profile-carry-row', carryProfileClass(playerView.profile)]">
                  <span>
                    输出能力
                    <b>
                      {{
                        abilityScoreText(
                          playerView.profile?.abilities.carry.averageScore || 0,
                          playerView.profile?.abilities.carry.games || 0,
                        )
                      }}
                    </b>
                  </span>
                  <span>
                    carry率
                    <b>{{ statPercent(playerView.profile?.abilities.carry.highlightRate) }}</b>
                  </span>
                  <span>
                    战犯率
                    <b>{{ statPercent(playerView.profile?.abilities.carry.disasterRate) }}</b>
                  </span>
                </div>
              </div>

              <div class="player-error" v-if="!playerView.player.stats">
                <strong>{{ playerView.player.isPlaceholder ? "占位玩家" : "读取失败" }}</strong>
                <span>{{ playerView.player.error || "暂无战绩数据" }}</span>
              </div>

              <div class="recent-strip" v-if="playerView.games.length">
                <div class="recent-header" aria-hidden="true">
                  <span></span>
                  <span>K/D/A</span>
                  <span>伤害</span>
                  <span>伤转</span>
                  <span>承伤</span>
                </div>
                <button
                  class="recent-game"
                  v-for="game in playerView.games"
                  :key="game.gameId"
                  :class="{ win: game.win, loss: !game.win }"
                  :title="`${championName(champions, game.championId)} · ${gameKda(game)}`"
                >
                  <ChampionAvatar :champion-id="game.championId" :champions="champions" :size="24" />
                  <b>{{ gameKda(game) }}</b>
                  <em>{{ statPercent(damageShare(game)) }}</em>
                  <em>{{ statFixed(damageConversion(game)) }}</em>
                  <em>{{ statPercent(mitigationShare(game)) }}</em>
                </button>
              </div>
              <div class="recent-empty" v-else-if="playerView.player.stats">暂无符合当前模式的近 50 局</div>
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
  min-height: calc(330px + var(--live-card-extra-height, 0px));
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

.locked-hero-line span {
  min-width: 0;
  color: inherit;
  font-size: 16px;
  font-weight: 950;
  line-height: 1;
  white-space: nowrap;
}

.identity {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 9px;
}

.identity-text {
  min-width: 0;
  width: 100%;
}

.name-line {
  display: flex;
  min-width: 0;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

.identity-text strong,
.player-name-button {
  display: block;
  overflow: hidden;
  min-width: 0;
  color: #233238;
  font-size: 15px;
  font-weight: 900;
  line-height: 1.15;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.player-name-button {
  flex: 1 1 auto;
  border: 0;
  background: transparent;
  color: #0f6f8f;
  cursor: pointer;
  padding: 0;
  text-align: left;
}

.player-name-button:hover,
.player-name-button:focus-visible {
  color: #0a526b;
  text-decoration: underline;
  text-underline-offset: 3px;
}

.player-name-button:focus-visible {
  outline: 2px solid rgba(15, 111, 143, 0.35);
  outline-offset: 2px;
}

.name-line > strong,
.name-line > .player-name-button {
  flex: 1 1 auto;
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
  background: linear-gradient(135deg, #fde68a, #f59e0b);
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

.kda-badge {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: baseline;
  gap: 4px;
  color: #1f2a2e;
  font-size: 15px;
  font-weight: 900;
}

.kda-badge span,
.kda-badge em {
  font-style: normal;
}

.summary-line {
  display: flex;
  flex-wrap: nowrap;
  gap: 6px;
  overflow: hidden;
  margin-top: 5px;
}

.summary-line span {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: baseline;
  gap: 2px;
  color: #1f2a2e;
  font-size: 14.4px;
  font-weight: 800;
  white-space: nowrap;
}

.summary-line i,
.summary-line b {
  font-style: normal;
}

.summary-line i {
  color: #1f2a2e;
}

.summary-line b {
  font-weight: 950;
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

.profile-carry-row {
  color: inherit;
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

.profile-carry-row b {
  color: #d22f2f;
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

.tone-good {
  color: #16844f;
}

.tone-warn {
  color: #c67900;
}

.tone-bad {
  color: #c9403d;
}

.identity-text > span {
  display: block;
  overflow: hidden;
  margin-top: 5px;
  color: #738188;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
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
  gap: 4px;
  min-width: 0;
  max-height: calc(185px + var(--live-card-extra-height, 0px));
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior-y: contain;
  margin-top: auto;
  padding: 1px 4px 1px 1px;
  scrollbar-width: thin;
}

.recent-header {
  position: sticky;
  top: 0;
  z-index: 1;
  display: grid;
  grid-template-columns: 24px 58px repeat(3, minmax(0, 1fr));
  align-items: center;
  gap: 4px;
  border-radius: 6px;
  color: #1f2a2e;
  background: rgba(255, 255, 255, 0.94);
  padding: 2px 6px;
  font-size: 10px;
  font-weight: 950;
}

.recent-header span {
  overflow: hidden;
  text-align: right;
  text-overflow: clip;
  white-space: nowrap;
}

.recent-header span:first-child {
  text-align: left;
}

.recent-game {
  display: grid;
  grid-template-columns: 24px 58px repeat(3, minmax(0, 1fr));
  align-items: center;
  gap: 4px;
  flex: 0 0 auto;
  width: 100%;
  border: 1px solid transparent;
  border-radius: 7px;
  padding: 5px 6px;
  color: #263238;
  font: inherit;
  text-align: left;
}

.recent-game.win {
  border-color: #b9d9f1;
  background: #edf7ff;
}

.recent-game.loss {
  border-color: #efc9c8;
  background: #fff1f1;
}

.recent-game b {
  overflow: visible;
  color: #202b31;
  font-size: 12px;
  font-weight: 900;
  text-overflow: clip;
  white-space: nowrap;
}

.recent-game em {
  overflow: hidden;
  color: #202b31;
  font-size: 11px;
  font-style: normal;
  font-weight: 800;
  text-align: right;
  text-overflow: clip;
  white-space: nowrap;
}

.recent-game:hover {
  border-color: #7dbdb2;
}

.recent-empty {
  display: grid;
  min-height: 120px;
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
