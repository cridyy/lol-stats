<script setup lang="ts">
import { computed, inject, ref, watch } from "vue"
import { Check, LogOut, MessageSquareText, RefreshCw, ShieldCheck } from "lucide-vue-next"
import {
  dismissEndOfGame,
  getChatStatus,
  getGameSettingsLocked,
  setChatAvailability,
  setChatStatusMessage,
  setGameSettingsLocked,
  setQuickBenchSwapEnabled,
} from "../api"
import {
  chatStatusLockAvailability,
  chatStatusLockEnabled,
  isLockableChatAvailability,
  setChatStatusLockAvailability,
  setChatStatusLockEnabled,
} from "../chatStatusLock"
import { notifyKey } from "../notifications"
import {
  AUTO_ACCEPT_ENABLED_KEY,
  GAME_SETTINGS_LOCKED_KEY,
  QUICK_BENCH_SWAP_ENABLED_KEY,
  readBooleanSetting,
  writeBooleanSetting,
} from "../toolSettings"

const props = defineProps<{ active: boolean }>()

const notify = inject(notifyKey, () => 0)
const autoAcceptEnabled = ref(readBooleanSetting(AUTO_ACCEPT_ENABLED_KEY))
const settingsLocked = ref(readBooleanSetting(GAME_SETTINGS_LOCKED_KEY))
const quickBenchSwapEnabled = ref(readBooleanSetting(QUICK_BENCH_SWAP_ENABLED_KEY))
const settingsLockBusy = ref(false)
const quickBenchSwapBusy = ref(false)
const exitBusy = ref(false)
const chatLoading = ref(false)
const availabilitySaving = ref(false)
const signatureSaving = ref(false)
const chatAvailability = ref("chat")
const chatSignature = ref("")
const loadedOnce = ref(false)
const availabilityOptions = [
  { value: "chat", label: "聊天" },
  { value: "mobile", label: "在线分组" },
  { value: "away", label: "离开" },
  { value: "offline", label: "离线" },
  { value: "dnd", label: "游戏中" },
  { value: "spectating", label: "观战中" },
  { value: "online", label: "在线" },
]

const chatSignatureCount = computed(() => Array.from(chatSignature.value).length)
const chatStatusLockSummary = computed(() => {
  if (!chatStatusLockEnabled.value) return "客户端自动管理"
  return `预设 ${availabilityLabel(chatStatusLockAvailability.value)}，游戏中自动暂停`
})

watch(
  () => props.active,
  (active) => {
    if (active && !loadedOnce.value) void loadClientToolState()
  },
  { immediate: true },
)

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function toggleAutoAccept() {
  autoAcceptEnabled.value = !autoAcceptEnabled.value
  writeBooleanSetting(AUTO_ACCEPT_ENABLED_KEY, autoAcceptEnabled.value)
}

async function toggleQuickBenchSwap() {
  if (quickBenchSwapBusy.value) return
  const next = !quickBenchSwapEnabled.value
  quickBenchSwapBusy.value = true
  try {
    await setQuickBenchSwapEnabled(next)
    quickBenchSwapEnabled.value = next
    writeBooleanSetting(QUICK_BENCH_SWAP_ENABLED_KEY, next)
  } catch (error) {
    notify({
      kind: "error",
      title: "快速交换设置失败",
      message: errorMessage(error),
      duration: 7000,
    })
  } finally {
    quickBenchSwapBusy.value = false
  }
}

async function loadClientToolState() {
  if (chatLoading.value) return
  chatLoading.value = true
  const [lockResult, chatResult] = await Promise.allSettled([
    getGameSettingsLocked(),
    getChatStatus(),
  ])
  const errors: string[] = []

  if (lockResult.status === "fulfilled") {
    settingsLocked.value = lockResult.value
    writeBooleanSetting(GAME_SETTINGS_LOCKED_KEY, lockResult.value)
  } else {
    errors.push(`设置文件：${errorMessage(lockResult.reason)}`)
  }

  if (chatResult.status === "fulfilled") {
    const currentAvailability = chatResult.value.availability || "chat"
    chatAvailability.value = chatStatusLockEnabled.value
      ? chatStatusLockAvailability.value
      : currentAvailability
    if (!chatStatusLockEnabled.value && isLockableChatAvailability(currentAvailability)) {
      setChatStatusLockAvailability(currentAvailability)
    }
    chatSignature.value = chatResult.value.statusMessage || ""
  } else {
    errors.push(`聊天状态：${errorMessage(chatResult.reason)}`)
  }

  loadedOnce.value = true
  if (errors.length) {
    notify({
      kind: "error",
      title: "部分快捷工具读取失败",
      message: errors.join("；"),
      duration: 7000,
    })
  }
  chatLoading.value = false
}

async function toggleSettingsLock() {
  if (settingsLockBusy.value) return
  const next = !settingsLocked.value
  settingsLockBusy.value = true
  try {
    settingsLocked.value = await setGameSettingsLocked(next)
    writeBooleanSetting(GAME_SETTINGS_LOCKED_KEY, settingsLocked.value)
    notify({
      kind: "success",
      title: settingsLocked.value ? "游戏设置已锁定" : "游戏设置已解锁",
      message: settingsLocked.value ? "客户端将无法覆盖当前游戏内设置" : "客户端可以继续保存游戏内设置",
    })
  } catch (error) {
    notify({ kind: "error", title: "修改游戏设置锁定失败", message: errorMessage(error), duration: 7000 })
  } finally {
    settingsLockBusy.value = false
  }
}

async function exitEndOfGame() {
  if (exitBusy.value) return
  exitBusy.value = true
  try {
    await dismissEndOfGame()
    notify({ kind: "success", title: "已退出结算页面" })
  } catch (error) {
    notify({ kind: "error", title: "退出结算页面失败", message: errorMessage(error), duration: 7000 })
  } finally {
    exitBusy.value = false
  }
}

async function saveAvailability() {
  if (availabilitySaving.value) return
  if (isLockableChatAvailability(chatAvailability.value)) {
    setChatStatusLockAvailability(chatAvailability.value)
  }
  availabilitySaving.value = true
  try {
    await setChatAvailability(chatAvailability.value)
    notify({ kind: "success", title: "聊天状态已更新" })
  } catch (error) {
    notify({ kind: "error", title: "设置聊天状态失败", message: errorMessage(error), duration: 7000 })
  } finally {
    availabilitySaving.value = false
  }
}

function chooseAvailability(value: string) {
  if (availabilitySaving.value || chatLoading.value) return
  chatAvailability.value = value
  if (chatStatusLockEnabled.value && !isLockableChatAvailability(value)) {
    setChatStatusLockEnabled(false)
    notify({
      kind: "success",
      title: "聊天状态锁定已关闭",
      message: "游戏中和观战中由客户端自动维护，不能作为锁定预设。",
    })
  }
  void saveAvailability()
}

function toggleChatStatusLock() {
  const next = !chatStatusLockEnabled.value
  if (next && !isLockableChatAvailability(chatAvailability.value)) {
    notify({
      kind: "error",
      title: "无法锁定当前状态",
      message: "请先选择聊天、在线分组、离开、离线或在线。游戏中和观战中由客户端自动维护。",
      duration: 7000,
    })
    return
  }

  if (next) setChatStatusLockAvailability(chatAvailability.value)
  setChatStatusLockEnabled(next)
  notify({
    kind: "success",
    title: next ? "聊天状态锁定已开启" : "聊天状态锁定已关闭",
    message: next ? "大厅和房间会保持预设状态，游戏中与观战中自动暂停。" : "客户端将继续自动管理聊天状态。",
  })
}

function availabilityLabel(value: string) {
  return availabilityOptions.find((option) => option.value === value)?.label || value
}

async function saveSignature() {
  if (signatureSaving.value || chatSignatureCount.value > 200) return
  signatureSaving.value = true
  try {
    await setChatStatusMessage(chatSignature.value)
    notify({ kind: "success", title: "聊天签名已更新" })
  } catch (error) {
    notify({ kind: "error", title: "设置聊天签名失败", message: errorMessage(error), duration: 7000 })
  } finally {
    signatureSaving.value = false
  }
}

</script>

<template>
  <section class="reserved-tools">
    <header class="reserved-heading">
      <strong>快捷工具</strong>
      <button class="icon-action" :disabled="chatLoading" title="重新读取客户端状态" @click="loadClientToolState">
        <RefreshCw :class="{ spin: chatLoading }" :size="17" />
      </button>
    </header>

    <div class="tool-row">
      <div class="tool-title">
        <strong>自动接受对局</strong>
        <span>{{ autoAcceptEnabled ? "已开启" : "已关闭" }}</span>
      </div>
      <button
        class="switch-control"
        :class="{ active: autoAcceptEnabled }"
        role="switch"
        :aria-checked="autoAcceptEnabled"
        @click="toggleAutoAccept"
      >
        <span></span>
      </button>
    </div>

    <div class="tool-row">
      <div class="tool-title">
        <strong>快速交换备战英雄</strong>
        <span>{{ quickBenchSwapEnabled ? "保留原点击，100ms 后自动兜底交换" : "已关闭" }}</span>
      </div>
      <button
        class="switch-control"
        :class="{ active: quickBenchSwapEnabled }"
        role="switch"
        :aria-checked="quickBenchSwapEnabled"
        :disabled="quickBenchSwapBusy"
        @click="toggleQuickBenchSwap"
      >
        <span></span>
      </button>
    </div>

    <div class="tool-row">
      <div class="tool-title">
        <strong>锁定游戏内设置</strong>
        <span>{{ settingsLocked ? "只读" : "可写" }}</span>
      </div>
      <button
        class="switch-control"
        :class="{ active: settingsLocked }"
        role="switch"
        :aria-checked="settingsLocked"
        :disabled="settingsLockBusy"
        @click="toggleSettingsLock"
      >
        <span></span>
      </button>
    </div>

    <div class="tool-row">
      <div class="tool-title">
        <strong>退出结算页面</strong>
        <span>EndOfGame</span>
      </div>
      <button class="command-button" :disabled="exitBusy" @click="exitEndOfGame">
        <LogOut :size="16" />
        {{ exitBusy ? "执行中" : "退出" }}
      </button>
    </div>

    <div class="chat-row">
      <div class="chat-label">
        <ShieldCheck :size="18" />
        <strong>聊天状态</strong>
      </div>
      <div class="availability-options">
        <button
          v-for="option in availabilityOptions"
          :key="option.value"
          class="availability-option"
          :class="{ active: chatAvailability === option.value }"
          :disabled="chatLoading || availabilitySaving"
          @click="chooseAvailability(option.value)"
        >
          {{ option.label }}
        </button>
      </div>
    </div>

    <div class="tool-row">
      <div class="tool-title">
        <strong>锁定聊天状态</strong>
        <span>{{ chatStatusLockSummary }}</span>
      </div>
      <button
        class="switch-control"
        :class="{ active: chatStatusLockEnabled }"
        role="switch"
        :aria-checked="chatStatusLockEnabled"
        :disabled="chatLoading || availabilitySaving"
        @click="toggleChatStatusLock"
      >
        <span></span>
      </button>
    </div>

    <div class="chat-row signature-row">
      <div class="chat-label">
        <MessageSquareText :size="18" />
        <strong>聊天签名</strong>
      </div>
      <div class="signature-input">
        <input v-model="chatSignature" maxlength="200" :disabled="chatLoading || signatureSaving" @keyup.enter="saveSignature" />
        <span>{{ chatSignatureCount }}/200</span>
      </div>
      <button
        class="command-button"
        :disabled="chatLoading || signatureSaving || chatSignatureCount > 200"
        @click="saveSignature"
      >
        <Check :size="16" />
        保存
      </button>
    </div>
  </section>
</template>

<style scoped>
.reserved-tools {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  overflow: auto;
  border-top: 1px solid #dce7e4;
}

.reserved-heading,
.tool-row,
.chat-row,
.tool-title,
.chat-label,
.signature-input {
  display: flex;
  align-items: center;
}

.reserved-heading {
  justify-content: space-between;
  padding: 16px 18px;
}

.reserved-heading strong {
  color: #20333a;
  font-size: 18px;
  font-weight: 950;
}

.tool-row,
.chat-row {
  min-height: 70px;
  border-top: 1px solid #e2ebe9;
  background: #ffffff;
  padding: 12px 18px;
}

.tool-row {
  justify-content: space-between;
}

.tool-title {
  flex-direction: column;
  align-items: flex-start;
  gap: 5px;
}

.tool-title strong,
.chat-label strong {
  color: #20333a;
  font-size: 15px;
  font-weight: 950;
}

.tool-title span {
  color: #6b7d81;
  font-size: 12px;
  font-weight: 800;
}

.switch-control {
  position: relative;
  width: 44px;
  height: 24px;
  flex: 0 0 auto;
  border: 0;
  border-radius: 12px;
  background: #c8d3d1;
  padding: 2px;
  cursor: pointer;
  transition: background 0.16s ease;
}

.switch-control span {
  display: block;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: #ffffff;
  box-shadow: 0 2px 5px rgba(24, 43, 47, 0.2);
  transition: transform 0.16s ease;
}

.switch-control.active {
  background: #237565;
}

.switch-control.active span {
  transform: translateX(20px);
}

.switch-control:disabled,
.command-button:disabled,
.icon-action:disabled {
  opacity: 0.55;
  cursor: wait;
}

.chat-row {
  display: grid;
  grid-template-columns: 150px minmax(180px, 1fr);
  gap: 12px;
}

.chat-label {
  gap: 8px;
  color: #237565;
}

.signature-input {
  height: 38px;
  min-width: 0;
  border: 1px solid #d3e1de;
  border-radius: 7px;
  color: #20333a;
  background: #fbfdfc;
  font-size: 14px;
  font-weight: 800;
}

.availability-options {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
}

.signature-row {
  grid-template-columns: 150px minmax(180px, 1fr) auto;
}

.availability-option {
  height: 34px;
  border: 1px solid #d2dfdc;
  border-radius: 7px;
  color: #53676c;
  background: #f7faf9;
  font-size: 13px;
  font-weight: 900;
  padding: 0 12px;
  cursor: pointer;
}

.availability-option.active {
  border-color: #23816d;
  color: #f8fffc;
  background: #23816d;
  box-shadow: 0 3px 9px rgba(35, 129, 109, 0.2);
}

.availability-option:disabled {
  opacity: 0.55;
  cursor: wait;
}

.signature-input {
  padding: 0 10px;
}

.signature-input input {
  min-width: 0;
  flex: 1;
  border: 0;
  outline: 0;
  color: #20333a;
  background: transparent;
  font: inherit;
}

.signature-input span {
  color: #829094;
  font-size: 11px;
  font-weight: 800;
}

.command-button,
.icon-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border: 1px solid #b9d3cd;
  border-radius: 7px;
  color: #1f6256;
  background: #f5fbf9;
  font-size: 13px;
  font-weight: 900;
  cursor: pointer;
}

.command-button {
  height: 36px;
  padding: 0 13px;
}

.icon-action {
  width: 34px;
  height: 34px;
  padding: 0;
}

.spin {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@media (max-width: 820px) {
  .chat-row {
    grid-template-columns: 1fr;
  }

  .chat-label {
    grid-column: 1 / -1;
  }

  .signature-row {
    grid-template-columns: 1fr auto;
  }
}
</style>
