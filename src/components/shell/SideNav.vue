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

    <!-- 折叠按钮 -->
    <button class="sidenav-collapse-btn" @click="toggleCollapse">
      <n-icon :size="16">
        <ChevronBackOutline v-if="!isCollapsed" />
        <ChevronForwardOutline v-else />
      </n-icon>
      <span v-if="!isCollapsed" class="sidenav-collapse-label">折叠</span>
    </button>
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

// 从 router 提取导航条目，按 meta.group 分组
const navGroups = computed(() => {
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
</style>
