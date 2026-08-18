import { invoke } from "@tauri-apps/api/core";
import type {
  CreateRuntimeConfigRequest,
  RuntimeApplicationConfig,
  RuntimeConfigSummary,
  UpdateRuntimeConfigRequest,
} from "@/types/runtime";

export function createRuntimeConfig(
  req: CreateRuntimeConfigRequest,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("create_runtime_config", { req });
}

export function updateRuntimeConfig(
  req: UpdateRuntimeConfigRequest,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("update_runtime_config", { req });
}

export function deleteRuntimeConfig(
  workspaceId: number,
  name: string,
): Promise<void> {
  return invoke<void>("delete_runtime_config", { workspaceId, name });
}

/** Metadata-only list; the backend intentionally does not open JSON files here. */
export function listRuntimeConfigs(
  workspaceId: number,
): Promise<RuntimeConfigSummary[]> {
  return invoke<RuntimeConfigSummary[]>("list_runtime_configs", { workspaceId });
}

export function getRuntimeConfig(
  workspaceId: number,
  name: string,
): Promise<RuntimeApplicationConfig> {
  return invoke<RuntimeApplicationConfig>("get_runtime_config", {
    workspaceId,
    name,
  });
}

export function resolveRuntimeEnvironment(
  workspaceId: number,
  name: string,
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("resolve_runtime_environment", {
    workspaceId,
    name,
  });
}

export function getWorkspaceRuntimeEnvironment(
  workspaceId: number,
): Promise<Record<string, string>> {
  return invoke<Record<string, string>>("get_workspace_runtime_environment", {
    workspaceId,
  });
}

export function setWorkspaceRuntimeEnvironment(
  workspaceId: number,
  environment: Record<string, string>,
): Promise<void> {
  return invoke<void>("set_workspace_runtime_environment", {
    workspaceId,
    environment,
  });
}
