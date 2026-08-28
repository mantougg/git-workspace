// R-21 §49 Git 操作保护：Checkout / 切分支前检查运行中应用。
//
// 约束（任务文档「架构/性能注意点」）：保护查询是轻量 IPC（后端纯 DB 读，
// `runtime_running_briefs`），无运行中应用时**零额外开销**（一次空列表
// 查询，不拖慢正常 Git 操作）；Runtime 侧不修改 Git 状态——Stop 由
// `runtime_stop_blocking` 同步完成后再继续切换。

import { useDialog } from "naive-ui";
import { runtimeRunningBriefs, runtimeStopBlocking } from "@/api/runtime";
import type { RuntimeRunningBrief } from "@/types/runtime";
import { errMsg } from "@/utils/error";

type Dialog = ReturnType<typeof useDialog>;
type Message = ReturnType<typeof import("naive-ui").useMessage>;

/**
 * Checkout 前的运行中应用确认（§49）：
 * - 无运行中应用 → 直接放行（一次轻量查询）；
 * - 有运行中应用 → 弹确认（`Stop & Switch / Cancel`，§49 提示语），列出
 *   受影响的应用与风险说明；用户选 Stop & Switch 时先同步优雅停止全部
 *   运行中应用再继续。
 *
 * 返回 true = 可以继续执行切换；false = 用户取消。
 */
export async function guardRuntimeRunning(
  dialog: Dialog,
  message: Message,
): Promise<boolean> {
  let briefs: RuntimeRunningBrief[];
  try {
    briefs = await runtimeRunningBriefs();
  } catch (e) {
    // 保护查询失败不应阻断正常 Git 操作（降级放行并提示）。
    console.error("R-21: running briefs query failed:", e);
    return true;
  }
  if (briefs.length === 0) return true;

  const appList = briefs
    .map((b) => `· ${b.runtimeName}（${b.status}）`)
    .join("\n");
  const confirmed = await new Promise<boolean>((resolve) => {
    dialog.warning({
      title: "Runtime 应用正在运行",
      content:
        `以下 Runtime 应用正在运行：\n${appList}\n\n` +
        "切换分支可能使运行时产物失效或导致应用异常。\n\n" +
        "Stop & Switch：先优雅停止这些应用，再继续切换；\nCancel：放弃本次切换。",
      positiveText: "Stop & Switch",
      negativeText: "Cancel",
      onPositiveClick: () => resolve(true),
      onNegativeClick: () => resolve(false),
      onClose: () => resolve(false),
    });
  });
  if (!confirmed) return false;

  for (const brief of briefs) {
    try {
      await runtimeStopBlocking(brief.workspaceId, brief.runtimeName);
    } catch (e) {
      message.error(`停止 ${brief.runtimeName} 失败：${errMsg(e)}`);
      return false;
    }
  }
  return true;
}
