import { spawn } from "node:child_process"
import path from "node:path"
import { fileURLToPath } from "node:url"

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const rootDir = path.resolve(__dirname, "..")
const children = []

function pipeWithPrefix(stream, prefix, writer) {
  let buffer = ""
  stream.on("data", (chunk) => {
    buffer += chunk.toString()
    const lines = buffer.split(/\r?\n/)
    buffer = lines.pop() ?? ""
    for (const line of lines) {
      writer.write(`[${prefix}] ${line}\n`)
    }
  })
  stream.on("end", () => {
    if (buffer) {
      writer.write(`[${prefix}] ${buffer}\n`)
    }
  })
}

function killChildren() {
  for (const child of children) {
    if (!child.killed) {
      child.kill()
    }
  }
}

function spawnNamedProcess(name, command) {
  const child = spawn(command, {
    cwd: rootDir,
    stdio: ["inherit", "pipe", "pipe"],
    shell: true,
  })
  children.push(child)

  pipeWithPrefix(child.stdout, name, process.stdout)
  pipeWithPrefix(child.stderr, name, process.stderr)

  child.on("exit", (code, signal) => {
    if (signal === "SIGINT" || signal === "SIGTERM") {
      return
    }

    if ((code ?? 0) !== 0) {
      console.error(`[${name}] 已退出，退出码 ${code ?? "unknown"}`)
      killChildren()
      process.exit(code ?? 1)
    }
  })

  child.on("error", (error) => {
    console.error(`[${name}] 启动失败：${error.message}`)
    killChildren()
    process.exit(1)
  })
}

process.on("SIGINT", () => {
  killChildren()
  process.exit(0)
})

process.on("SIGTERM", () => {
  killChildren()
  process.exit(0)
})

spawnNamedProcess("tauri", "npm run tauri dev")
spawnNamedProcess("verify", "npm run verify:watch")
