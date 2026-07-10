import largeBronze from "./assets/ranked-icons-large/bronze.png"
import largeChallenger from "./assets/ranked-icons-large/challenger.png"
import largeDiamond from "./assets/ranked-icons-large/diamond.png"
import largeEmerald from "./assets/ranked-icons-large/emerald.png"
import largeGold from "./assets/ranked-icons-large/gold.png"
import largeGrandmaster from "./assets/ranked-icons-large/grandmaster.png"
import largeIron from "./assets/ranked-icons-large/iron.png"
import largeMaster from "./assets/ranked-icons-large/master.png"
import largePlatinum from "./assets/ranked-icons-large/platinum.png"
import largeSilver from "./assets/ranked-icons-large/silver.png"
import largeUnranked from "./assets/ranked-icons-large/unranked.png"

import medalBronze from "./assets/ranked-icons/bronze.png"
import medalChallenger from "./assets/ranked-icons/challenger.png"
import medalDiamond from "./assets/ranked-icons/diamond.png"
import medalEmerald from "./assets/ranked-icons/emerald.png"
import medalGold from "./assets/ranked-icons/gold.png"
import medalGrandmaster from "./assets/ranked-icons/grandmaster.png"
import medalIron from "./assets/ranked-icons/iron.png"
import medalMaster from "./assets/ranked-icons/master.png"
import medalPlatinum from "./assets/ranked-icons/platinum.png"
import medalSilver from "./assets/ranked-icons/silver.png"

const LARGE_RANK_ICONS: Record<string, string> = {
  IRON: largeIron,
  BRONZE: largeBronze,
  SILVER: largeSilver,
  GOLD: largeGold,
  PLATINUM: largePlatinum,
  EMERALD: largeEmerald,
  DIAMOND: largeDiamond,
  MASTER: largeMaster,
  GRANDMASTER: largeGrandmaster,
  CHALLENGER: largeChallenger,
  UNRANKED: largeUnranked,
}

const MEDAL_RANK_ICONS: Record<string, string> = {
  IRON: medalIron,
  BRONZE: medalBronze,
  SILVER: medalSilver,
  GOLD: medalGold,
  PLATINUM: medalPlatinum,
  EMERALD: medalEmerald,
  DIAMOND: medalDiamond,
  MASTER: medalMaster,
  GRANDMASTER: medalGrandmaster,
  CHALLENGER: medalChallenger,
}

function normalizeTier(tier?: string) {
  const normalized = tier?.trim().toUpperCase() || ""
  return normalized && normalized !== "NA" && normalized !== "NONE" ? normalized : "UNRANKED"
}

export function rankIconLarge(tier?: string) {
  return LARGE_RANK_ICONS[normalizeTier(tier)] || largeUnranked
}

export function rankIconMedal(tier?: string) {
  return MEDAL_RANK_ICONS[normalizeTier(tier)] || largeUnranked
}
