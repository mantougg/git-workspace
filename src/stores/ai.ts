/**
 * AI-10：统一 Assistant 全局会话 store（设计文档 §9.1 / §9.2 / §12.3 / §12.4）。
 *
 * Drawer 是全局唯一会话状态持有者；领域页面只经 `openWithContext` 带入
 * 作用域与补充上下文，不各自实现聊天状态。作用域是会话属性：
 * 不同 Workspace/Repository/Runtime 范围切换时开启新会话（显式切换），
 * 不串上下文。
 *
 * 安全约束（全局约束 §2/§4/§5）：
 * - 发送一律走「Preview → 用户确认 → Gateway」，本 store 不发起任何绕过
 *   Preview 的网络请求；
 * - 输入草稿在请求失败时保留（§12.4 离线/失败不丢上下文）；
 * - API Key 永不进本 store（凭证管理在 AI 设置页）。
 */

import { defineStore } from "pinia";
import { computed, ref } from "vue";
import * as aiApi from "@/api/ai";
import { AiStreamFrameBuffer, onAiRequestEvent } from "@/api/aiStream";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type {
  AiContextPreview,
  AiRequestSnapshot,
  AiSession,
  AiSessionDetail,
  AiSessionPersistence,
  AiSessionMessage,
  AiSessionRole,
  AiSettingsSummary,
  AiToolDefinition,
  ContextKind,
  ContextPreviewRequest,
  SupplementaryContext,
  ToolInvocation,
  ToolRole,
} from "@/types/ai";
import { errMsg, type ErrorResponse } from "@/utils/error";

/** Drawer 上下文作用域（会话属性；切换需显式经 openWithContext）。 */
export interface AssistantScope {
  workspaceId: number | null;
  workspaceName: string | null;
  repositoryPaths: string[];
  runtimeName: string | null;
  processId: number | null;
  /** 入口带入的补充上下文（选中日志、结构化错误等），随发送消耗。 */
  supplementary: SupplementaryContext[];
  /** 入口来源描述（UI 展示，如「Runtime Logs · 选中 86 行」）。 */
  origin: string | null;
}

/** openWithContext 入参。 */
export interface OpenAssistantOptions {
  workspaceId?: number | null;
  workspaceName?: string | null;
  repositoryPaths?: string[];
  runtimeName?: string | null;
  processId?: number | null;
  /** 入口自动推断的角色（§9.2：自动推断结果必须在 UI 可见）。 */
  inferredRole?: AiSessionRole;
  supplementary?: SupplementaryContext[];
  origin?: string | null;
  /** 带入的输入草稿（如「解释这个错误」）。 */
  draft?: string;
}

/** 工具读取摘要卡片（§12.3 中部「工具读取摘要」）。 */
export interface ToolReadCard {
  id: string;
  toolName: string;
  /** 参数摘要（JSON 短格式）。 */
  argsSummary: string;
  /** 结果摘要（截断展示）。 */
  resultSummary: string;
  /** 完整结果 JSON（展开查看来源）。 */
  resultJson: string;
  truncated: boolean;
  durationMs: number;
  error: string | null;
}

const TERMINAL_PHASES = new Set(["succeeded", "cancelled", "rejected", "failed", "degraded"]);
/** §9.4：一次用户请求的工具调用上限。 */
const TOOL_CALL_LIMIT = 8;
const SESSION_PAGE_SIZE = 50;
const MESSAGE_PAGE_SIZE = 50;
/** 多轮会话带入的历史轮数与字符上限（防止历史无限膨胀占预算）。 */
const HISTORY_ROUNDS = 3;
const HISTORY_MAX_CHARS = 4000;

function emptyScope(): AssistantScope {
  return {
    workspaceId: null,
    workspaceName: null,
    repositoryPaths: [],
    runtimeName: null,
    processId: null,
    supplementary: [],
    origin: null,
  };
}

function scopeSignature(scope: {
  workspaceId: number | null;
  repositoryPaths: string[];
  runtimeName: string | null;
  processId: number | null;
}): string {
  return JSON.stringify([
    scope.workspaceId,
    [...scope.repositoryPaths].sort(),
    scope.runtimeName,
    scope.processId,
  ]);
}

function sessionSignature(session: AiSession): string {
  const runtime = (session.runtimeScope ?? {}) as Record<string, unknown>;
  return scopeSignature({
    workspaceId: session.workspaceId,
    repositoryPaths: session.repositoryScope,
    runtimeName: typeof runtime.runtimeName === "string" ? runtime.runtimeName : null,
    processId: typeof runtime.processId === "number" ? runtime.processId : null,
  });
}

/** 从持久化消息中提取可读文本（用户取最后指令；助手取结果文本）。 */
function messageText(message: AiSessionMessage): string {
  const content = message.content as Record<string, unknown> | null;
  if (!content) return "";
  if (message.role === "user") {
    const arr = content.messages as Array<{ content?: unknown }> | undefined;
    const last = arr?.[arr.length - 1];
    return typeof last?.content === "string" ? last.content : "";
  }
  if (typeof content.text === "string") return content.text;
  const payload = content.payload as Record<string, unknown> | undefined;
  for (const key of ["headline", "summary", "title", "rationale"]) {
    const value = payload?.[key];
    if (typeof value === "string" && value) return value;
  }
  return "";
}

/** 构建多轮历史补充上下文（经 Secret 管道与预算，Manifest 可见）。 */
function historySupplementary(messages: AiSessionMessage[]): SupplementaryContext | null {
  const lines: string[] = [];
  const recent = messages.slice(-HISTORY_ROUNDS * 2);
  for (const message of recent) {
    const text = messageText(message).trim();
    if (!text) continue;
    lines.push(`[${message.role === "user" ? "用户" : "助手"}] ${text}`);
  }
  if (lines.length === 0) return null;
  let content = lines.join("\n");
  if (content.length > HISTORY_MAX_CHARS) {
    content = content.slice(content.length - HISTORY_MAX_CHARS);
  }
  return {
    role: "userNote",
    kind: "log",
    sourceId: "session:history",
    displayName: `会话历史（近 ${Math.min(HISTORY_ROUNDS, Math.ceil(lines.length / 2))} 轮）`,
    content,
  };
}

/** 工具结果 → 下一条消息的补充上下文。 */
function toolScopeToKind(tool: AiToolDefinition): ContextKind {
  return tool.contextScope === "runtime" ||
    tool.contextScope === "jdk" ||
    tool.contextScope === "maven" ||
    tool.contextScope === "task"
    ? "runtime"
    : "repository";
}

/** 从作用域构建工具参数；必需字段缺失时返回 null（不可一键运行）。 */
export function buildToolArguments(
  tool: AiToolDefinition,
  scope: AssistantScope,
): Record<string, unknown> | null {
  const required = (tool.inputSchema.required as string[] | undefined) ?? [];
  const args: Record<string, unknown> = {};
  for (const field of required) {
    switch (field) {
      case "workspaceId":
        if (scope.workspaceId == null) return null;
        args.workspaceId = scope.workspaceId;
        break;
      case "repoPath": {
        const repoPath = scope.repositoryPaths[0];
        if (!repoPath) return null;
        args.repoPath = repoPath;
        break;
      }
      case "runtimeName":
        if (!scope.runtimeName) return null;
        args.runtimeName = scope.runtimeName;
        break;
      case "processId":
        if (scope.processId == null) return null;
        args.processId = scope.processId;
        break;
      default:
        // project / taskIds 等无法从 Drawer 作用域推断的字段。
        return null;
    }
  }
  return args;
}

export const useAiStore = defineStore("ai", () => {
  // -- Drawer / 角色 / 作用域 -------------------------------------------------
  const drawerOpen = ref(false);
  const role = ref<AiSessionRole>("workspaceAssistant");
  /** false = 入口自动推断（UI 必须可见）；true = 用户手动覆盖。 */
  const roleIsManual = ref(false);
  const scope = ref<AssistantScope>(emptyScope());

  // -- 设置 / 降级 ------------------------------------------------------------
  const settingsSummary = ref<AiSettingsSummary | null>(null);
  const settingsLoaded = ref(false);

  // -- 会话列表与当前会话 ------------------------------------------------------
  const sessions = ref<AiSession[]>([]);
  const sessionsTotal = ref(0);
  const sessionsLoading = ref(false);
  const detail = ref<AiSessionDetail | null>(null);
  const persistence = ref<AiSessionPersistence | null>(null);

  // -- 发送链路（Preview → 确认 → Gateway） -----------------------------------
  const input = ref("");
  const building = ref(false);
  const confirming = ref(false);
  const previewVisible = ref(false);
  const preview = ref<AiContextPreview | null>(null);
  const previewRequest = ref<ContextPreviewRequest | null>(null);
  const activeSnapshot = ref<AiRequestSnapshot | null>(null);
  const streamingText = ref("");
  /** 最近一次失败（降级卡片：重试 / 配置 AI / 缩小范围）。 */
  const lastError = ref<{ message: string; code: string | null } | null>(null);

  // -- 工具读取 ----------------------------------------------------------------
  const tools = ref<AiToolDefinition[]>([]);
  const toolReads = ref<ToolReadCard[]>([]);
  /** 本轮用户请求内的工具调用计数（§9.4 上限 8）。 */
  const toolCallCount = ref(0);
  const toolRunning = ref<string | null>(null);

  let unlisten: UnlistenFn | null = null;
  let frameBuffer: AiStreamFrameBuffer | null = null;
  let pollTimer: ReturnType<typeof setTimeout> | null = null;

  const currentSession = computed(() => detail.value?.session ?? null);
  const configured = computed(
    () =>
      !!settingsSummary.value &&
      settingsSummary.value.enabledProviderCount > 0 &&
      settingsSummary.value.enabledModelCount > 0,
  );
  const sending = computed(() => {
    const phase = activeSnapshot.value?.phase;
    return phase != null && !TERMINAL_PHASES.has(phase);
  });
  /** 当前角色可用的工具（§9.2 角色即工具白名单准入）。 */
  const availableTools = computed(() =>
    tools.value.filter((tool) => tool.allowedRoles.includes(role.value as ToolRole)),
  );

  // ---------------------------------------------------------------------------
  // Drawer 开关与上下文入口
  // ---------------------------------------------------------------------------

  function toggleDrawer(open?: boolean) {
    drawerOpen.value = open ?? !drawerOpen.value;
    if (drawerOpen.value) {
      void refreshSettings();
      void loadSessions();
      void loadPersistence();
      void loadTools();
    }
  }

  /**
   * 领域页面的唯一上下文入口（§9.1）。带入作用域 + 自动推断角色 +
   * 补充上下文；作用域变化时开启新会话草稿（不串上下文）。
   */
  function openWithContext(options: OpenAssistantOptions) {
    const next: AssistantScope = {
      // Every entry point supplies a complete new scope. Retaining omitted
      // values here would leak a prior repository or Runtime into a new chat.
      workspaceId: options.workspaceId ?? null,
      workspaceName: options.workspaceName ?? null,
      repositoryPaths: [...(options.repositoryPaths ?? [])],
      runtimeName: options.runtimeName ?? null,
      processId: options.processId ?? null,
      supplementary: [...(options.supplementary ?? [])],
      origin: options.origin ?? null,
    };
    // Entering a new domain context starts from that entry's inferred role.
    // A manual choice still remains stable while the user stays in the Drawer.
    if (options.inferredRole) {
      role.value = options.inferredRole;
      roleIsManual.value = false;
    }
    if (options.draft != null) input.value = options.draft;

    const active = currentSession.value;
    scope.value = next;
    if (active && sessionSignature(active) !== scopeSignature(next)) {
      // 显式切换作用域 → 新会话草稿（§12.3「不串上下文」）。
      detail.value = null;
      toolReads.value = [];
      toolCallCount.value = 0;
    }
    toggleDrawer(true);
  }

  /** 手动覆盖角色（§9.2：自动推断结果可见、可手动覆盖）。 */
  function setRoleManual(next: AiSessionRole) {
    role.value = next;
    roleIsManual.value = true;
  }

  /** 清空带入的上下文（底部「清空上下文」；不动会话消息）。 */
  function clearContext() {
    scope.value = { ...scope.value, supplementary: [], origin: null };
    toolReads.value = [];
    toolCallCount.value = 0;
    lastError.value = null;
  }

  // ---------------------------------------------------------------------------
  // 设置 / 工具 / 持久化
  // ---------------------------------------------------------------------------

  async function refreshSettings() {
    try {
      settingsSummary.value = await aiApi.aiGetSettingsSummary();
    } catch {
      settingsSummary.value = null;
    } finally {
      settingsLoaded.value = true;
    }
  }

  async function loadTools() {
    if (tools.value.length > 0) return;
    try {
      tools.value = await aiApi.aiListTools();
    } catch {
      tools.value = [];
    }
  }

  async function loadPersistence() {
    try {
      persistence.value = await aiApi.aiGetSessionPersistence();
    } catch {
      persistence.value = null;
    }
  }

  async function togglePersistence() {
    const next = !(persistence.value?.persistSessions ?? false);
    persistence.value = await aiApi.aiSetSessionPersistence(next);
  }

  // ---------------------------------------------------------------------------
  // 会话管理（§4.2 Phase D：重命名 / 清除 / 导出 / 列表分页复用 AI-04）
  // ---------------------------------------------------------------------------

  async function loadSessions(append = false) {
    if (sessionsLoading.value) return;
    sessionsLoading.value = true;
    try {
      const result = await aiApi.aiListSessions({
        workspaceId: scope.value.workspaceId,
        includeArchived: false,
        limit: SESSION_PAGE_SIZE,
        offset: append ? sessions.value.length : 0,
      });
      sessions.value = append ? [...sessions.value, ...result.items] : result.items;
      sessionsTotal.value = result.total;
    } catch {
      if (!append) {
        sessions.value = [];
        sessionsTotal.value = 0;
      }
    } finally {
      sessionsLoading.value = false;
    }
  }

  async function loadMoreSessions() {
    if (sessionsLoading.value || sessions.value.length >= sessionsTotal.value) return;
    await loadSessions(true);
  }

  async function selectSession(id: string) {
    const loaded = await aiApi.aiGetSession(id);
    if (!loaded) return;
    detail.value = loaded;
    role.value = loaded.session.role;
    roleIsManual.value = false;
    toolReads.value = [];
    toolCallCount.value = 0;
    lastError.value = null;
    const runtime = (loaded.session.runtimeScope ?? {}) as Record<string, unknown>;
    scope.value = {
      ...scope.value,
      workspaceId: loaded.session.workspaceId,
      repositoryPaths: loaded.session.repositoryScope,
      runtimeName: typeof runtime.runtimeName === "string" ? runtime.runtimeName : null,
      processId: typeof runtime.processId === "number" ? runtime.processId : null,
      supplementary: [],
      origin: null,
    };
  }

  const sessionsHasMore = computed(() => sessions.value.length < sessionsTotal.value);
  const messagesHasMore = computed(() => {
    const current = detail.value;
    return !!current && current.messages.length < current.totalMessages;
  });

  async function loadEarlierMessages() {
    const current = detail.value;
    const oldest = current?.messages[0];
    if (!current || !oldest || current.messages.length >= current.totalMessages) return;
    const loaded = await aiApi.aiGetSession(current.session.id, MESSAGE_PAGE_SIZE, oldest.sequence);
    if (!loaded) return;
    detail.value = {
      ...loaded,
      messages: [...loaded.messages, ...current.messages],
    };
  }

  /** 新会话草稿（不落库，首次发送时创建）。 */
  function newSession() {
    detail.value = null;
    input.value = "";
    streamingText.value = "";
    activeSnapshot.value = null;
    preview.value = null;
    previewRequest.value = null;
    toolReads.value = [];
    toolCallCount.value = 0;
    lastError.value = null;
    roleIsManual.value = false;
  }

  async function renameCurrent(title: string) {
    const active = currentSession.value;
    if (!active || !title.trim()) return;
    const updated = await aiApi.aiRenameSession(active.id, title.trim());
    if (detail.value) detail.value = { ...detail.value, session: updated };
    sessions.value = sessions.value.map((s) => (s.id === updated.id ? updated : s));
  }

  async function removeSession(id: string) {
    await aiApi.aiDeleteSession(id);
    sessions.value = sessions.value.filter((s) => s.id !== id);
    if (currentSession.value?.id === id) newSession();
    await loadPersistence();
  }

  /** 导出当前会话（§10.4：内容由后端渲染，不含 Secret 原文）。 */
  async function exportCurrent(destPath: string) {
    const active = currentSession.value;
    if (!active) return null;
    return aiApi.aiExportSession(active.id, destPath);
  }

  // ---------------------------------------------------------------------------
  // 发送链路
  // ---------------------------------------------------------------------------

  async function ensureSession(): Promise<AiSession> {
    const active = currentSession.value;
    if (active) return active;
    const created = await aiApi.aiCreateSession({
      title: input.value.trim().slice(0, 24) || "新会话",
      role: role.value,
      workspaceId: scope.value.workspaceId,
      repositoryScope: scope.value.repositoryPaths,
      runtimeScope: {
        runtimeName: scope.value.runtimeName,
        processId: scope.value.processId,
      },
    });
    detail.value = { session: created, messages: [], totalMessages: 0 };
    await loadSessions();
    return created;
  }

  function makePreviewRequest(): ContextPreviewRequest {
    const supplementary = [...scope.value.supplementary];
    const history = historySupplementary(detail.value?.messages ?? []);
    if (history) supplementary.push(history);
    return {
      taskKind: "chat",
      gitScenario: null,
      providerId: null,
      modelId: null,
      workspaceId: scope.value.workspaceId,
      repoPath: scope.value.repositoryPaths[0] ?? null,
      conflict: null,
      runtimeName: scope.value.runtimeName,
      processId: scope.value.processId,
      project: null,
      userInstruction: input.value.trim(),
      diffScope: null,
      diffSelection: null,
      supplementary,
      exclusions: [...(previewRequest.value?.exclusions ?? [])],
      secretPolicy: { strategy: "block", warnConfirmed: false },
      budgetStrategy: null,
      stream: true,
      tokenEstimateFactor: null,
      logTailLines: null,
      tokenBudget: null,
      includeRuntimeLogs: false,
    };
  }

  /** 发送 = 构建 Preview（零网络）；确认在 confirmSend。 */
  async function send() {
    if (!input.value.trim() || building.value || sending.value) return;
    building.value = true;
    lastError.value = null;
    try {
      const request = makePreviewRequest();
      preview.value = await aiApi.aiBuildContextPreview(request);
      previewRequest.value = request;
      previewVisible.value = true;
    } catch (error) {
      lastError.value = normalizeError(error);
    } finally {
      building.value = false;
    }
  }

  async function rebuildPreview(request: ContextPreviewRequest) {
    building.value = true;
    try {
      preview.value = await aiApi.aiBuildContextPreview(request);
      previewRequest.value = request;
    } catch (error) {
      lastError.value = normalizeError(error);
      previewVisible.value = false;
    } finally {
      building.value = false;
    }
  }

  async function toggleExclusion(sourceId: string, included: boolean) {
    const current = previewRequest.value;
    if (!current || building.value || confirming.value) return;
    const exclusions = new Set(current.exclusions ?? []);
    if (included) exclusions.delete(sourceId);
    else exclusions.add(sourceId);
    await rebuildPreview({ ...current, exclusions: [...exclusions] });
  }

  async function confirmWarn() {
    const current = previewRequest.value;
    if (!current || current.secretPolicy?.strategy !== "warn") return;
    await rebuildPreview({
      ...current,
      secretPolicy: { ...current.secretPolicy, warnConfirmed: true },
    });
  }

  /** Preview 确认 = Gateway 唯一联网入口（全局约束 §2）。 */
  async function confirmSend() {
    const current = preview.value;
    if (!current || current.blocked || confirming.value) return;
    confirming.value = true;
    lastError.value = null;
    try {
      const session = await ensureSession();
      const submitted = await aiApi.aiSubmitRequest({
        ...current.request,
        sessionId: session.id,
        useCache: true,
      });
      activeSnapshot.value = submitted;
      const approved =
        submitted.phase === "previewRequired"
          ? await aiApi.aiApproveRequest(submitted.requestId)
          : submitted;
      activeSnapshot.value = approved;
      previewVisible.value = false;
      streamingText.value = "";
      if (TERMINAL_PHASES.has(approved.phase)) {
        await finalize(approved);
      } else {
        await follow(approved.requestId);
      }
    } catch (error) {
      lastError.value = normalizeError(error);
    } finally {
      confirming.value = false;
    }
  }

  /** 订阅流式事件；listen 失败（异常环境）降级轮询（§16.1 合帧渲染）。 */
  async function follow(requestId: string) {
    disposeFollow();
    frameBuffer = new AiStreamFrameBuffer((merged) => {
      streamingText.value += merged;
    });
    try {
      unlisten = await onAiRequestEvent((event) => {
        if (event.chunk?.type === "textDelta") {
          frameBuffer?.push(event.chunk.text);
        }
        if (TERMINAL_PHASES.has(event.phase)) {
          void finishFromStatus(requestId);
        }
      }, requestId);
    } catch {
      schedulePoll(requestId);
    }
  }

  async function finishFromStatus(requestId: string) {
    const snapshot = await aiApi.aiGetRequestStatus(requestId);
    if (snapshot) await finalize(snapshot);
  }

  function schedulePoll(requestId: string) {
    clearPoll();
    pollTimer = setTimeout(async () => {
      try {
        const snapshot = await aiApi.aiGetRequestStatus(requestId);
        if (!snapshot) return;
        activeSnapshot.value = snapshot;
        if (TERMINAL_PHASES.has(snapshot.phase)) {
          await finalize(snapshot);
        } else {
          schedulePoll(requestId);
        }
      } catch (error) {
        lastError.value = normalizeError(error);
      }
    }, 350);
  }

  /** 请求进入终态：成功则消耗输入与带入上下文并刷新会话消息。 */
  async function finalize(snapshot: AiRequestSnapshot) {
    disposeFollow();
    frameBuffer?.finish();
    activeSnapshot.value = snapshot;
    if (snapshot.phase === "succeeded") {
      input.value = "";
      streamingText.value = "";
      scope.value = { ...scope.value, supplementary: [], origin: null };
      toolCallCount.value = 0;
      if (currentSession.value) {
        const loaded = await aiApi.aiGetSession(currentSession.value.id);
        if (loaded) detail.value = loaded;
      }
      await loadSessions();
    } else if (snapshot.phase !== "cancelled") {
      // §12.4：失败保留输入与上下文，允许重试。
      lastError.value = {
        message: snapshot.error ?? "请求未完成",
        code: snapshot.errorCode,
      };
    }
  }

  async function cancel() {
    const active = activeSnapshot.value;
    if (!active || !sending.value) return;
    try {
      activeSnapshot.value = await aiApi.aiCancelRequest(active.requestId);
    } catch (error) {
      lastError.value = normalizeError(error);
    } finally {
      disposeFollow();
    }
  }

  /** 失败后重试：沿用上次 Preview 请求重建（Secret/预算/hash 重算）。 */
  async function retry() {
    const current = previewRequest.value;
    if (!current || building.value || sending.value) return;
    lastError.value = null;
    await rebuildPreview({ ...current, exclusions: [...(current.exclusions ?? [])] });
    previewVisible.value = preview.value != null;
  }

  // ---------------------------------------------------------------------------
  // 只读工具（§9.3 / §9.4：白名单 + 单次请求上限，不自动操作）
  // ---------------------------------------------------------------------------

  async function runTool(tool: AiToolDefinition) {
    if (toolRunning.value || toolCallCount.value >= TOOL_CALL_LIMIT) return;
    const args = buildToolArguments(tool, scope.value);
    if (!args) return;
    toolRunning.value = tool.name;
    toolCallCount.value += 1;
    const cardId = `tool-${Date.now()}-${tool.name}`;
    try {
      const invocation: ToolInvocation = await aiApi.aiExecuteTool({
        requestId: currentSession.value?.id ?? cardId,
        toolName: tool.name,
        role: role.value,
        arguments: args,
      });
      const resultJson = JSON.stringify(invocation.result, null, 2);
      toolReads.value.push({
        id: cardId,
        toolName: tool.name,
        argsSummary: JSON.stringify(args),
        resultSummary: resultJson.slice(0, 280),
        resultJson,
        truncated: invocation.truncated,
        durationMs: invocation.durationMs,
        error: null,
      });
      // 结果进入下一条消息的上下文（发送前经 Secret 管道与 Preview）。
      scope.value = {
        ...scope.value,
        supplementary: [
          ...scope.value.supplementary,
          {
            role: "userNote",
            kind: toolScopeToKind(tool),
            sourceId: `tool:${tool.name}`,
            displayName: `工具读取 ${tool.name}`,
            content: resultJson,
          },
        ],
      };
    } catch (error) {
      toolReads.value.push({
        id: cardId,
        toolName: tool.name,
        argsSummary: JSON.stringify(args),
        resultSummary: "",
        resultJson: "",
        truncated: false,
        durationMs: 0,
        error: normalizeError(error).message,
      });
    } finally {
      toolRunning.value = null;
    }
  }

  // ---------------------------------------------------------------------------

  function normalizeError(error: unknown): { message: string; code: string | null } {
    const code =
      error && typeof error === "object" && "code" in error
        ? ((error as ErrorResponse).code ?? null)
        : null;
    return { message: errMsg(error), code };
  }

  function clearPoll() {
    if (pollTimer) clearTimeout(pollTimer);
    pollTimer = null;
  }

  function disposeFollow() {
    clearPoll();
    unlisten?.();
    unlisten = null;
    frameBuffer?.dispose();
    frameBuffer = null;
  }

  return {
    // state
    drawerOpen,
    role,
    roleIsManual,
    scope,
    settingsSummary,
    settingsLoaded,
    sessions,
    sessionsTotal,
    sessionsLoading,
    sessionsHasMore,
    detail,
    messagesHasMore,
    persistence,
    input,
    building,
    confirming,
    previewVisible,
    preview,
    activeSnapshot,
    streamingText,
    lastError,
    tools,
    toolReads,
    toolCallCount,
    toolRunning,
    // computed
    currentSession,
    configured,
    sending,
    availableTools,
    toolCallLimit: TOOL_CALL_LIMIT,
    // actions
    toggleDrawer,
    openWithContext,
    setRoleManual,
    clearContext,
    refreshSettings,
    togglePersistence,
    loadSessions,
    loadMoreSessions,
    selectSession,
    loadEarlierMessages,
    newSession,
    renameCurrent,
    removeSession,
    exportCurrent,
    send,
    confirmSend,
    toggleExclusion,
    confirmWarn,
    cancel,
    retry,
    runTool,
    disposeFollow,
  };
});
