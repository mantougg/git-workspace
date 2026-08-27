/** F-01b：提交热力图。 */
export interface HeatmapDay {
  date: string;
  count: number;
}

export interface CommitHeatmap {
  /** 匹配到的提交者标识（email 优先）；未配置 git 身份时为 null。 */
  identity: string | null;
  days: HeatmapDay[];
}
