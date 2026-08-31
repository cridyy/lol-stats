import {
  calculateGlobalRating,
  calculateOfficialRating,
  type GlobalRating,
  type OutputRating,
} from "./scoring"
import { buildGameTitleSet, titleSelectionVersion, type GameTitleSet } from "./titles"
import type { RatingContext } from "./roleClassifier"
import type { GlobalChampionBaseline, RecentGame } from "./types"

export interface GamePresentation {
  rating: OutputRating
  globalRating: GlobalRating
  titleSet: GameTitleSet
}

interface PresentationCacheEntry {
  baselines?: ReadonlyMap<number, GlobalChampionBaseline>
  titleVersion: number
  value: GamePresentation
}

// 战绩列表会在同一帧内多次读取评分、称号和全服分；按对局和定位上下文缓存，
// 避免每个模板表达式都重新扫描同局参与者。刷新称号种子后通过 titleVersion 自动失效。
const presentationCache = new WeakMap<RecentGame, Map<RatingContext, PresentationCacheEntry>>()

export function gamePresentation(
  game: RecentGame,
  context: RatingContext = {},
  baselines?: ReadonlyMap<number, GlobalChampionBaseline>,
): GamePresentation {
  const titleVersion = titleSelectionVersion()
  let contextCache = presentationCache.get(game)
  if (!contextCache) {
    contextCache = new Map<RatingContext, PresentationCacheEntry>()
    presentationCache.set(game, contextCache)
  }

  const cached = contextCache.get(context)
  if (cached && cached.baselines === baselines && cached.titleVersion === titleVersion) {
    return cached.value
  }

  const rating = calculateOfficialRating(game, context, baselines)
  const globalRating = calculateGlobalRating(game, context, baselines)
  const titleSet = buildGameTitleSet(game, rating, {
    ratingContext: context,
    globalBaselines: baselines,
  })
  const value = { rating, globalRating, titleSet }
  contextCache.set(context, { baselines, titleVersion, value })
  return value
}
