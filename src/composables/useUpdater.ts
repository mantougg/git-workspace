import { onBeforeUnmount, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  check,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";

export type UpdaterStatus =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "ready"
  | "error";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Owns the updater resource and exposes a small, view-friendly state machine. */
export function useUpdater() {
  const status = ref<UpdaterStatus>("idle");
  const update = ref<Update | null>(null);
  const updateVersion = ref("");
  const updateBody = ref("");
  const downloadProgress = ref<number | null>(null);
  const error = ref("");
  let contentLength = 0;
  let downloadedBytes = 0;

  async function closePendingUpdate() {
    if (update.value && status.value !== "ready") {
      await update.value.close().catch(() => undefined);
    }
    update.value = null;
  }

  async function checkForUpdates() {
    if (status.value === "checking" || status.value === "downloading") return;

    error.value = "";
    downloadProgress.value = null;
    status.value = "checking";

    try {
      await closePendingUpdate();
      const result = await check();
      if (!result) {
        status.value = "upToDate";
        return;
      }

      update.value = result;
      updateVersion.value = result.version;
      updateBody.value = result.body ?? "";
      status.value = "available";
    } catch (cause) {
      status.value = "error";
      error.value = errorMessage(cause);
    }
  }

  async function downloadAndInstall() {
    const currentUpdate = update.value;
    if (!currentUpdate || status.value !== "available") return;

    error.value = "";
    status.value = "downloading";
    contentLength = 0;
    downloadedBytes = 0;
    downloadProgress.value = null;

    const onEvent = (event: DownloadEvent) => {
      if (event.event === "Started") {
        contentLength = event.data.contentLength ?? 0;
        downloadedBytes = 0;
        downloadProgress.value = contentLength > 0 ? 0 : null;
      } else if (event.event === "Progress") {
        downloadedBytes += event.data.chunkLength;
        downloadProgress.value =
          contentLength > 0
            ? Math.min(100, Math.round((downloadedBytes / contentLength) * 100))
            : null;
      } else {
        downloadProgress.value = 100;
      }
    };

    try {
      await currentUpdate.downloadAndInstall(onEvent);
      status.value = "ready";
      downloadProgress.value = 100;
    } catch (cause) {
      status.value = "error";
      error.value = errorMessage(cause);
    }
  }

  async function restartApp() {
    if (status.value !== "ready") return;
    try {
      await invoke("restart_app");
    } catch (cause) {
      status.value = "error";
      error.value = errorMessage(cause);
    }
  }

  onBeforeUnmount(() => {
    if (update.value && status.value !== "ready") {
      void update.value.close();
    }
  });

  return {
    status,
    updateVersion,
    updateBody,
    downloadProgress,
    error,
    checkForUpdates,
    downloadAndInstall,
    restartApp,
  };
}
