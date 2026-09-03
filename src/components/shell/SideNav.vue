<template>
  <nav class="sidenav" :class="{ collapsed: isCollapsed }">
    <!-- 产品名区 -->
    <div class="sidenav-brand">GitWorkspace</div>

    <!-- 导航分组（router meta 驱动） -->
    <div class="sidenav-groups">
      <div v-for="group in navGroups" :key="group.label" class="sidenav-group">
        <div class="sidenav-group-label">{{ group.label }}</div>
        <router-link
          v-for="item in group.items"
          :key="item.name"
          :to="{ name: item.name }"
          class="sidenav-item"
          :class="{ active: isActive(item.name) }"
          :title="isCollapsed ? item.title : undefined"
        >
          <n-icon :size="16">
            <component :is="item.icon" />
          </n-icon>
          <span v-if="!isCollapsed" class="sidenav-item-label">{{ item.title }}</span>
        </router-link>
      </div>
    </div>

    <!-- F-33：菜单可见性配置入口（折叠按钮上方，不占导航分组） -->
    <button class="sidenav-collapse-btn" @click="openNavSettings">
      <n-icon :size="16"><SettingsOutline /></n-icon>
      <span v-if="!isCollapsed" class="sidenav-collapse-label">菜单配置</span>
    </button>

    <!-- 折叠按钮 -->
    <button class="sidenav-collapse-btn" @click="toggleCollapse">
      <n-icon :size="16">
        <ChevronBackOutline v-if="!isCollapsed" />
        <ChevronForwardOutline v-else />
      </n-icon>
      <span v-if="!isCollapsed" class="sidenav-collapse-label">折叠</span>
    </button>

    <!-- F-33：菜单可见性设置弹窗（黑名单持久化；命令面板/URL 直达不受影响） -->
    <n-modal v-model:show="navSettingsShow">
      <n-card
        class="nav-settings-card"
        title="菜单可见性配置"
        :bordered="false"
        size="small"
        role="dialog"
        aria-modal="true"
      >
        <div class="nav-settings-body">
          <div v-for="group in allNavGroups" :key="group.label" class="nav-settings-group">
            <div class="nav-settings-group-head">
              <span class="nav-settings-group-label">{{ group.label }}</span>
              <n-button text size="tiny" @click="toggleDraftGroup(group)">
                {{ isDraftGroupAllVisible(group) ? "全部隐藏" : "全部显示" }}
              </n-button>
            </div>
            <div class="nav-settings-items">
              <n-checkbox
                v-for="item in group.items"
                :key="item.name"
                :checked="!draftHiddenNav.includes(item.name)"
                @update:checked="(visible: boolean) => toggleDraftItem(item.name, visible)"
              >
                {{ item.title }}
              </n-checkbox>
            </div>
          </div>
        </div>
        <template #footer>
          <div class="nav-settings-footer">
            <span class="nav-settings-hint">隐藏仅影响侧边栏展示；页面仍可经 URL 与 Ctrl+K 命令面板直达</span>
            <n-button size="small" @click="resetDraftDefaults">恢复默认</n-button>
            <n-button size="small" @click="navSettingsShow = false">取消</n-button>
            <n-button size="small" type="primary" @click="saveNavSettings">保存</n-button>
          </div>
        </template>
      </n-card>
    </n-modal>
  </nav>
</template>

<script setup lang="ts">
import { ref, computed, markRaw } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NIcon } from "naive-ui";
import type { NavGroup } from "@/router";
import {
  GridOutline,
  SwapHorizontalOutline,
  HeartOutline,
  GitNetworkOutline,
  GitBranchOutline,
  ArchiveOutline,
  FolderOpenOutline,
  TimeOutline,
  LayersOutline,
  ConstructOutline,
  ListOutline,
  TerminalOutline,
  RocketOutline,
  CodeSlashOutline,
  ServerOutline,
  FolderOutline,
  BuildOutline,
  OptionsOutline,
  HardwareChipOutline,
  PulseOutline,
  InformationCircleOutline,
  ChevronBackOutline,
  ChevronForwardOutline,
  SettingsOutline,
} from "@vicons/ionicons5";

const STORAGE_KEY = "gw-sidenav-collapsed";
const isCollapsed = ref(localStorage.getItem(STORAGE_KEY) === "true");
const route = useRoute();
const router = useRouter();

// 任务型页面不高亮任何条目
const TASK_ROUTES = ["diff-viewer", "conflict-resolver", "runtime-app-wizard"];

function isActive(routeName: string): boolean {
  if (TASK_ROUTES.includes(route.name as string)) return false;
  return route.name === routeName;
}

function toggleCollapse() {
  isCollapsed.value = !isCollapsed.value;
  localStorage.setItem(STORAGE_KEY, String(isCollapsed.value));
}

// ------------------------------------------------------------------
// F-33：菜单可见性（黑名单式持久化——新增菜单默认可见，不被旧配置误伤）
// ------------------------------------------------------------------

const HIDDEN_NAV_KEY = "gw-sidenav-hidden-nav";

/** 首次使用时的默认隐藏集（用户点名的低频入口）。 */
const DEFAULT_HIDDEN_NAV = [
  "symbol-search",
  "repo-tools",
  "automation",
  "reflog-view",
  "pipeline",
  "runtime-environments",
];

function loadHiddenNav(): string[] {
  const raw = localStorage.getItem(HIDDEN_NAV_KEY);
  if (raw == null) {
    localStorage.setItem(HIDDEN_NAV_KEY, JSON.stringify(DEFAULT_HIDDEN_NAV));
    return [...DEFAULT_HIDDEN_NAV];
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((name): name is string => typeof name === "string")
      : [];
  } catch {
    return [];
  }
}

const hiddenNav = ref<string[]>(loadHiddenNav());

const navSettingsShow = ref(false);
/** 弹窗草稿：保存前不生效。 */
const draftHiddenNav = ref<string[]>([]);

function openNavSettings() {
  draftHiddenNav.value = [...hiddenNav.value];
  navSettingsShow.value = true;
}

function toggleDraftItem(name: string, visible: boolean) {
  if (visible) {
    draftHiddenNav.value = draftHiddenNav.value.filter((n) => n !== name);
  } else if (!draftHiddenNav.value.includes(name)) {
    draftHiddenNav.value = [...draftHiddenNav.value, name];
  }
}

function isDraftGroupAllVisible(group: { items: { name: string }[] }): boolean {
  return group.items.every((item) => !draftHiddenNav.value.includes(item.name));
}

function toggleDraftGroup(group: { items: { name: string }[] }) {
  if (isDraftGroupAllVisible(group)) {
    draftHiddenNav.value = [
      ...new Set([...draftHiddenNav.value, ...group.items.map((i) => i.name)]),
    ];
  } else {
    const names = new Set(group.items.map((i) => i.name));
    draftHiddenNav.value = draftHiddenNav.value.filter((n) => !names.has(n));
  }
}

function resetDraftDefaults() {
  draftHiddenNav.value = [...DEFAULT_HIDDEN_NAV];
}

function saveNavSettings() {
  hiddenNav.value = [...draftHiddenNav.value];
  localStorage.setItem(HIDDEN_NAV_KEY, JSON.stringify(hiddenNav.value));
  navSettingsShow.value = false;
}

// 路由 name → 图标映射
const ICON_MAP: Record<string, any> = {
  dashboard: GridOutline,
  changes: SwapHorizontalOutline,
  health: HeartOutline,
  "git-graph": GitNetworkOutline,
  "branch-manager": GitBranchOutline,
  "stash-manager": ArchiveOutline,
  "worktree-manager": FolderOpenOutline,
  "reflog-view": TimeOutline,
  "change-sets": LayersOutline,
  pipeline: ConstructOutline,
  manifest: ListOutline,
  "operation-log": TerminalOutline,
  "runtime-dashboard": RocketOutline,
  "runtime-dependencies": CodeSlashOutline,
  "runtime-scope": ServerOutline,
  "runtime-logs": TerminalOutline,
  workspaces: FolderOutline,
  "jdk-manager": BuildOutline,
  "maven-settings": OptionsOutline,
  "node-toolchain": HardwareChipOutline,
  "port-tool": PulseOutline,
  about: InformationCircleOutline,
};

// 分组顺序
const GROUP_ORDER: NavGroup[] = ["工作区", "Git", "Runtime", "设置"];

// 从 router 提取导航条目，按 meta.group 分组（全量，弹窗用）
const allNavGroups = computed(() => {
  const groups = new Map<NavGroup, { name: string; title: string; icon: any }[]>();

  for (const r of router.getRoutes()) {
    const meta = r.meta;
    if (meta.nav === false) continue; // 任务型页面跳过
    const group = (meta.group as NavGroup) ?? "无";
    if (group === "无") continue;
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group)!.push({
      name: r.name as string,
      title: (meta.title as string) ?? r.name as string,
      icon: markRaw(ICON_MAP[r.name as string] ?? GridOutline),
    });
  }

  return GROUP_ORDER.filter((g) => groups.has(g)).map((g) => ({
    label: g,
    items: groups.get(g)!,
  }));
});

// F-33：按黑名单过滤展示项（折叠态共用；某组全部隐藏时整组不渲染）
const navGroups = computed(() =>
  allNavGroups.value
    .map((group) => ({
      label: group.label,
      items: group.items.filter((item) => !hiddenNav.value.includes(item.name)),
    }))
    .filter((group) => group.items.length > 0),
);
</script>

<style scoped>
.sidenav {
  width: var(--gw-sidenav-w);
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--gw-bg-panel);
  border-right: 1px solid var(--gw-border);
  user-select: none;
  transition: width 0.2s ease;
  overflow: hidden;
}

.sidenav.collapsed {
  width: var(--gw-sidenav-w-collapsed);
}

/* 产品名区 */
.sidenav-brand {
  height: 40px;
  display: flex;
  align-items: center;
  padding: 0 var(--gw-space-3);
  font-size: var(--gw-text-md);
  font-weight: 600;
  color: var(--gw-text);
  border-bottom: 1px solid var(--gw-border);
  white-space: nowrap;
  overflow: hidden;
}

/* 分组 */
.sidenav-groups {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: var(--gw-space-1) 0;
}

.sidenav-group-label {
  padding: var(--gw-space-2) var(--gw-space-3) var(--gw-space-1);
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  white-space: nowrap;
  overflow: hidden;
}

.collapsed .sidenav-group-label {
  text-align: center;
  padding: var(--gw-space-2) 0 var(--gw-space-1);
  font-size: 0;
}

/* 条目 */
.sidenav-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  height: 32px;
  padding: 0 var(--gw-space-3);
  font-size: var(--gw-text-md);
  color: var(--gw-text);
  text-decoration: none;
  white-space: nowrap;
  overflow: hidden;
  position: relative;
  transition: background 0.15s;
}

.sidenav-item:hover {
  background: var(--gw-bg-hover);
}

.sidenav-item.active {
  background: var(--gw-bg-hover);
}

.sidenav-item.active::before {
  content: "";
  position: absolute;
  left: 0;
  top: 4px;
  bottom: 4px;
  width: 2px;
  background: var(--gw-accent);
  border-radius: 1px;
}

.collapsed .sidenav-item {
  justify-content: center;
  padding: 0;
}

.collapsed .sidenav-item.active::before {
  left: 0;
}

/* 折叠按钮 */
.sidenav-collapse-btn {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  height: 36px;
  padding: 0 var(--gw-space-3);
  border: none;
  background: none;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
  cursor: pointer;
  border-top: 1px solid var(--gw-border);
  white-space: nowrap;
  overflow: hidden;
}

.sidenav-collapse-btn:hover {
  background: var(--gw-bg-hover);
  color: var(--gw-text);
}

.collapsed .sidenav-collapse-btn {
  justify-content: center;
  padding: 0;
}

/* F-33 菜单可见性弹窗 */
.nav-settings-card {
  width: 420px;
  max-width: 90vw;
}

.nav-settings-body {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  max-height: 55vh;
  overflow-y: auto;
}

.nav-settings-group-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--gw-space-1);
}

.nav-settings-group-label {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.nav-settings-items {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: var(--gw-space-1) var(--gw-space-3);
}

.nav-settings-footer {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.nav-settings-hint {
  flex: 1;
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}
</style>
