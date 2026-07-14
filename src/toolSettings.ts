export const AUTO_ACCEPT_ENABLED_KEY = "lol-stats.tools.auto-accept.enabled"
export const GAME_SETTINGS_LOCKED_KEY = "lol-stats.tools.game-settings.locked"

export function readBooleanSetting(key: string, fallback = false) {
  const value = localStorage.getItem(key)
  if (value === null) return fallback
  return value === "1" || value === "true"
}

export function writeBooleanSetting(key: string, value: boolean) {
  localStorage.setItem(key, value ? "1" : "0")
}
