<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { loadAssetDataUrl } from "../assetLoader"
import type { ChampionSummaryItem } from "../types"
import { championName } from "../utils"

const props = withDefaults(
  defineProps<{
    championId?: number
    champions: Record<number, ChampionSummaryItem>
    size?: number
  }>(),
  {
    size: 34,
  },
)

const src = ref("")
const champion = computed(() => (props.championId ? props.champions[props.championId] : undefined))
const label = computed(() => championName(props.champions, props.championId))
const fallback = computed(() => (props.championId ? String(props.championId) : "-"))
const boxStyle = computed(() => ({
  width: `${props.size}px`,
  height: `${props.size}px`,
}))

watch(
  () => [props.championId, champion.value?.squarePortraitPath] as const,
  async ([championId, path]) => {
    src.value = ""
    if (!championId || !path) return

    try {
      const dataUrl = await loadAssetDataUrl(path)
      if (props.championId === championId) src.value = dataUrl
    } catch {
      // 头像是展示增强，失败时保留稳定的 ID 占位。
    }
  },
  { immediate: true },
)
</script>

<template>
  <div class="champion-avatar" :style="boxStyle" :title="label" :aria-label="label">
    <img v-if="src" :src="src" :alt="label" />
    <span v-else>{{ fallback }}</span>
  </div>
</template>

<style scoped>
.champion-avatar {
  display: inline-grid;
  flex: 0 0 auto;
  place-items: center;
  overflow: hidden;
  border: 1px solid rgba(31, 95, 86, 0.18);
  border-radius: 8px;
  color: #48605b;
  background: #edf5f3;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.32);
}

.champion-avatar img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.champion-avatar span {
  max-width: 100%;
  overflow: hidden;
  padding: 0 3px;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  font-weight: 800;
}
</style>
