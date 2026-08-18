import { invoke } from "@tauri-apps/api/core";
import type { SpringBootWorkspaceResult } from "@/types/springBoot";

/** Discover Spring Boot modules and Main Class candidates in a workspace. */
export function detectSpringBoot(
  workspaceRoot: string,
  scanDepth?: number,
): Promise<SpringBootWorkspaceResult> {
  return invoke<SpringBootWorkspaceResult>("detect_spring_boot", {
    workspaceRoot,
    scanDepth: scanDepth ?? null,
  });
}
