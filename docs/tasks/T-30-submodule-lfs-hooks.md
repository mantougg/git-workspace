# T-30 Submodule / LFS / Hooks

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 6（P2） |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | T-02 |
| 对应 Roadmap | §30 Submodule、§31 Git LFS、§29 Git Hooks |

## 目标

补齐 Submodule、Git LFS、Git Hooks 三类能力，覆盖大型工程常见场景。

## 需求范围

- [x] Submodule：Init / Update / Sync / Status / Add / Remove，列表展示 synced/modified 状态
- [x] Git LFS：LFS Status / Fetch / Pull / Push / Locks
- [x] Git Hooks：pre-commit / prepare-commit-msg / commit-msg / post-commit / pre-push / post-checkout / post-merge 的 View / Edit / Run / Enable / Disable

## 架构 / 性能注意点

- Submodule 状态是递归/较重操作，按需计算并缓存，不进常驻 status 路径（健康检测 T-19 也如此）。
- LFS 走系统 `git lfs` CLI，与现有网络操作一致；Hook 编辑直接读写 `.git/hooks` 文件，注意权限与编码。

## 验收标准

- [x] Submodule 六类操作可用，状态展示正确
- [x] LFS status/fetch/pull/push/locks 可用
- [x] Hooks 查看/编辑/启停/运行可用，编辑后立即生效

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 开始开发。方案：`commands/repo_tools.rs`——Submodule 走 git CLI（status --recursive / init / update / sync / add / deinit+rm），状态前缀映射 synced/modified/uninitialized/conflict；LFS 走 `git lfs` CLI（ls-files/fetch/pull/push/locks/lock/unlock，缺失报可行动错误）；Hooks 读写 .git/hooks 文件（known 7 类，启停用 .disabled 重命名跨平台一致，运行 unix 直跑 / Windows 经 Git Bash）；git 子进程统一 CREATE_NO_WINDOW（F-27 教训）+ 超时；解析纯函数可单测。前端新增「仓库工具」视图（Submodules/LFS/Hooks 三 Tab） |
| 2026-09-02 | ✅ | 完成。后端 `commands/repo_tools.rs`：git 子进程封装（分离流读线程防管道阻塞 + 超时 kill + CREATE_NO_WINDOW）、`list_submodules`（status --recursive 前缀语义 + .gitmodules 元数据 URL 映射）、`submodule_op`（init/update/sync/add/remove，Dangerous 二次确认在前端）、LFS（ensure_lfs 可行动错误 + ls-files --long 解析剥离尺寸后缀 + fetch/pull/push + locks/lock/unlock）、Hooks（known 7 类、.disabled 重命名启停、unix 0o755 执行位、run_hook unix 直跑/Windows Git Bash，输出截尾 4000 字符）；前端 `api/repoTools.ts` + 「仓库工具」视图（/repo-tools，Git 分组，三 Tab：Submodules 状态标签与行内操作/LFS 文件与锁/Hooks 列表与编辑器）。边界说明：运行 hook 视为用户显式动作（hook 可修改仓库），Windows 运行依赖 Git Bash。验证：`cargo test --lib` 805 通过（repo_tools 5 项：状态/元数据/LFS/锁解析 + hooks 真实读写启停运行生命周期）；`pnpm build` 通过 |

### 子任务清单

- [x] Submodule 操作与状态
- [x] LFS 操作
- [x] Hooks 管理
