import { computed, readonly, shallowRef } from "vue"
import { loadGlobalBaselines } from "./api"
import type { GlobalBaselineDataset, GlobalChampionBaseline } from "./types"

const dataset = shallowRef<GlobalBaselineDataset | null>(null)
const loadError = shallowRef("")
let pendingLoad: Promise<boolean> | null = null

export const globalBaselineDataset = readonly(dataset)
export const globalBaselineLoadError = readonly(loadError)
export const globalBaselinesByChampionId = computed<ReadonlyMap<number, GlobalChampionBaseline>>(
  () => new Map((dataset.value?.champions || []).map((entry) => [entry.id, entry])),
)

export function initializeGlobalBaselines() {
  if (pendingLoad) return pendingLoad

  pendingLoad = loadGlobalBaselines()
    .then((result) => {
      dataset.value = result
      loadError.value = ""
      return true
    })
    .catch((error: unknown) => {
      loadError.value = error instanceof Error ? error.message : String(error)
      return false
    })
    .finally(() => {
      pendingLoad = null
    })

  return pendingLoad
}
