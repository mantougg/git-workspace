<template>
  <n-modal
    :show="show"
    preset="card"
    title="Smart Merge — 冲突解决"
    style="width: 90%; max-width: 1200px; margin-top: 3vh"
    :mask-closable="false"
    :close-on-esc="false"
    @update:show="$emit('update:show', $event)"
  >
    <div class="smart-merge">
      <!-- File list sidebar -->
      <div class="file-list">
        <div class="file-list-header">
          冲突文件（{{ resolvedCount }}/{{ conflictFiles.length }} 已解决）
        </div>
        <div class="file-list-body">
          <div
            v-for="f in conflictFiles"
            :key="f.path"
            :class="['file-item', { active: selectedPath === f.path, resolved: isResolved(f.path) }]"
            @click="selectFile(f.path)"
          >
            <span
              class="file-status-icon"
              :class="iconClass(f.conflictType)"
              :title="typeLabel(f.conflictType)"
            >{{ typeIcon(f.conflictType) }}</span>
            <span class="file-name">{{ f.path }}</span>
            <n-icon v-if="isResolved(f.path)" class="resolved-icon" size="14">
              <CheckmarkOutline />
            </n-icon>
          </div>
          <n-empty v-if="conflictFiles.length === 0" description="没有冲突文件" />
        </div>
      </div>

      <!-- Resolution panel -->
      <div class="resolution-panel">
        <template v-if="selectedPath">
          <div class="resolution-header">
            <span class="resolution-file">{{ selectedPath }}</span>
            <n-tag size="small" :type="typeTagType(selectedType)">
              {{ typeLabel(selectedType) }}
            </n-tag>
            <n-tag v-if="!isDeletionConflict" size="small" :type="allResolved ? 'success' : 'warning'">
              {{ hunkProgress }}
            </n-tag>
            <div class="resolution-actions">
              <template v-if="isDeletionConflict">
                <n-button
                  size="small"
                  :disabled="isResolved(selectedPath)"
                  :type="recommendedStrategy === 'ours' ? 'primary' : 'default'"
                  @click="resolveWholeFile('ours')"
                >
                  {{ recommendedStrategy === "ours" ? "采用 Ours（推荐）" : "采用 Ours" }}
                </n-button>
                <n-button
                  size="small"
                  :disabled="isResolved(selectedPath)"
                  :type="recommendedStrategy === 'theirs' ? 'primary' : 'default'"
                  @click="resolveWholeFile('theirs')"
                >
                  {{ recommendedStrategy === "theirs" ? "采用 Theirs（推荐）" : "采用 Theirs" }}
                </n-button>
              </template>
              <template v-else>
                <n-button size="small" @click="acceptAllOurs">全部采用 Ours</n-button>
                <n-button size="small" @click="acceptAllTheirs">全部采用 Theirs</n-button>
              </template>
              <n-button
                size="small"
                type="primary"
                :disabled="!allResolved"
                @click="applyResolution"
              >
                应用并标记已解决
              </n-button>
            </div>
          </div>

          <div v-if="loadingContent" class="resolution-loading">
            <n-spin size="medium" />
          </div>

          <div v-else class="resolution-body">
            <!-- Normal (non-conflict) lines -->
            <div v-if="normalLinesBefore.length > 0" class="hunk-normal">
              <pre class="normal-content">{{ normalLinesBefore.join("\n") }}</pre>
            </div>

            <!-- Conflict hunks -->
            <div
              v-for="(hunk, idx) in hunks"
              :key="idx"
              :class="['conflict-hunk', { resolved: hunkResolutions.get(idx) }]"
            >
              <div v-if="hunkResolutions.get(idx)" class="hunk-resolved-preview">
                <div class="hunk-resolved-label">
                  <n-tag size="tiny" type="success">已解决</n-tag>
                  <span class="hunk-resolved-strategy">{{ strategyLabel(hunkResolutions.get(idx)!) }}</span>
                  <n-button size="tiny" quaternary @click="resetHunk(idx)">撤销</n-button>
                </div>
                <pre class="resolved-content">{{ getHunkResult(idx) }}</pre>
              </div>
              <template v-else>
                <div class="hunk-ours">
                  <div class="hunk-side-header">
                    <span class="side-label ours-label">OURS（本地）</span>
                    <n-button size="tiny" type="primary" @click="acceptHunkOurs(idx)">
                      ← 采用 Ours
                    </n-button>
                  </div>
                  <pre class="side-content ours-content">{{ hunk.ours.join("\n") }}</pre>
                </div>
                <div class="hunk-theirs">
                  <div class="hunk-side-header">
                    <n-button size="tiny" type="warning" @click="acceptHunkTheirs(idx)">
                      采用 Theirs →
                    </n-button>
                    <span class="side-label theirs-label">THEIRS（远程）</span>
                  </div>
                  <pre class="side-content theirs-content">{{ hunk.theirs.join("\n") }}</pre>
                </div>
              </template>
            </div>

            <!-- Normal lines after last conflict -->
            <div v-if="normalLinesAfter.length > 0" class="hunk-normal">
              <pre class="normal-content">{{ normalLinesAfter.join("\n") }}</pre>
            </div>

            <div v-if="hunks.length === 0 && !loadingContent">
              <!-- GF-12: 删除型冲突没有冲突标记（git 只把存活侧写入工作区），
                   展示三态内容而不是「没有冲突标记」的死角提示 -->
              <div v-if="isDeletionConflict" class="delete-conflict">
                <div class="delete-conflict-desc">{{ deleteConflictDesc }}</div>
                <div class="delete-sides">
                  <div class="delete-side">
                    <div class="delete-side-title">BASE</div>
                    <pre class="delete-side-content">{{ content?.base ?? "（无共同祖先）" }}</pre>
                  </div>
                  <div class="delete-side">
                    <div class="delete-side-title">OURS（本地）</div>
                    <pre class="delete-side-content">{{ content?.ours ?? "（本侧已删除）" }}</pre>
                  </div>
                  <div class="delete-side">
                    <div class="delete-side-title">THEIRS（远程）</div>
                    <pre class="delete-side-content">{{ content?.theirs ?? "（对侧已删除）" }}</pre>
                  </div>
                </div>
              </div>
              <div v-else class="no-conflicts">
                该文件没有冲突标记
              </div>
            </div>
          </div>
        </template>
        <div v-else class="resolution-empty">
          选择左侧文件开始解决冲突
        </div>
      </div>
    </div>

    <template #footer>
      <div class="dialog-footer">
        <n-button type="error" dashed @click="handleAbort">中止（Abort）</n-button>
        <div class="footer-right">
          <span v-if="allFilesResolved" class="footer-hint">全部冲突已解决，可以继续</span>
          <n-button
            type="primary"
            :disabled="!allFilesResolved"
            @click="handleContinue"
          >
            继续 Merge（Continue）
          </n-button>
        </div>
      </div>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { CheckmarkOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import {
  getConflictContent,
  getOperationState,
  resolveConflict,
  resolveConflictWithContent,
} from "@/api/conflict";
import { mergeContinue, mergeAbort } from "@/api/merge";
import type { ConflictContent } from "@/types/conflict";
import { errMsg } from "@/utils/error";

interface ConflictFile {
  path: string;
  conflictType: string;
}

interface ConflictHunk {
  ours: string[];
  theirs: string[];
}

type HunkStrategy = "ours" | "theirs";

const props = defineProps<{
  show: boolean;
  repoPath: string;
  conflicts: string[];
  baseOid: string | null;
}>();

const emit = defineEmits<{
  "update:show": [value: boolean];
  resolved: [];
  aborted: [];
}>();

const message = useMessage();
const dialog = useDialog();

const selectedPath = ref("");
const loadingContent = ref(false);
const content = ref<ConflictContent | null>(null);
const resolvedPaths = ref<Set<string>>(new Set());
const hunkResolutions = ref<Map<number, HunkStrategy>>(new Map());

/**
 * GF-12: 逐文件真实冲突类型。数据源是 `get_operation_state`
 * （`src-tauri/src/core/conflict.rs::conflict_files` 按 index stage 1/2/3
 * 条目存在性判定 both-modified / both-added / deleted-by-us /
 * deleted-by-them），与 props.conflicts 同出一源（index conflicts），
 * path 一一对应。只合并、不清除、换仓库才重置——已解决文件退出 index
 * 冲突后仍能保留原类型，避免侧栏图标回跳。取不到时回落
 * "both-modified"，与修复前行为一致。
 */
const conflictTypeMap = ref<Map<string, string>>(new Map());
let conflictTypeRepo = "";

async function loadConflictTypes() {
  if (!props.repoPath) return;
  if (conflictTypeRepo !== props.repoPath) {
    conflictTypeRepo = props.repoPath;
    conflictTypeMap.value = new Map();
  }
  try {
    const state = await getOperationState(props.repoPath);
    const next = new Map(conflictTypeMap.value);
    for (const c of state.conflicts) next.set(c.path, c.conflictType);
    conflictTypeMap.value = next;
  } catch {
    // 拉取失败时保持既有类型（缺省 both-modified）
  }
}

// Build conflict file list from props (真实类型来自 conflictTypeMap)
const conflictFiles = computed<ConflictFile[]>(() =>
  props.conflicts.map((p) => ({
    path: p,
    conflictType: conflictTypeMap.value.get(p) ?? "both-modified",
  })),
);

const resolvedCount = computed(() => resolvedPaths.value.size);
const allFilesResolved = computed(
  () => props.conflicts.length > 0 && resolvedPaths.value.size >= props.conflicts.length,
);

/** 当前选中文件的冲突类型。 */
const selectedType = computed(
  () => conflictTypeMap.value.get(selectedPath.value) ?? "both-modified",
);
const isDeletionConflict = computed(
  () => selectedType.value === "deleted-by-us" || selectedType.value === "deleted-by-them",
);
/**
 * GF-12: 删除型冲突的默认推荐动作——保留仍然持有该文件的一侧。
 * deleted-by-us（本侧删除）→ 推荐 Theirs；deleted-by-them（对侧删除）→ 推荐 Ours。
 */
const recommendedStrategy = computed<"ours" | "theirs" | null>(() => {
  if (selectedType.value === "deleted-by-us") return "theirs";
  if (selectedType.value === "deleted-by-them") return "ours";
  return null;
});

const deleteConflictDesc = computed(() =>
  selectedType.value === "deleted-by-us"
    ? "本侧已删除该文件，对侧仍保留了修改。推荐采用 Theirs 保留对侧版本；如确认删除，选择采用 Ours 将移除该文件。"
    : "对侧已删除该文件，本侧仍保留了修改。推荐采用 Ours 保留本侧版本；如确认删除，选择采用 Theirs 将移除该文件。",
);

// Parse conflict markers into hunks
const hunks = computed<ConflictHunk[]>(() => {
  const wt = content.value?.worktree;
  if (!wt) return [];
  return parseConflictMarkers(wt);
});

// Normal lines before first conflict and after last conflict
const normalLinesBefore = computed(() => {
  const wt = content.value?.worktree;
  if (!wt) return [];
  const lines = wt.split("\n");
  const result: string[] = [];
  for (const line of lines) {
    if (line.startsWith("<<<<<<<")) break;
    result.push(line);
  }
  return result;
});

const normalLinesAfter = computed(() => {
  const wt = content.value?.worktree;
  if (!wt) return [];
  const lines = wt.split("\n");
  const result: string[] = [];
  for (let i = lines.length - 1; i >= 0; i--) {
    if (lines[i].startsWith(">>>>>>>") || lines[i].startsWith("=======") || lines[i].startsWith("<<<<<<<")) {
      break;
    }
    result.unshift(lines[i]);
  }
  return result;
});

const hunkProgress = computed(() => {
  const resolved = [...hunkResolutions.value.values()].length;
  return `Hunk ${resolved}/${hunks.value.length} 已解决`;
});

const allResolved = computed(
  () => hunks.value.length > 0 && hunkResolutions.value.size >= hunks.value.length,
);

function parseConflictMarkers(text: string): ConflictHunk[] {
  const lines = text.split("\n");
  const result: ConflictHunk[] = [];
  let i = 0;
  while (i < lines.length) {
    if (lines[i].startsWith("<<<<<<<")) {
      const ours: string[] = [];
      const theirs: string[] = [];
      i++;
      // Collect OURS side
      while (i < lines.length && !lines[i].startsWith("=======")) {
        ours.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++; // skip =======
      // Collect THEIRS side
      while (i < lines.length && !lines[i].startsWith(">>>>>>>")) {
        theirs.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++; // skip >>>>>>>
      result.push({ ours, theirs });
    } else {
      i++;
    }
  }
  return result;
}

function isResolved(path: string): boolean {
  return resolvedPaths.value.has(path);
}

function typeIcon(t: string): string {
  switch (t) {
    case "both-modified": return "M";
    case "both-added": return "A";
    case "deleted-by-us": return "D";
    case "deleted-by-them": return "D";
    default: return "!";
  }
}

/** 冲突类型中文标签（与 ConflictResolver.vue 的 typeLabel 保持一致）。 */
function typeLabel(t: string): string {
  switch (t) {
    case "both-modified": return "双方修改";
    case "both-added": return "双方新增";
    case "deleted-by-us": return "本侧删除";
    case "deleted-by-them": return "对侧删除";
    default: return t;
  }
}

/** 头部类型标签配色：删除=error、双方新增=info、内容冲突=warning。 */
function typeTagType(t: string): "warning" | "info" | "error" {
  switch (t) {
    case "both-added": return "info";
    case "deleted-by-us":
    case "deleted-by-them":
      return "error";
    default: return "warning";
  }
}

/** 侧栏图标配色：删除一律 danger，其余按冲突形状区分。 */
function iconClass(t: string): string {
  switch (t) {
    case "both-added": return "icon-added";
    case "deleted-by-us":
    case "deleted-by-them":
      return "icon-deleted";
    default: return "icon-modified";
  }
}

function strategyLabel(s: HunkStrategy): string {
  return s === "ours" ? "已采用 Ours" : "已采用 Theirs";
}

function getHunkResult(idx: number): string {
  const hunk = hunks.value[idx];
  const strategy = hunkResolutions.value.get(idx);
  if (!hunk || !strategy) return "";
  return strategy === "ours" ? hunk.ours.join("\n") : hunk.theirs.join("\n");
}

async function selectFile(path: string) {
  selectedPath.value = path;
  hunkResolutions.value = new Map();
  loadingContent.value = true;
  try {
    content.value = await getConflictContent(props.repoPath, path);
  } catch (e) {
    message.error("加载冲突内容失败: " + errMsg(e));
  } finally {
    loadingContent.value = false;
  }
}

function acceptHunkOurs(idx: number) {
  hunkResolutions.value = new Map([...hunkResolutions.value, [idx, "ours"]]);
}

function acceptHunkTheirs(idx: number) {
  hunkResolutions.value = new Map([...hunkResolutions.value, [idx, "theirs"]]);
}

function resetHunk(idx: number) {
  const next = new Map(hunkResolutions.value);
  next.delete(idx);
  hunkResolutions.value = next;
}

function acceptAllOurs() {
  const next = new Map<number, HunkStrategy>();
  hunks.value.forEach((_, idx) => next.set(idx, "ours"));
  hunkResolutions.value = next;
}

function acceptAllTheirs() {
  const next = new Map<number, HunkStrategy>();
  hunks.value.forEach((_, idx) => next.set(idx, "theirs"));
  hunkResolutions.value = next;
}

/**
 * GF-12: 删除型冲突整文件解决。这类冲突没有冲突标记（git 只把存活侧
 * 写入工作区），逐 hunk 流程不适用——直接走后端策略命令，所选侧缺席时
 * 由后端移除文件（与 ConflictResolver 的 Use Ours / Use Theirs 同语义）。
 */
async function resolveWholeFile(strategy: "ours" | "theirs") {
  const path = selectedPath.value;
  if (!path) return;
  try {
    await resolveConflict(props.repoPath, path, strategy);
    message.success(`已按 ${strategy === "ours" ? "Ours" : "Theirs"} 解决：${path}`);
    resolvedPaths.value = new Set([...resolvedPaths.value, path]);
    // Auto-select next unresolved file
    const nextUnresolved = props.conflicts.find(
      (p) => p !== path && !resolvedPaths.value.has(p),
    );
    if (nextUnresolved) {
      await selectFile(nextUnresolved);
    }
  } catch (e) {
    message.error("解决失败: " + errMsg(e));
  }
}

/** Build resolved file content from hunks + normal lines. */
function buildResolvedContent(): string {
  const wt = content.value?.worktree;
  if (!wt) return "";

  const lines = wt.split("\n");
  const result: string[] = [];
  let i = 0;
  let hunkIdx = 0;

  while (i < lines.length) {
    if (lines[i].startsWith("<<<<<<<")) {
      // Skip past <<<<<<<
      i++;
      // Skip OURS lines until =======
      while (i < lines.length && !lines[i].startsWith("=======")) i++;
      if (i < lines.length) i++; // skip =======
      // Skip THEIRS lines until >>>>>>>
      while (i < lines.length && !lines[i].startsWith(">>>>>>>")) i++;
      if (i < lines.length) i++; // skip >>>>>>>

      // Insert resolved content
      const strategy = hunkResolutions.value.get(hunkIdx);
      const hunk = hunks.value[hunkIdx];
      if (strategy && hunk) {
        const chosen = strategy === "ours" ? hunk.ours : hunk.theirs;
        result.push(...chosen);
      }
      hunkIdx++;
    } else {
      result.push(lines[i]);
      i++;
    }
  }
  return result.join("\n");
}

async function applyResolution() {
  const path = selectedPath.value;
  if (!path) return;
  try {
    const resolved = buildResolvedContent();
    await resolveConflictWithContent(props.repoPath, path, resolved);
    message.success(`已应用解决：${path}`);
    resolvedPaths.value = new Set([...resolvedPaths.value, path]);
    // Auto-select next unresolved file
    const nextUnresolved = props.conflicts.find(
      (p) => p !== path && !resolvedPaths.value.has(p),
    );
    if (nextUnresolved) {
      await selectFile(nextUnresolved);
    }
  } catch (e) {
    message.error("解决失败: " + errMsg(e));
  }
}

async function handleContinue() {
  try {
    const oid = await mergeContinue(props.repoPath);
    message.success(`Merge 已完成（${oid.slice(0, 7)}）`);
    emit("resolved");
    emit("update:show", false);
  } catch (e) {
    message.error("Continue 失败: " + errMsg(e));
  }
}

/**
 * GF-12: baseOid 的落地用途。它不是 merge base（共同祖先）——smart_pull /
 * merge 传入的是 **Merge 前 HEAD**，即 `merge_abort`（hard reset 回 HEAD）
 * 的落点，所以按 §46 危险操作规范把它展示在 Abort 二次确认里；三路合并
 * 的 BASE 内容由 get_conflict_content 的 base 字段单独提供（删除型冲突
 * 的三态预览已用它渲染），两者不混用。
 */
const abortTargetHint = computed(() =>
  props.baseOid ? ` 回到操作前 HEAD ${props.baseOid.slice(0, 7)}` : "",
);

async function handleAbort() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Abort 确认（Dangerous）",
        content: `仓库：${props.repoPath}\n将放弃当前 Merge 操作并恢复到操作前状态（hard reset${abortTargetHint.value}），冲突中的修改将丢失。`,
        positiveText: "中止并恢复",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject(new Error("cancelled")),
        onClose: () => reject(new Error("cancelled")),
      });
    });
  } catch {
    return;
  }
  try {
    await mergeAbort(props.repoPath);
    message.success("已中止并恢复");
    emit("aborted");
    emit("update:show", false);
  } catch (e) {
    message.error("Abort 失败: " + errMsg(e));
  }
}

// Auto-select first file when dialog opens
watch(
  () => props.show,
  async (val) => {
    if (val && props.conflicts.length > 0) {
      await loadConflictTypes();
      if (!selectedPath.value) {
        resolvedPaths.value = new Set();
        hunkResolutions.value = new Map();
        await selectFile(props.conflicts[0]);
      }
    }
  },
);

// Reset state when conflicts change
watch(
  () => props.conflicts,
  async (files) => {
    resolvedPaths.value = new Set();
    hunkResolutions.value = new Map();
    await loadConflictTypes();
    if (files.length > 0) {
      await selectFile(files[0]);
    } else {
      selectedPath.value = "";
      content.value = null;
    }
  },
);
</script>

<style scoped>
.smart-merge {
  display: flex;
  height: 65vh;
  min-height: 400px;
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius);
  overflow: hidden;
}

/* File list sidebar */
.file-list {
  width: 260px;
  border-right: 1px solid var(--gw-border);
  display: flex;
  flex-direction: column;
  background: var(--gw-bg-hover);
}

.file-list-header {
  padding: 8px 12px;
  font-size: 13px;
  font-weight: 600;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.file-list-body {
  flex: 1;
  overflow-y: auto;
}

.file-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 8px 12px;
  cursor: pointer;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
  transition: background 0.15s;
}

.file-item:hover {
  background: var(--gw-bg-panel);
}

.file-item.active {
  background: var(--gw-bg-panel);
  border-left: 3px solid var(--gw-accent);
}

.file-item.resolved .file-name {
  color: var(--gw-success);
  text-decoration: line-through;
}

.file-status-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 3px;
  font-size: 11px;
  font-weight: 700;
  background: var(--gw-warning);
  color: #fff;
}

/* GF-12: 按冲突形状着色的图标修饰类（与 typeTagType 同色谱） */
.file-status-icon.icon-modified {
  background: var(--gw-warning);
}

.file-status-icon.icon-added {
  background: var(--gw-info);
}

.file-status-icon.icon-deleted {
  background: var(--gw-danger);
}

.file-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--gw-font-mono);
  font-size: 12px;
}

.resolved-icon {
  color: var(--gw-success);
}

/* Resolution panel */
.resolution-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.resolution-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 8px 12px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.resolution-file {
  flex: 1;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.resolution-actions {
  display: flex;
  gap: var(--gw-space-2);
}

.resolution-loading {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.resolution-body {
  flex: 1;
  overflow-y: auto;
  padding: 0;
}

.resolution-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--gw-text-dim);
  font-size: 14px;
}

/* Normal (non-conflict) lines */
.hunk-normal {
  border-bottom: 1px solid var(--gw-border);
}

.normal-content {
  margin: 0;
  padding: 4px 12px;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  white-space: pre;
  color: var(--gw-text-dim);
  max-height: 80px;
  overflow: auto;
}

/* Conflict hunk */
.conflict-hunk {
  border-bottom: 2px solid var(--gw-border);
  margin-bottom: 0;
}

.conflict-hunk.resolved {
  border-left: 3px solid var(--gw-success);
}

/* Ours / Theirs side-by-side within a hunk */
.hunk-ours,
.hunk-theirs {
  border-bottom: 1px dashed var(--gw-border);
}

.hunk-side-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 4px 10px;
  background: var(--gw-bg-hover);
  border-bottom: 1px solid var(--gw-border);
}

.side-label {
  flex: 1;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.ours-label {
  color: var(--gw-info);
}

.theirs-label {
  color: var(--gw-warning);
  text-align: right;
}

.side-content {
  margin: 0;
  padding: 6px 12px;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  white-space: pre;
  min-height: 24px;
  max-height: 200px;
  overflow: auto;
}

.ours-content {
  background: color-mix(in srgb, var(--gw-info) 8%, transparent);
}

.theirs-content {
  background: color-mix(in srgb, var(--gw-warning) 8%, transparent);
}

/* Resolved hunk preview */
.hunk-resolved-preview {
  padding: 6px 12px;
  background: color-mix(in srgb, var(--gw-success) 6%, transparent);
}

.hunk-resolved-label {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  margin-bottom: 4px;
}

.hunk-resolved-strategy {
  font-size: 12px;
  color: var(--gw-success);
  font-weight: 500;
}

.resolved-content {
  margin: 0;
  padding: 4px 0;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  white-space: pre;
  color: var(--gw-text);
}

.no-conflicts {
  padding: 20px;
  text-align: center;
  color: var(--gw-text-dim);
}

/* GF-12: 删除型冲突（无冲突标记）——类型说明 + BASE/OURS/THEIRS 三态预览 */
.delete-conflict {
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.delete-conflict-desc {
  font-size: var(--gw-text-md);
  line-height: 1.6;
  color: var(--gw-text-dim);
}

.delete-sides {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--gw-space-2);
}

.delete-side {
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius);
  overflow: hidden;
  background: var(--gw-bg-panel);
}

.delete-side-title {
  padding: 4px 10px;
  font-size: var(--gw-text-xs);
  font-weight: 600;
  letter-spacing: 0.5px;
  color: var(--gw-text-dim);
  background: var(--gw-bg-hover);
  border-bottom: 1px solid var(--gw-border);
}

.delete-side-content {
  margin: 0;
  padding: 6px 10px;
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-xs);
  white-space: pre;
  max-height: 200px;
  overflow: auto;
  color: var(--gw-text);
}

/* Footer */
.dialog-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.footer-right {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}

.footer-hint {
  font-size: 13px;
  color: var(--gw-success);
}
</style>
