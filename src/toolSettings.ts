import type { AramkitProxySettings } from "./types"

export const AUTO_ACCEPT_ENABLED_KEY = "lol-stats.tools.auto-accept.enabled"
export const GAME_SETTINGS_LOCKED_KEY = "lol-stats.tools.game-settings.locked"
export const QUICK_BENCH_SWAP_ENABLED_KEY = "lol-stats.tools.quick-bench-swap.enabled"
export const DARK_MODE_ENABLED_KEY = "lol-stats.ui.dark-mode.enabled"
export const LIVE_HEX_WINRATE_RECOGNITION_ENABLED_KEY = "lol-stats.live.hex-winrate-recognition.enabled"
export const LIVE_HEX_WINRATE_CAPTURE_MODE_KEY = "lol-stats.live.hex-winrate-recognition.capture-mode"
export const LIVE_HEX_WINRATE_DETECTION_MODE_KEY = "lol-stats.live.hex-winrate-recognition.detection-mode"
export const LIVE_HEX_WINRATE_RECOGNITION_TEXT_VISIBLE_KEY = "lol-stats.live.hex-winrate-recognition.show-result"
export const ARAMKIT_PROXY_SETTINGS_KEY = "lol-stats.aramkit.proxy"
export const DEFAULT_ARAMKIT_PROXY_ADDRESS = "http://127.0.0.1:7890"
export const DEFAULT_ARAMKIT_TIMEOUT_SECONDS = 20

export type LiveHexWinrateCaptureMode = "auto" | "wgc" | "dxgi" | "desktop-dc"
export type LiveHexWinrateDetectionMode = "high-precision" | "compatibility"

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
  const value = readStringSetting(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, "dxgi")
  if (value === "window-dc") {
    writeStringSetting(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, "desktop-dc")
    return "desktop-dc"
  }
  if (value === "auto" || value === "wgc" || value === "dxgi" || value === "desktop-dc") return value
  return "dxgi"
}

export function writeLiveHexWinrateCaptureMode(mode: LiveHexWinrateCaptureMode) {
  writeStringSetting(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, mode)
}

export function readLiveHexWinrateDetectionMode(): LiveHexWinrateDetectionMode {
  const value = readStringSetting(LIVE_HEX_WINRATE_DETECTION_MODE_KEY, "high-precision")
  return value === "compatibility" ? "compatibility" : "high-precision"
}

export function writeLiveHexWinrateDetectionMode(mode: LiveHexWinrateDetectionMode) {
  writeStringSetting(LIVE_HEX_WINRATE_DETECTION_MODE_KEY, mode)
}

export function readAramkitProxySettings(): AramkitProxySettings {
  const fallback: AramkitProxySettings = {
    enabled: false,
    address: DEFAULT_ARAMKIT_PROXY_ADDRESS,
    wtlsEnabled: false,
    timeoutSeconds: DEFAULT_ARAMKIT_TIMEOUT_SECONDS,
  }
  try {
    const value: unknown = JSON.parse(localStorage.getItem(ARAMKIT_PROXY_SETTINGS_KEY) || "null")
    if (!value || typeof value !== "object") return fallback
    const settings = value as Partial<AramkitProxySettings>
    return {
      enabled: settings.enabled === true,
      address: typeof settings.address === "string" ? settings.address.trim() : fallback.address,
      wtlsEnabled: settings.wtlsEnabled === true,
      timeoutSeconds: isAramkitTimeoutSeconds(settings.timeoutSeconds)
        ? settings.timeoutSeconds
        : fallback.timeoutSeconds,
    }
  } catch {
    return fallback
  }
}

export function isAramkitTimeoutSeconds(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 120
}

export function writeAramkitProxySettings(settings: AramkitProxySettings) {
  localStorage.setItem(ARAMKIT_PROXY_SETTINGS_KEY, JSON.stringify(settings))
}
