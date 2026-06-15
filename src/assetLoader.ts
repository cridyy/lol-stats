import { loadLcuAssets } from "./api"

type Waiter = {
  resolve: (value: string) => void
  reject: (reason?: unknown) => void
}

const cache = new Map<string, string>()
const waiters = new Map<string, Waiter[]>()
let queuedPaths = new Set<string>()
let flushTimer: number | null = null

export function loadAssetDataUrl(path: string) {
  const cached = cache.get(path)
  if (cached) return Promise.resolve(cached)

  return new Promise<string>((resolve, reject) => {
    const existingWaiters = waiters.get(path)
    if (existingWaiters) {
      existingWaiters.push({ resolve, reject })
      return
    }

    waiters.set(path, [{ resolve, reject }])
    queuedPaths.add(path)
    scheduleFlush()
  })
}

function scheduleFlush() {
  if (flushTimer !== null) return
  flushTimer = window.setTimeout(flushAssetQueue, 0)
}

async function flushAssetQueue() {
  flushTimer = null
  const paths = Array.from(queuedPaths)
  queuedPaths = new Set()
  if (paths.length === 0) return

  try {
    const result = await loadLcuAssets(paths)
    for (const path of paths) {
      const dataUrl = result[path]
      const pathWaiters = waiters.get(path) || []
      waiters.delete(path)

      if (!dataUrl) {
        pathWaiters.forEach(({ reject }) => reject(new Error(`asset not found: ${path}`)))
        continue
      }

      cache.set(path, dataUrl)
      pathWaiters.forEach(({ resolve }) => resolve(dataUrl))
    }
  } catch (error) {
    for (const path of paths) {
      const pathWaiters = waiters.get(path) || []
      waiters.delete(path)
      pathWaiters.forEach(({ reject }) => reject(error))
    }
  }
}
