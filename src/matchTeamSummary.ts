import type { MatchDetailTeam } from "./types"

export type MatchTeamSummary = {
  kills: number
  deaths: number
  assists: number
  goldEarned: number
  damageToChampions: number
  towerKills: number
}

export function matchTeamSummary(team: MatchDetailTeam): MatchTeamSummary {
  return team.players.reduce<MatchTeamSummary>(
    (summary, player) => ({
      kills: summary.kills + Number(player.kills || 0),
      deaths: summary.deaths + Number(player.deaths || 0),
      assists: summary.assists + Number(player.assists || 0),
      goldEarned: summary.goldEarned + Number(player.goldEarned || 0),
      damageToChampions: summary.damageToChampions + Number(player.damageToChampions || 0),
      towerKills: summary.towerKills,
    }),
    {
      kills: 0,
      deaths: 0,
      assists: 0,
      goldEarned: 0,
      damageToChampions: 0,
      towerKills: Number(team.towerKills || 0),
    },
  )
}
