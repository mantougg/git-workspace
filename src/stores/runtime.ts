// R-13 Runtime Workspace store：与 Git 侧 store 完全解耦。
//
// 数据全部来自 R-12 IPC + §64 事件订阅；状态变化局部更新，禁止全量重拉
// （高频 process_output / build_progress 走内存缓冲，进程生命周期事件才
// 触发进程列表刷新）。

import { defineStore } from "pinia";
import { ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as runtimeApi from "@/api/runtime";
import { RUNTIME_EVENTS } from "@/api/runtime";
import type {
  BuildProgressPayload,
  HealthChangedPayload,
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
  const workspaceId = ref<number | null>(null);
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
  /** runtimeName → 健康状态（health_changed 事件驱动）。 */
  const health = ref<Map<string, "up" | "down">>(new Map());
  /** runtimeName → 进程输出环形缓冲（process_output 事件驱动，已脱敏）。 */
  const logBuffers = ref<Map<string, LogLine[]>>(new Map());

  let unlisteners: UnlistenFn[] = [];

  // ------------------------------------------------------------------
  // 加载（事件驱动 + 显式刷新双通道）
  // ------------------------------------------------------------------

  async function setWorkspace(id: number) {
    workspaceId.value = id;
    await reloadAll();
  }

  async function reloadAll() {
    if (workspaceId.value == null) return;
    loading.value = true;
    try {
      await Promise.all([loadConfigs(), loadProjects(), loadProcesses()]);
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
  }

  async function removeConfig(name: string): Promise<void> {
    await runtimeApi.deleteRuntimeConfig(requireWorkspace(), name);
    configDetails.value.delete(name);
    await loadConfigs();
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
    setWorkspace,
    reloadAll,
    loadConfigs,
    loadProjects,
    loadProcesses,
    loadConfigDetail,
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
