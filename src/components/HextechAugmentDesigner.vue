<script setup lang="ts">
import { computed, inject, nextTick, ref, watch } from "vue"
import {
  ClipboardCopy,
  LoaderCircle,
  Save,
  Search,
  Sparkles,
  Trash2,
} from "lucide-vue-next"
import augmentManifest from "../assets/hextech/augments.zh-CN.json"
import goldBorder from "../assets/hextech/borders/Gold.png"
import prismaticBorder from "../assets/hextech/borders/Prismatic.png"
import silverBorder from "../assets/hextech/borders/Silver.png"
import { copyElementAsPng } from "../imageShare"
import { notifyKey } from "../notifications"

type HextechRarity = "silver" | "gold" | "prismatic"

type ManifestEntry = {
  file: string
  id: number
  name: string
  englishName: string
  rarity: string
}

type LocalAugmentAsset = ManifestEntry & {
  url: string
}

type HextechDraft = {
  name: string
  category: string
  description: string
  rarity: HextechRarity
  iconFile: string
}

type SavedHextechDesign = HextechDraft & {
  saveName: string
  updatedAt: number
}

const DRAFT_KEY = "lol-stats.tools.hextech-designer.draft"
const SAVED_KEY = "lol-stats.tools.hextech-designer.saved"
const iconModules = import.meta.glob("../assets/hextech/icons/*.png", {
  eager: true,
  import: "default",
  query: "?url",
}) as Record<string, string>
const iconUrlByFile = new Map(
  Object.entries(iconModules).map(([path, url]) => [path.split("/").pop() || path, url]),
)
const augmentList: LocalAugmentAsset[] = (augmentManifest as ManifestEntry[])
  .map((augment) => ({ ...augment, url: iconUrlByFile.get(augment.file) || "" }))
  .filter((augment) => augment.url)
  .sort((left, right) => left.name.localeCompare(right.name, "zh-CN"))

const rarityOptions: Array<{ value: HextechRarity; label: string }> = [
  { value: "silver", label: "白银" },
  { value: "gold", label: "黄金" },
  { value: "prismatic", label: "棱彩" },
]
const borderByRarity: Record<HextechRarity, string> = {
  silver: silverBorder,
  gold: goldBorder,
  prismatic: prismaticBorder,
}

const notify = inject(notifyKey, () => 0)
const draft = ref<HextechDraft>(loadDraft())
const savedDesigns = ref<Record<string, SavedHextechDesign>>(loadSavedDesigns())
const activeSaveName = ref("")
const saveDialogOpen = ref(false)
const deleteDialogOpen = ref(false)
const saveNameInput = ref("")
const iconSearch = ref("")
const sharing = ref(false)
const captureRef = ref<HTMLElement | null>(null)

const selectedAugment = computed(() =>
  augmentList.find((augment) => augment.file === draft.value.iconFile) || null,
)
const selectedBorder = computed(() => borderByRarity[draft.value.rarity])
const filteredAugments = computed(() => {
  const keyword = iconSearch.value.trim().toLocaleLowerCase("zh-CN")
  const result = keyword
    ? augmentList.filter((augment) =>
      [augment.name, augment.englishName, String(augment.id), augment.rarity]
        .some((value) => value.toLocaleLowerCase("zh-CN").includes(keyword)),
    )
    : augmentList

  return result
})
const savedNames = computed(() =>
  Object.keys(savedDesigns.value).sort((left, right) => left.localeCompare(right, "zh-CN")),
)
const rarityLabel = computed(() =>
  rarityOptions.find((option) => option.value === draft.value.rarity)?.label || "黄金",
)
const displayName = computed(() => draft.value.name.trim() || "未命名强化")
const displayCategory = computed(() => draft.value.category.trim() || "综合")
const displayDescription = computed(() => draft.value.description.trim() || "在这里填写强化效果。")
const titleStyle = computed(() => {
  const length = displayName.value.length
  return { fontSize: length > 16 ? "19px" : length > 10 ? "22px" : "26px" }
})
const descriptionStyle = computed(() => {
  const length = displayDescription.value.length
  return { fontSize: length > 105 ? "13px" : length > 70 ? "14px" : "16px" }
})

watch(
  draft,
  (value) => localStorage.setItem(DRAFT_KEY, JSON.stringify(value)),
  { deep: true },
)

function defaultIconFile() {
  return augmentList.find((augment) => augment.rarity === "gold")?.file || augmentList[0]?.file || ""
}

function defaultDraft(): HextechDraft {
  return {
    name: "",
    category: "综合",
    description: "",
    rarity: "gold",
    iconFile: defaultIconFile(),
  }
}

function sanitizeDraft(value: unknown): HextechDraft {
  const source = (value || {}) as Partial<HextechDraft> & { iconId?: unknown }
  const rarity = rarityOptions.some((option) => option.value === source.rarity)
    ? source.rarity as HextechRarity
    : "gold"
  const savedFile = typeof source.iconFile === "string" ? source.iconFile : ""
  const legacyIconId = Number(source.iconId)
  const iconFile = augmentList.some((augment) => augment.file === savedFile)
    ? savedFile
    : augmentList.find((augment) => legacyIconId > 0 && augment.id === legacyIconId)?.file || defaultIconFile()

  return {
    name: typeof source.name === "string" ? source.name : "",
    category: typeof source.category === "string" ? source.category : "综合",
    description: typeof source.description === "string" ? source.description : "",
    rarity,
    iconFile,
  }
}

function loadDraft() {
  try {
    const raw = localStorage.getItem(DRAFT_KEY)
    return raw ? sanitizeDraft(JSON.parse(raw)) : defaultDraft()
  } catch {
    return defaultDraft()
  }
}

function loadSavedDesigns() {
  try {
    const raw = localStorage.getItem(SAVED_KEY)
    if (!raw) return {}
    const value = JSON.parse(raw) as Record<string, Partial<SavedHextechDesign>>
    return Object.fromEntries(
      Object.entries(value)
        .filter(([name]) => name.trim())
        .map(([name, design]) => [
          name,
          {
            ...sanitizeDraft(design),
            saveName: name,
            updatedAt: Number(design.updatedAt) || 0,
          },
        ]),
    )
  } catch {
    return {}
  }
}

function persistSavedDesigns() {
  localStorage.setItem(SAVED_KEY, JSON.stringify(savedDesigns.value))
}

function selectAugment(augment: LocalAugmentAsset) {
  draft.value.iconFile = augment.file
  if (rarityOptions.some((option) => option.value === augment.rarity)) {
    draft.value.rarity = augment.rarity as HextechRarity
  }
}

function openSaveDialog() {
  saveNameInput.value = activeSaveName.value || displayName.value
  saveDialogOpen.value = true
  void nextTick(() => document.querySelector<HTMLInputElement>(".hextech-save-dialog input")?.focus())
}

function confirmSave() {
  const saveName = saveNameInput.value.trim()
  if (!saveName) return

  const saved: SavedHextechDesign = {
    ...sanitizeDraft(draft.value),
    saveName,
    updatedAt: Date.now(),
  }
  savedDesigns.value = { ...savedDesigns.value, [saveName]: saved }
  activeSaveName.value = saveName
  persistSavedDesigns()
  saveDialogOpen.value = false
  notify({ kind: "success", title: "海克斯方案已保存", message: saveName })
}

function switchSavedDesign(event: Event) {
  const name = (event.target as HTMLSelectElement).value
  activeSaveName.value = name
  const saved = savedDesigns.value[name]
  if (saved) draft.value = sanitizeDraft(saved)
}

function confirmDelete() {
  const name = activeSaveName.value
  if (!name || !savedDesigns.value[name]) return
  const next = { ...savedDesigns.value }
  delete next[name]
  savedDesigns.value = next
  persistSavedDesigns()
  activeSaveName.value = ""
  deleteDialogOpen.value = false
  notify({ kind: "success", title: "海克斯方案已删除", message: name })
}

async function shareDesign() {
  if (!captureRef.value || sharing.value) return
  sharing.value = true

  try {
    await nextTick()
    await copyElementAsPng(captureRef.value, {
      backgroundColor: "transparent",
      pixelRatio: 3,
    })
    notify({
      kind: "success",
      title: "海克斯图片已复制",
      message: "可以直接粘贴到聊天窗口",
    })
  } catch (error) {
    notify({
      kind: "error",
      title: "海克斯图片生成失败",
      message: error instanceof Error ? error.message : String(error),
      duration: 7000,
    })
  } finally {
    sharing.value = false
  }
}
</script>

<template>
  <section class="hextech-designer">
    <header class="designer-toolbar">
      <div class="designer-title">
        <Sparkles :size="18" />
        <strong>海克斯设计</strong>
      </div>

      <label class="saved-design-switch">
        <span>方案</span>
        <select :value="activeSaveName" @change="switchSavedDesign">
          <option value="">当前草稿</option>
          <option v-for="name in savedNames" :key="name" :value="name">{{ name }}</option>
        </select>
      </label>

      <div class="designer-actions">
        <button @click="openSaveDialog">
          <Save :size="15" />
          保存
        </button>
        <button :disabled="!activeSaveName" title="删除当前保存方案" @click="deleteDialogOpen = true">
          <Trash2 :size="15" />
          删除
        </button>
        <button class="primary" :disabled="sharing" @click="shareDesign">
          <LoaderCircle v-if="sharing" class="spin" :size="15" />
          <ClipboardCopy v-else :size="15" />
          {{ sharing ? "生成中" : "分享" }}
        </button>
      </div>
    </header>

    <div class="designer-workspace">
      <section class="designer-editor">
        <div class="field-group">
          <span class="field-label">品质</span>
          <div class="rarity-control">
            <button
              v-for="option in rarityOptions"
              :key="option.value"
              :class="[option.value, { active: draft.rarity === option.value }]"
              @click="draft.rarity = option.value"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <label class="field-group">
          <span class="field-label">名称</span>
          <input v-model="draft.name" maxlength="28" placeholder="未命名强化" />
        </label>

        <label class="field-group">
          <span class="field-label">类型</span>
          <input v-model="draft.category" maxlength="12" placeholder="综合" />
        </label>

        <label class="field-group description-field">
          <span class="field-label">描述</span>
          <textarea v-model="draft.description" maxlength="180" placeholder="填写强化效果"></textarea>
          <small>{{ draft.description.length }}/180</small>
        </label>

        <section class="icon-picker">
          <header>
            <strong>海克斯图标</strong>
            <span>{{ filteredAugments.length }}/{{ augmentList.length }}</span>
          </header>
          <label class="icon-search">
            <Search :size="15" />
            <input v-model="iconSearch" placeholder="搜索强化名称或 ID" />
          </label>

          <div class="icon-grid" v-if="filteredAugments.length">
            <button
              v-for="augment in filteredAugments"
              :key="augment.file"
              :class="['icon-option', { active: draft.iconFile === augment.file }]"
              :title="`${augment.name} · ${augment.englishName}`"
              @click="selectAugment(augment)"
            >
              <span class="local-icon">
                <img :src="augment.url" :alt="augment.name" loading="lazy" decoding="async" />
              </span>
            </button>
          </div>
          <div class="icon-empty" v-else>
            没有匹配的强化图标
          </div>
        </section>
      </section>

      <section class="designer-preview">
        <div class="preview-heading">
          <strong>预览</strong>
          <span>{{ rarityLabel }}</span>
        </div>

        <div ref="captureRef" :class="['hextech-share-canvas', draft.rarity]">
          <article class="hextech-card" :style="{ backgroundImage: `url(${selectedBorder})` }">
            <div class="hextech-card-content">
              <div class="hextech-card-icon">
                <img
                  v-if="selectedAugment?.url"
                  :src="selectedAugment.url"
                  :alt="selectedAugment?.name || displayName"
                />
                <Sparkles v-else :size="52" />
              </div>

              <h2 :style="titleStyle">{{ displayName }}</h2>
              <span class="hextech-category">{{ displayCategory }}</span>
              <p :style="descriptionStyle">{{ displayDescription }}</p>
            </div>
          </article>
        </div>
      </section>
    </div>

    <div class="designer-overlay" v-if="saveDialogOpen" @click.self="saveDialogOpen = false">
      <section class="designer-dialog hextech-save-dialog">
        <header>
          <strong>保存海克斯方案</strong>
          <button @click="saveDialogOpen = false">关闭</button>
        </header>
        <label>
          <span>方案名称</span>
          <input v-model="saveNameInput" maxlength="30" @keyup.enter="confirmSave" />
        </label>
        <footer>
          <button @click="saveDialogOpen = false">取消</button>
          <button class="primary" :disabled="!saveNameInput.trim()" @click="confirmSave">保存</button>
        </footer>
      </section>
    </div>

    <div class="designer-overlay" v-if="deleteDialogOpen" @click.self="deleteDialogOpen = false">
      <section class="designer-dialog">
        <header>
          <strong>删除海克斯方案</strong>
          <button @click="deleteDialogOpen = false">关闭</button>
        </header>
        <p>确认删除“{{ activeSaveName }}”？当前草稿内容会继续保留。</p>
        <footer>
          <button @click="deleteDialogOpen = false">取消</button>
          <button class="danger" @click="confirmDelete">确认删除</button>
        </footer>
      </section>
    </div>
  </section>
</template>

<style scoped>
@font-face {
  font-family: "Beaufort Hextech";
  src: url("../assets/hextech/BeaufortforLOL-Bold.ttf") format("truetype");
  font-display: swap;
  font-weight: 700;
}

.hextech-designer {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid #dce7e4;
  border-radius: 8px;
  background: #f5f9f8;
}

.designer-toolbar,
.designer-title,
.saved-design-switch,
.designer-actions,
.icon-picker header,
.icon-search,
.preview-heading,
.designer-dialog header,
.designer-dialog footer {
  display: flex;
  align-items: center;
}

.designer-toolbar {
  min-height: 52px;
  gap: 14px;
  border-bottom: 1px solid #dce7e4;
  background: #ffffff;
  padding: 8px 12px;
}

.designer-title {
  flex: 0 0 auto;
  gap: 7px;
  color: #20353a;
}

.designer-title strong {
  font-size: 16px;
  font-weight: 950;
}

.saved-design-switch {
  min-width: 0;
  gap: 7px;
  color: #5e7176;
  font-size: 12px;
  font-weight: 900;
}

.saved-design-switch select {
  width: min(220px, 24vw);
}

.designer-actions {
  margin-left: auto;
  gap: 7px;
}

.designer-actions button,
.designer-dialog button {
  display: inline-flex;
  height: 34px;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border: 1px solid #c9dad6;
  border-radius: 7px;
  color: #29464b;
  background: #f7fbfa;
  font-size: 12px;
  font-weight: 900;
  padding: 0 11px;
  cursor: pointer;
}

.designer-actions button.primary,
.designer-dialog button.primary {
  border-color: #18715f;
  color: #ffffff;
  background: #18715f;
}

.designer-actions button:disabled,
.designer-dialog button:disabled {
  opacity: 0.48;
  cursor: default;
}

.designer-workspace {
  display: grid;
  min-height: 0;
  flex: 1;
  grid-template-columns: minmax(420px, 1fr) minmax(390px, 0.78fr);
  overflow: hidden;
}

.designer-editor {
  min-height: 0;
  overflow: auto;
  border-right: 1px solid #dce7e4;
  background: #ffffff;
  padding: 16px;
}

.field-group {
  display: grid;
  grid-template-columns: 72px minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
}

.field-label {
  color: #334b51;
  font-size: 13px;
  font-weight: 950;
}

.field-group input,
.field-group textarea,
.saved-design-switch select,
.icon-search input,
.designer-dialog input {
  border: 1px solid #cddbd8;
  border-radius: 7px;
  outline: 0;
  color: #20343a;
  background: #fbfdfc;
  font-size: 13px;
  font-weight: 800;
}

.field-group input,
.saved-design-switch select,
.designer-dialog input {
  height: 35px;
  padding: 0 10px;
}

.field-group input:focus,
.field-group textarea:focus,
.icon-search:focus-within,
.designer-dialog input:focus {
  border-color: #27806f;
  box-shadow: 0 0 0 3px rgba(39, 128, 111, 0.11);
}

.description-field {
  align-items: start;
}

.description-field textarea {
  min-height: 108px;
  resize: vertical;
  line-height: 1.55;
  padding: 9px 10px;
}

.description-field small {
  grid-column: 2;
  justify-self: end;
  margin-top: -8px;
  color: #839195;
  font-size: 11px;
  font-weight: 800;
}

.rarity-control {
  display: grid;
  grid-template-columns: repeat(3, minmax(78px, 1fr));
  gap: 6px;
}

.rarity-control button {
  height: 35px;
  border: 1px solid #d2dfdc;
  border-radius: 7px;
  color: #40565b;
  background: #f8fbfa;
  font-size: 13px;
  font-weight: 950;
  cursor: pointer;
}

.rarity-control button.silver.active {
  border-color: #899da0;
  color: #20363b;
  background: #dfe8e8;
}

.rarity-control button.gold.active {
  border-color: #b88a3d;
  color: #4c3510;
  background: #f6d991;
}

.rarity-control button.prismatic.active {
  border-color: #9b84d8;
  color: #362c63;
  background: linear-gradient(100deg, #ddf8f5, #ded4ff 50%, #f7d7eb);
}

.icon-picker {
  margin-top: 16px;
  border-top: 1px solid #e0e9e7;
  padding-top: 14px;
}

.icon-picker header,
.preview-heading {
  justify-content: space-between;
  gap: 10px;
}

.icon-picker header strong,
.preview-heading strong {
  color: #243a40;
  font-size: 14px;
  font-weight: 950;
}

.icon-picker header span,
.preview-heading span {
  color: #708187;
  font-size: 12px;
  font-weight: 900;
}

.icon-search {
  height: 35px;
  gap: 7px;
  border: 1px solid #cddbd8;
  border-radius: 7px;
  color: #718388;
  background: #fbfdfc;
  margin: 10px 0;
  padding: 0 10px;
}

.icon-search input {
  min-width: 0;
  flex: 1;
  border: 0;
  background: transparent;
  box-shadow: none;
}

.icon-grid {
  display: grid;
  max-height: 310px;
  grid-template-columns: repeat(auto-fill, minmax(56px, 1fr));
  gap: 7px;
  overflow: auto;
  padding: 2px 4px 2px 0;
}

.icon-option {
  display: flex;
  min-width: 0;
  height: 58px;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border: 1px solid #d9e4e1;
  border-radius: 7px;
  color: #3d5358;
  background: #f7faf9;
  padding: 5px;
  cursor: pointer;
}

.icon-option:hover,
.icon-option.active {
  border-color: #27806f;
  background: #edf7f4;
}

.icon-option.active {
  box-shadow: inset 0 0 0 1px #27806f;
}

.local-icon {
  display: grid;
  width: 46px;
  height: 46px;
  flex: 0 0 auto;
  place-items: center;
  border-radius: 5px;
  background: #102729;
}

.local-icon img {
  display: block;
  width: 42px;
  height: 42px;
  object-fit: contain;
}

.icon-empty {
  display: grid;
  min-height: 150px;
  place-items: center;
  color: #77888d;
  font-size: 13px;
  font-weight: 900;
}

.designer-preview {
  display: flex;
  min-height: 0;
  flex-direction: column;
  align-items: center;
  overflow: auto;
  background:
    linear-gradient(rgba(255, 255, 255, 0.82), rgba(245, 249, 248, 0.92)),
    repeating-linear-gradient(45deg, #e7efed 0 12px, #f5f8f7 12px 24px);
  padding: 16px 20px 28px;
}

.preview-heading {
  width: min(340px, 100%);
  margin-bottom: 12px;
}

.hextech-share-canvas {
  width: 340px;
  height: 561px;
  flex: 0 0 auto;
  background: transparent;
}

.hextech-card {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  background-position: center;
  background-repeat: no-repeat;
  background-size: 100% 100%;
}

.hextech-card-content {
  position: relative;
  z-index: 3;
  display: flex;
  height: 100%;
  flex-direction: column;
  align-items: center;
  padding: 62px 42px 58px;
  text-align: center;
}

.hextech-card-icon {
  display: grid;
  width: 124px;
  height: 124px;
  flex: 0 0 auto;
  place-items: center;
  color: #eef6f3;
}

.hextech-card-icon img {
  display: block;
  width: 112px;
  height: 112px;
  object-fit: contain;
}

.hextech-card h2 {
  max-width: 100%;
  margin: 22px 0 14px;
  color: #f7faf8;
  font-family: "Beaufort Hextech", "Microsoft YaHei UI", sans-serif;
  font-weight: 950;
  line-height: 1.25;
  overflow-wrap: anywhere;
}

.hextech-category {
  display: inline-flex;
  min-height: 24px;
  align-items: center;
  justify-content: center;
  border: 1px solid rgba(238, 242, 234, 0.35);
  border-radius: 3px;
  color: #e4e9e4;
  background: rgba(230, 236, 230, 0.08);
  font-family: "Beaufort Hextech", "Microsoft YaHei UI", sans-serif;
  font-size: 13px;
  font-weight: 950;
  padding: 2px 9px;
}

.hextech-card p {
  display: flex;
  min-height: 0;
  flex: 1;
  align-items: center;
  overflow: hidden;
  margin: 18px 0 0;
  color: #eef3ef;
  font-family: "Beaufort Hextech", "Microsoft YaHei UI", sans-serif;
  font-weight: 850;
  line-height: 1.55;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.designer-overlay {
  position: fixed;
  z-index: 1800;
  inset: 0;
  display: grid;
  place-items: center;
  background: rgba(15, 25, 28, 0.46);
}

.designer-dialog {
  width: min(420px, calc(100vw - 40px));
  border: 1px solid rgba(255, 255, 255, 0.74);
  border-radius: 8px;
  background: #f8fbfa;
  box-shadow: 0 24px 70px rgba(20, 40, 44, 0.28);
  padding: 16px;
}

.designer-dialog header {
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.designer-dialog header strong {
  color: #20333a;
  font-size: 18px;
  font-weight: 950;
}

.designer-dialog label {
  display: flex;
  flex-direction: column;
  gap: 7px;
  color: #52666b;
  font-size: 13px;
  font-weight: 900;
}

.designer-dialog input {
  width: 100%;
}

.designer-dialog p {
  margin: 0;
  color: #52666b;
  font-size: 14px;
  font-weight: 800;
  line-height: 1.6;
}

.designer-dialog footer {
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}

.designer-dialog button.danger {
  border-color: #bd3f48;
  color: #ffffff;
  background: #bd3f48;
}

.spin {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@media (max-width: 980px) {
  .designer-workspace {
    grid-template-columns: 1fr;
    overflow: auto;
  }

  .designer-editor {
    overflow: visible;
    border-right: 0;
    border-bottom: 1px solid #dce7e4;
  }

  .designer-preview {
    overflow: visible;
  }
}

@media (max-width: 720px) {
  .designer-toolbar {
    align-items: stretch;
    flex-wrap: wrap;
  }

  .saved-design-switch {
    order: 3;
    width: 100%;
  }

  .saved-design-switch select {
    width: 100%;
  }

  .designer-actions {
    flex-wrap: wrap;
  }

  .field-group {
    grid-template-columns: 1fr;
  }

  .description-field small {
    grid-column: 1;
  }
}
</style>
