import { ref } from "vue"
import {
  readBooleanSetting,
  readStringSetting,
  writeBooleanSetting,
  writeStringSetting,
} from "./toolSettings"

export const CHAT_STATUS_LOCK_ENABLED_KEY = "lol-stats.tools.chat-status-lock.enabled"
export const CHAT_STATUS_LOCK_AVAILABILITY_KEY = "lol-stats.tools.chat-status-lock.availability"

export const CHAT_AVAILABILITIES = [
  "chat",
  "mobile",
  "away",
  "offline",
  "dnd",
  "spectating",
  "online",
] as const

export type ChatAvailability = (typeof CHAT_AVAILABILITIES)[number]

const DEFAULT_CHAT_AVAILABILITY: ChatAvailability = "chat"
const LOCKABLE_CHAT_AVAILABILITIES = new Set<ChatAvailability>([
  "chat",
  "mobile",
  "away",
  "offline",
  "online",
])

/**
 * 这两个 ref 是进程内共享的设置源。根组件负责同步，快捷工具只负责修改预设，
 * 因此切换工具页或导航页不会停掉状态锁定。
 */
export const chatStatusLockEnabled = ref(readBooleanSetting(CHAT_STATUS_LOCK_ENABLED_KEY))
export const chatStatusLockAvailability = ref<ChatAvailability>(
  normalizeLockableChatAvailability(readStringSetting(CHAT_STATUS_LOCK_AVAILABILITY_KEY)),
)

export function isLockableChatAvailability(value: string): value is ChatAvailability {
  return LOCKABLE_CHAT_AVAILABILITIES.has(
    value.trim().toLocaleLowerCase("en-US") as ChatAvailability,
  )
}

export function setChatStatusLockEnabled(enabled: boolean) {
  chatStatusLockEnabled.value = enabled
  writeBooleanSetting(CHAT_STATUS_LOCK_ENABLED_KEY, enabled)
}

export function setChatStatusLockAvailability(availability: string) {
  const normalized = normalizeLockableChatAvailability(availability)
  chatStatusLockAvailability.value = normalized
  writeStringSetting(CHAT_STATUS_LOCK_AVAILABILITY_KEY, normalized)
}

function normalizeLockableChatAvailability(value: string): ChatAvailability {
  const normalized = normalizeChatAvailability(value)
  return isLockableChatAvailability(normalized) ? normalized : DEFAULT_CHAT_AVAILABILITY
}

function normalizeChatAvailability(value: string): ChatAvailability {
  const normalized = value.trim().toLocaleLowerCase("en-US")
  return (CHAT_AVAILABILITIES as readonly string[]).includes(normalized)
    ? (normalized as ChatAvailability)
    : DEFAULT_CHAT_AVAILABILITY
}
