<script setup lang="ts">
import { computed } from "vue"
import type { FunGoldRange } from "../funStats"

const props = withDefaults(
  defineProps<{
    modelValue: FunGoldRange
    limit?: number
    step?: number
  }>(),
  {
    limit: 20_000,
    step: 500,
  },
)

const emit = defineEmits<{
  "update:modelValue": [range: FunGoldRange]
  commit: [range: FunGoldRange]
}>()

const lowerValue = computed(() => props.modelValue.min ?? 0)
const upperValue = computed(() => props.modelValue.max ?? props.limit)
const trackStyle = computed(() => ({
  "--range-start": `${(lowerValue.value / props.limit) * 100}%`,
  "--range-end": `${(upperValue.value / props.limit) * 100}%`,
}))

function updateLower(event: Event, commit: boolean) {
  const raw = Number((event.target as HTMLInputElement).value)
  const value = Math.min(raw, upperValue.value)
  updateRange(
    {
      min: value <= 0 ? null : value,
      max: upperValue.value >= props.limit ? null : upperValue.value,
    },
    commit,
  )
}

function updateUpper(event: Event, commit: boolean) {
  const raw = Number((event.target as HTMLInputElement).value)
  const value = Math.max(raw, lowerValue.value)
  updateRange(
    {
      min: lowerValue.value <= 0 ? null : lowerValue.value,
      max: value >= props.limit ? null : value,
    },
    commit,
  )
}

function updateRange(range: FunGoldRange, commit: boolean) {
  emit("update:modelValue", range)
  if (commit) emit("commit", range)
}

function formatGold(value: number | null, edge: "min" | "max") {
  if (value === null) return "不限"
  if (edge === "min" && value <= 0) return "不限"
  if (edge === "max" && value >= props.limit) return "不限"
  const thousands = value / 1000
  return `${Number.isInteger(thousands) ? thousands : thousands.toFixed(1)}k`
}
</script>

<template>
  <div class="gold-range-slider">
    <div class="range-labels">
      <span>下限 <b>{{ formatGold(modelValue.min, "min") }}</b></span>
      <span>上限 <b>{{ formatGold(modelValue.max, "max") }}</b></span>
    </div>
    <div class="range-control" :style="trackStyle">
      <div class="range-track"></div>
      <input
        aria-label="经济下限"
        type="range"
        min="0"
        :max="limit"
        :step="step"
        :value="lowerValue"
        @input="updateLower($event, false)"
        @change="updateLower($event, true)"
      />
      <input
        aria-label="经济上限"
        type="range"
        min="0"
        :max="limit"
        :step="step"
        :value="upperValue"
        @input="updateUpper($event, false)"
        @change="updateUpper($event, true)"
      />
    </div>
  </div>
</template>

<style scoped>
.gold-range-slider {
  min-width: 0;
}

.range-labels {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  color: #647187;
  font-size: 11px;
  white-space: nowrap;
}

.range-labels b {
  margin-left: 3px;
  color: #243650;
  font-size: 12px;
}

.range-control {
  position: relative;
  height: 27px;
  margin-top: 2px;
}

.range-track {
  position: absolute;
  top: 12px;
  right: 7px;
  left: 7px;
  height: 4px;
  border-radius: 2px;
  background:
    linear-gradient(
      to right,
      #d6dde7 0 var(--range-start),
      #3c73b9 var(--range-start) var(--range-end),
      #d6dde7 var(--range-end) 100%
    );
}

.range-control input {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 27px;
  margin: 0;
  appearance: none;
  pointer-events: none;
  background: transparent;
}

.range-control input::-webkit-slider-runnable-track {
  height: 4px;
  background: transparent;
}

.range-control input::-webkit-slider-thumb {
  width: 15px;
  height: 15px;
  margin-top: -6px;
  appearance: none;
  border: 2px solid #fff;
  border-radius: 50%;
  background: #356eaf;
  box-shadow: 0 0 0 1px #356eaf;
  cursor: pointer;
  pointer-events: auto;
}

.range-control input::-moz-range-track {
  height: 4px;
  background: transparent;
}

.range-control input::-moz-range-thumb {
  width: 13px;
  height: 13px;
  border: 2px solid #fff;
  border-radius: 50%;
  background: #356eaf;
  box-shadow: 0 0 0 1px #356eaf;
  cursor: pointer;
  pointer-events: auto;
}
</style>
