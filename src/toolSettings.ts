export const AUTO_ACCEPT_ENABLED_KEY = "lol-stats.tools.auto-accept.enabled"
export const GAME_SETTINGS_LOCKED_KEY = "lol-stats.tools.game-settings.locked"
export const LIVE_HEX_WINRATE_RECOGNITION_ENABLED_KEY = "lol-stats.live.hex-winrate-recognition.enabled"
export const LIVE_HEX_WINRATE_CAPTURE_MODE_KEY = "lol-stats.live.hex-winrate-recognition.capture-mode"

export type LiveHexWinrateCaptureMode = "auto" | "wgc" | "desktop-dc"

export function readBooleanSetting(key: string, fallback = false) {
  const value = localStorage.getItem(key)
  if (value === null) return fallback
  return value === "1" || value === "true"
}

export function writeBooleanSetting(key: string, value: boolean) {
  localStorage.setItem(key, value ? "1" : "0")
}

export function readStringSetting(key: string, fallback = "") {
  return localStorage.getItem(key) ?? fallback
}

export function writeStringSetting(key: string, value: string) {
  localStorage.setItem(key, value)
}

export function readLiveHexWinrateCaptureMode(): LiveHexWinrateCaptureMode {
  const value = readStringSetting(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, "desktop-dc")
  if (value === "window-dc") {
    writeStringSetting(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, "desktop-dc")
    return "desktop-dc"
  }
  if (value === "auto" || value === "wgc" || value === "desktop-dc") return value
  return "desktop-dc"
}

export function writeLiveHexWinrateCaptureMode(mode: LiveHexWinrateCaptureMode) {
  writeStringSetting(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, mode)
}
