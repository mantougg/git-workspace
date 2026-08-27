<template>
  <div v-if="parsed" class="runtime-error-alert">
    <n-alert :title="title" type="error" :closable="closable" @close="emit('dismiss')">
      <div class="err-body">
        <div class="err-message">{{ parsed.message }}</div>
        <div v-if="contextLines.length" class="err-context">
          <div v-for="line in contextLines" :key="line" class="mono context-line">
            {{ line }}
          </div>
        </div>
        <div v-if="actions.length" class="err-actions">
          <n-button
            v-for="action in actions"
            :key="action.label"
            size="small"
            :type="action.type ?? 'primary'"
            @click="action.onClick"
          >
            {{ action.label }}
          </n-button>
        </div>
      </div>
    </n-alert>
  </div>
</template>

<script setup lang="ts">
// R-14 §80 可行动错误提示：解析后端结构化 ErrorResponse（code / message /
// details JSON），渲染 Reason + 上下文（PID / 端口 / 模块）+ Suggested
// Actions 按钮。禁止只显示 `Process exited with code 1` 这类裸消息。

import { computed } from "vue";
import { useRouter } from "vue-router";
import type { ErrorResponse } from "@/utils/error";

const props = withDefaults(
  defineProps<{
    error: unknown;
    closable?: boolean;
  }>(),
  { closable: true },
);

const emit = defineEmits<{
  (e: "dismiss"): void;
  /** 用户点击「确认并执行脚本」（details 为 ScriptConfirmationRequired 的上下文）。 */
  (e: "confirm-script", details: Record<string, unknown>): void;
  /** 用户点击「重试」。 */
  (e: "retry"): void;
  /** 用户点击「查看日志」。 */
  (e: "open-logs"): void;
}>();

const router = useRouter();

interface ParsedError {
  code?: string;
  message: string;
  details?: Record<string, unknown> | null;
}

const parsed = computed<ParsedError | null>(() => {
  const raw = props.error;
  if (!raw) return null;
  if (typeof raw === "string") {
    return { message: raw };
  }
  if (typeof raw === "object" && raw !== null && "message" in raw) {
    const err = raw as ErrorResponse;
    let details: Record<string, unknown> | null = null;
    if (typeof err.details === "string" && err.details) {
      try {
        const value = JSON.parse(err.details);
        if (value && typeof value === "object") {
          details = value as Record<string, unknown>;
        }
      } catch {
        // 非 JSON 的 details 原样展示
        details = { raw: err.details };
      }
    }
    return {
      code: err.code,
      message:
        typeof err.message === "string" && err.message.length > 0
          ? err.message
          : String(raw),
      details,
    };
  }
  return { message: String(raw) };
});

const TITLES: Record<string, string> = {
  ProjectNotFound: "项目未找到",
  MavenNotFound: "Maven 不可用",
  JdkNotFound: "JDK 不可用",
  InvalidPom: "POM 文件无效",
  DependencyResolveFailed: "依赖解析失败",
  SourceMappingFailed: "源码映射失败",
  BuildFailed: "构建失败",
  ProcessStartFailed: "进程启动失败",
  PortOccupied: "端口被占用",
  HealthCheckFailed: "健康检查失败",
  ProcessCrashed: "进程异常退出",
  ScriptConfirmationRequired: "需要确认脚本执行",
  ScriptFailed: "脚本执行失败",
  RuntimeConfigError: "Runtime 配置错误",
  PermissionError: "操作被拒绝",
};

const title = computed(() => {
  const code = parsed.value?.code;
  return (code && TITLES[code]) || "Runtime 操作失败";
});

/** 上下文行：details 中的结构化字段 → 可读文本（§80：PID / 端口 / 模块…）。 */
const contextLines = computed<string[]>(() => {
  const details = parsed.value?.details;
  if (!details) return [];
  const lines: string[] = [];
  const push = (label: string, value: unknown) => {
    if (value === null || value === undefined || value === "") return;
    lines.push(`${label}: ${String(value)}`);
  };
  push("模块", details.module);
  push("退出码", details.exitCode);
  push("PID", details.pid);
  push("端口", details.port);
  push("占用进程", details.processName);
  push("Runtime", details.runtime);
  push("路径", details.path);
  push("原因", details.reason);
  push("脚本类型", details.scriptType === "pre" ? "Pre-Build" : details.scriptType === "post" ? "Post-Build" : details.scriptType);
  if (typeof details.logTail === "string" && details.logTail) {
    const tail = details.logTail.length > 300 ? details.logTail.slice(-300) + "…" : details.logTail;
    lines.push(`日志尾部: ${tail}`);
  }
  return lines;
});

interface Action {
  label: string;
  type?: "primary" | "success" | "warning" | "error" | "info";
  onClick: () => void;
}

const actions = computed<Action[]>(() => {
  const code = parsed.value?.code;
  const details = parsed.value?.details;
  const list: Action[] = [];
  switch (code) {
    case "JdkNotFound":
      list.push({
        label: "打开 JDK 管理",
        onClick: () => router.push({ name: "jdk-manager" }),
      });
      break;
    case "MavenNotFound":
      list.push({
        label: "打开 Maven 设置",
        onClick: () => router.push({ name: "maven-settings" }),
      });
      break;
    case "ScriptConfirmationRequired":
      list.push({
        label: "查看脚本并确认执行",
        type: "warning",
        onClick: () => emit("confirm-script", details ?? {}),
      });
      break;
    case "BuildFailed":
      list.push({
        label: "查看日志",
        onClick: () => emit("open-logs"),
      });
      list.push({
        label: "重试",
        onClick: () => emit("retry"),
      });
      break;
    case "ProcessStartFailed":
    case "ProcessCrashed":
    case "ScriptFailed":
      list.push({
        label: "查看日志",
        onClick: () => emit("open-logs"),
      });
      break;
    case "PortOccupied":
      list.push({
        label: "查看日志",
        onClick: () => emit("open-logs"),
      });
      break;
    case "ProjectNotFound":
    case "DependencyResolveFailed":
      list.push({
        label: "解析依赖后重试",
        onClick: () => emit("retry"),
      });
      break;
    default:
      break;
  }
  return list;
});
</script>

<style scoped>
.runtime-error-alert {
  margin-bottom: 10px;
}
.err-body {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 2px 0;
}
.err-message {
  font-size: 13px;
  word-break: break-all;
}
.err-context {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.context-line {
  font-size: 12px;
  color: var(--n-text-color-regular, #666);
  word-break: break-all;
  white-space: pre-wrap;
}
.err-actions {
  display: flex;
  gap: var(--gw-space-2);
  flex-wrap: wrap;
  margin-top: 2px;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
</style>
