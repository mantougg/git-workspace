// R-13 Runtime Workspace store：与 Git 侧 store 完全解耦。
//
// 数据全部来自 R-12 IPC + §64 事件订阅；状态变化局部更新，禁止全量重拉
// （高频 process_output / build_progress 走内存缓冲，进程生命周期事件才
// 触发进程列表刷新）。

import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as runtimeApi from "@/api/runtime";
import { RUNTIME_EVENTS } from "@/api/runtime";
import { useWorkspaceStore } from "@/stores/workspace";
import type {
  BuildProgressPayload,
  HealthChangedPayload,
  HealthStatus,
  LogLine,
  ProcessOutputPayload,
  RuntimeApplicationConfig,
  RuntimeConfigSummary,
  RuntimeProcessInfo,
  RuntimeStage,
} from "@/types/runtime";
import type { MavenProjectNode } from "@/types/maven";

/** 日志环形缓冲上限（前端内存，防高频输出撑爆）。 */
const MAX_BUFFER_LINES = 5000;

export const useRuntimeStore = defineStore("runtime", () => {
  const workspaceStore = useWorkspaceStore();
  // F-15：workspaceId 派生自全局当前工作区（StatusBar 统一切换入口）。
  // 修复前它是独立 ref、只由 RuntimeDashboard 调 setWorkspace 写入——直接
  // 从 SideNav 进依赖/作用域/日志视图时永远为 null，数据加载全部早退。
  const workspaceId = computed(() => workspaceStore.currentWorkspace?.id ?? null);
  const loading = ref(false);

  /** 配置元数据列表（R-07 快索引）。 */
  const configs = ref<RuntimeConfigSummary[]>([]);
  /** 配置详情缓存（get_runtime_config 按需加载）。 */
  const configDetails = ref<Map<string, RuntimeApplicationConfig>>(new Map());
  /** workspace Maven 项目索引（runtime_list_projects）。 */
  const projects = ref<MavenProjectNode[]>([]);
  /** 运行中/历史进程（runtime_list_processes）。 */
  const processes = ref<RuntimeProcessInfo[]>([]);
  /** runtimeName → Start 流水线阶段（§65，build_progress 事件驱动）。 */
  const stages = ref<Map<string, RuntimeStage>>(new Map());
  /** runtimeName → 健康状态（health_changed 事件驱动；R-16 起含探针状态机取值）。 */
  const health = ref<Map<string, HealthStatus>>(new Map());
  /** runtimeName → 进程输出环形缓冲（process_output 事件驱动，已脱敏）。 */
  const logBuffers = ref<Map<string, LogLine[]>>(new Map());
  /** F-23：runtimeName → 闭包摘要（源码依赖数与名称）；
   *  null = 依赖图未解析（未跑「解析依赖」）或计算失败。 */
  const closureInfo = ref<Map<string, { sourceCount: number; sourceNames: string[] } | null>>(new Map());

  let unlisteners: UnlistenFn[] = [];

  // ------------------------------------------------------------------
  // 加载（事件驱动 + 显式刷新双通道）
  // ------------------------------------------------------------------

  // 当前工作区变化（含首次赋值）→ 重拉本 store 数据；置空时清空。
  watch(
    workspaceId,
    async (id) => {
      if (id == null) {
        configs.value = [];
        configDetails.value.clear();
        projects.value = [];
        processes.value = [];
        stages.value.clear();
        health.value.clear();
        logBuffers.value.clear();
        closureInfo.value.clear();
        return;
      }
      await reloadAll();
    },
    { immediate: true },
  );

  async function reloadAll() {
    if (workspaceId.value == null) return;
    loading.value = true;
    try {
      await Promise.all([loadConfigs(), loadProjects(), loadProcesses()]);
      await loadClosureInfo();
    } catch (e) {
      console.error("R-13: runtime workspace reload failed:", e);
    } finally {
      loading.value = false;
    }
  }

  async function loadConfigs() {
    if (workspaceId.value == null) return;
    configs.value = await runtimeApi.listRuntimeConfigs(workspaceId.value);
  }

  async function loadProjects() {
    if (workspaceId.value == null) return;
    projects.value = await runtimeApi.runtimeListProjects(workspaceId.value);
  }

  async function loadProcesses() {
    if (workspaceId.value == null) return;
    processes.value = await runtimeApi.runtimeListProcesses(workspaceId.value);
    // 进程终止/失败时清理对应的健康与阶段标记，避免残留旧状态。
    const alive = new Set(processes.value.map((p) => p.runtimeName));
    for (const name of [...health.value.keys()]) {
      if (!alive.has(name)) health.value.delete(name);
    }
    for (const name of [...stages.value.keys()]) {
      if (!alive.has(name)) stages.value.delete(name);
    }
  }

  /** 按需加载配置详情（get_runtime_config 打开 JSON 文件，勿全量拉取）。 */
  async function loadConfigDetail(name: string): Promise<RuntimeApplicationConfig> {
    if (workspaceId.value == null) {
      throw new Error("未选择 workspace");
    }
    const cached = configDetails.value.get(name);
    if (cached) return cached;
    const config = await runtimeApi.getRuntimeConfig(workspaceId.value, name);
    configDetails.value.set(name, config);
    return config;
  }

  /**
   * F-23：逐配置计算闭包摘要（判定「直接启动 / 源码启动 + 源码依赖名」）。
   * 闭包走 R-03 服务端双层缓存（依赖图 + closure fingerprint），额外成本
   * 只是读 N 个配置 JSON；单个配置失败记 null，不影响其他配置。
   */
  async function loadClosureInfo() {
    const ws = workspaceId.value;
    if (ws == null) return;
    const map = new Map<string, { sourceCount: number; sourceNames: string[] } | null>();
    await Promise.all(
      configs.value.map(async (c) => {
        try {
          const detail = await loadConfigDetail(c.name);
          const preview = await runtimeApi.runtimeGetClosure(ws, c.project, detail.scope);
          const sources = preview.closure.projects.filter(
            (p) => p.projectId !== preview.closure.rootProjectId,
          );
          map.set(c.name, {
            sourceCount: sources.length,
            sourceNames: sources.map((p) => p.coordinates.artifactId),
          });
        } catch {
          map.set(c.name, null);
        }
      }),
    );
    closureInfo.value = map;
  }

  async function refreshProcess(processId: number) {
    const info = await runtimeApi.runtimeProcessStatus(processId);
    if (info) upsertProcess(info);
  }

  function upsertProcess(info: RuntimeProcessInfo) {
    const idx = processes.value.findIndex((p) => p.processId === info.processId);
    if (idx >= 0) processes.value[idx] = info;
    else processes.value.unshift(info);
  }

  // ------------------------------------------------------------------
  // 操作（长操作经 R-12 Task Engine，返回任务 id；进度走事件）
  // ------------------------------------------------------------------

  function requireWorkspace(): number {
    if (workspaceId.value == null) throw new Error("未选择 workspace");
    return workspaceId.value;
  }

  async function start(name: string): Promise<string> {
    return runtimeApi.runtimeStart({
      workspaceId: requireWorkspace(),
      runtimeName: name,
    });
  }

  async function stop(name: string): Promise<string> {
    return runtimeApi.runtimeStop(requireWorkspace(), name);
  }

  async function restart(name: string, skipBuild = false): Promise<string> {
    return runtimeApi.runtimeRestart({
      workspaceId: requireWorkspace(),
      runtimeName: name,
      options: { skipBuild },
    });
  }

  async function build(name: string): Promise<string> {
    return runtimeApi.runtimeBuild({
      workspaceId: requireWorkspace(),
      runtimeName: name,
    });
  }

  async function resolveDependencies(): Promise<string> {
    return runtimeApi.runtimeResolveDependencies(requireWorkspace());
  }

  async function startEnvironment(): Promise<string[]> {
    return runtimeApi.runtimeStartEnvironment(requireWorkspace());
  }

  async function stopEnvironment(): Promise<string[]> {
    return runtimeApi.runtimeStopEnvironment(requireWorkspace());
  }

  async function saveConfig(config: RuntimeApplicationConfig): Promise<void> {
    const ws = requireWorkspace();
    const existing = configs.value.find((c) => c.name === config.name);
    if (existing) {
      await runtimeApi.updateRuntimeConfig({
        workspaceId: ws,
        name: config.name,
        config,
      });
    } else {
      await runtimeApi.createRuntimeConfig({ workspaceId: ws, config });
    }
    configDetails.value.set(config.name, config);
    await loadConfigs();
    await loadClosureInfo();
  }

  async function removeConfig(name: string): Promise<void> {
    await runtimeApi.deleteRuntimeConfig(requireWorkspace(), name);
    configDetails.value.delete(name);
    await loadConfigs();
    await loadClosureInfo();
  }

  // ------------------------------------------------------------------
  // §64 事件订阅（幂等；进程域事件触发轻量刷新）
  // ------------------------------------------------------------------

  async function subscribe() {
    if (unlisteners.length > 0) return;

    const onProcessEvent = async () => {
      try {
        await loadProcesses();
      } catch (e) {
        console.error("R-13: process event refresh failed:", e);
      }
    };

    unlisteners = [
      // 高频事件走内存缓冲，不触发 IPC 往返。
      await listen<ProcessOutputPayload>(RUNTIME_EVENTS.processOutput, (e) => {
        const name = e.payload.runtimeName;
        const buf = logBuffers.value.get(name) ?? [];
        buf.push(...e.payload.lines);
        if (buf.length > MAX_BUFFER_LINES) {
          buf.splice(0, buf.length - MAX_BUFFER_LINES);
        }
        logBuffers.value.set(name, buf);
      }),
      await listen<BuildProgressPayload>(RUNTIME_EVENTS.buildProgress, (e) => {
        stages.value.set(e.payload.runtimeName, e.payload.stage);
      }),
      await listen<HealthChangedPayload>(RUNTIME_EVENTS.healthChanged, (e) => {
        health.value.set(e.payload.runtimeName, e.payload.health);
      }),
      await listen(RUNTIME_EVENTS.processStarted, onProcessEvent),
      await listen(RUNTIME_EVENTS.processStopped, onProcessEvent),
      await listen(RUNTIME_EVENTS.processFailed, onProcessEvent),
      await listen(RUNTIME_EVENTS.buildCompleted, onProcessEvent),
      // 依赖索引变化 → 项目列表可能新增；dependency_resolved 是聚合汇总，
      // 依赖解析是低频操作，这里刷新一次可以接受（§64 高频约束不覆盖）。
      await listen(RUNTIME_EVENTS.dependencyResolved, async () => {
        try {
          await loadProjects();
          // 依赖图变化 → 各配置闭包可能变化，闭包摘要一并刷新。
          await loadClosureInfo();
        } catch (e) {
          console.error("R-13: dependency resolved refresh failed:", e);
        }
      }),
      await listen(RUNTIME_EVENTS.projectDiscovered, async () => {
        try {
          await loadProjects();
        } catch (e) {
          console.error("R-13: project discovered refresh failed:", e);
        }
      }),
    ];
  }

  async function unsubscribe() {
    for (const un of unlisteners) {
      try {
        un();
      } catch {
        // ignore
      }
    }
    unlisteners = [];
  }

  return {
    workspaceId,
    loading,
    configs,
    configDetails,
    projects,
    processes,
    stages,
    health,
    logBuffers,
    closureInfo,
    reloadAll,
    loadConfigs,
    loadProjects,
    loadProcesses,
    loadConfigDetail,
    loadClosureInfo,
    refreshProcess,
    start,
    stop,
    restart,
    build,
    resolveDependencies,
    startEnvironment,
    stopEnvironment,
    saveConfig,
    removeConfig,
    subscribe,
    unsubscribe,
  };
});
