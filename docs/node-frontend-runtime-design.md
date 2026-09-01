# 前端工程启动支持设计（Node.js Runtime）

> 状态：设计草案（未开始实现）。
> 任务拆分：**N-01 ~ N-09**，见 [tasks-node/README.md](./tasks-node/README.md) 总索引；
> 开发流程见 `.agents/skills/gitworkspace-node-dev/SKILL.md`。
> 约束基线：[tasks-runtime/00-全局开发约束.md](./tasks-runtime/00-全局开发约束.md)
> + 根 `AGENTS.md` 平台兼容规范；本文只写本扩展特有内容。

| 项 | 值 |
|---|---|
| 目标 | Runtime Workspace 支持启动前端工程：发现 `package.json`、解析 `scripts`、以 `npm/pnpm/yarn run <script>` 启动并纳入统一进程管理 |
| 范围 | 本地开发服务器 / 任意 npm script 的启动、停止、日志、状态观测 |
| 非目标 | 依赖安装自动化（网络行为，仅显式触发）、Node 版本管理（不做 nvm/fnm 托管）、前端构建产物部署 |

## 1. 背景

Runtime Workspace 当前是 **Maven / Spring Boot 单技术栈**：项目 = `maven_projects` 表一行，
构建走 `MavenBuildEngine`，启动走 `LaunchPlan::{MavenGoal, JavaJar, JavaClasspath}`。
真实工作区普遍是混合技术栈（Java 后端 + 前端工程并存），`docs/feature-proposals-2026-08-26.md`
已提出「Node.js 应用支持」方向。本设计在不动既有闭环的前提下，把前端工程纳入同一套
Runtime 生命周期（发现 → 配置 → 启动 → 日志 → 停止 → 观测）。

## 2. 现状盘点

### 2.1 零改动可复用（技术栈无关）

| 能力 | 落点 |
|---|---|
| 进程生命周期状态机（Created→…→Running）、启动/停止/重启/强杀 | `runtime/launch/manager/`（start.rs / control.rs / monitor.rs） |
| 流式输出监控（字节读 + `from_utf8_lossy`，GBK 安全） | `process/streaming.rs` |
| 进程树终止 | `process/kill_tree.rs` |
| 日志引擎（聚合 / 脱敏 / 落盘 / 导出，R-11） | `runtime/launch/manager/output.rs` 等 |
| 事件桥（`runtime_process_output` 等 Tauri 事件） | `runtime/events.rs` |
| 健康探针（HTTP 探测，R-16）、端口占用检测 | `runtime/health/`、`process/port.rs` |
| 任务队列 / 取消 / 进度（T-05）、Task Engine 接入 | `runtime/service/task_handler.rs` |
| Pre/Post Script 确认制（R-14 §75） | `runtime/build/script_approval.rs` |
| `runtime_processes` 表（pid / status / command_preview / ports_json） | `db/schema.rs` SCHEMA_V12 |
| PATH 可执行检测（`.exe→.cmd→.bat→裸名`） | `java/detect.rs::find_in_path`（`pub(crate)`） |
| `.cmd/.bat` 需 `cmd /C` 判定、带超时版本探测 | `maven/detect_exec.rs::needs_cmd_c` / `probe_version` |

### 2.2 缺口（必须新增/解耦）

| 缺口 | 现状 | 位置 |
|---|---|---|
| 无项目类型概念 | 体系内无 `ProjectType`/language 枚举，项目 ≡ Maven 项目 | 全局 |
| 无 `package.json` 发现/解析 | 发现入口只扫 `pom.xml` | `maven/discovery.rs`、`runtime/spring_boot.rs` |
| 配置模型无 Node 字段 | `RuntimeApplicationConfig` 全是 JVM 语义字段 | `runtime/config/model.rs:20` |
| 引擎分发只认 maven/mvnd | `engine_for` 对未知 id 报 `RuntimeConfig` | `runtime/build/mod.rs:264` |
| LaunchPlan 无脚本变体 | 三个变体全部 Java 语义 | `runtime/build/mod.rs:133`、`launch/launcher.rs:27` |
| 启动成功/端口检测 Spring 硬编码 | banner 正则 `Started \S+ in [\d.]+ seconds`、端口正则 `started on port...` | `launch/manager/monitor.rs`、`manager/output.rs:55` |
| 项目列表 IPC 强耦合 Maven | `runtime_list_projects` 返回 `MavenProjectNode` | `commands/runtime.rs` |
| 向导项目选择器只认 Maven | `form.project` 为 Maven 选择器 | `src/views/RuntimeAppWizard.vue:51` |

## 3. 总体设计

```text
┌─ 发现层 ─────────────────────────────────────────────┐
│ node/discovery.rs  扫 package.json（跳过 node_modules）│
│ node/mod.rs        解析 scripts / packageManager       │
│ db SCHEMA_V17      node_projects 表（元数据索引）       │
└──────────────┬───────────────────────────────────────┘
               ▼
┌─ 配置层 ─────────────────────────────────────────────┐
│ RuntimeApplicationConfig + kind / script /           │
│ package_manager（向后兼容，缺省 = springBoot）          │
│ runtime_projects 表 + kind 列（同 V17 迁移）           │
└──────────────┬───────────────────────────────────────┘
               ▼
┌─ 构建/计划层 ─────────────────────────────────────────┐
│ engine_for("node") → NodeBuildEngine（默认无构建步骤）  │
│ LaunchPlan::Script { executable, args, env, wd }      │
└──────────────┬───────────────────────────────────────┘
               ▼
┌─ 启动/观测层（复用现有 Process Manager）───────────────┐
│ launcher.rs + Script 分支（Windows 走 cmd /C）         │
│ monitor：检测器按 kind 策略化（banner/端口正则集）       │
│ stop / kill / logs / events / health 全部复用          │
└──────────────────────────────────────────────────────┘
```

核心原则：**Node 路径只做「检测可执行 + 拼命令 + 复用进程管理」，不实现任何
npm script 语义解析**（对齐全局约束 §1「不重新实现构建工具」：script 内容交给
包管理器自身执行，GitWorkspace 不解读、不改写）。

## 4. 详细设计

### 4.1 Node 工具链检测（新模块 `src-tauri/src/node/`）

仿 `java/` + `maven/detect_exec.rs` 结构：

- `node/detect.rs`：`detect_node` / `detect_package_manager`，一律走
  `java/detect.rs::find_in_path`（需提升可见性为 `pub(crate)` 已满足，同 crate 内直接复用）。
  **Windows 上 `npm`/`pnpm`/`yarn` 实体是 `.cmd` shim，必须先命中扩展名候选，
  否则选中不可执行的 Unix shim（CreateProcess os error 193，R-14 同款坑）。**
- `node/mod.rs`：版本探测仿 `probe_version`（超时 + 输出上限，`node -v` / `npm -v`）。
- **包管理器决策链**（优先级从高到低）：
  1. Runtime 配置显式指定的 `packageManager`（用户覆盖）；
  2. `package.json` 的 `packageManager` 字段（Corepack 标准，如 `pnpm@9.1.0`）；
  3. lockfile 推断：`pnpm-lock.yaml` → pnpm、`package-lock.json`/`npm-shrinkwrap.json` → npm、
     `yarn.lock` → yarn、`bun.lockb` → bun（MVP 不支持 bun，见 §7）；
  4. 回退 PATH 上的 `npm`。
- 检测不到 → 可行动错误（§4.7），**探测不到就报错的只限产品逻辑；测试一律
  skip 并打印原因**（AGENTS.md §4）。
- 注册表（可选，后置）：仿 `jdks` / `maven_executables` 表 + `commands/jdk.rs` 同款 IPC，
  支持用户登记自定义 node/npm 路径。MVP 仅 PATH 检测 + 配置覆盖。

### 4.2 package.json 发现与索引

- `node/discovery.rs::discover_package_jsons`：复用 T-01 Scanner 的 workspace 清单语义
  （R-27 已将发现语义改为「workspace 边界、与 git 解耦」，直接沿用同一入口补扫）；
  **跳过目录**：`node_modules` / `dist` / `build` / `.git` / 各类 dotdir。
- 解析（`serde_json` 已有依赖，无新增 crate）：提取 `name` / `version` /
  `scripts`（有序 map）/ `packageManager` / lockfile 标记。**不解析
  `dependencies`/`devDependencies`**（本期不做前端依赖图）。
- 落库 `SCHEMA_V17`（当前最新 V16，顺延一号）：

```sql
CREATE TABLE IF NOT EXISTS node_projects (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id     INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repository_id    INTEGER REFERENCES repositories(id) ON DELETE SET NULL,
    path             TEXT NOT NULL,              -- package.json 所在目录，path_key 归一化
    name             TEXT NOT NULL,
    version          TEXT NOT NULL DEFAULT '',
    package_manager  TEXT,                       -- 决策链推断结果（展示用；运行时再判定）
    scripts_json     TEXT NOT NULL,              -- {"dev":"vite",...} 原样存储
    pkg_hash         TEXT NOT NULL,              -- 内容 hash，未变不解析（对齐 POM Cache 语义）
    last_scanned_at  TEXT NOT NULL,
    UNIQUE(workspace_id, path)
);
```

- 缓存语义对齐全局约束 §5：`pkg_hash` 未变不重新解析（等价 POM Cache）。
- 同库共生约束：`runtime_projects.root_project_id` 当前引用 `maven_projects(id)`。
  Node 配置不引用该列——新增 `kind` 列 + `project` 文本列存 node 项目 path
  （`project` 列 V11 已存在，语义复用：`kind=maven` 时存 Maven path，`kind=node`
  时存 package.json 目录 path），**不新建外键纠缠**。

### 4.3 Runtime 配置模型扩展（`runtime/config/model.rs`）

`RuntimeApplicationConfig` 新增字段（全部 `#[serde(default)]`，向后兼容，
对齐全局约束 §8「缺省字段有默认值」）：

```rust
/// 运行时类型：缺省 springBoot，历史配置零迁移。
#[serde(default = "default_kind_spring_boot")]
pub kind: RuntimeKind,             // "springBoot" | "node"
/// kind=node：要执行的 scripts 名（如 "dev"）。
#[serde(default)]
pub node_script: Option<String>,
/// kind=node：包管理器覆盖（None = 走 §4.1 决策链）。
#[serde(default)]
pub node_package_manager: Option<String>,
```

- `CURRENT_SCHEMA_VERSION` 升版；旧 JSON 加载后 `kind` 缺省为 `springBoot`，
  语义完全不变。
- `runtime_projects` 表加 `kind TEXT NOT NULL DEFAULT 'springBoot'`（`SCHEMA_V18`，
  N-03；与 N-02 的 V17 顺序推进不并号）。
- 校验：保存时 `kind=node` 必须有 `node_script` 且该 script 存在于目标
  package.json 的 `scripts`（否则 `ScriptNotFound`，§4.7）；`kind=springBoot`
  时 node 字段必须为 `None`（防配置漂移）。
- 复用既有字段：`environment` / `runtime_environment`（注入 PORT 等）、
  `program_arguments`（追加到 `run <script> --` 之后）、`health_check`（R-16
  配置结构天然通用，前端工程填 `/` 或任意路径即可）。

### 4.4 LaunchPlan 扩展与 launcher 分支

`runtime/build/mod.rs:133` 新增变体（内部类型，不进 IPC golden，无快照影响）：

```rust
/// 以包管理器执行 npm script（`npm run dev` / `pnpm dev`）。
Script {
    executable: PathBuf,           // 决策链解析出的 npm(.cmd)/pnpm/yarn 绝对路径
    args: Vec<String>,             // ["run", "dev", "--", ...program_arguments]
    env: Vec<(String, String)>,
    working_dir: PathBuf,          -- package.json 所在目录
    preview: String,               // §75 可预览/可追溯
},
```

`launch/launcher.rs:27` 加分支：`needs_cmd_c(&executable)` 为真时包
`cmd /C`（对齐 `maven/executor.rs::build_process` 的 `via_cmd_c` 既有模式），
注入托管标记 env（`GITWORKSPACE_PROCESS_ID` 等，launcher.rs:79 现有逻辑不变）。
`plan_preview` / `plan_working_dir`（launcher.rs:86/94）两个 match 同步补 arm。

### 4.5 BuildEngine 接入（`build/mod.rs:264 engine_for`）

- `engine_for` 加 `"node" => Ok(Box::new(node_engine::NodeBuildEngine))`；
  错误文案同步更新（现有单测 `engine_for_rejects_unknown_ids_actionably`
  需补断言）。
- `NodeBuildEngine.build` **默认是直通**：前端 dev server 不需要构建步骤，
  校验（node/pm 检测、script 存在性）后直接产出 `LaunchPlan::Script`。
  不触碰 Maven 特有段（依赖图 / Closure / Reactor / Classpath）——
  `execute_build`（build/pipeline/mod.rs:75）九步按 engine 分叉，Node 引擎
  只走「加载配置 → 校验 → env 合并 → Pre/Post Script（复用确认制）→ 产出 plan」。
- **不做自动 `npm install`**：install 是网络行为，违反全局约束 §10「本地能力
  不依赖网络」的默认假设。提供显式 IPC `node_install`（带任务进度 + 首次确认），
  由用户主动触发；启动前检测 `node_modules` 缺失时给出可行动提示而非代劳。

### 4.6 启动成功与端口检测策略化

现状硬编码（Spring）：banner `Started \S+ in [\d.]+ seconds` 提前翻 Running；
端口 `started on port(?:\(s\))?:?\s+(\d+)`（`manager/output.rs:55`）。

改为按 `kind` 选择检测器集（monitor.rs 注入，检测器本身是纯函数可单测）：

| kind | Running 判定 | 端口探测 |
|---|---|---|
| springBoot | 现有 banner 正则 → 宽限期回退（不变） | 现有正则（不变） |
| node | **不设 banner**；宽限期（`start_grace`）到且进程存活即 Running | 通用 URL 正则 `https?://(?:localhost\|127\.0\.0\.1\|0\.0\.0\.0\|\[::1\]):(\d+)` 覆盖 Vite `Local: http://localhost:5173/`、webpack `Project is running at`、Next/Nuxt `ready on` 等主流输出 |

- 端口预检（`launch/port_preflight.rs`）：Node 侧从三处解析显式端口——
  配置 `program_arguments` 中的 `--port/-p`、`environment` 的 `PORT`、
  以及未来 wizard 的显式端口字段；均缺省时跳过预检（dev server 自选端口是常态）。
- 前端工程的「编译失败但不退进程」（vite/webpack 常驻报错）语义：进程活着即
  Running，错误靠日志呈现——与现状「宽限期回退」语义一致，不引入新状态。

### 4.7 错误分类扩展

全局约束 §9 固定了 §79 错误集合；本设计**显式扩展**（已在引入它们的
N-01 / N-03 spec 与 `tasks-node/00-全局开发约束.md` §4 中声明为对 §79 的增补）：

| 新错误码 | 场景 | Suggested Actions |
|---|---|---|
| `NodeNotFound` | PATH/配置均无 node | 安装 Node 或登记自定义路径 |
| `PackageManagerNotFound` | 决策链选中的 pm 不可执行（如 `pnpm-lock.yaml` 存在但没装 pnpm） | 安装该 pm 或在配置中改选 npm |
| `ScriptNotFound` | 配置的 script 不在 `scripts` 中 | 列出可用 scripts 供改选 |

复用现有：`PortOccupied`（R-16）、`ProcessStartFailed` / `ProcessCrashed`（R-10）。

### 4.8 IPC 与契约

- 新增：`node_list_projects(workspace_id)` → `NodeProjectNode[]`（path/name/
  version/packageManager/scripts）；`node_detect_toolchain()`；`node_install`（后置）。
- 复用不变：`runtime_start/stop/restart/kill`、`runtime_list_configs` 等全套
  （`RuntimeConfigSummary` 加 `kind` 字段，serde 缺省兼容）。
- MVP **不做**统一项目视图抽象（把 Maven/Node 项目合成一个列表）——保持
  两个并列 IPC；wizard 内部按 kind 分源取数。N-09 触发后经用户决策**已做**：
  新增 `runtime_list_unified_projects`（扁平结构 + node/maven 专属 payload，
  golden/TS 同步登记），wizard node 分支已切换为统一列表取数。
- `src/types/runtime.ts` 同步 + `models/ipc_golden` 快照重新生成
  （`GW_UPDATE_GOLDEN=1`，全局约束 §7 单一事实来源）。

### 4.9 前端 UI

- `RuntimeAppWizard.vue`：第一步加「运行时类型」选择（Spring Boot / 前端工程）；
  选 node 后项目选择器改取 `node_list_projects`，追加 script 下拉（来自所选
  项目的 `scripts`）与包管理器选择（默认「自动推断」）。jdk/profile/closure
  等 JVM 字段按 kind 隐藏。
- `RuntimeDashboard.vue`：「启动方式」列（closure 语义，F-23）对 `kind=node`
  降级显示为 script 名（如 `npm run dev`）；其余列（状态/端口/日志/操作）天然通用。
- `src/api/node.ts` 新封装；`stores/runtime.ts` 事件订阅零改动（事件流与类型无关）。

## 5. 平台兼容要点（对齐 AGENTS.md）

- 可执行检测一律 `find_in_path`，禁止 `Command::new("npm")` 裸名兜底；
  Windows 命中 `.cmd` 后执行必须 `cmd /C`（`needs_cmd_c`）。
- 路径比较两侧 `replace('\\', "/")` 归一化（`path_key` 语义）；新增
  `node_projects.path` 与同库路径匹配点遵守同一规则。
- 子进程 spawn 走 `process/streaming.rs`（已含 `CREATE_NO_WINDOW`）；
  输出监控沿用字节读 + `from_utf8_lossy`（中文 Windows 下 npm 输出同样可能非 UTF-8）。
- 命令行长度：npm script 命令短，不触及 32767 上限；无需 pathing jar 类设施。
- 端口占用检测复用 `process/port.rs`。进程树终止（N-07 修订）：unix 上 launch
  子进程经 `process_group(0)` 独立成组，优雅停止/升级强杀先走 `killpg`
  （`kill_tree.rs::signal_process_group`）——SIGTERM 会先杀死 npm 这类
  「不等待子孙退出」的中间进程，vite 等孙子进程被 reparent 到 init 后
  parent 链枚举再也找不到，只有组信号能保证 Stop 后端口真实释放；
  parent 链遍历保留作非组长 root（adopted 进程）的回退。Windows 无进程组
  语义：root `cmd /C` 在 vite 存活期间整链存活，维持 kill_tree parent 链路径。

## 6. 安全与边界

- **命令可预览/可追溯**（§75）：`preview` 字段入 `runtime_processes.command_preview`，
  与 Java 路径同规。
- **Pre/Post Build Script 确认制原样复用**（`script_approval.rs`），Node 引擎无例外。
- **绝不修改用户文件**：不写 `package.json`、不自动生成 lockfile、不主动 install。
- **Secret 脱敏**：`environment` 中 `TOKEN`/`API_KEY` 等 key 走 T-08 既有脱敏，
  日志/预览/UI 三处一致（前端工程常带 API key，重点回归）。
- **Git 联动安全**（§11）：运行中的 node 进程与 Java 进程同受「Stop & Switch」保护，
  `runtime_processes` 表语义不变。

## 7. 分阶段实施

| 阶段 | 内容 | 说明 |
|---|---|---|
| **MVP（N-01 ~ N-07）** | §4.1 PATH 检测 + npm；§4.2 发现/解析/落库；§4.3 配置扩展；§4.4/§4.5 LaunchPlan + 引擎直通；§4.6 通用 URL 端口探测；§4.7 错误；§4.8 两个 IPC；§4.9 wizard/dashboard；端到端验收 | 只保 `npm run <script>`，pnpm/yarn 仅决策链识别、不可用时报可行动错误 |
| P2 增强（N-08） | pnpm/yarn 执行链；显式 `node_install` 动作；node 注册表（自定义路径）；显式端口预检 | 按真实用户环境排期 |
| P3 增强（N-09，2026-09-02 触发并完成） | monorepo（workspaces 解析 + 安装路由到根 + workspaceRoot 展示）；bun 可执行（run/install/注册表同权）；R-19 node 模板；统一项目视图 IPC；node 纳入 R-15 分组启停（端口覆盖按 kind 分流） | watch 联动经用户确认不做（dev server 自带 HMR） |

## 8. 验收标准（MVP）

- [x] 含 `dev` script 的样例前端工程（Vite）：发现 → 建配置 → 启动 → 端口正确识别 → 停止，全闭环可用（N-07 自动化集成测试 `real_vite_project_full_loop_with_port_release`：真实 `npm create vite` 产物 + 真实启动/停止 + 端口释放断言；Linux 实测通过，Windows/macOS 真机复核待补）。
- [x] Windows 上 npm 经 `.cmd` + `cmd /C` 执行，无 os error 193；日志中文不丢行（纯函数测试覆盖扩展名候选序与 `needs_cmd_c`：`node::detect::tests::candidate_order_windows_prefers_extensions` 等；真机复核待补）。
- [x] 旧 `springBoot` 配置零迁移加载、语义不变；新配置缺省 kind 兼容（真实 Spring Boot 闭环测试通过 + golden/schema 测试）。
- [x] `node_modules` 缺失 / script 不存在 / pm 缺失三类错误均为可行动提示（`node_engine_reports_missing_dependencies_without_installing`、`ScriptNotFound` 校验测试、`bun_decision_resolves_like_other_managers`；N-09 起 bun 可执行，pm 缺失错误仍可行动）。
- [x] 启动命令 preview 落库可查；敏感 env 在日志与 UI 脱敏（E2E 断言 `command_preview` 落库；脱敏三处共用 `is_sensitive_environment_key`，`core::secret` 8 测试 + `logs::redact` 4 测试 + IPC 不回传敏感值）。
- [x] ipc_golden 快照更新；schema V17 迁移在旧库上幂等通过（`golden_samples_match_snapshot`、`migrate_creates_full_schema_and_bumps_version`）。
- [x] 性能：package.json 发现 < 500ms（N-02 100 包 fixture 断言实测通过）。

## 9. 风险与开放问题

- **检测器误报**：通用 URL 正则可能命中应用自己打印的无关 URL。缓解：取首个
  localhost URL 且仅在宽限期内采纳；上线前用 Vite/webpack/Next 三样例回归。
- **交互式 script**：少数 script 启动后等待 stdin（如某些脚手架）。当前进程模型
  不支持 stdin 注入——文档化为限制，遇到时建议用户改用非交互 script。
- **`npm run` 的信号传播**：Windows 上 `cmd /C npm run dev` 的孙子进程（vite）
  需靠 `kill_tree` 整树终止——R-10 已有此语义，回归测试必须覆盖「Stop 后
  端口真实释放」。**N-07 实锤并修复**：unix 上 SIGTERM 先杀死 npm（npm 不
  等待 vite 退出就终止），vite 被 reparent 到 init 且继续持有输出管道——
  parent 链 kill_tree 对「父死孙活」失效，Stop 后端口不释放。修复：launch
  子进程 `process_group(0)` 独立成组 + `killpg` 组信号（§5 已更新）；
  E2E `real_vite_project_full_loop_with_port_release` 覆盖该回归。
- **开放问题**：是否在 `runtime_list_projects` 之外再做统一项目视图（§4.8 暂不做）；
  前端工程是否纳入 R-15 Runtime Environment 的分组启停（倾向纳入，实现成本低，
  连同 monorepo/bun 等一并留作 N-09 触发条件，见 N-09 spec）。
