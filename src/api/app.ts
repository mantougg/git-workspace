import { invoke } from "@tauri-apps/api/core";

/** F-38：清除数据结果（单表：表名 + 清除行数）。 */
export interface TableClearResult {
  table: string;
  deleted: number;
}

/**
 * 清除本地历史与缓存数据（运行历史 / 仓库索引 / 符号索引 / Maven 索引 /
 * AI 历史缓存等可重建数据）；工作区、JDK、Runtime 配置等手动配置保留。
 */
export function clearCachedData() {
  return invoke<TableClearResult[]>("clear_cached_data");
}
