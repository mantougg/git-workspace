<template>
  <div class="repo-tools">
    <div class="tools-header">
      <RepoSwitcher @change="onRepoSwitch" />
      <n-button size="small" :loading="loading" @click="loadAll">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
    </div>

    <n-spin :show="loading">
      <div class="tools-body">
        <n-tabs type="line" v-model:value="activeTab">
          <!-- ── Submodules ──────────────────────────────── -->
          <n-tab-pane name="submodules" tab="Submodules">
            <div class="tab-toolbar">
              <n-button size="small" @click="subOp('init')">Init</n-button>
              <n-button size="small" :loading="subBusy" @click="subOp('update')">Update（--init --recursive）</n-button>
              <n-button size="small" @click="subOp('sync')">Sync</n-button>
              <n-button size="small" type="primary" ghost @click="subAdd">Add…</n-button>
            </div>
            <div v-for="sm in submodules" :key="sm.path" class="row">
              <n-tag size="small" :type="statusTagType(sm.status)" :bordered="false">
                {{ statusLabel(sm.status) }}
              </n-tag>
              <span class="row-main mono">{{ sm.path }}</span>
              <span class="row-dim mono">{{ sm.sha.slice(0, 7) }}</span>
              <span v-if="sm.url" class="row-dim">{{ sm.url }}</span>
              <n-dropdown
                trigger="click"
                :options="[
                  { label: 'Init', key: 'init' },
                  { label: 'Update', key: 'update' },
                  { label: 'Sync', key: 'sync' },
                  { type: 'divider', key: 'd' },
                  { label: 'Remove…', key: 'remove' },
                ]"
                @select="(key: string) => subOpFor(sm, key)"
              >
                <n-button size="small" text @click.stop>
                  <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                </n-button>
              </n-dropdown>
            </div>
            <n-empty v-if="submodules.length === 0" description="无子模块" class="tab-empty" />
          </n-tab-pane>

          <!-- ── LFS ─────────────────────────────────────── -->
          <n-tab-pane name="lfs" tab="Git LFS">
            <div class="tab-toolbar">
              <n-button size="small" :loading="lfsBusy" @click="lfsRun('fetch')">Fetch</n-button>
              <n-button size="small" :loading="lfsBusy" @click="lfsRun('pull')">Pull</n-button>
              <n-button size="small" :loading="lfsBusy" @click="lfsRun('push')">Push --all</n-button>
              <n-button size="small" @click="loadLocks">Locks</n-button>
            </div>
            <div v-for="f in lfsFiles" :key="f.path" class="row">
              <n-tag size="small" :type="f.state === 'synced' ? 'success' : 'warning'" :bordered="false">
                {{ f.state }}
              </n-tag>
              <span class="row-main mono">{{ f.path }}</span>
              <n-button
                v-if="activeTab === 'lfs'"
                size="tiny"
                quaternary
                @click="lockToggle('lock', f.path)"
              >lock</n-button>
            </div>
            <n-empty v-if="lfsFiles.length === 0" description="无 LFS 文件（或 LFS 未安装）" class="tab-empty" />

            <template v-if="locks.length > 0">
              <div class="section-title">活动锁</div>
              <div v-for="l in locks" :key="l.id" class="row">
                <span class="row-main mono">{{ l.path }}</span>
                <span class="row-dim">{{ l.owner ?? "?" }}</span>
                <n-button size="tiny" quaternary @click="lockToggle('unlock', l.path)">unlock</n-button>
              </div>
            </template>
          </n-tab-pane>

          <!-- ── Hooks ───────────────────────────────────── -->
          <n-tab-pane name="hooks" tab="Hooks">
            <div v-for="h in hooks" :key="h.name" class="row">
              <n-tag size="small" :type="h.enabled ? 'success' : h.exists ? 'warning' : 'default'" :bordered="false">
                {{ h.enabled ? "启用" : h.exists ? "已停用" : "未创建" }}
              </n-tag>
              <span class="row-main mono">{{ h.name }}</span>
              <n-button size="tiny" @click="openHook(h.name)">查看 / 编辑</n-button>
              <n-button
                v-if="h.enabled"
                size="tiny"
                @click="toggleHook(h.name, false)"
              >停用</n-button>
              <n-button v-else-if="h.exists" size="tiny" @click="toggleHook(h.name, true)">启用</n-button>
              <n-button v-if="h.enabled" size="tiny" type="warning" @click="runNow(h.name)">运行</n-button>
            </div>

            <!-- Hook 编辑器 -->
            <n-modal v-model:show="hookEditor.show" preset="card" :title="`Hook：${hookEditor.name}`" style="width: 720px">
              <n-input
                v-model:value="hookEditor.content"
                type="textarea"
                :rows="18"
                class="mono"
                placeholder="#!/bin/sh"
              />
              <template #footer>
                <n-button v-if="hookEditor.enabled" :loading="hookEditor.running" @click="runNow(hookEditor.name)">
                  运行
                </n-button>
                <n-button @click="hookEditor.show = false">取消</n-button>
                <n-button type="primary" @click="saveCurrentHook">保存（并启用）</n-button>
              </template>
            </n-modal>
          </n-tab-pane>
        </n-tabs>
      </div>
    </n-spin>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useDialog, useMessage } from "naive-ui";
import { EllipsisVerticalOutline, RefreshOutline } from "@vicons/ionicons5";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import { prompt } from "@/utils/prompt";
import {
  getHook,
  listHooks,
  listSubmodules,
  lfsList,
  lfsLockOp,
  lfsLocks,
  lfsOp,
  runHook,
  saveHook,
  setHookEnabled,
  submoduleOp,
  type HookInfo,
  type LfsFile,
  type LfsLock,
  type SubmoduleEntry,
} from "@/api/repoTools";
import { errMsg } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const dialog = useDialog();
const { resolveCurrentRepo } = useCurrentRepo();

const repoPath = ref("");
const loading = ref(false);
const activeTab = ref<"submodules" | "lfs" | "hooks">("submodules");

const submodules = ref<SubmoduleEntry[]>([]);
const subBusy = ref(false);

const lfsFiles = ref<LfsFile[]>([]);
const lfsBusy = ref(false);
const locks = ref<LfsLock[]>([]);

const hooks = ref<HookInfo[]>([]);
const hookEditor = ref<{ show: boolean; name: string; content: string; enabled: boolean; running: boolean }>({
  show: false,
  name: "",
  content: "",
  enabled: false,
  running: false,
});

onMounted(async () => {
  // F-14/F-17：query → 全局当前仓库 → 工作区首仓库兜底（SideNav 直达）。
  const repo = await resolveCurrentRepo();
  if (!repo) {
    message.warning("当前工作区没有可用仓库，请先在变更页扫描");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await loadAll();
});

async function onRepoSwitch(path: string) {
  repoPath.value = path;
  await loadAll();
}

async function loadAll() {
  loading.value = true;
  try {
    submodules.value = await listSubmodules(repoPath.value);
  } catch (e) {
    submodules.value = [];
    message.error("子模块列表失败: " + errMsg(e));
  }
  try {
    hooks.value = await listHooks(repoPath.value);
  } catch (e) {
    hooks.value = [];
    message.error("Hook 列表失败: " + errMsg(e));
  }
  try {
    lfsFiles.value = await lfsList(repoPath.value);
  } catch {
    // LFS 未安装：可行动错误不阻塞其余 Tab
    lfsFiles.value = [];
  }
  loading.value = false;
}

// ── Submodule ────────────────────────────────────────────────

function statusLabel(status: SubmoduleEntry["status"]): string {
  return (
    {
      synced: "已同步",
      modified: "有改动",
      uninitialized: "未初始化",
      conflict: "冲突",
      unknown: "未知",
    } as Record<string, string>
  )[status];
}

function statusTagType(status: SubmoduleEntry["status"]) {
  return (
    {
      synced: "success",
      modified: "warning",
      uninitialized: "default",
      conflict: "error",
      unknown: "default",
    } as Record<string, "success" | "warning" | "default" | "error">
  )[status];
}

async function subOp(op: string) {
  subBusy.value = true;
  try {
    const out = await submoduleOp(repoPath.value, op);
    message.success(`${op} 完成${out ? "：" + out.trim().slice(0, 200) : ""}`);
    submodules.value = await listSubmodules(repoPath.value);
  } catch (e) {
    message.error(`${op} 失败: ` + errMsg(e));
  } finally {
    subBusy.value = false;
  }
}

/** Add：依次询问 URL 与路径。 */
async function subAdd() {
  try {
    const url = await prompt(dialog, {
      title: "添加子模块",
      content: "子模块仓库 URL：",
      confirmText: "下一步",
      cancelText: "取消",
    });
    if (!url) return;
    const path = await prompt(dialog, {
      title: "添加子模块",
      content: "本地路径（如 libs/core）：",
      confirmText: "添加",
      cancelText: "取消",
    });
    if (!path) return;
    subBusy.value = true;
    try {
      await submoduleOp(repoPath.value, "add", path, url);
      message.success(`子模块 ${path} 已添加（产生待提交变更）`);
      submodules.value = await listSubmodules(repoPath.value);
    } finally {
      subBusy.value = false;
    }
  } catch (e) {
    if (e !== "cancel") message.error("添加子模块失败: " + errMsg(e));
  }
}

function subOpFor(sm: SubmoduleEntry, key: string) {
  if (key === "remove") {
    dialog.error({
      title: "移除子模块（Dangerous）",
      content: `仓库：${repoPath.value}\n目标：${sm.path}\n\n将执行 deinit -f 与 git rm，并产生待提交变更。`,
      positiveText: "确认移除",
      negativeText: "取消",
      onPositiveClick: async () => {
        subBusy.value = true;
        try {
          await submoduleOp(repoPath.value, "remove", sm.path);
          message.success("子模块已移除（产生待提交变更）");
          submodules.value = await listSubmodules(repoPath.value);
        } catch (e) {
          message.error("移除失败: " + errMsg(e));
        } finally {
          subBusy.value = false;
        }
      },
    });
    return;
  }
  subBusy.value = true;
  void submoduleOp(repoPath.value, key, sm.path)
    .then(() => listSubmodules(repoPath.value))
    .then((list) => (submodules.value = list))
    .catch((e) => message.error(`${key} 失败: ` + errMsg(e)))
    .finally(() => (subBusy.value = false));
}

// ── LFS ──────────────────────────────────────────────────────

async function lfsRun(op: string) {
  lfsBusy.value = true;
  try {
    const out = await lfsOp(repoPath.value, op);
    message.success(`LFS ${op} 完成${out ? "：" + out.trim().slice(0, 200) : ""}`);
  } catch (e) {
    message.error(`LFS ${op} 失败: ` + errMsg(e));
  } finally {
    lfsBusy.value = false;
  }
}

async function loadLocks() {
  try {
    locks.value = await lfsLocks(repoPath.value);
  } catch (e) {
    locks.value = [];
    message.error("获取锁失败: " + errMsg(e));
  }
}

async function lockToggle(op: "lock" | "unlock", path: string) {
  try {
    await lfsLockOp(repoPath.value, op, path);
    message.success(`${op} 完成：${path}`);
    await loadLocks();
  } catch (e) {
    message.error(`${op} 失败: ` + errMsg(e));
  }
}

// ── Hooks ────────────────────────────────────────────────────

async function openHook(name: string) {
  try {
    hookEditor.value = { show: true, name, content: await getHook(repoPath.value, name), enabled: true, running: false };
  } catch (e) {
    message.error("读取 hook 失败: " + errMsg(e));
  }
}

async function saveCurrentHook() {
  try {
    await saveHook(repoPath.value, hookEditor.value.name, hookEditor.value.content);
    message.success(`Hook ${hookEditor.value.name} 已保存并启用`);
    hookEditor.value.show = false;
    hooks.value = await listHooks(repoPath.value);
  } catch (e) {
    message.error("保存 hook 失败: " + errMsg(e));
  }
}

async function toggleHook(name: string, enabled: boolean) {
  try {
    await setHookEnabled(repoPath.value, name, enabled);
    hooks.value = await listHooks(repoPath.value);
    message.success(`Hook ${name} 已${enabled ? "启用" : "停用"}`);
  } catch (e) {
    message.error("切换失败: " + errMsg(e));
  }
}

async function runNow(name: string) {
  hookEditor.value.running = true;
  try {
    const result = await runHook(repoPath.value, name);
    if (result.exitCode === 0) {
      message.success(`Hook ${name} 运行成功${result.output ? "：" + result.output.trim().slice(0, 200) : ""}`);
    } else {
      message.error(`Hook ${name} 退出码 ${result.exitCode ?? "?"}：${result.output.trim().slice(0, 400)}`);
    }
  } catch (e) {
    message.error("运行 hook 失败: " + errMsg(e));
  } finally {
    hookEditor.value.running = false;
  }
}
</script>

<style scoped>
.repo-tools {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.tools-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.tools-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 8px 16px;
  background: var(--gw-bg-panel);
}

.tab-toolbar {
  display: flex;
  gap: var(--gw-space-2);
  margin-bottom: var(--gw-space-3);
}

.row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 0;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.row-main {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-dim {
  color: var(--gw-text-dim);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 320px;
}

.mono {
  font-family: var(--gw-font-mono);
}

.section-title {
  font-size: 12px;
  color: var(--gw-text-dim);
  margin: var(--gw-space-3) 0 var(--gw-space-1);
}

.tab-empty {
  margin-top: var(--gw-space-6);
}
</style>
