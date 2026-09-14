---
name: gitworkspace-audit
description: GitWorkspace 项目全景审计工作流：多智能体并行探索功能领域 → 逐条事实核查（所有结论必须 file:line 源码背书）→ 生成分析报告（docs/project-analysis-YYYY-MM-DD.md）→ 拆分修复任务到 docs/tasks-fix/。当用户要求「全面分析这个项目」「评估功能成熟度」「系统排查 bug」「基于分析报告拆任务」时使用。
---

# GitWorkspace 项目全景审计流程

本 skill 教你在 **GitWorkspace** 项目中执行一次**可审计**的全景分析：
从代码与文档出发，经两轮迭代产出事实核查版分析报告，并把发现拆成
`docs/tasks-fix/` 的 F-XX 任务。

## 铁律：所有结论必须以事实为基础

- **禁止不看源码下结论**。探索智能体的输出只是线索，不是结论。
- 每条进入报告的断言（bug、成熟度、量化数字）必须有 `文件:行号 + 代码摘录`
  背书；行号会漂移，正文以**函数名/结构名**为锚。
- 量化数字（测试数、benchmark 毫秒数）必须 grep/文档实测，不接受估计。

## 阶段一：并行探索（功能领域）

1. 先自行快速扫描：README、package.json、`docs/tasks*/README.md`（任务状态
   总表）、src-tauri/src 与 src 目录结构。
2. 按功能领域拆分派 Explore 智能体（本项目已验证的划分：多仓库引擎 /
   Git 客户端 / Java Runtime / Node 运行时 + 终端 / AI 助手 / 前端 UI /
   任务队列与远程，外加 2 个专职 bug 排查——Rust 后端与 Vue 前端）。
3. 每个智能体的输出要求固定四段：① 子功能实现位置（文件:行号）与核心设计；
   ② 成熟度证据（文档状态、测试函数名）；③ 未完成/技术债；④ 潜在 bug。
4. **智能体会间歇性失败**（Model request failed / captcha verify failed）：
   小批次派发（2~3 个一批），失败即重试，同一 prompt 反复失败时改为自己
   直接 grep/阅读核查。

## 阶段二：逐条事实核查（必须做）

1. 把第一阶段报告中的所有断言整理成**核查清单**（每条含断言原文 + 声称的
   文件:行号）。
2. 派核查智能体（或亲自）逐条验证，提示词必须明确：「**不要相信断言，
   必须亲自打开文件读源码**」，输出格式为逐条 ✅ 成立 / ❌ 不成立 /
   ⚠️ 需修正 + 证据摘录。
3. 高价值结论（P0/P1 级 bug）优先由主控亲自 Read/Grep 复核。
4. 产出核查统计（N 成立 / M 推翻 / K 修正）与差异记录。

## 阶段三：生成报告

写入 `docs/project-analysis-YYYY-MM-DD.md`，结构参照
`docs/project-analysis-2026-09-13.md`：

1. 项目概况（规模、测试基数、文档完成度——全部实测值）
2. 各功能点成熟度评估（🟢/🟡/🟠/⚪ 评级 + 证据表）
3. Bug 清单（P0/P1/P2 分级，每条带位置与影响）
4. 新功能建议（规划内缺口 + 基于发现的高价值建议）
5. **核查记录**（被推翻项、修正项、新发现——保证报告可审计）
6. 结论与修复顺序建议

## 阶段四：拆分任务

- 分析报告产出的 Bug/缺陷 → `docs/tasks-project-analysis-fix/PAF-XX-<slug>.md`
  （接续现有编号；批次编排由 `gitworkspace-project-analysis-fix` skill 覆盖，
  逐任务流程规范与 `gitworkspace-fix` 一致）。用户日常反馈的问题仍进入
  `docs/tasks-fix/` F 系列，不要混放。PAF 文档格式照既有 F-XX spec：头部表格
  （优先级/状态/来源/关联任务）、问题描述（含实证证据摘录）、定位与修复建议、
  验收标准、进度（时间线首行记录入）。关联性强的小项可合为一个主题任务
  （参照 tasks-fix 的 F-09/F-34 先例）；P2 级汇总为一个批次清单文档。
- 同步更新 `docs/tasks-project-analysis-fix/README.md`：来源行、总体进度
  计数、任务索引表。
- 新功能类不进入修复系列；属于 R/T/N/AI 规划系列的在报告 §4 列出即可。

## 必须遵守

- **报告落 docs/ 根目录**，命名 `project-analysis-YYYY-MM-DD.md`（沿用
  code-review / feature-proposals 惯例）。
- 修复类改动遵守 `AGENTS.md`：改符号前先跑 GitNexus impact 分析；涉及
  Windows/macOS/Linux 差异先对照「平台兼容性开发规范」。
- 文档维护规范以各 tasks 系列 README 末尾为准（状态两处同步：
  总索引 + 任务文档进度章节）。
