import type { ChampionSummaryItem, SummonerInfo } from "./types"

export function riotId(summoner?: SummonerInfo) {
  if (!summoner) return "未知玩家"
  if (summoner.gameName && summoner.tagLine) return `${summoner.gameName}#${summoner.tagLine}`
  return summoner.displayName || summoner.internalName || "未知玩家"
}

export function percent(value: number) {
  return `${Math.round(value * 100)}%`
}

export function fixed(value: number, digits = 2) {
  return Number.isFinite(value) ? value.toFixed(digits) : "0.00"
}

export function compactNumber(value: number) {
  return new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 0 }).format(value)
}

export function formatDate(timestamp: number) {
  if (!timestamp) return "-"
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp))
}

export function championName(
  champions: Record<number, ChampionSummaryItem>,
  championId?: number,
) {
  if (!championId) return "未选择"
  const champion = champions[championId]
  return champion?.name || champion?.alias || `英雄 ${championId}`
}

export function phaseName(phase?: string) {
  const map: Record<string, string> = {
    None: "空闲",
    Lobby: "房间",
    Matchmaking: "匹配中",
    ReadyCheck: "等待接受",
    ChampSelect: "选人中",
    GameStart: "游戏开始",
    InProgress: "游戏中",
    Reconnect: "可重连",
    WaitingForStats: "等待结算",
    PreEndOfGame: "结算中",
    EndOfGame: "游戏结束",
  }
  return phase ? map[phase] || phase : "-"
}
