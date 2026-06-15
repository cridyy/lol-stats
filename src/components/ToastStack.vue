<script setup lang="ts">
import type { AppToast } from "../notifications"

defineProps<{
  toasts: AppToast[]
}>()

const emit = defineEmits<{
  dismiss: [id: number]
  action: [id: number]
}>()

function durationStyle(toast: AppToast) {
  return {
    "--toast-duration": `${toast.duration ?? 4800}ms`,
  }
}

function dismissToast(id: number) {
  emit("dismiss", id)
}

function runAction(event: MouseEvent, id: number) {
  event.stopPropagation()
  emit("action", id)
}
</script>

<template>
  <Teleport to="body">
    <TransitionGroup name="toast" tag="div" class="toast-stack" aria-live="polite">
      <article
        v-for="toast in toasts"
        :key="toast.id"
        class="toast-card"
        :class="toast.kind"
        :style="durationStyle(toast)"
        role="status"
        @click="dismissToast(toast.id)"
      >
        <div class="toast-content">
          <strong>{{ toast.title }}</strong>
          <span v-if="toast.message">{{ toast.message }}</span>
        </div>

        <button
          v-if="toast.actionLabel"
          class="toast-action"
          @click="runAction($event, toast.id)"
        >
          {{ toast.actionLabel }}
        </button>

        <div class="toast-progress" @animationend="dismissToast(toast.id)" />
      </article>
    </TransitionGroup>
  </Teleport>
</template>

<style scoped>
.toast-stack {
  position: fixed;
  top: 16px;
  left: 50%;
  z-index: 1600;
  display: flex;
  width: min(520px, calc(100vw - 32px));
  flex-direction: column;
  gap: 8px;
  pointer-events: none;
  transform: translateX(-50%);
}

.toast-card {
  position: relative;
  display: flex;
  min-height: 54px;
  align-items: center;
  gap: 12px;
  overflow: hidden;
  border: 1px solid #d7e5e1;
  border-left-width: 5px;
  border-radius: 8px;
  color: #253238;
  background: rgba(255, 255, 255, 0.96);
  box-shadow: 0 18px 44px rgba(24, 46, 50, 0.18);
  cursor: pointer;
  pointer-events: auto;
  padding: 10px 12px;
  backdrop-filter: blur(12px);
}

.toast-card.info {
  border-left-color: #2f78d6;
}

.toast-card.success {
  border-left-color: #2f8b63;
}

.toast-card.warning {
  border-left-color: #d69b2f;
}

.toast-card.error {
  border-left-color: #c94d4b;
}

.toast-content {
  display: flex;
  min-width: 0;
  flex: 1;
  flex-direction: column;
  gap: 3px;
}

.toast-content strong {
  overflow: hidden;
  font-size: 13px;
  line-height: 1.2;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toast-content span {
  overflow: hidden;
  color: #64747a;
  font-size: 12px;
  line-height: 1.35;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.toast-action {
  flex: 0 0 auto;
  border-radius: 7px;
  color: #ffffff;
  background: #1f5f56;
  cursor: pointer;
  font-size: 12px;
  font-weight: 800;
  padding: 7px 10px;
}

.toast-card.warning .toast-action {
  background: #a96516;
}

.toast-card.error .toast-action {
  background: #a94745;
}

.toast-progress {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  height: 3px;
  background: currentColor;
  opacity: 0.28;
  transform-origin: left center;
  animation: toast-progress var(--toast-duration) linear forwards;
}

.toast-card:hover .toast-progress {
  animation-play-state: paused;
}

.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 0.16s ease,
    transform 0.16s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(-10px);
}

@keyframes toast-progress {
  from {
    transform: scaleX(1);
  }

  to {
    transform: scaleX(0);
  }
}
</style>
