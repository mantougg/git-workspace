import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  MavenExecutable,
  MavenExecutionRequest,
  ResolvedMaven,
} from "@/types/maven";

/**
 * R-05 Maven 检测与执行策略 API。
 * 所有调用走 Tauri IPC，对应 src-tauri/src/commands/maven.rs 的 #[tauri::command]。
 */

/** 对项目跑优先级链检测 + `mvn -v` 探测，返回 ResolvedMaven 并缓存。 */
export function detectMaven(
  projectDir: string,
  configuredPath?: string | null,
): Promise<ResolvedMaven> {
  return invoke<ResolvedMaven>("detect_maven", {
    projectDir,
    configuredPath: configuredPath ?? null,
  });
}

/** 列出注册表全部 Maven 可执行体（有效优先）。 */
export function listMavenExecutables(): Promise<MavenExecutable[]> {
  return invoke<MavenExecutable[]>("list_maven_executables_cmd");
}

/** 按 id 取单条 Maven 可执行体。 */
export function getMavenExecutable(id: number): Promise<MavenExecutable | null> {
  return invoke<MavenExecutable | null>("get_maven_executable_cmd", { id });
}

/** 强制复检单条 Maven（fork `mvn -v`）。 */
export function validateMavenExecutable(id: number): Promise<MavenExecutable> {
  return invoke<MavenExecutable>("validate_maven_executable", { id });
}

/** 惰性校验：把路径已不存在的条目标记失效。返回被标记的条数。 */
export function pruneInvalidMaven(): Promise<number> {
  return invoke<number>("prune_invalid_maven");
}

/** 按 id 删除单条 Maven 可执行体。 */
export function removeMavenExecutable(id: number): Promise<void> {
  return invoke<void>("remove_maven_executable_cmd", { id });
}

/** 探测生效本地仓库路径（settings.xml 覆盖 `~/.m2/repository`）。 */
export function resolveLocalRepo(globalSettingsPath?: string | null): Promise<string> {
  return invoke<string>("resolve_local_repo", {
    globalSettingsPath: globalSettingsPath ?? null,
  });
}

/** 预览 Maven 命令行（§75 可追溯）。 */
export function previewMavenCommand(req: MavenExecutionRequest): Promise<string> {
  return invoke<string>("preview_maven_command", { req });
}

/** 仅跑优先级链检测（不 fork），返回候选列表。 */
export function listMavenCandidates(
  projectDir?: string | null,
  configuredPath?: string | null,
): Promise<MavenExecutable[]> {
  return invoke<MavenExecutable[]>("list_maven_candidates", {
    projectDir: projectDir ?? null,
    configuredPath: configuredPath ?? null,
  });
}

/** 构造完整命令行数组（供 R-09 Build Engine 直接 spawn）。 */
export function buildMavenCommand(req: MavenExecutionRequest): Promise<string[]> {
  return invoke<string[]>("build_maven_command", { req });
}

// ---------------------------------------------------------------------------
// F-16：扫描安装 / 手动添加 / 本地仓库覆盖
// ---------------------------------------------------------------------------

/** 全量扫描本机 Maven 安装（mise / SDKMAN / PATH），探测版本并入库。 */
export function scanMavenInstallations(): Promise<MavenExecutable[]> {
  return invoke<MavenExecutable[]>("scan_maven_installations");
}

/** 手动添加 Maven 可执行路径（source=configured；探测失败返回可行动错误）。 */
export function addMavenExecutable(path: string): Promise<MavenExecutable> {
  return invoke<MavenExecutable>("add_maven_executable", { path });
}

/** 弹文件选择器手动添加 Maven 可执行文件；用户取消返回 null。 */
export async function addMavenExecutableByPicker(): Promise<MavenExecutable | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    title: "选择 Maven 可执行文件（Windows 选 bin/mvn.cmd）",
  });
  if (typeof selected !== "string" || !selected) {
    return null;
  }
  return addMavenExecutable(selected);
}

/** 查询应用级本地仓库覆盖（null = 按 settings.xml 探测）。 */
export function getMavenLocalRepoOverride(): Promise<string | null> {
  return invoke<string | null>("get_maven_local_repo_override");
}

/** 设置/清除应用级本地仓库覆盖（null = 恢复 settings.xml 探测）。 */
export function setMavenLocalRepoOverride(path?: string | null): Promise<void> {
  return invoke<void>("set_maven_local_repo_override", { path: path ?? null });
}

/** 弹目录选择器设置本地仓库覆盖；用户取消返回 null，返回最终生效路径。 */
export async function pickMavenLocalRepoOverride(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择 Maven 本地仓库目录",
  });
  if (typeof selected !== "string" || !selected) {
    return null;
  }
  await setMavenLocalRepoOverride(selected);
  return resolveLocalRepo();
}

/**
 * 弹出目录选择器，选一个项目目录并检测其 Maven。
 * 返回 ResolvedMaven 或 null（用户取消）。
 */
export async function detectMavenByPicker(
  configuredPath?: string | null,
): Promise<ResolvedMaven | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "选择项目根目录（含 pom.xml / mvnw）",
  });
  if (typeof selected !== "string" || !selected) {
    return null;
  }
  return invoke<ResolvedMaven>("detect_maven", {
    projectDir: selected,
    configuredPath: configuredPath ?? null,
  });
}
