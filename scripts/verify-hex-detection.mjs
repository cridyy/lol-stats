import assert from "node:assert/strict"
import { beforeEach, test } from "node:test"
import { fileURLToPath } from "node:url"
import { build } from "esbuild"

const bundle = await build({
  entryPoints: [fileURLToPath(new URL("../src/toolSettings.ts", import.meta.url))],
  bundle: true,
  platform: "node",
  format: "esm",
  write: false,
})
const {
  LIVE_HEX_WINRATE_CAPTURE_MODE_KEY,
  LIVE_HEX_WINRATE_DETECTION_MODE_KEY,
  LIVE_HEX_WINRATE_RECOGNITION_ENABLED_KEY,
  readLiveHexWinrateDetectionMode,
  writeLiveHexWinrateDetectionMode,
} = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`)

let values
beforeEach(() => {
  values = new Map()
  globalThis.localStorage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value) },
  }
})

test("a new installation or upgrade keeps high precision as the default", () => {
  assert.equal(readLiveHexWinrateDetectionMode(), "high-precision")
  assert.equal(values.size, 0)
})

test("both modes survive subsequent reads", () => {
  for (const mode of ["compatibility", "high-precision"]) {
    writeLiveHexWinrateDetectionMode(mode)
    assert.equal(values.get(LIVE_HEX_WINRATE_DETECTION_MODE_KEY), mode)
    assert.equal(readLiveHexWinrateDetectionMode(), mode)
  }
})

test("unknown saved modes do not enable compatibility or an automatic mode", () => {
  for (const mode of ["auto", "calibrate", "", "null", "{broken", "1"]) {
    values.set(LIVE_HEX_WINRATE_DETECTION_MODE_KEY, mode)
    assert.equal(readLiveHexWinrateDetectionMode(), "high-precision")
  }
})

test("detection settings do not change the capture source or recognition toggle", () => {
  values.set(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY, "wgc")
  values.set(LIVE_HEX_WINRATE_RECOGNITION_ENABLED_KEY, "0")
  writeLiveHexWinrateDetectionMode("compatibility")
  assert.equal(values.get(LIVE_HEX_WINRATE_CAPTURE_MODE_KEY), "wgc")
  assert.equal(values.get(LIVE_HEX_WINRATE_RECOGNITION_ENABLED_KEY), "0")
})
