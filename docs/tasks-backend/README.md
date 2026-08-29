# GitWorkspace 后端模块拆分任务总览

> 来源：`docs/backend-module-split-plan.md`（设计稿，下称「设计文档」，各任务表格中的 `§` 号指其章节号）。
> 拆分原则：**按目标模块拆分**，每个任务一个独立文档（同目录下 `B-XX-<slug>.md`），对应设计文档的一个 Phase 或一个模块的拆分，可独立跟踪进度与验收。
> 本文件是唯一的总进度索引；每个任务文档内另有自己的「进度」章节。
>
> 编号用 **B-XX**（Backend 重构），与 Git 任务（T-XX）、Runtime 任务（R-XX）、Desktop 任务（D-XX）、Fix（F-XX）、AI 任务（AI-XX）区分。
> 本套任务是**纯后端重构**：不改变 IPC 契约、任务语义、数据库结构、事件名和跨平台行为（设计文档 §1）。
>
> 横切约束：本目录 [00-全局开发约束.md](./00-全局开发约束.md) 为所有 B-XX 任务**必读**；根 `AGENTS.md` 平台兼容性规范一并生效（各任务文档顶部标注了最小加载集）。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**10**
- 已完成：**6** · 进行中：**0** · 未开始：**4**
- 完成度：**6 / 10（60%）**

## 总体口径（设计文档 §1 / §11）

- 保持「模块化单体」：不拆多 crate、不引微服务、不立即全面 Hexagonal（§10）。
- 两步法：**模块文件拆分**（`xxx.rs` → `xxx/mod.rs` + 子模块，公共路径不变）→ **按需收敛依赖边界**（B-10，条件触发）。
- 完成标准以设计文档 §11 为准：最终判断标准是「一次常见修改的影响范围」，不是文件数量。
- `task/dag.rs`（算法内聚性高）与 `runtime/build/pipeline.rs` 生产代码本轮**不拆**（§2.2 / §4.3）；pipeline 只外移测试（含在 B-01）。

---

## 阶段与任务索引

### Phase 0 · 基线与测试外移（前置，P0，1 个）

> 对应设计文档 §6 Phase 0：只调整文件组织，不改变生产逻辑。是全部后续任务的前置。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| B-01 | 基线固定与测试外移（含 ipc_golden 按领域拆） | P0 | ✅ | — | [B-01-baseline-test-extraction.md](./B-01-baseline-test-extraction.md) |

### Phase 1/2 · Runtime 核心（P0，2 个）

> 对应设计文档 §6 Phase 1/2。`service.rs` 是当前最该拆的文件（§2.2）；`manager.rs` 并发与平台风险最高，紧随其后。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| B-02 | 拆 RuntimeService（service.rs → service/） | P0 | ✅ | B-01 | [B-02-runtime-service.md](./B-02-runtime-service.md) |
| B-03 | 拆 RuntimeProcessManager（manager.rs → manager/） | P0 | ✅ | B-01（建议 B-02 后） | [B-03-runtime-process-manager.md](./B-03-runtime-process-manager.md) |

### Phase 3 · 数据索引与日志引擎（P1，2 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| B-04 | 拆 Maven Index（index.rs → index/） | P1 | ✅ | B-01 | [B-04-maven-index.md](./B-04-maven-index.md) |
| B-05 | 拆 Log Engine（logs/engine.rs → engine/） | P1 | ✅ | B-01 | [B-05-log-engine.md](./B-05-log-engine.md) |

### Phase 4 · 支撑模块（P1，4 个）

> 对应设计文档 §6 Phase 4（含 Operation Log，§4.6）。四个模块相互独立，可并行推进。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| B-06 | 拆 Runtime Config（config.rs → config/） | P1 | ✅ | B-01 | [B-06-runtime-config.md](./B-06-runtime-config.md) |
| B-07 | 拆 Watch（watch.rs → watch/） | P1 | ⬜ | B-01 | [B-07-watch.md](./B-07-watch.md) |
| B-08 | 拆 GitOps（git_ops.rs → git_ops/） | P1 | ⬜ | B-01 | [B-08-git-ops.md](./B-08-git-ops.md) |
| B-09 | 拆 Operation Log（operation_log.rs → operation_log/） | P1 | ⬜ | B-01 | [B-09-operation-log.md](./B-09-operation-log.md) |

### Phase 5 · 依赖边界收敛（P2，条件触发，1 个）

> 对应设计文档 §6 Phase 5：**只在触发条件出现时执行**，不为每个函数机械加 trait。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| B-10 | 按需引入 Port/Adapter（BuildExecutor / ProcessSupervisor / RuntimeRepository） | P2 | ⬜ | B-02, B-03 + 触发条件（设计文档 §6 Phase 5） | [B-10-port-adapter.md](./B-10-port-adapter.md) |

---

## 关键依赖链

```text
B-01 基线/测试外移 ──┬──► B-02 RuntimeService ──┐
                    ├──► B-03 ProcessManager ──┴──► B-10 Port/Adapter（条件触发）
                    ├──► B-04 Maven Index
                    ├──► B-05 Log Engine
                    ├──► B-06 Runtime Config ─┐
                    ├──► B-07 Watch           ├ 相互独立，可并行
                    ├──► B-08 GitOps          │
                    └──► B-09 Operation Log ──┘
```

- **B-01 是唯一硬前置**；B-02 ~ B-09 建议按 Phase 顺序推进，Phase 3/4 内部可并行。
- 每个任务内部按设计文档给定的迁移顺序**小步移动、每步验证**（§9 四件套：fmt / check / test / clippy）。
- 回滚粒度 = 一个职责组，不做整个后端大回滚（§9）。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录。
3. 新增/调整任务时，重新编号并同步依赖字段。
4. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因（回滚粒度见 §9）。
5. 全局横切约束统一记录在 `00-全局开发约束.md`；各任务文档的「架构/性能注意点」只写该任务特有内容，与全局约束叠加，不重复。
6. 设计文档 `docs/backend-module-split-plan.md` 是单一事实来源；任务 spec 与之冲突时，先改设计文档或在 spec 中显式说明原因与边界。
7. 任务完成时若移动了被文档引用的符号路径，**同步更新**根 `AGENTS.md`「参照实现」与相关任务文档（设计文档 §9 第 6 条）。
