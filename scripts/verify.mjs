import { spawn } from "node:child_process"
import { watch } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const rootDir = path.resolve(__dirname, "..")
const watchMode = process.argv.includes("--watch")
const watchTargets = [
  path.join(rootDir, "src"),
  path.join(rootDir, "src-tauri", "src"),
  path.join(rootDir, "scripts"),
  path.join(rootDir, "package.json"),
  path.join(rootDir, "tsconfig.json"),
  path.join(rootDir, "vite.config.ts"),
  path.join(rootDir, "src-tauri", "Cargo.toml"),
  path.join(rootDir, "src-tauri", "tauri.conf.json"),
]

const fullSteps = [
  { label: "前端全量构建", command: "npm run build" },
  { label: "Rust 编译检查", command: "npm run check:backend" },
  { label: "Rust 单元测试", command: "npm run test:backend" },
]

const watchSteps = [
  { label: "前端类型检查", command: "npm run check:frontend" },
  { label: "Rust 编译检查", command: "npm run check:backend" },
  { label: "Rust 单元测试", command: "npm run test:backend" },
]

let running = false
let rerunRequested = false
let rerunTimer = null

function log(message) {
  console.log(`[verify] ${message}`)
}

function runStep(step) {
  return new Promise((resolve, reject) => {
    log(`开始 ${step.label}`)
    const child = spawn(step.command, {
      cwd: rootDir,
      stdio: "inherit",
      shell: true,
    })

    child.on("exit", (code) => {
      if (code === 0) {
        resolve()
        return
      }
      reject(new Error(`${step.label} 失败，退出码 ${code ?? "unknown"}`))
    })

    child.on("error", reject)
  })
}

async function runPipeline(reason = "手动触发") {
  if (running) {
    rerunRequested = true
    return
  }

  running = true
  const startedAt = Date.now()
  const steps = watchMode ? watchSteps : fullSteps
  log(`${reason}，开始执行${watchMode ? "快速校验" : "全量校验"}`)

  try {
    for (const step of steps) {
      await runStep(step)
    }
    const elapsedSeconds = ((Date.now() - startedAt) / 1000).toFixed(1)
    log(`全部通过，用时 ${elapsedSeconds}s`)
  } catch (error) {
    const elapsedSeconds = ((Date.now() - startedAt) / 1000).toFixed(1)
    log(`执行失败，用时 ${elapsedSeconds}s`)
    console.error(error instanceof Error ? error.message : error)
    if (!watchMode) {
      process.exitCode = 1
    }
  } finally {
    running = false
    if (rerunRequested) {
      rerunRequested = false
      void runPipeline("收到新的文件变更，重新执行")
    }
  }
}

function scheduleRerun(filePath = "") {
  const reason = filePath ? `检测到变更：${path.relative(rootDir, filePath)}` : "检测到文件变更"
  if (rerunTimer) {
    clearTimeout(rerunTimer)
  }
  rerunTimer = setTimeout(() => {
    rerunTimer = null
    if (running) {
      rerunRequested = true
      log(`${reason}，当前仍在执行，稍后自动重跑`)
      return
    }
    void runPipeline(reason)
  }, 250)
}

function startWatchers() {
  for (const target of watchTargets) {
    watch(
      target,
      { recursive: true },
      (_eventType, fileName) => {
        const changedPath =
          typeof fileName === "string" && fileName.length > 0
            ? path.join(target, fileName)
            : target
        scheduleRerun(changedPath)
      },
    )
  }
  log("监听已启动，保存文件后会自动重新校验")
}

if (watchMode) {
  startWatchers()
}

await runPipeline(watchMode ? "启动监听模式" : "启动全量校验")
