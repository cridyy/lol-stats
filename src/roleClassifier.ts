import type { ChampionSummaryItem, GameAssetEntry, RecentGame } from "./types"

export type RoleKey = "adc" | "mage" | "assassin" | "fighter" | "tank" | "support"

export type PlayerRole =
  | RoleKey
  | "bruiser"
  | "pureAssassin"
  | "fighterAssassin"
  | "utilityCarry"
  | "unknown"

export type RoleVector = Record<RoleKey, number>

export interface RatingContext {
  items?: Record<number, GameAssetEntry>
  champions?: Record<number, ChampionSummaryItem>
}

export interface RoleAnalysis {
  role: PlayerRole
  label: string
  confidence: number
  itemWeightRatio: number
  championWeightRatio: number
  itemGold: number
  finalWeights: RoleVector
  itemWeights: RoleVector
  championWeights: RoleVector
  championRoles: string[]
}

const ROLE_LABELS: Record<PlayerRole, string> = {
  adc: "ADC",
  mage: "法师",
  assassin: "刺客",
  fighter: "战士",
  tank: "坦克",
  support: "辅助",
  bruiser: "半肉战士",
  pureAssassin: "纯刺客",
  fighterAssassin: "战士刺客",
  utilityCarry: "功能输出",
  unknown: "输出",
}

const EMPTY_VECTOR: RoleVector = {
  adc: 0,
  mage: 0,
  assassin: 0,
  fighter: 0,
  tank: 0,
  support: 0,
}

const ITEM_FIXES: Record<number, Partial<RoleVector>> = {
  // ADC / 普攻输出
  3004: { adc: 1 },
  3031: { adc: 1.4 },
  3032: { adc: 1.2 },
  3033: { adc: 1.2 },
  3036: { adc: 1.2 },
  3046: { adc: 1.2 },
  3085: { adc: 1.2 },
  3094: { adc: 1.1 },
  3124: { adc: 0.8, fighter: 0.4 },
  3139: { adc: 0.9 },
  3302: { adc: 0.8, fighter: 0.4 },
  6672: { adc: 1.3 },
  6673: { adc: 1.2 },
  6675: { adc: 1.2 },
  6676: { adc: 1.2 },
  6677: { adc: 1.2 },

  // 法师 / AP 爆发或持续输出
  2503: { mage: 1.3 },
  2522: { mage: 1 },
  3003: { mage: 1.1 },
  3089: { mage: 1.4 },
  3100: { mage: 1 },
  3102: { mage: 0.9 },
  3115: { mage: 0.8, adc: 0.3 },
  3116: { mage: 1 },
  3118: { mage: 1.2 },
  3135: { mage: 1.2 },
  3137: { mage: 1.2 },
  3152: { mage: 1, assassin: 0.2 },
  3157: { mage: 0.9 },
  3165: { mage: 1 },
  4628: { mage: 1 },
  4629: { mage: 1 },
  4633: { mage: 1 },
  4644: { mage: 1 },
  4645: { mage: 1.2 },
  4646: { mage: 1.1, assassin: 0.2 },
  6653: { mage: 1 },
  6655: { mage: 1.1 },
  6656: { mage: 1 },
  6657: { mage: 1 },
  8010: { mage: 1 },

  // 刺客 / 穿甲爆发
  2520: { assassin: 1.1 },
  3142: { assassin: 1.3 },
  3147: { assassin: 1.2 },
  3814: { assassin: 1.1 },
  6691: { assassin: 1.2 },
  6692: { assassin: 0.8, fighter: 0.4 },
  6693: { assassin: 1.1 },
  6694: { assassin: 1 },
  6695: { assassin: 1 },
  6696: { assassin: 1 },
  6697: { assassin: 1 },
  6701: { assassin: 1.1 },

  // 战士 / 半肉输出
  2501: { fighter: 0.9, tank: 0.4 },
  2517: { fighter: 1.1 },
  3053: { fighter: 0.9, tank: 0.3 },
  3071: { fighter: 1.2 },
  3072: { fighter: 0.8, adc: 0.3 },
  3073: { fighter: 1 },
  3074: { fighter: 1.1 },
  3078: { fighter: 1.3 },
  3153: { fighter: 0.8, adc: 0.4 },
  3156: { fighter: 0.8 },
  3161: { fighter: 1.1 },
  3181: { fighter: 0.9, tank: 0.2 },
  6333: { fighter: 1 },
  6610: { fighter: 1.1 },
  6631: { fighter: 1 },
  6632: { fighter: 1 },

  // 坦克 / 前排
  2502: { tank: 1.2 },
  2504: { tank: 1.2 },
  2524: { tank: 1, support: 0.2 },
  2525: { tank: 1 },
  3065: { tank: 1.2 },
  3068: { tank: 1.2 },
  3075: { tank: 1.3 },
  3083: { tank: 1.2 },
  3084: { tank: 1.2 },
  3109: { tank: 0.8, support: 0.4 },
  3110: { tank: 1.2 },
  3119: { tank: 0.9 },
  3143: { tank: 1.2 },
  3190: { tank: 0.9, support: 0.4 },
  3742: { tank: 1.1 },
  6660: { tank: 1 },
  6662: { tank: 1.1 },
  6664: { tank: 1.1 },
  6665: { tank: 1.1 },
  6667: { tank: 1.1 },
  8020: { tank: 1.1 },

  // 辅助 / 功能装
  2065: { support: 1.1 },
  3050: { support: 0.8, tank: 0.3 },
  3107: { support: 1.3 },
  3222: { support: 1.2 },
  3504: { support: 1.3 },
  4005: { support: 1.2 },
  6616: { support: 1.2 },
  6617: { support: 1.2 },
  6620: { support: 1.2 },
}

function cloneVector(): RoleVector {
  return { ...EMPTY_VECTOR }
}

function addRole(vector: RoleVector, role: RoleKey, value: number) {
  vector[role] += value
}

function addVector(target: RoleVector, source: Partial<RoleVector>, scale = 1) {
  for (const role of Object.keys(EMPTY_VECTOR) as RoleKey[]) {
    target[role] += (source[role] || 0) * scale
  }
}

function normalizeVector(vector: RoleVector): RoleVector {
  const total = vectorTotal(vector)
  if (total <= 0) return cloneVector()

  const result = cloneVector()
  for (const role of Object.keys(EMPTY_VECTOR) as RoleKey[]) {
    result[role] = vector[role] / total
  }
  return result
}

function vectorTotal(vector: RoleVector) {
  return Object.values(vector).reduce((sum, value) => sum + value, 0)
}

function hasAny(text: string, patterns: string[]) {
  return patterns.some((pattern) => text.includes(pattern))
}

function itemWeight(item: GameAssetEntry) {
  const price = item.priceTotal || item.price || 0
  if (price <= 0) return 0

  // 小件、鞋子、消耗品会稀释定位，成装才是“这局想怎么玩”的主要证据。
  if (price < 900) return price * 0.2
  if (price < 1800) return price * 0.45
  return Math.min(price, 3600)
}

function classifyItem(item: GameAssetEntry): RoleVector {
  const vector = cloneVector()
  const path = item.iconPath.toLowerCase()
  const description = item.description || ""
  const categories = new Set((item.categories || []).map((category) => category.toLowerCase()))

  if (path.includes("marksman") || path.includes("adc")) addRole(vector, "adc", 1.3)
  if (path.includes("mage")) addRole(vector, "mage", 1.3)
  if (path.includes("assassin")) addRole(vector, "assassin", 1.3)
  if (path.includes("fighter")) addRole(vector, "fighter", 1.3)
  if (path.includes("tank")) addRole(vector, "tank", 1.3)
  if (path.includes("enchanter") || path.includes("support")) addRole(vector, "support", 1.3)

  if (categories.has("criticalstrike")) addRole(vector, "adc", 0.75)
  if (categories.has("attackspeed")) addRole(vector, "adc", 0.45)
  if (categories.has("onhit")) {
    addRole(vector, "adc", 0.35)
    addRole(vector, "fighter", 0.2)
  }
  if (categories.has("damage")) {
    addRole(vector, "fighter", 0.35)
    addRole(vector, "assassin", 0.15)
  }
  if (categories.has("armorpenetration")) {
    addRole(vector, "assassin", 0.55)
    addRole(vector, "fighter", 0.25)
  }
  if (categories.has("spelldamage")) addRole(vector, "mage", 0.65)
  if (categories.has("magicpenetration")) {
    addRole(vector, "mage", 0.55)
    addRole(vector, "assassin", 0.15)
  }
  if (categories.has("health")) {
    addRole(vector, "tank", 0.35)
    addRole(vector, "fighter", 0.22)
  }
  if (categories.has("armor") || categories.has("spellblock") || categories.has("healthregen")) {
    addRole(vector, "tank", 0.45)
  }
  if (categories.has("manaregen") || categories.has("aura")) addRole(vector, "support", 0.5)
  if (categories.has("lifesteal") || categories.has("spellvamp") || categories.has("tenacity")) {
    addRole(vector, "fighter", 0.2)
  }

  if (description.includes("治疗和护盾强度") || description.includes("治疗和护盾")) {
    addRole(vector, "support", 1.4)
  }
  if (description.includes("基础法力回复")) addRole(vector, "support", 0.65)
  if (description.includes("法术强度")) addRole(vector, "mage", 0.45)
  if (description.includes("法术穿透") || description.includes("魔法穿透")) addRole(vector, "mage", 0.55)
  if (description.includes("攻击力")) addVector(vector, { fighter: 0.35, assassin: 0.25, adc: 0.2 })
  if (description.includes("攻击速度")) addRole(vector, "adc", 0.4)
  if (description.includes("暴击")) addRole(vector, "adc", 0.75)
  if (description.includes("穿甲")) addVector(vector, { assassin: 0.6, fighter: 0.25 })
  if (hasAny(description, ["生命值", "护甲", "魔法抗性"])) addVector(vector, { tank: 0.4, fighter: 0.15 })

  addVector(vector, ITEM_FIXES[item.id] || {})

  return normalizeVector(vector)
}

function championRoleVector(champion?: ChampionSummaryItem): RoleVector {
  const vector = cloneVector()
  const roles = champion?.roles || []

  roles.forEach((role, index) => {
    const weight = index === 0 ? 1.25 : 0.85
    switch (role.toLowerCase()) {
      case "marksman":
        addRole(vector, "adc", weight)
        break
      case "mage":
        addRole(vector, "mage", weight)
        break
      case "assassin":
        addRole(vector, "assassin", weight)
        break
      case "fighter":
        addRole(vector, "fighter", weight)
        break
      case "tank":
        addRole(vector, "tank", weight)
        break
      case "support":
        addRole(vector, "support", weight)
        break
    }
  })

  return normalizeVector(vector)
}

function itemRoleVector(game: RecentGame, items: Record<number, GameAssetEntry> | undefined) {
  const vector = cloneVector()
  let itemGold = 0

  if (!items) {
    return { vector, itemGold }
  }

  for (const itemId of game.itemIds || []) {
    const item = items[itemId]
    if (!item) continue

    const weight = itemWeight(item)
    if (weight <= 0) continue

    itemGold += weight
    addVector(vector, classifyItem(item), weight)
  }

  return { vector: normalizeVector(vector), itemGold }
}

function combineRoles(itemWeights: RoleVector, championWeights: RoleVector, itemGold: number) {
  const itemDominance = Math.max(...Object.values(itemWeights))
  const itemRatio = (() => {
    if (itemGold >= 4500) {
      return itemDominance >= 0.75 ? 0.78 : 0.65
    }

    if (itemGold >= 2200) {
      if (itemDominance >= 0.8) return 0.72
      if (itemDominance >= 0.65) return 0.62
      return 0.52
    }

    return itemDominance >= 0.85 ? 0.55 : 0.4
  })()
  const championRatio = 1 - itemRatio
  const finalWeights = cloneVector()

  addVector(finalWeights, itemWeights, itemRatio)
  addVector(finalWeights, championWeights, championRatio)

  return {
    itemWeightRatio: itemRatio,
    championWeightRatio: championRatio,
    finalWeights: normalizeVector(finalWeights),
  }
}

function resolveRole(weights: RoleVector): PlayerRole {
  const { adc, mage, assassin, fighter, tank, support } = weights
  const sorted = (Object.keys(weights) as RoleKey[]).sort((a, b) => weights[b] - weights[a])
  const top = sorted[0]

  if (fighter >= 0.3 && tank >= 0.24) return "bruiser"
  if (fighter >= 0.28 && assassin >= 0.24) return "fighterAssassin"
  if (assassin >= 0.46 && tank < 0.16 && fighter < 0.26) return "pureAssassin"
  if (support >= 0.38 && Math.max(adc, mage, assassin, fighter) < 0.42) return "support"
  if (support >= 0.28 && Math.max(adc, mage) >= 0.28) return "utilityCarry"
  if (tank >= 0.42 && fighter < 0.24) return "tank"
  if (top && weights[top] >= 0.22) return top

  return "unknown"
}

export function roleLabel(role: PlayerRole) {
  return ROLE_LABELS[role]
}

export function roleVectorText(vector: RoleVector, limit = 3) {
  return (Object.keys(vector) as RoleKey[])
    .sort((a, b) => vector[b] - vector[a])
    .slice(0, limit)
    .filter((role) => vector[role] > 0.01)
    .map((role) => `${roleLabel(role)} ${Math.round(vector[role] * 100)}%`)
    .join("，")
}

export function analyzePlayerRole(game: RecentGame, context: RatingContext = {}): RoleAnalysis {
  const champion = context.champions?.[game.championId]
  const championWeights = championRoleVector(champion)
  const { vector: itemWeights, itemGold } = itemRoleVector(game, context.items)
  const { finalWeights, itemWeightRatio, championWeightRatio } = combineRoles(
    itemWeights,
    championWeights,
    itemGold,
  )
  const role = resolveRole(finalWeights)
  const confidence = Math.max(...Object.values(finalWeights))

  return {
    role,
    label: roleLabel(role),
    confidence,
    itemWeightRatio,
    championWeightRatio,
    itemGold,
    finalWeights,
    itemWeights,
    championWeights,
    championRoles: champion?.roles || [],
  }
}
