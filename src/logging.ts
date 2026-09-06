import { invoke as rawInvoke, isTauri, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core"
import type { App } from "vue"

export interface RuntimeLogStatus {
  directory: string
  detailedEnabled: boolean
  detailedUntilMs: number | null
  droppedLines: number
  writeError: string | null
}

interface DiagnosticExport {
  path: string
  revealed: boolean
}

type LogLevel = "debug" | "info" | "warn" | "error"
type RequestContext = { command?: string; requestId?: string; elapsedMs?: number }
type LogEntry = RequestContext & { level: LogLevel; source: string; message: string }

const MAX_QUEUE = 100
const BATCH_SIZE = 32
const MAX_MESSAGE_LENGTH = 4000
const SLOW_REQUEST_MS = 2000
const TRACKED_COMMANDS = new Set([
  "load_self_stats_with_progress",
  "refresh_self_recent_stats",
  "search_player_with_progress",
  "search_summoner_candidates",
  "load_match_detail",
  "load_match_detail_premades",
  "test_aramkit_connection",
])
const QUIET_COMMANDS = new Set([
  "load_lcu_asset", "load_lcu_assets", "load_gameflow_phase", "connection_status",
  "complete_local_api_request", "get_chat_status", "load_friends",
])

// 普通队列，不参与 Vue 响应式更新；只在有日志时批量发送。
let queue: LogEntry[] = []
let flushTimer: number | null = null
let flushPromise: Promise<void> | null = null
let droppedEntries = 0
let detailedUntilMs = 0
let requestCounter = 0
let installed = false
let lastTransportWarningAt = 0
const instanceId = Date.now().toString(36)
const recentErrors = new Map<string, { at: number; command?: string }>()

function scheduleFlush() {
  if (flushTimer !== null || flushPromise !== null || !queue.length) return
  flushTimer = window.setTimeout(() => {
    flushTimer = null
    void flushFrontendLogs()
  }, 1000)
}

export function logFrontend(
  level: LogLevel,
  source: string,
  message: string,
  context: RequestContext = {},
) {
  if (!isTauri() || (level === "debug" && Date.now() >= detailedUntilMs)) return
  if (queue.length >= MAX_QUEUE) {
    const disposable = queue.findIndex((entry) => entry.level === "debug" || entry.level === "info")
    if (disposable >= 0) queue.splice(disposable, 1)
    else if (level === "debug" || level === "info") {
      droppedEntries += 1
      return
    } else queue.shift()
    droppedEntries += 1
  }
  queue.push({ level, source, message: message.slice(0, MAX_MESSAGE_LENGTH), ...context })
  scheduleFlush()
}

async function sendLogBatch(entries: LogEntry[]) {
  let timer: number | undefined
  try {
    await Promise.race([
      rawInvoke<void>("write_frontend_logs", { entries }),
      new Promise<never>((_, reject) => {
        timer = window.setTimeout(() => reject(new Error("日志提交超时")), 3000)
      }),
    ])
  } finally {
    if (timer !== undefined) window.clearTimeout(timer)
  }
}

export async function flushFrontendLogs(): Promise<void> {
  while (flushPromise) await flushPromise
  if (flushTimer !== null) {
    window.clearTimeout(flushTimer)
    flushTimer = null
  }
  if (!queue.length) return
  const entries = queue
  queue = []
  if (droppedEntries > 0) {
    entries.unshift({
      level: "warn", source: "logging",
      message: `前端日志队列繁忙，已丢弃 ${droppedEntries} 条日志`,
    })
    droppedEntries = 0
  }
  const pending = (async () => {
    try {
      for (let offset = 0; offset < entries.length; offset += BATCH_SIZE) {
        // 日志提交必须走原始 invoke，不能被接口日志再次拦截。
        await sendLogBatch(entries.slice(offset, offset + BATCH_SIZE))
      }
    } catch {
      if (Date.now() - lastTransportWarningAt > 30_000) {
        lastTransportWarningAt = Date.now()
        console.warn("前端日志提交失败，当前批次已丢弃")
      }
    }
  })()
  flushPromise = pending
  await pending
  if (flushPromise === pending) flushPromise = null
  scheduleFlush()
}

function briefError(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === "string") return error
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") {
    return error.message
  }
  return "未提供错误详情"
}

function errorDetails(error: unknown): string {
  if (!(error instanceof Error)) return briefError(error)
  const parts = [error.stack?.split("\n").slice(0, 7).join("\n") || error.message]
  let current: unknown = error
  const visited = new Set<unknown>([error])
  for (let depth = 0; depth < 4; depth += 1) {
    if (!(current instanceof Error) || !("cause" in current) || !current.cause) break
    current = current.cause
    if (visited.has(current)) break
    visited.add(current)
    parts.push(`原因：${briefError(current)}`)
  }
  return parts.join("\n").slice(0, MAX_MESSAGE_LENGTH)
}

export function reportFrontendError(source: string, error: unknown, context: RequestContext = {}) {
  if (!isTauri()) return
  const key = briefError(error).slice(0, MAX_MESSAGE_LENGTH)
  const previous = recentErrors.get(key)
  if (previous && Date.now() - previous.at < 1500
    && (!context.command || context.command === previous.command)) return
  if (recentErrors.size >= 128) recentErrors.clear()
  recentErrors.set(key, { at: Date.now(), command: context.command })
  logFrontend("error", source, errorDetails(error), context)
}

export async function invokeLogged<T>(command: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  const startedAt = performance.now()
  const requestId = `${instanceId}-${++requestCounter}`
  const tracked = TRACKED_COMMANDS.has(command)
  if (tracked) logFrontend("info", "ipc", "调用开始", { command, requestId })
  try {
    const result = await rawInvoke<T>(command, args, options)
    const elapsedMs = Math.round(performance.now() - startedAt)
    if (elapsedMs >= SLOW_REQUEST_MS) {
      logFrontend("warn", "ipc", "调用完成，耗时偏长", { command, requestId, elapsedMs })
    } else if (tracked || !QUIET_COMMANDS.has(command)) {
      logFrontend(tracked ? "info" : "debug", "ipc", "调用完成", { command, requestId, elapsedMs })
    }
    return result
  } catch (error) {
    const context = { command, requestId, elapsedMs: Math.round(performance.now() - startedAt) }
    if (briefError(error).includes("已取消")) {
      logFrontend("info", "ipc", "调用已取消", context)
    } else {
      reportFrontendError("ipc", error, context)
    }
    throw error
  }
}

export function installFrontendLogging(app: App) {
  if (installed) return
  installed = true
  const previousHandler = app.config.errorHandler
  app.config.errorHandler = (error, instance, info) => {
    reportFrontendError("vue", error)
    logFrontend("debug", "vue", `异常阶段：${info}`)
    previousHandler?.(error, instance, info)
    console.error(error)
  }
  window.addEventListener("error", (event) => {
    reportFrontendError("javascript", event.error || event.message)
  })
  window.addEventListener("unhandledrejection", (event) => {
    reportFrontendError("promise", event.reason)
  })
  window.addEventListener("pagehide", () => { void flushFrontendLogs() })
  logFrontend("info", "app", "前端启动")
}

export async function getRuntimeLogStatus() {
  const status = await rawInvoke<RuntimeLogStatus>("runtime_log_status")
  detailedUntilMs = status.detailedUntilMs ?? 0
  return status
}

export async function setDetailedLogging(enabled: boolean) {
  const status = await rawInvoke<RuntimeLogStatus>("set_detailed_logging", { enabled })
  detailedUntilMs = status.detailedUntilMs ?? 0
  return status
}

export function openLogDirectory() {
  return rawInvoke<void>("open_log_directory")
}

export async function exportDiagnosticLogs() {
  await flushFrontendLogs()
  return rawInvoke<DiagnosticExport>("export_diagnostic_logs")
}
