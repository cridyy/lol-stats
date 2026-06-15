<script setup lang="ts">
import { computed, ref, watch } from "vue"
import { loadAssetDataUrl } from "../assetLoader"

const props = withDefaults(
  defineProps<{
    path?: string
    label?: string
    fallback?: string
    size?: number
  }>(),
  {
    size: 22,
  },
)

const src = ref("")
const boxStyle = computed(() => ({
  width: `${props.size}px`,
  height: `${props.size}px`,
}))

watch(
  () => props.path,
  async (path) => {
    src.value = ""
    if (!path) return

    try {
      const dataUrl = await loadAssetDataUrl(path)
      if (props.path === path) src.value = dataUrl
    } catch {
      // 图标失败时保留 ID 占位，避免战绩行布局跳动。
    }
  },
  { immediate: true },
)
</script>

<template>
  <div class="asset-icon" :style="boxStyle" :title="label" :aria-label="label">
    <img v-if="src" :src="src" :alt="label || fallback || '-'" />
    <span v-else>{{ fallback || "-" }}</span>
  </div>
</template>

<style scoped>
.asset-icon {
  display: inline-grid;
  flex: 0 0 auto;
  place-items: center;
  overflow: hidden;
  border: 1px solid rgba(38, 50, 56, 0.12);
  border-radius: 5px;
  color: #52646a;
  background: #edf3f1;
}

.asset-icon img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.asset-icon span {
  max-width: 100%;
  overflow: hidden;
  padding: 0 2px;
  font-size: 9px;
  font-weight: 800;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
