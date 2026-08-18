/** R-07 Runtime configuration IPC types. */

export interface RuntimeApplicationConfig {
  schemaVersion: number;
  name: string;
  project: string;
  mainClass: string | null;
  jdk: string | null;
  profile: string | null;
  vmOptions: string[];
  programArguments: string[];
  environment: Record<string, string>;
  runtimeEnvironment: Record<string, string>;
  buildEngine: string | null;
}

export interface RuntimeConfigSummary {
  id: number;
  workspaceId: number;
  name: string;
  project: string;
  mainClass: string | null;
  jdk: string | null;
  profile: string | null;
  buildEngine: string | null;
  configPath: string;
  createdAt: string;
  updatedAt: string;
}

export interface CreateRuntimeConfigRequest {
  workspaceId: number;
  config: RuntimeApplicationConfig;
}

export interface UpdateRuntimeConfigRequest {
  workspaceId: number;
  name: string;
  config: RuntimeApplicationConfig;
}
