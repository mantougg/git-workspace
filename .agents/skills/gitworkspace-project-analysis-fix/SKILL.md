---
name: gitworkspace-project-analysis-fix
description: GitWorkspace 项目分析报告修复批次（docs/tasks-project-analysis-fix/ PAF-01~PAF-26，源自 docs/project-analysis-2026-09-13.md 事实核查）的编排执行：修复顺序、任务簇协同、证据锚点复核、分析报告回写。当用户点名 PAF-XX 任务，或说「分析报告批次 / 审计批次 / 修复报告里的 bug」时使用；单个通用 F-XX 用户反馈修复走 gitworkspace-fix。
---

# GitWorkspace 项目分析报告修复批次（PAF-01~PAF-26）

本 skill 编排 **2026-09-13 项目全景分析批次** 的 26 个修复任务
（`docs/tasks-project-analysis-fix/PAF-01`~`PAF-26`）。逐任务的开始/继续/
完成流程与两处状态同步规范与 `gitworkspace-fix` 一致；本 skill 只补充该批次
特有的**顺序、簇协同与回写纪律**。

## 必读文档（最小加载集）

| 文件 | 作用 |
|---|---|
| `docs/tasks-project-analysis-fix/README.md` | 总索引：26 个任务的优先级/状态总表 + 维护规范 |
| `docs/tasks-project-analysis-fix/00-全局开发约束.md` | 批次横切硬规则（证据纪律、三处同步、簇协同、验证基线） |
| `docs/tasks-project-analysis-fix/PAF-XX-*.md` | 目标任务 spec（含已实证的 file:line 证据） |
| `docs/project-analysis-2026-09-13.md` | 分析报告原文（§3 bug 清单需回写） |

## 批次地图

| 簇 | 任务 | 说明 |
|---|---|---|
| P0 独立项 | PAF-01、PAF-04、PAF-05 | 几行级修复（UTF-8 切片 / btoa spread / hasMore 比较顺序），优先清掉 |
| 启动链路簇 | PAF-02、PAF-03、PAF-07、PAF-06（+PAF-09） | 同一 spawn/stop/restart 状态机与 launch_cache，**建议同批实施**，互相验证不回归 |
| 内存三连 | PAF-12 | gateway records / 终端 writeBuffer / chat known_addrs，一次做完 |
| Git 安全簇 | PAF-10、PAF-11 | rebase/merge/cherry-pick 前置校验 + batch_add/restore 收编队列 |
| watcher | PAF-13 | core watcher 四缺陷，一处文件集中修 |
| 前端簇 | PAF-14、PAF-15、PAF-16、PAF-17 | 终端失效三连 / loadSeq 推广 / 监听锁死 / 冲突横幅 |
| 后端加固 | PAF-18~PAF-24 | 路径边界 / git_link expect / MCP 鉴权 / 凭证缓存 / 变更树并行 / 索引持锁 / PTY 生命周期 |
| 流式底座 | PAF-08、PAF-25 | 共用 `run_git_streaming`，**必须同批实施**（取消语义 + 实时输出一起接） |
| P2 清单 | PAF-26 | 约 40 项加固清单，触及对应文件时顺手修或拆分独立任务 |

## 执行纪律

1. **修复前先复核证据**：任务文档内的 file:line 来自 2026-09-13 核查，行号
   可能已漂移——以函数名/结构名为锚重新定位；若证据已不成立（已被其他
   提交修复），在时间线注明并将任务置 ⏸️ 说明原因，不要强行修改。
2. **顺序建议**：P0 三项独立修复（PAF-01/PAF-04/PAF-05）→ 启动链路簇
   （PAF-02→PAF-03→PAF-07→PAF-06）→ 内存三连（PAF-12）→ 其余 P1 按簇
   推进 → PAF-26。
3. **簇内回归**：启动链路簇每改一处，跑 `cargo test --lib` 中 runtime::launch
   相关测试（含 boot_fixture 系列，无 Maven/JDK 环境会按约定 skip；Windows
   需 `GW_TEST_MANIFEST=1`）；流式底座簇跑 task::worker 与 git_ops 测试。
4. **完成回写三处**：
   - `docs/tasks-project-analysis-fix/README.md` 总表 + 任务文档进度章节；
   - `docs/project-analysis-2026-09-13.md` §3 对应条目行尾追加
     「✅ 已修复（PAF-XX，2026-MM-DD）」，保持分析报告与任务状态一致。
5. **修复牵出新问题**：新增 PAF-XX 文档（接续编号），不在原任务里扩张
   范围；若新发现推翻报告结论，同步修正报告 §3/§5 对应条目。
6. **PAF-26 的处理**：清单项被修复或拆分后，在 PAF-26 文档中勾掉该项并
   记录去向；全部处理完（或明确不修并注明理由）才能置 ✅。
7. **改动前影响分析**：修改函数/类/方法前按 AGENTS.md 跑 GitNexus
   `impact`；提交前 `detect_changes()` 核对改动范围。

## 验收出口

批次全部完成的标志：README 总表 PAF-01~PAF-26 全 ✅（或 ⏸️ 带理由）、
分析报告 §3 每条都有处置记录、`cargo test --lib` 与 `pnpm build` 通过。
