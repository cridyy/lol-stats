<script setup lang="ts">
import type {
  ChampionSummaryItem,
  LiveGameResponse,
  LivePlayer,
  LiveTeam,
  RecentGame,
} from "../types"
import ChampionAvatar from "./ChampionAvatar.vue"
import { championName, fixed, percent, phaseName, riotId } from "../utils"

defineProps<{
  liveGame: LiveGameResponse | null
  champions: Record<number, ChampionSummaryItem>
  loading: boolean
  error: string
}>()

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

function recentGames(player: LivePlayer) {
  return player.stats?.recentGames.slice(0, 20) || []
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
  return ratio(game.damageSelfMitigated, game.teamDamageSelfMitigated)
}

function damageConversion(game: RecentGame) {
  const goldShare = ratio(game.goldEarned, game.teamGoldEarned)
  return goldShare > 0 ? damageShare(game) / goldShare : 0
}

function recentSummary(player: LivePlayer) {
  const games = recentGames(player)
  const wins = games.filter((game) => game.win).length
  const damage = games.reduce((sum, game) => sum + game.damageToChampions, 0)
  const teamDamage = games.reduce((sum, game) => sum + game.teamDamageToChampions, 0)
  const gold = games.reduce((sum, game) => sum + game.goldEarned, 0)
  const teamGold = games.reduce((sum, game) => sum + game.teamGoldEarned, 0)
  const mitigation = games.reduce((sum, game) => sum + game.damageSelfMitigated, 0)
  const teamMitigation = games.reduce((sum, game) => sum + game.teamDamageSelfMitigated, 0)
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

function teamSummary(team: LiveTeam) {
  const summaries = team.players
    .filter((player) => recentGames(player).length)
    .map((player) => recentSummary(player))

  const average = (selector: (summary: ReturnType<typeof recentSummary>) => number) => {
    if (!summaries.length) return 0
    return summaries.reduce((sum, summary) => sum + selector(summary), 0) / summaries.length
  }

  return {
    players: summaries.length,
    winRate: average((summary) => summary.winRate),
    damageShare: average((summary) => summary.damageShare),
    averageKda: average((summary) => summary.averageKda),
    damageConversion: average((summary) => summary.damageConversion),
    mitigationShare: average((summary) => summary.mitigationShare),
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
</script>

<template>
  <section class="live">
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

    <div class="live-empty" v-if="loading">正在读取双方近 20 局</div>
    <div class="live-empty error" v-else-if="error">{{ error }}</div>
    <div class="live-empty" v-else-if="!liveGame">当前没有可读取的对局</div>

    <template v-else-if="liveGame.teams.some((team) => team.players.length)">
      <div class="team-averages">
        <section
          class="team-average"
          v-for="(team, index) in liveGame.teams"
          :key="`average-${team.name}`"
        >
          <header>
            <strong>{{ teamAverageName(team, index, liveGame.teams) }}</strong>
            <span>{{ teamSummary(team).players }} 人样本</span>
          </header>
          <div class="average-metrics">
            <span>
              <i>平均胜率</i>
              <b>{{ statPercent(teamSummary(team).winRate) }}</b>
            </span>
            <span>
              <i>平均伤害</i>
              <b>{{ statPercent(teamSummary(team).damageShare) }}</b>
            </span>
            <span>
              <i>平均 KDA</i>
              <b>{{ statFixed(teamSummary(team).averageKda) }}</b>
            </span>
            <span>
              <i>平均伤转</i>
              <b>{{ statFixed(teamSummary(team).damageConversion) }}</b>
            </span>
            <span>
              <i>平均承伤</i>
              <b>{{ statPercent(teamSummary(team).mitigationShare) }}</b>
            </span>
          </div>
        </section>
      </div>

      <div class="teams">
        <section class="team" v-for="team in liveGame.teams" :key="team.name">
          <div class="team-title">
            <strong>{{ team.name }}</strong>
            <span>{{ team.players.length }} 人</span>
          </div>

          <div class="player-grid">
            <article
              class="player-card"
              v-for="player in team.players"
              :key="`${team.name}-${player.puuid}`"
            >
              <div class="identity">
                <ChampionAvatar
                  v-if="player.championId"
                  :champion-id="player.championId"
                  :champions="champions"
                  :size="42"
                />
                <div class="identity-text">
                  <div class="name-line">
                    <strong>{{ riotId(player.summoner) }}</strong>
                    <b v-if="player.stats" class="kda-badge">
                      <span>KDA</span>
                      <em :class="kdaTone(recentSummary(player).averageKda)">
                        {{ statFixed(recentSummary(player).averageKda) }}
                      </em>
                    </b>
                  </div>
                  <div class="summary-line" v-if="player.stats">
                    <span>
                      <i>胜率</i>
                      <b :class="winRateTone(recentSummary(player).winRate)">
                        {{ statPercent(recentSummary(player).winRate) }}
                      </b>
                    </span>
                    <span>
                      <i>伤转</i>
                      <b :class="damageConversionTone(recentSummary(player).damageConversion)">
                        {{ statFixed(recentSummary(player).damageConversion) }}
                      </b>
                    </span>
                    <span>
                      <i>承伤</i>
                      <b>{{ statPercent(recentSummary(player).mitigationShare) }}</b>
                    </span>
                  </div>
                  <span v-else>{{ championName(champions, player.championId) }} · {{ positionName(player) }}</span>
                </div>
              </div>

              <div class="player-error" v-if="!player.stats">
                <strong>{{ player.isPlaceholder ? "占位玩家" : "读取失败" }}</strong>
                <span>{{ player.error || "暂无战绩数据" }}</span>
              </div>

              <div class="recent-strip" v-if="recentGames(player).length">
                <div class="recent-header" aria-hidden="true">
                  <span></span>
                  <span>K/D/A</span>
                  <span>伤害</span>
                  <span>伤转</span>
                  <span>承伤</span>
                </div>
                <button
                  class="recent-game"
                  v-for="game in recentGames(player)"
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
              <div class="recent-empty" v-else-if="player.stats">暂无符合当前模式的近 20 局</div>
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
  grid-template-columns: repeat(5, minmax(0, 1fr));
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
  min-height: 298px;
  flex-direction: column;
  gap: 8px;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #ffffff;
  box-shadow: 0 10px 24px rgba(32, 67, 73, 0.06);
  padding: 9px;
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

.identity-text strong {
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
  max-height: 205px;
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
