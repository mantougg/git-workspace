import { computed, ref } from "vue";
import {
  aiApproveRequest,
  aiGetRequestStatus,
  aiRuntimeDiagnosticPreview,
  aiSubmitRequest,
} from "@/api/ai";
import type { AiContextPreview, AiRequestSnapshot, RuntimeDiagnosticRequest } from "@/types/ai";

const TERMINAL_PHASES = new Set(["succeeded", "cancelled", "rejected", "failed", "degraded"]);

export function useRuntimeAiDiagnostic() {
  const request = ref<RuntimeDiagnosticRequest | null>(null);
  const preview = ref<AiContextPreview | null>(null);
  const snapshot = ref<AiRequestSnapshot | null>(null);
  const error = ref<unknown>(null);
  const loading = ref(false);
  const confirming = ref(false);
  const pollTimer = ref<ReturnType<typeof setTimeout> | null>(null);

  const running = computed(() => {
    const phase = snapshot.value?.phase;
    return phase != null && !TERMINAL_PHASES.has(phase);
  });

  async function build(input: RuntimeDiagnosticRequest) {
    loading.value = true;
    error.value = null;
    try {
      const next = await aiRuntimeDiagnosticPreview(input);
      request.value = {
        ...input,
        exclusions: [...(input.exclusions ?? [])],
      };
      preview.value = next;
      snapshot.value = null;
      return next;
    } catch (cause) {
      error.value = cause;
      throw cause;
    } finally {
      loading.value = false;
    }
  }

  async function open(input: RuntimeDiagnosticRequest) {
    clearPoll();
    snapshot.value = null;
    return build({ ...input, exclusions: [...(input.exclusions ?? [])] });
  }

  async function toggleExclusion(sourceId: string, included: boolean) {
    const current = request.value;
    if (!current || loading.value || confirming.value) return;
    const exclusions = new Set(current.exclusions ?? []);
    if (included) exclusions.delete(sourceId);
    else exclusions.add(sourceId);
    await build({ ...current, exclusions: [...exclusions] });
  }

  async function confirmWarn() {
    const current = request.value;
    if (!current || current.secretPolicy?.strategy !== "warn") return;
    await build({
      ...current,
      secretPolicy: { ...current.secretPolicy, warnConfirmed: true },
    });
  }

  async function confirm() {
    if (!preview.value || preview.value.blocked || confirming.value) return;
    confirming.value = true;
    error.value = null;
    try {
      const submitted = await aiSubmitRequest({
        ...preview.value.request,
        useCache: true,
      });
      snapshot.value = submitted;
      // submit is intentionally still stopped at PreviewRequired; only this
      // explicit confirmation crosses the Gateway network boundary.
      if (submitted.phase === "previewRequired") {
        snapshot.value = await aiApproveRequest(submitted.requestId);
      }
      schedulePoll(snapshot.value.requestId);
    } catch (cause) {
      error.value = cause;
    } finally {
      confirming.value = false;
    }
  }

  async function poll(requestId: string) {
    try {
      const next = await aiGetRequestStatus(requestId);
      if (!next) return;
      snapshot.value = next;
      if (!TERMINAL_PHASES.has(next.phase)) schedulePoll(requestId);
    } catch (cause) {
      error.value = cause;
    }
  }

  function schedulePoll(requestId: string) {
    clearPoll();
    pollTimer.value = setTimeout(() => void poll(requestId), 350);
  }

  function clearPoll() {
    if (pollTimer.value) clearTimeout(pollTimer.value);
    pollTimer.value = null;
  }

  function dispose() {
    clearPoll();
  }

  return {
    request,
    preview,
    snapshot,
    error,
    loading,
    confirming,
    running,
    open,
    confirm,
    confirmWarn,
    toggleExclusion,
    dispose,
  };
}
