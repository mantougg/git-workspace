# N-01 Node 工具链检测

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§4.1](../node-frontend-runtime-design.md)；根 `AGENTS.md` 平台兼容规范 §2（可执行文件检测）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 工具链与发现 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | — |
| 对应设计文档 | §4.1 Node 工具链检测、§4.7 错误分类 |

## 目标

新增 `src-tauri/src/node/` 模块：检测 node 与包管理器（npm/pnpm/yarn）可执行文件与版本，实现包管理器决策链，为后续启动链路提供「解析出可执行绝对路径」的能力。

## 需求范围

- [x] `node/detect.rs`：`detect_node` / `detect_package_manager(name)`，一律走 `java/detect.rs::find_in_path`（`.exe → .cmd → .bat` → 裸名）
- [x] 版本探测：仿 `maven/detect_exec.rs::probe_version`（超时 + 输出上限，`node -v` / `<pm> -v`），失败降级为「未知版本」
- [x] **包管理器决策链**（纯函数，单测覆盖）：配置显式指定 → `package.json` 的 `packageManager` 字段 → lockfile 推断（`pnpm-lock.yaml`→pnpm、`package-lock.json`/`npm-shrinkwrap.json`→npm、`yarn.lock`→yarn）→ 回退 PATH `npm`
- [x] 决策链结果解析为可执行绝对路径；选中但不可执行 → `PackageManagerNotFound`（可行动错误）
- [x] 新错误码 `NodeNotFound` / `PackageManagerNotFound`（§79 显式扩展，见 00 约束 §4），带 Suggested Actions 穿透 IPC
- [x] `src-tauri/src/lib.rs` 注册 `node` 模块；MVP 不做注册表（自定义路径登记属 N-08）

## 架构 / 性能注意点

- **不引入新 crate**；决策链是纯函数（输入：配置值 + package.json 摘要 + lockfile 存在性快照；输出：枚举 + 原因），系统调用只留检测入口。
- Windows 上 npm/pnpm/yarn 实体是 `.cmd` shim：测试必须覆盖「命中 `.cmd` 候选」的顺序语义（候选排序为纯函数可单测，不依赖真实 PATH）。
- 依赖真实 node/npm 的测试探测不到就 skip 并打印原因，不硬失败。
- `bun.lockb` 只识别不执行（报可行动错误引导改选），bun 支持属 N-09。

## 验收标准

- [x] 决策链单测：四层优先级 + lockfile 冲突场景（多 lockfile 并存时按固定顺序 pnpm > yarn > npm > bun）+ `packageManager` 字段解析（`pnpm@9.1.0` 形式取名字段）
- [x] `.cmd` 候选命中顺序单测；Unix 裸名回退单测
- [x] 真实环境冒烟：检测到 node/npm 并返回版本（无环境时 skip 且打印原因）
- [x] `NodeNotFound` / `PackageManagerNotFound` 错误含 Suggested Actions（安装 / 改选 npm）
- [x] `cargo fmt --check` / `check` / `test` / `clippy -D warnings`（`--manifest-path src-tauri/Cargo.toml`）全绿（附注见时间线：本机基线问题与本次改动的关系）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-31

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 | 开始开发：读 spec / 设计文档 §4.1 §4.7 / 00 约束；确认改动面（新增 node/ 模块 + AppError 两个变体 + find_in_path 候选排序抽纯函数） |
| 2026-08-31 | ✅ | 完成。新增 `node/{mod,model,decision,detect}.rs`：决策链纯函数（四层优先级 + lockfile 固定顺序 pnpm > yarn > npm > bun + bun 只识别不执行引导改选）、`detect_node` / `detect_package_manager` / `resolve_package_manager`、`-v` 版本探测（超时 10s + 输出上限，复用 `maven::detect_exec::{wait_with_timeout, needs_cmd_c}`，失败降级未知版本）；`java::detect` 抽出纯函数 `executable_candidates(name, windows)` 与 `find_executable_in_dirs`（find_in_path 语义不变，调用方 java/maven 两侧回归通过）；`AppError` 新增 `NodeNotFound` / `PackageManagerNotFound`（details 携带 suggestedActions）。验证：`cargo check` ✅；`cargo test` node 模块 17/17 ✅（含本机 node/npm 真实冒烟，返回 22.14.0 / 10.x 版本）、error/java/maven 回归 59/59 ✅、全套 695 通过（2 个失败均为本机先行环境问题：`golden/ai_tools.json` CRLF 检出、benchmark 性能预算超 500ms，与本次改动无关；real_maven 3 个测试在本机 mise 全局钉 JDK8 时失败为先行环境问题，`MISE_JAVA_VERSION=temurin-17` + `JAVA_HOME=temurin-17` 下 3/3 ✅）；本次改动文件 `rustfmt --check` ✅、`clippy` 零新增警告（仓基线 85 个先行警告未触碰） |

### 子任务清单

- [x] `node/` 模块骨架与 `find_in_path` 复用
- [x] 版本探测（超时/上限/降级）
- [x] 包管理器决策链纯函数 + 单测
- [x] 新错误码与 Suggested Actions
- [x] 测试与四件套验证
