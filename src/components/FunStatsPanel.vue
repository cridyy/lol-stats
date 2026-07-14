<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue"
import { RefreshCw, Search, X } from "lucide-vue-next"
import {
  FUN_EQUIPMENT_ROLE_OPTIONS,
  type FunAugmentFilter,
  type FunEquipmentFilter,
  type FunEquipmentRole,
  type FunGoldRange,
  type FunStatsBucket,
  type FunStatsResult,
  normalizeAugmentRarity,
} from "../funStats"
import type { ChampionSummaryItem, GameAssetEntry, RecentGame } from "../types"
import AssetIcon from "./AssetIcon.vue"
import ChampionAvatar from "./ChampionAvatar.vue"
import GoldRangeSlider from "./GoldRangeSlider.vue"

const props = defineProps<{
  result: FunStatsResult
  loading?: boolean
  games: RecentGame[]
  items: GameAssetEntry[]
  augments: GameAssetEntry[]
  champions: Record<number, ChampionSummaryItem>
}>()

const emit = defineEmits<{
  refresh: []
  equipmentChange: [filter: FunEquipmentFilter]
  penetrationChange: [goldRange: FunGoldRange]
  augmentChange: [filter: FunAugmentFilter]
}>()

const selectedItemId = ref(props.result.equipment.filter.itemId)
const selectedChampionId = ref(props.result.equipment.filter.championId || 0)
const selectedRole = ref<FunEquipmentRole | "">(props.result.equipment.filter.role || "")
const equipmentGoldRange = ref<FunGoldRange>({
  min: props.result.equipment.filter.min,
  max: props.result.equipment.filter.max,
})
const penetrationGoldRange = ref<FunGoldRange>({ ...props.result.penetration.goldRange })
const selectedAugmentId = ref(props.result.augment.augmentId || 0)
const selectedAugmentChampionId = ref(props.result.augment.championId || 0)
const selectedAugmentRole = ref<FunEquipmentRole | "">(props.result.augment.role || "")
const openPicker = ref<
  "item" | "equipmentChampion" | "augment" | "augmentChampion" | null
>(null)
const pickerSearch = ref("")
const pickerLimit = ref(84)
const pickerRootRef = ref<HTMLElement | null>(null)
const pickerSearchRef = ref<HTMLInputElement | null>(null)

const itemMap = computed(() => new Map(props.items.map((item) => [item.id, item])))
const augmentMap = computed(() => new Map(props.augments.map((augment) => [augment.id, augment])))
const itemOptions = computed(() => {
  // 客户端总装备表混有其他地图、历史和测试装备。统计选择器只保留当前模式
  // 样本中真实出现在结算装备栏里的条目，避免依赖不准确的全局商店标记。
  const observedItemIds = new Set(props.games.flatMap((game) => game.itemIds))
  observedItemIds.add(selectedItemId.value)
  return [...observedItemIds]
    .map((itemId) => itemMap.value.get(itemId))
    .filter((item): item is GameAssetEntry => Boolean(item?.name && item.iconPath))
    .sort((left, right) => left.name.localeCompare(right.name, "zh-CN"))
})
const championOptions = computed(() =>
  Object.values(props.champions)
    .filter((champion) => champion.id > 0 && champion.name)
    .sort((left, right) => left.name.localeCompare(right.name, "zh-CN")),
)
const augmentOptions = computed(() => {
  const observedAugmentIds = new Set(props.games.flatMap((game) => game.augmentIds))
  return [...observedAugmentIds]
    .map((augmentId) => augmentMap.value.get(augmentId))
    .filter((augment): augment is GameAssetEntry =>
      Boolean(augment?.name && augment.iconPath && normalizeAugmentRarity(augment.rarity)),
    )
    .sort((left, right) => left.name.localeCompare(right.name, "zh-CN"))
})
const filteredItems = computed(() => {
  const keyword = pickerSearch.value.trim().toLowerCase()
  if (!keyword) return itemOptions.value
  return itemOptions.value.filter((item) =>
    `${item.name} ${item.id}`.toLowerCase().includes(keyword),
  )
})
const filteredChampions = computed(() => {
  const keyword = pickerSearch.value.trim().toLowerCase()
  if (!keyword) return championOptions.value
  return championOptions.value.filter((champion) =>
    `${champion.name} ${champion.title} ${champion.alias} ${champion.id}`
      .toLowerCase()
      .includes(keyword),
  )
})
const filteredAugments = computed(() => {
  const keyword = pickerSearch.value.trim().toLowerCase()
  if (!keyword) return augmentOptions.value
  return augmentOptions.value.filter((augment) =>
    `${augment.name} ${augment.id}`.toLowerCase().includes(keyword),
  )
})
const visibleItems = computed(() => filteredItems.value.slice(0, pickerLimit.value))
const visibleChampions = computed(() => filteredChampions.value.slice(0, pickerLimit.value))
const visibleAugments = computed(() => filteredAugments.value.slice(0, pickerLimit.value))
const selectedItem = computed(() => itemMap.value.get(selectedItemId.value))
const selectedChampion = computed(() => props.champions[selectedChampionId.value])
const selectedAugmentChampion = computed(() => props.champions[selectedAugmentChampionId.value])
const selectedAugment = computed(() => augmentMap.value.get(selectedAugmentId.value))
const selectedAugmentRarity = computed(() =>
  normalizeAugmentRarity(selectedAugment.value?.rarity),
)

watch(
  () => props.result.equipment.filter,
  (filter) => {
    selectedItemId.value = filter.itemId
    selectedChampionId.value = filter.championId || 0
    selectedRole.value = filter.role || ""
    equipmentGoldRange.value = { min: filter.min, max: filter.max }
  },
)

watch(
  () => props.result.penetration.goldRange,
  (goldRange) => {
    penetrationGoldRange.value = { ...goldRange }
  },
)

watch(
  () => props.result.augment,
  (augment) => {
    selectedAugmentId.value = augment.augmentId || 0
    selectedAugmentChampionId.value = augment.championId || 0
    selectedAugmentRole.value = augment.role || ""
  },
)

watch(pickerSearch, () => {
  pickerLimit.value = 84
})

onMounted(() => document.addEventListener("pointerdown", closePickerOnOutsideClick))
onBeforeUnmount(() => document.removeEventListener("pointerdown", closePickerOnOutsideClick))

async function togglePicker(
  type: "item" | "equipmentChampion" | "augment" | "augmentChampion",
) {
  openPicker.value = openPicker.value === type ? null : type
  pickerSearch.value = ""
  pickerLimit.value = 84
  if (openPicker.value) {
    await nextTick()
    pickerSearchRef.value?.focus()
  }
}

function closePickerOnOutsideClick(event: PointerEvent) {
  if (!pickerRootRef.value?.contains(event.target as Node)) openPicker.value = null
}

function loadMorePicker(event: Event) {
  const element = event.currentTarget as HTMLElement
  if (element.scrollTop + element.clientHeight >= element.scrollHeight - 80) {
    pickerLimit.value += 84
  }
}

function selectItem(itemId: number) {
  selectedItemId.value = itemId
  openPicker.value = null
  updateEquipmentAnalysis()
}

function selectEquipmentChampion(championId: number) {
  selectedChampionId.value = championId
  openPicker.value = null
  updateEquipmentAnalysis()
}

function selectAugmentChampion(championId: number) {
  selectedAugmentChampionId.value = championId
  openPicker.value = null
  updateAugmentAnalysis()
}

function selectAugment(augmentId: number) {
  selectedAugmentId.value = augmentId
  openPicker.value = null
  updateAugmentAnalysis()
}

function updateEquipmentAnalysis() {
  emit("equipmentChange", {
    itemId: selectedItemId.value,
    championId: selectedChampionId.value || null,
    role: selectedRole.value || null,
    min: equipmentGoldRange.value.min,
    max: equipmentGoldRange.value.max,
  })
}

function updatePenetrationAnalysis(goldRange: FunGoldRange) {
  emit("penetrationChange", goldRange)
}

function updateAugmentAnalysis() {
  emit("augmentChange", {
    augmentId: selectedAugmentId.value || null,
    championId: selectedAugmentChampionId.value || null,
    role: selectedAugmentRole.value || null,
  })
}

function percent(value: number) {
  return `${Math.round(value * 100)}%`
}

function signedPercent(value: number | undefined, games: number) {
  if (!games || value === undefined) return "样本不足"
  const rounded = Math.round(value * 100)
  return `${rounded > 0 ? "+" : ""}${rounded}%`
}

function bucketMeta(bucket: FunStatsBucket) {
  return bucket.games > 0
    ? `${bucket.games} 场 · 胜率 ${percent(bucket.winRate)}`
    : "0 场 · 样本不足"
}

function rateText(bucket: FunStatsBucket, denominator: number) {
  return denominator > 0 ? percent(bucket.rate) : "样本不足"
}

function winRateText(bucket: FunStatsBucket) {
  return bucket.games > 0 ? percent(bucket.winRate) : "样本不足"
}

function augmentRarityLabel() {
  switch (props.result.augment.rarity) {
    case "prismatic":
      return "棱彩"
    case "gold":
      return "黄金"
    case "silver":
      return "白银"
    case "bronze":
      return "青铜"
    default:
      return "未知"
  }
}
</script>

<template>
  <section ref="pickerRootRef" class="fun-stats-panel">
    <header class="fun-stats-heading">
      <div>
        <strong>数据筛选</strong>
        <span>基于当前筛选的 {{ result.sampleGames }} 场</span>
      </div>
      <button
        type="button"
        :disabled="loading"
        title="重新统计数据筛选"
        @click="emit('refresh')"
      >
        <RefreshCw :class="{ spin: loading }" :size="16" />
      </button>
    </header>

    <div class="fun-stats-grid">
      <section class="fun-group side-group">
        <header>
          <strong>红蓝方表现</strong>
          <span>阵营场次与胜率</span>
        </header>
        <div class="fun-row blue-side">
          <span>蓝色方</span>
          <strong>{{ result.side.blue.games }} 场</strong>
          <em>胜率 {{ winRateText(result.side.blue) }}</em>
        </div>
        <div class="fun-row red-side">
          <span>红色方</span>
          <strong>{{ result.side.red.games }} 场</strong>
          <em>胜率 {{ winRateText(result.side.red) }}</em>
        </div>
      </section>

      <section class="fun-group penetration-group">
        <header>
          <strong>百分比穿透</strong>
          <span>符合经济条件的输出位 {{ result.penetration.eligibleGames }} 场</span>
        </header>
        <div class="inline-gold-range">
          <GoldRangeSlider
            v-model="penetrationGoldRange"
            @commit="updatePenetrationAnalysis"
          />
        </div>
        <div class="fun-row">
          <span>已出百分比穿透</span>
          <strong>{{ rateText(result.penetration.withItem, result.penetration.eligibleGames) }}</strong>
          <em>{{ bucketMeta(result.penetration.withItem) }}</em>
        </div>
        <div class="fun-row">
          <span>未出百分比穿透</span>
          <strong>{{ rateText(result.penetration.withoutItem, result.penetration.eligibleGames) }}</strong>
          <em>{{ bucketMeta(result.penetration.withoutItem) }}</em>
        </div>
      </section>

      <section class="fun-group leader-group">
        <header>
          <strong>核心担当</strong>
          <span>队内第一时的胜率</span>
        </header>
        <div class="fun-row">
          <span>队伍经济最高</span>
          <strong>{{ winRateText(result.leaders.gold) }}</strong>
          <em>{{ result.leaders.gold.games }} 场</em>
        </div>
        <div class="fun-row">
          <span>队伍伤害最高</span>
          <strong>{{ winRateText(result.leaders.damage) }}</strong>
          <em>{{ result.leaders.damage.games }} 场</em>
        </div>
      </section>

      <section class="fun-group equipment-group">
        <header>
          <strong>装备分析</strong>
          <span>符合英雄、位置与经济条件的 {{ result.equipment.eligibleGames }} 场</span>
        </header>

        <div class="equipment-filters">
          <div class="equipment-filter picker-field">
            <span>装备</span>
            <button
              class="square-picker-button"
              type="button"
              :title="selectedItem?.name || '选择装备'"
              @click="togglePicker('item')"
            >
              <AssetIcon
                :path="selectedItem?.iconPath"
                :label="selectedItem?.name"
                :fallback="String(selectedItemId)"
                :size="42"
              />
            </button>
            <div v-if="openPicker === 'item'" class="icon-picker-panel item-picker-panel">
              <label class="picker-search">
                <Search :size="16" />
                <input
                  ref="pickerSearchRef"
                  v-model="pickerSearch"
                  type="search"
                  placeholder="搜索装备名称或 ID"
                  @keydown.esc="openPicker = null"
                />
              </label>
              <div class="icon-picker-grid" @scroll.passive="loadMorePicker">
                <button
                  v-for="item in visibleItems"
                  :key="item.id"
                  type="button"
                  :class="{ selected: item.id === selectedItemId }"
                  :title="item.name"
                  @click="selectItem(item.id)"
                >
                  <AssetIcon
                    :path="item.iconPath"
                    :label="item.name"
                    :fallback="String(item.id)"
                    :size="44"
                  />
                </button>
                <span v-if="filteredItems.length === 0" class="picker-empty">没有匹配的装备</span>
              </div>
            </div>
          </div>

          <div class="equipment-filter picker-field">
            <span>英雄</span>
            <button
              class="square-picker-button"
              type="button"
              :title="selectedChampion ? `${selectedChampion.name} · ${selectedChampion.title}` : '不限英雄'"
              @click="togglePicker('equipmentChampion')"
            >
              <ChampionAvatar
                v-if="selectedChampion"
                :champion-id="selectedChampionId"
                :champions="champions"
                :size="42"
              />
              <span v-else class="champion-any">全</span>
            </button>
            <div
              v-if="openPicker === 'equipmentChampion'"
              class="icon-picker-panel champion-picker-panel"
            >
              <label class="picker-search">
                <Search :size="16" />
                <input
                  ref="pickerSearchRef"
                  v-model="pickerSearch"
                  type="search"
                  placeholder="搜索英雄名称、称号或英文名"
                  @keydown.esc="openPicker = null"
                />
              </label>
              <div class="icon-picker-grid" @scroll.passive="loadMorePicker">
                <button
                  type="button"
                  :class="{ selected: selectedChampionId === 0 }"
                  title="不限英雄"
                  @click="selectEquipmentChampion(0)"
                >
                  <span class="champion-any picker-any">全</span>
                </button>
                <button
                  v-for="champion in visibleChampions"
                  :key="champion.id"
                  type="button"
                  :class="{ selected: champion.id === selectedChampionId }"
                  :title="`${champion.name} · ${champion.title}`"
                  @click="selectEquipmentChampion(champion.id)"
                >
                  <ChampionAvatar
                    :champion-id="champion.id"
                    :champions="champions"
                    :size="44"
                  />
                </button>
                <span v-if="filteredChampions.length === 0" class="picker-empty">没有匹配的英雄</span>
              </div>
            </div>
          </div>

          <label class="equipment-filter">
            <span>位置</span>
            <div class="filter-control text-only">
              <select v-model="selectedRole" @change="updateEquipmentAnalysis">
                <option value="">不限</option>
                <option
                  v-for="option in FUN_EQUIPMENT_ROLE_OPTIONS"
                  :key="option.value"
                  :value="option.value"
                >
                  {{ option.label }}
                </option>
              </select>
            </div>
          </label>

          <label class="equipment-filter economy-filter">
            <span>经济区间</span>
            <GoldRangeSlider
              v-model="equipmentGoldRange"
              @commit="updateEquipmentAnalysis"
            />
          </label>
        </div>

        <div class="equipment-results">
          <div class="equipment-result equipped">
            <div class="equipment-result-heading">
              <div class="equipment-result-title">
                <AssetIcon
                  :path="selectedItem?.iconPath"
                  :label="selectedItem?.name"
                  :fallback="String(selectedItemId)"
                  :size="42"
                />
                <span>出{{ selectedItem?.name || "该装备" }}</span>
              </div>
              <strong>{{ result.equipment.withItem.games }} 场</strong>
            </div>
            <dl>
              <div><dt>出装概率</dt><dd>{{ rateText(result.equipment.withItem, result.equipment.eligibleGames) }}</dd></div>
              <div><dt>对应胜率</dt><dd>{{ winRateText(result.equipment.withItem) }}</dd></div>
              <div title="(个人经济占比 - 20%) × 5">
                <dt>领先平均经济</dt>
                <dd>{{ signedPercent(result.equipment.withItem.averageEconomyDelta, result.equipment.withItem.games) }}</dd>
              </div>
            </dl>
          </div>
          <div class="equipment-result unequipped">
            <div class="equipment-result-heading">
              <div class="equipment-result-title">
                <span class="unequipped-icon">
                  <AssetIcon
                    :path="selectedItem?.iconPath"
                    :label="selectedItem?.name"
                    :fallback="String(selectedItemId)"
                    :size="42"
                  />
                  <X :size="34" stroke-width="3.4" />
                </span>
                <span>未出{{ selectedItem?.name || "该装备" }}</span>
              </div>
              <strong>{{ result.equipment.withoutItem.games }} 场</strong>
            </div>
            <dl>
              <div><dt>未出概率</dt><dd>{{ rateText(result.equipment.withoutItem, result.equipment.eligibleGames) }}</dd></div>
              <div><dt>对应胜率</dt><dd>{{ winRateText(result.equipment.withoutItem) }}</dd></div>
              <div title="(个人经济占比 - 20%) × 5">
                <dt>领先平均经济</dt>
                <dd>{{ signedPercent(result.equipment.withoutItem.averageEconomyDelta, result.equipment.withoutItem.games) }}</dd>
              </div>
            </dl>
          </div>
        </div>
      </section>

      <section class="fun-group augment-group">
        <header>
          <strong>海克斯分析</strong>
          <span>筛选后 {{ result.augment.eligibleGames }} 场 · 海克斯池仅含当前样本已出现项</span>
        </header>

        <div v-if="selectedAugment" class="augment-analysis-body">
          <div class="equipment-filter picker-field augment-selector">
            <span>海克斯</span>
            <button
              :class="['square-picker-button', 'augment-picker-button', selectedAugmentRarity]"
              type="button"
              :title="selectedAugment.name"
              @click="togglePicker('augment')"
            >
              <AssetIcon
                :path="selectedAugment.iconPath"
                :label="selectedAugment.name"
                :fallback="String(selectedAugmentId)"
                :size="46"
              />
            </button>
            <div v-if="openPicker === 'augment'" class="icon-picker-panel augment-picker-panel">
              <label class="picker-search">
                <Search :size="16" />
                <input
                  ref="pickerSearchRef"
                  v-model="pickerSearch"
                  type="search"
                  placeholder="搜索海克斯名称或 ID"
                  @keydown.esc="openPicker = null"
                />
              </label>
              <div class="icon-picker-grid" @scroll.passive="loadMorePicker">
                <button
                  v-for="augment in visibleAugments"
                  :key="augment.id"
                  type="button"
                  :class="[
                    normalizeAugmentRarity(augment.rarity),
                    { selected: augment.id === selectedAugmentId },
                  ]"
                  :title="augment.name"
                  @click="selectAugment(augment.id)"
                >
                  <AssetIcon
                    :path="augment.iconPath"
                    :label="augment.name"
                    :fallback="String(augment.id)"
                    :size="44"
                  />
                </button>
                <span v-if="filteredAugments.length === 0" class="picker-empty">
                  没有匹配的海克斯
                </span>
              </div>
            </div>
          </div>

          <div class="equipment-filter picker-field augment-champion-selector">
            <span>英雄</span>
            <button
              class="square-picker-button"
              type="button"
              :title="selectedAugmentChampion ? `${selectedAugmentChampion.name} · ${selectedAugmentChampion.title}` : '不限英雄'"
              @click="togglePicker('augmentChampion')"
            >
              <ChampionAvatar
                v-if="selectedAugmentChampion"
                :champion-id="selectedAugmentChampionId"
                :champions="champions"
                :size="42"
              />
              <span v-else class="champion-any">全</span>
            </button>
            <div
              v-if="openPicker === 'augmentChampion'"
              class="icon-picker-panel champion-picker-panel"
            >
              <label class="picker-search">
                <Search :size="16" />
                <input
                  ref="pickerSearchRef"
                  v-model="pickerSearch"
                  type="search"
                  placeholder="搜索英雄名称、称号或英文名"
                  @keydown.esc="openPicker = null"
                />
              </label>
              <div class="icon-picker-grid" @scroll.passive="loadMorePicker">
                <button
                  type="button"
                  :class="{ selected: selectedAugmentChampionId === 0 }"
                  title="不限英雄"
                  @click="selectAugmentChampion(0)"
                >
                  <span class="champion-any picker-any">全</span>
                </button>
                <button
                  v-for="champion in visibleChampions"
                  :key="champion.id"
                  type="button"
                  :class="{ selected: champion.id === selectedAugmentChampionId }"
                  :title="`${champion.name} · ${champion.title}`"
                  @click="selectAugmentChampion(champion.id)"
                >
                  <ChampionAvatar
                    :champion-id="champion.id"
                    :champions="champions"
                    :size="44"
                  />
                </button>
                <span v-if="filteredChampions.length === 0" class="picker-empty">
                  没有匹配的英雄
                </span>
              </div>
            </div>
          </div>

          <label class="equipment-filter augment-role-selector">
            <span>位置</span>
            <div class="filter-control text-only">
              <select v-model="selectedAugmentRole" @change="updateAugmentAnalysis">
                <option value="">不限</option>
                <option
                  v-for="option in FUN_EQUIPMENT_ROLE_OPTIONS"
                  :key="option.value"
                  :value="option.value"
                >
                  {{ option.label }}
                </option>
              </select>
            </div>
          </label>

          <div :class="['augment-result', result.augment.rarity]">
            <div class="augment-result-title">
              <AssetIcon
                :path="selectedAugment.iconPath"
                :label="selectedAugment.name"
                :fallback="String(selectedAugmentId)"
                :size="50"
              />
              <div>
                <strong>{{ selectedAugment.name }}</strong>
                <span>{{ augmentRarityLabel() }}海克斯</span>
              </div>
            </div>
            <dl>
              <div>
                <dt>出现局数</dt>
                <dd>{{ result.augment.appearances }} 场</dd>
              </div>
              <div>
                <dt>同色有效机会</dt>
                <dd>{{ result.augment.opportunities }} 次</dd>
              </div>
              <div>
                <dt>登场率</dt>
                <dd>
                  {{ result.augment.opportunities > 0 ? percent(result.augment.pickRate) : "样本不足" }}
                </dd>
              </div>
              <div>
                <dt>对应胜率</dt>
                <dd>
                  {{ result.augment.appearances > 0 ? percent(result.augment.winRate) : "样本不足" }}
                </dd>
              </div>
            </dl>
          </div>
        </div>

        <div v-else class="augment-empty">
          当前筛选样本中没有可识别颜色的海克斯
        </div>
      </section>
    </div>
  </section>
</template>

<style scoped>
.fun-stats-panel {
  padding: 16px 18px;
  border: 1px solid #d8dee8;
  border-radius: 7px;
  background: #fff;
  box-shadow: 0 8px 24px rgb(30 45 70 / 6%);
}

.fun-stats-heading,
.fun-stats-heading > div,
.fun-group header,
.fun-row {
  display: flex;
  align-items: center;
}

.fun-stats-heading {
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.fun-stats-heading > div {
  gap: 10px;
}

.fun-stats-heading strong {
  font-size: 17px;
  color: #172033;
}

.fun-stats-heading span,
.fun-group header span,
.fun-subtitle {
  color: #788397;
  font-size: 12px;
}

.fun-stats-heading button {
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  border: 1px solid #ccd5e2;
  border-radius: 6px;
  color: #35445d;
  background: #f8fafc;
  cursor: pointer;
}

.fun-stats-heading button:hover:not(:disabled) {
  border-color: #8ea4c2;
  background: #eef3f9;
}

.fun-stats-heading button:disabled {
  cursor: default;
  opacity: 0.55;
}

.fun-stats-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  column-gap: 24px;
  row-gap: 18px;
  align-items: start;
}

.fun-group {
  min-width: 0;
  border-top: 2px solid #e8edf4;
  padding-top: 10px;
}

.equipment-group {
  grid-column: 2;
  grid-row: 1 / span 3;
}

.side-group {
  grid-column: 1;
  grid-row: 1;
}

.penetration-group {
  grid-column: 1;
  grid-row: 2;
}

.leader-group {
  grid-column: 1;
  grid-row: 3;
}

.augment-group {
  grid-column: 2;
  grid-row: 4;
}

.equipment-filters {
  display: grid;
  grid-template-columns: 58px 58px minmax(130px, 1fr);
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid #e1e6ee;
  border-radius: 6px;
  background: #f8fafc;
}

.equipment-filter {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
  color: #536078;
  font-size: 12px;
  font-weight: 700;
}

.equipment-filter > span {
  display: flex;
  min-height: 17px;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.equipment-filter b {
  color: #263752;
  font-size: 12px;
}

.picker-field {
  position: relative;
}

.square-picker-button {
  display: grid;
  width: 50px;
  height: 50px;
  padding: 3px;
  place-items: center;
  border: 1px solid #cbd5e2;
  border-radius: 6px;
  background: #fff;
  cursor: pointer;
}

.square-picker-button:hover {
  border-color: #678bb7;
  background: #f1f5fa;
}

.champion-any {
  display: grid;
  width: 42px;
  height: 42px;
  place-items: center;
  border: 1px solid #cfd8e4;
  border-radius: 6px;
  color: #43546c;
  background: #e9eef5;
  font-size: 14px;
  font-weight: 800;
}

.icon-picker-panel {
  position: absolute;
  z-index: 30;
  top: 76px;
  left: 0;
  display: flex;
  width: min(420px, calc(100vw - 90px));
  height: 420px;
  flex-direction: column;
  padding: 12px;
  border: 1px solid #bcc9da;
  border-radius: 7px;
  background: #fff;
  box-shadow: 0 18px 44px rgb(31 45 68 / 24%);
}

.picker-search {
  display: flex;
  height: 38px;
  flex: 0 0 auto;
  align-items: center;
  gap: 8px;
  padding: 0 10px;
  border: 1px solid #cbd5e2;
  border-radius: 6px;
  color: #68768b;
  background: #f8fafc;
}

.picker-search:focus-within {
  border-color: #668fbe;
  box-shadow: 0 0 0 2px rgb(63 116 178 / 12%);
}

.picker-search input {
  min-width: 0;
  width: 100%;
  border: 0;
  outline: 0;
  color: #26364e;
  background: transparent;
  font-size: 13px;
}

.icon-picker-grid {
  display: grid;
  min-height: 0;
  flex: 1;
  grid-template-columns: repeat(auto-fill, 50px);
  grid-auto-rows: 50px;
  align-content: start;
  gap: 7px;
  overflow-y: auto;
  margin-top: 10px;
  padding-right: 4px;
}

.icon-picker-grid button {
  display: grid;
  width: 50px;
  height: 50px;
  padding: 2px;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  cursor: pointer;
}

.icon-picker-grid button:hover {
  border-color: #9fb2ca;
  background: #eef3f8;
}

.icon-picker-grid button.selected {
  border-color: #3f78bb;
  background: #e5eef8;
  box-shadow: inset 0 0 0 1px #3f78bb;
}

.picker-any {
  width: 44px;
  height: 44px;
}

.picker-empty {
  grid-column: 1 / -1;
  padding: 22px 4px;
  color: #7c8798;
  font-size: 13px;
}

.augment-analysis-body {
  display: grid;
  grid-template-columns: 58px 58px minmax(130px, 1fr);
  align-items: start;
  gap: 12px;
  margin-top: 8px;
}

.augment-selector {
  align-self: start;
}

.augment-picker-button.silver,
.icon-picker-grid button.silver {
  border-color: #6d7f91;
  background: #75879a;
}

.augment-picker-button.gold,
.icon-picker-grid button.gold {
  border-color: #8b620c;
  background: #a87713;
}

.augment-picker-button.prismatic,
.icon-picker-grid button.prismatic {
  border-color: #58357e;
  background: #70469d;
}

.augment-picker-button.bronze,
.icon-picker-grid button.bronze {
  border-color: #6d412e;
  background: #86543d;
}

.augment-picker-button.silver :deep(.asset-icon),
.icon-picker-grid button.silver :deep(.asset-icon),
.augment-result.silver :deep(.asset-icon) {
  border-color: #98a7b5;
  background: #657789;
}

.augment-picker-button.gold :deep(.asset-icon),
.icon-picker-grid button.gold :deep(.asset-icon),
.augment-result.gold :deep(.asset-icon) {
  border-color: #d6ad4c;
  background: #8f640f;
}

.augment-picker-button.prismatic :deep(.asset-icon),
.icon-picker-grid button.prismatic :deep(.asset-icon),
.augment-result.prismatic :deep(.asset-icon) {
  border-color: #a77cd0;
  background: #613b89;
}

.augment-picker-button.bronze :deep(.asset-icon),
.icon-picker-grid button.bronze :deep(.asset-icon),
.augment-result.bronze :deep(.asset-icon) {
  border-color: #b78669;
  background: #734631;
}

.icon-picker-grid button.silver.selected {
  box-shadow: inset 0 0 0 2px #8696a8;
}

.icon-picker-grid button.gold.selected {
  box-shadow: inset 0 0 0 2px #c28e1c;
}

.icon-picker-grid button.prismatic.selected {
  box-shadow: inset 0 0 0 2px #8551ba;
}

.icon-picker-grid button.bronze.selected {
  box-shadow: inset 0 0 0 2px #976044;
}

.augment-result {
  display: grid;
  min-width: 0;
  grid-column: 1 / -1;
  grid-template-columns: minmax(180px, 1fr) minmax(0, 2fr);
  align-items: center;
  gap: 18px;
  padding: 12px 14px;
  border-left: 3px solid #8997aa;
  background: #f8fafc;
}

.augment-result.silver {
  border-left-color: #8998a8;
  background: #e4eaf0;
}

.augment-result.gold {
  border-left-color: #c99726;
  background: #fff0bd;
}

.augment-result.prismatic {
  border-left-color: #8858b9;
  background: #efddff;
}

.augment-result.bronze {
  border-left-color: #976044;
  background: #f0d7c7;
}

.augment-result-title {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 11px;
}

.augment-result-title > div {
  min-width: 0;
}

.augment-result-title strong,
.augment-result-title span {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.augment-result-title strong {
  color: #223149;
  font-size: 15px;
}

.augment-result-title span {
  margin-top: 3px;
  color: #768297;
  font-size: 11px;
}

.augment-result dl {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  margin: 0;
}

.augment-result dt,
.augment-result dd {
  margin: 0;
}

.augment-result dt {
  color: #7a8597;
  font-size: 11px;
}

.augment-result dd {
  margin-top: 4px;
  color: #1d2c43;
  font-size: 15px;
  font-weight: 800;
  white-space: nowrap;
}

.augment-empty {
  padding: 18px 0 8px;
  color: #7a8698;
  font-size: 13px;
}

.filter-control {
  display: flex;
  min-width: 0;
  height: 34px;
  align-items: center;
  gap: 7px;
  padding: 4px 7px;
  border: 1px solid #ccd5e2;
  border-radius: 6px;
  background: #fff;
}

.filter-control.text-only {
  padding-left: 9px;
}

.filter-control select {
  min-width: 0;
  width: 100%;
  border: 0;
  outline: 0;
  color: #26344b;
  background: transparent;
  font: inherit;
  cursor: pointer;
}

.economy-filter {
  grid-column: 1 / -1;
}

.inline-gold-range {
  margin: 6px 0 8px;
  padding: 8px 10px 4px;
  border: 1px solid #e1e6ee;
  border-radius: 6px;
  background: #f8fafc;
}

.equipment-results {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
  margin-top: 12px;
}

.equipment-result {
  min-width: 0;
  padding: 10px 12px;
  border-left: 3px solid #8697ae;
  background: #f8fafc;
}

.equipment-result.equipped {
  border-left-color: #397dc5;
}

.equipment-result.unequipped {
  border-left-color: #d17b55;
}

.equipment-result-heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  color: #2e3b51;
  font-size: 13px;
  font-weight: 700;
}

.equipment-result-title {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 9px;
}

.equipment-result-title > span:last-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.unequipped-icon {
  position: relative;
  display: block;
  width: 42px;
  height: 42px;
  flex: 0 0 auto;
}

.unequipped-icon > svg {
  position: absolute;
  z-index: 2;
  inset: 4px;
  color: #d52f3f;
  filter: drop-shadow(0 1px 1px #fff) drop-shadow(0 -1px 1px #fff);
}

.equipment-result-heading strong {
  flex: 0 0 auto;
  color: #17243a;
  font-size: 14px;
}

.equipment-result dl {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  margin: 10px 0 0;
}

.equipment-result dl > div {
  min-width: 0;
}

.equipment-result dt,
.equipment-result dd {
  margin: 0;
}

.equipment-result dt {
  color: #7a8597;
  font-size: 11px;
}

.equipment-result dd {
  margin-top: 3px;
  color: #1d2c43;
  font-size: 15px;
  font-weight: 800;
}

.fun-group header {
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 7px;
}

.fun-group header strong {
  color: #27344b;
  font-size: 14px;
}

.fun-group header span {
  text-align: right;
}

.fun-row {
  display: grid;
  grid-template-columns: minmax(120px, 1fr) auto minmax(170px, auto);
  gap: 12px;
  min-height: 34px;
  padding: 6px 0;
  border-bottom: 1px solid #edf0f5;
}

.fun-row > span {
  min-width: 0;
  color: #3b4659;
  font-size: 13px;
}

.fun-row > strong {
  color: #18243a;
  font-size: 14px;
  white-space: nowrap;
}

.fun-row > em {
  color: #69758a;
  font-size: 12px;
  font-style: normal;
  text-align: right;
  white-space: nowrap;
}

.blue-side {
  box-shadow: inset 3px 0 #4b91e2;
  padding-left: 10px;
}

.red-side {
  box-shadow: inset 3px 0 #df6871;
  padding-left: 10px;
}

.leader-group .fun-row {
  grid-template-columns: minmax(160px, 1fr) auto minmax(80px, auto);
}

.spin {
  animation: fun-spin 0.8s linear infinite;
}

@keyframes fun-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (max-width: 900px) {
  .fun-stats-grid {
    grid-template-columns: 1fr;
  }

  .side-group,
  .penetration-group,
  .equipment-group,
  .leader-group,
  .augment-group {
    grid-row: auto;
    grid-column: auto;
  }

  .equipment-filters {
    grid-template-columns: 58px 58px minmax(130px, 1fr);
  }
}

@media (max-width: 640px) {
  .fun-stats-panel {
    padding: 14px;
  }

  .fun-stats-heading > div,
  .fun-group header {
    align-items: flex-start;
    flex-direction: column;
    gap: 3px;
  }

  .fun-group header span {
    text-align: left;
  }

  .fun-row,
  .leader-group .fun-row {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .fun-row > em {
    grid-column: 1 / -1;
    text-align: left;
    white-space: normal;
  }

  .equipment-results {
    grid-template-columns: 1fr;
  }

  .equipment-filters {
    grid-template-columns: 58px 58px minmax(110px, 1fr);
  }

  .icon-picker-panel {
    position: fixed;
    top: 50%;
    left: 50%;
    width: min(420px, calc(100vw - 28px));
    height: min(420px, calc(100vh - 80px));
    transform: translate(-50%, -50%);
  }

  .equipment-result dl {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .augment-result {
    grid-template-columns: 1fr;
  }

  .augment-result dl {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
