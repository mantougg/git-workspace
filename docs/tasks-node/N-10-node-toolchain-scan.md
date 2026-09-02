# N-10 Node 工具链扫描登记（本机常见位置 + 版本管理器）

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md) §3（可执行检测硬规则）/ §5（纯函数）；根 `AGENTS.md` 平台规范 §1（路径归一化）/ §2（PATHEXT）；设计文档 §4.1。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 增强与展望 |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | N-08 |
| 对应设计文档 | §4.1（工具链检测）+ N-08 注册表 |

## 目标

给「Node 工具链」设置页补上「扫描本机」能力：枚举常见安装位置与 nvm / nvm-windows / fnm / volta / mise / asdf / n / scoop 等版本管理器目录中的 node 与包管理器可执行，用户勾选后走既有注册链路，免除手动逐个找路径。

## 需求范围

- [x] `node/scan.rs`：扫描根枚举（系统位置 + 版本管理器数据目录 + `PNPM_HOME` 等）与可执行定位；目录布局定位、去重为纯函数可单测
- [x] `node_scan_executables` IPC：返回候选列表（kind / packageManager / path / version / probeOk / source / registered），**只读不写注册表**；`registered` 标记已在注册表中的条目
- [x] 前端 `NodeToolchainView`：「扫描本机」按钮 + 候选弹窗（复选、已注册禁选），登记复用既有 `node_add_executable`（探测/校验逻辑单源）

## 架构 / 性能注意点

- **不静默入库**：注册表条目在决策链中优先于 PATH（N-08），自动登记会改变用户决策行为——必须勾选确认（对齐 §75 确认制精神），这是与 JDK `discover_jdks` 直接入库的**有意差异**。
- 扫描只读文件系统 + `-v` 探测，全程无网络（全局约束 §10）；单个候选探测失败降级「未知版本」，不阻断其他候选。
- 候选去重键 = 分隔符归一化（`\` → `/`）+ 小写化：去重是集合语义，按 AGENTS §1 的边界说明采用小写化以覆盖 Windows/macOS 大小写不敏感文件系统上的 junction/symlink 重复（如 nvm-windows 的 `C:\Program Files\nodejs` junction 与 `NVM_HOME` 版本目录、Homebrew 的 `bin` 符号链接与 `opt/node*/bin`）；Linux 上 node 路径仅大小写不同的场景实际不存在。
- Windows companion 检测：node 安装目录内的 npm/pnpm/yarn/bun shim 必须按 `.exe → .cmd → .bat → 裸名` 候选序查找（复用 `find_executable_in_dirs`）。
- 布局差异集中在定位纯函数：版本目录试 `<dir>` 与 `<dir>/bin`，fnm 再试 `<dir>/installation(/bin)`；volta 只扫 `~/.volta/bin` shim 目录、不下钻 `tools/image`（shim 即 PATH 实体，且避免同物重复）。

## 验收标准

- [x] 纯函数单测：版本目录/fnm installation 布局定位、companion shim 查找、归一化去重
- [x] 真实环境冒烟：扫描返回 ≥1 个有效 node（本机无 node 则 skip 并打印原因）
- [x] golden 更新：`NodeScanCandidate` 注册 + `GW_UPDATE_GOLDEN=1` 重新生成并核对 diff
- [x] 前端扫描 → 勾选 → 登记闭环；已注册候选标记且不可重复登记
- [x] 四件套（fmt / check / test / clippy）+ `pnpm build`；`detect_changes()` 影响面核对

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-09-02

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 开始开发：用户反馈工具链页缺扫描入口（N-08 有意做成纯手动注册的覆盖层，未纳入扫描）；确定「扫描出候选 → 勾选登记」方案（不静默入库），spec 落地 |
| 2026-09-02 | ✅ | 全部完成。`node/scan.rs`（scan_roots + collect_candidates + dedupe + scan_node_toolchain，6 纯函数 + 7 单测全绿：版本目录/plain+bin 布局、fnm installation、companion shim、name_prefix 过滤、去重折叠、归一化键）；`node_scan_executables` IPC（registered 标记与注册表归一化比对）；`NodeScanCandidate` IPC 样本注册 + golden 更新（+9 行）；前端 `NodeToolchainView`「扫描本机」按钮 + 候选勾选弹窗（已注册禁选）+ 批量登记（逐条走 `node_add_executable`）；影响分析 `run` 上游 LOW 风险。四件套：fmt/node 文件清零、clippy/node 文件零告警、node 模块 46 全绿（2 个 workspace 失败为 N-09 预存 Windows verbatim 问题，HEAD 同样失败已验证）、`pnpm build` 通过、golden 2/2 通过、`detect_changes()` 影响面符合预期 |

### 子任务清单

- [x] `node/scan.rs` 扫描 + 定位纯函数 + 单测
- [x] `node_scan_executables` IPC + lib.rs 注册 + golden
- [x] 前端 api / types / NodeToolchainView 扫描弹窗
- [x] 四件套 + pnpm build + detect_changes 收尾
