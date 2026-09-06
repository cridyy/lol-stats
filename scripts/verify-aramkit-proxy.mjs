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
  ARAMKIT_PROXY_SETTINGS_KEY,
  DEFAULT_ARAMKIT_PROXY_ADDRESS,
  DEFAULT_ARAMKIT_TIMEOUT_SECONDS,
  isAramkitTimeoutSeconds,
  readAramkitProxySettings,
  writeAramkitProxySettings,
} = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}`)

let values
beforeEach(() => {
  values = new Map()
  globalThis.localStorage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value) },
  }
})

test("a fresh installation keeps the local proxy disabled", () => {
  assert.deepEqual(readAramkitProxySettings(), {
    enabled: false,
    address: DEFAULT_ARAMKIT_PROXY_ADDRESS,
    wtlsEnabled: false,
    timeoutSeconds: 20,
  })
  assert.equal(values.size, 0)
})

test("saved proxy configuration survives a new read without touching other settings", () => {
  values.set("lol-stats.ui.dark-mode.enabled", "1")
  const settings = {
    enabled: true, address: "http://127.0.0.1:10809", wtlsEnabled: true, timeoutSeconds: 35,
  }
  writeAramkitProxySettings(settings)
  assert.deepEqual(readAramkitProxySettings(), settings)
  assert.equal(values.get("lol-stats.ui.dark-mode.enabled"), "1")
})

test("turning the proxy off retains its configured address", () => {
  const settings = {
    enabled: false, address: "http://localhost:7897", wtlsEnabled: false, timeoutSeconds: 45,
  }
  writeAramkitProxySettings(settings)
  assert.deepEqual(readAramkitProxySettings(), settings)
})

test("malformed local data does not prevent startup or enable the proxy", () => {
  for (const value of ["{broken", "null", "123", "[]", "{}", '{"enabled":"true"}']) {
    values.set(ARAMKIT_PROXY_SETTINGS_KEY, value)
    assert.deepEqual(readAramkitProxySettings(), {
      enabled: false,
      address: DEFAULT_ARAMKIT_PROXY_ADDRESS,
      wtlsEnabled: false,
      timeoutSeconds: 20,
    })
  }
})

test("the saved address is trimmed when restored", () => {
  writeAramkitProxySettings({ enabled: true, address: " http://127.0.0.1:7890 " })
  assert.equal(readAramkitProxySettings().address, DEFAULT_ARAMKIT_PROXY_ADDRESS)
})

test("an unavailable local store falls back safely and reports write failures", () => {
  globalThis.localStorage = {
    getItem() { throw new Error("storage unavailable") },
    setItem() { throw new Error("storage full") },
  }
  assert.equal(readAramkitProxySettings().enabled, false)
  assert.throws(() => writeAramkitProxySettings({ enabled: false, address: "" }), /storage full/)
})

test("older proxy settings gain defaults without changing their saved port", () => {
  values.set(ARAMKIT_PROXY_SETTINGS_KEY, JSON.stringify({
    enabled: true, address: "http://127.0.0.1:7897",
  }))
  assert.deepEqual(readAramkitProxySettings(), {
    enabled: true,
    address: "http://127.0.0.1:7897",
    wtlsEnabled: false,
    timeoutSeconds: DEFAULT_ARAMKIT_TIMEOUT_SECONDS,
  })
})

test("invalid timeouts fall back to 20 seconds and cannot enable wtls", () => {
  for (const timeoutSeconds of [0, -1, 121, 2.5, "30", null]) {
    values.set(ARAMKIT_PROXY_SETTINGS_KEY, JSON.stringify({
      enabled: true, address: "http://127.0.0.1:7897", wtlsEnabled: "true", timeoutSeconds,
    }))
    const settings = readAramkitProxySettings()
    assert.equal(settings.wtlsEnabled, false)
    assert.equal(settings.timeoutSeconds, 20)
    assert.equal(settings.address, "http://127.0.0.1:7897")
  }
})

test("timeout validation rejects empty, fractional, infinite, and out of range values", () => {
  for (const value of ["", "20", undefined, null, NaN, Infinity, -1, 0, 1.5, 121]) {
    assert.equal(isAramkitTimeoutSeconds(value), false)
  }
  for (const value of [1, 20, 45, 120]) {
    assert.equal(isAramkitTimeoutSeconds(value), true)
  }
})
