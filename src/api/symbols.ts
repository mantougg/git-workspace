import { invoke } from "@tauri-apps/api/core";
import type {
  SymbolCallHit,
  SymbolIndexStats,
  SymbolRefHit,
  SymbolHit,
} from "@/types/symbols";

/** 构建/更新符号索引（全量增量；files 给定时只重解析这些相对路径）。 */
export function buildSymbolIndex(
  repoPath: string,
  files?: string[],
): Promise<SymbolIndexStats> {
  return invoke<SymbolIndexStats>("build_symbol_index", {
    repoPath,
    files: files ?? null,
  });
}

/** 符号搜索：@repo:/@group:/@status:/@ext:/@path: 过滤 + 名称关键字。 */
export function searchSymbols(
  query: string,
  workspaceId?: number,
): Promise<SymbolHit[]> {
  return invoke<SymbolHit[]>("search_symbols", {
    query,
    workspaceId: workspaceId ?? null,
  });
}

/** 精确名称 → 定义列表（Go To Definition）。 */
export function findSymbolDefinitions(
  name: string,
  query?: string,
  workspaceId?: number,
): Promise<SymbolHit[]> {
  return invoke<SymbolHit[]>("find_symbol_definitions", {
    name,
    query: query ?? null,
    workspaceId: workspaceId ?? null,
  });
}

/** 精确名称 → 引用列表（Find References）。 */
export function findSymbolReferences(
  name: string,
  query?: string,
  workspaceId?: number,
): Promise<SymbolRefHit[]> {
  return invoke<SymbolRefHit[]>("find_symbol_references", {
    name,
    query: query ?? null,
    workspaceId: workspaceId ?? null,
  });
}

/** 调用层级：direction = "callers" | "callees"。 */
export function symbolCallHierarchy(
  name: string,
  direction: "callers" | "callees",
  query?: string,
  workspaceId?: number,
): Promise<SymbolCallHit[]> {
  return invoke<SymbolCallHit[]>("symbol_call_hierarchy", {
    name,
    direction,
    query: query ?? null,
    workspaceId: workspaceId ?? null,
  });
}
