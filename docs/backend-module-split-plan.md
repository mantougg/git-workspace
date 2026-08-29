# GitWorkspace 后端模块拆分设计

> 状态：设计稿
>
> 日期：2026-08-29
>
> 范围：`src-tauri/src` Rust 后端
>
> 目标：在不改变 IPC 契约、任务语义、数据库结构和跨平台行为的前提下，逐步拆分超长文件，改善职责边界、可测试性和后续演进能力。

## 1. 决策摘要

当前后端应继续保持“模块化单体”形态，不建议为了文件长度拆成多个 crate，也不建议引入微服务。

本设计采用两步法：

1. **模块文件拆分**：把一个职责混杂的 `xxx.rs` 改成 `xxx/mod.rs` 加多个子模块，保留原有 Rust 公共路径和调用方式。
2. **依赖边界收敛**：逐步把 Runtime、Git、Task 对 SQLite、外部命令、操作系统进程和 Tauri 事件的直接依赖收敛到明确的应用服务或基础设施边界。

第一阶段不追求目录数量最大化，而是保证每个模块具有单一的主要变化原因：

- 一个模块负责一个领域能力或一个基础设施适配。
- `mod.rs` 只保留公共类型、共享状态、构造函数和必要的 `pub use`。
- 查询、写操作、后台 worker、平台适配和测试不混在同一个实现文件中。
- 现有公共类型路径尽量不变，例如 `runtime::RuntimeService`、`maven::sync_workspace_index`。

## 2. 当前架构判断

### 2.1 已有的正确边界

当前工程已经具备模块化单体的基础：

- [lib.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/lib.rs:144) 是组合根，负责组装数据库、Git、Task、Runtime、Watch 和 Git Link。
- [commands/mod.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/commands/mod.rs:1) 按 IPC 领域组织命令。
- [runtime/mod.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/runtime/mod.rs:3) 已经按 Build、Launch、Logs、Health、Watch、Config 等能力划分模块。
- [maven/mod.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/maven/mod.rs:16) 已经区分发现、解析、Effective Model、Resolver、Reactor、Executor 等能力。
- 任务文档也已经按 R-01 到 R-26 建立了领域依赖链，Runtime 的设计顺序是 Discovery → Index → Closure → Build → Process → Logs → IPC。

因此，问题不是缺少顶层模块，而是部分模块内部的实现文件承载了太多职责。

### 2.2 主要问题

#### RuntimeService 是应用层 God Object

[service.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/runtime/service.rs:236) 当前同时包含：

- Runtime 查询 DTO 和返回模型。
- Scheduler 配置加载与保存。
- Maven 项目、依赖图和 Closure 查询。
- 进程、健康检查和日志查询。
- Build、Start、Stop、Restart、Resolve 任务组装。
- 多服务 Environment 的拓扑编排。
- RuntimeTaskHandler 实现。
- 任务取消 watcher 和事件发送。

生产代码约 1,700 行，测试约 1,380 行。它是当前最应该拆分的文件。

#### RuntimeProcessManager 混合了生命周期和基础设施细节

[launch/manager.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/runtime/launch/manager.rs:213) 同时处理：

- Start 的准备、构建和 spawn。
- Stop、Restart、Force Kill 和孤儿进程接管。
- monitor 线程和退出分类。
- 生命周期状态迁移与数据库落库。
- sysinfo 指标采样。
- Build 日志接入和启动横幅/端口探测。

这些职责都围绕 Runtime 进程，但变化原因不同。特别是 monitor、取消和 Windows 进程树终止具有较高并发与平台风险，应单独成为可审查模块。

#### Maven index.rs 混合了领域模型、事务写入和查询

[maven/index.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/maven/index.rs:160) 同时包含：

- Workspace 索引同步。
- Maven project、dependency、module、artifact 的事务写入。
- Dependency Graph 查询。
- Source Mapping 刷新。
- Graph Cache。
- 路径 key、Windows verbatim path 和规范化。

它是 Maven 数据索引的一个完整子域，但不是一个适合长期维护的单文件。

#### 日志引擎混合实时流和历史文件查询

[logs/engine.rs](/home/mantou/projects/Rust/git-workspace/src-tauri/src/runtime/logs/engine.rs:213) 同时包含：

- LogSession 和内存环形缓冲。
- worker 线程、批量聚合、脱敏后的落盘。
- 文件滚动和容量上限。
- Search、Tail、Export、Clear。
- 应用日志只读查询。

实时日志和历史文件查询共享格式，但并不共享相同的生命周期，适合拆开。

#### 测试代码放大了文件规模

当前主要文件的生产/测试规模如下：

| 文件 | 总行数 | 测试开始位置 | 生产代码估算 | 结论 |
|---|---:|---:|---:|---|
| `runtime/service.rs` | 3,104 | 1,724 | 1,723 | 需要拆生产代码和测试 |
| `runtime/launch/manager.rs` | 2,888 | 1,417 | 1,416 | 需要拆生命周期实现和测试 |
| `runtime/build/pipeline.rs` | 2,034 | 713 | 712 | 先外移测试，生产代码暂可保持 |
| `maven/index.rs` | 1,358 | 1,012 | 1,011 | 需要拆同步、查询和映射 |
| `runtime/logs/engine.rs` | 1,187 | 756 | 755 | 需要拆 worker、查询和会话 |
| `core/operation_log.rs` | 1,181 | 756 | 755 | 需要拆记录、查询和 Undo |
| `runtime/watch.rs` | 1,075 | 728 | 727 | 需要拆监听和影响分析 |
| `core/git_ops.rs` | 1,004 | 659 | 658 | 需要拆 Git 操作和提交安全扫描 |
| `runtime/config.rs` | 999 | 850 | 849 | 作为第二批拆分 |
| `task/dag.rs` | 925 | 695 | 694 | 算法内聚性较高，暂不优先 |
| `models/ipc_golden_tests.rs` | 2,830 | 无 | 测试文件 | 按领域拆测试，不属于生产重构 |

## 3. 目标依赖方向

目标不是简单把文件移动到更多目录，而是形成以下依赖方向：

```text
Tauri Commands
       |
       v
Application Services / Use Cases
       |
       +--> Runtime Domain: closure, lifecycle, environment policy
       +--> Build Port ------> Maven CLI Adapter
       +--> Process Port ----> OS Process Adapter
       +--> Log Port --------> Workspace Log Storage
       +--> Repository Port -> SQLite DAO
       +--> Event Port ------> Tauri Event Adapter
       |
       v
Task Engine / Shared Infrastructure
```

当前代码可以继续直接使用现有实现。只有当某个边界出现多实现、难测试或明显耦合时，才引入 trait；不为每个函数机械增加接口。

### 3.1 分层职责

#### IPC Adapter

位置：`commands/`

职责：

- 解析 Tauri 参数。
- 获取 `AppState`。
- 调用应用服务。
- 将错误转换为现有 IPC 错误结构。
- 提交长任务，不能在 IPC 线程内直接执行长时间 Build。

禁止：

- 直接拼接 Maven 或 Java 命令。
- 直接操作进程状态机。
- 在 command 内编写业务编排。

#### Application Service

位置：`runtime/service/`、未来的 `core/application/`

职责：

- 组织一个完整用户用例。
- 管理任务、并发 permit、取消和事件。
- 调用领域能力和基础设施适配器。
- 返回适合 IPC 或 Task Engine 的结果。

#### Domain Logic

位置：`maven/closure.rs`、`runtime/launch/lifecycle.rs`、`runtime/environment.rs` 等。

职责：

- Closure 计算。
- 生命周期迁移规则。
- Environment 拓扑排序和失败传播。
- 路径、策略、退出分类等纯逻辑。

领域逻辑应尽量不直接依赖 `AppHandle`、`rusqlite::Connection` 或具体 OS 命令。

#### Infrastructure Adapter

位置：`db/`、`process/`、`runtime/build/runner.rs`、`runtime/launch/launcher.rs`、`runtime/events.rs`。

职责：

- SQLite 查询和事务。
- Maven、Java、Shell 的进程执行。
- Windows/macOS/Linux 差异。
- Tauri event 发布。

## 4. 推荐目标目录

### 4.1 Runtime Service

```text
runtime/
└── service/
    ├── mod.rs              # RuntimeService、共享依赖、构造函数、公共 re-export
    ├── dto.rs              # IPC 请求/返回 DTO 和 SchedulerConfig
    ├── queries.rs          # 项目、依赖图、Closure、进程、日志查询
    ├── operations.rs       # Build、Start、Stop、Restart、Resolve
    ├── environment.rs      # Start/Stop Environment 和服务就绪编排
    ├── task_handler.rs     # RuntimeTaskHandler::execute
    ├── cancellation.rs     # CancelWatch、选项映射
    └── tests.rs             # Service 级闭环测试
```

拆分映射：

| 当前区域 | 目标文件 | 说明 |
|---|---|---|
| `SchedulerConfig`、请求/返回 DTO | `dto.rs` | 保持 `runtime::SchedulerConfig` 等 re-export |
| `list_projects` 到日志/进程查询 | `queries.rs` | 只读用例，不修改状态 |
| `operation_task_request`、`exec_build`、`exec_start`、`exec_stop`、`exec_restart`、`exec_resolve` | `operations.rs` | 单 Runtime 操作 |
| `start_environment_requests`、`exec_start_environment`、`exec_stop_environment` | `environment.rs` | 多服务编排，不放入 RuntimeProcessManager |
| `impl RuntimeTaskHandler` | `task_handler.rs` | 只负责 TaskType 分发 |
| `CancelWatch`、`build_options_of`、`start_options_of` | `cancellation.rs` | 取消和 DTO 到领域选项的转换 |
| 当前 `#[cfg(test)] mod tests` | `tests.rs` | 保持同一父模块，继续覆盖私有辅助函数 |

`mod.rs` 只保留 `RuntimeService` 字段、`new/assemble`、共享构造逻辑和跨子模块需要的 `pub(super)` 辅助方法。

### 4.2 Runtime Process Manager

```text
runtime/launch/
├── manager/
│   ├── mod.rs              # RuntimeProcessManager、RuntimeProcessDeps、共享状态
│   ├── types.rs            # ActiveProcess、Progress、MonitorOutcome、Prepared
│   ├── start.rs            # start、start_inner、prepare、run_build、spawn 前流程
│   ├── control.rs          # stop、stop_runtime、kill、restart、reconcile
│   ├── monitor.rs          # monitor、finalize_exit、启动宽限、退出分类
│   ├── metrics.rs          # sampler 线程和 DB 指标刷新
│   ├── output.rs           # BuildLogSink、启动横幅和端口探测
│   └── tests.rs
├── launcher.rs
├── lifecycle.rs
├── port_preflight.rs
└── store.rs
```

关键边界：

- `start.rs` 负责“如何启动”。
- `control.rs` 负责“用户如何控制”。
- `monitor.rs` 负责“进程实际发生了什么”。
- `store.rs` 负责进程记录的 SQL，不在 `monitor.rs` 内拼接大量 SQL。
- `metrics.rs` 只读取 OS 指标并按节流策略写回。

状态迁移仍然由同一个 `RuntimeProcessManager` 统一执行，避免拆分后出现多个地方可以修改生命周期状态。

### 4.3 Build Pipeline

```text
runtime/build/
├── pipeline/
│   ├── mod.rs              # execute_build 公共入口
│   ├── orchestrator.rs     # JDK/Maven/Graph/Closure/Reactor/Build 主流程
│   ├── scripts.rs          # Pre/Post Build Script、安全确认和平台 Shell
│   ├── errors.rs           # Maven 退出码、超时和失败模块推断
│   └── tests.rs
├── classpath.rs
├── dep_cache.rs
├── pathing_jar.rs
├── runner.rs
├── scheduler.rs
└── strategy.rs
```

`pipeline.rs` 的生产代码目前只有约 700 行，第一轮只外移测试即可。后续发生以下变化时再拆生产代码：

- 增加 Gradle 或其他 Build Engine。
- Pre/Post Script 逻辑明显扩展。
- 失败诊断从简单尾部解析升级为独立诊断管道。

`resolve_classpath` 应优先移动到已有的 `classpath.rs` 或新的 `classpath/` 子模块，避免重复建立 Classpath 抽象。

### 4.4 Maven Index

```text
maven/
└── index/
    ├── mod.rs              # 公共入口和 re-export
    ├── types.rs            # MavenProjectNode、DependencyGraph、SourceMapping
    ├── cache.rs            # DependencyGraphCache
    ├── sync.rs             # sync_workspace_index 及事务写入
    ├── query.rs            # graph/project/dependency/module 查询
    ├── mapping.rs          # Source Mapping、Artifact 刷新和清理
    ├── path.rs             # path_key、verbatim path、lexical normalize
    └── tests.rs
```

`sync.rs` 是风险最高的部分之一。一次同步必须继续保证：

1. 项目、父子模块、依赖、artifact、source mapping 在同一事务语义下更新。
2. 同步失败不能留下半套索引。
3. `graph_cache` 和 `closure_cache` 在索引变化后失效。
4. Windows 路径比较统一经过正斜杠归一化和 verbatim prefix 清理。

### 4.5 Log Engine

```text
runtime/logs/
├── engine/
│   ├── mod.rs              # RuntimeLogEngine 公共门面
│   ├── session.rs          # LogSession、Ring、SessionMsg
│   ├── worker.rs           # 聚合、落盘、事件、滚动
│   ├── query.rs            # search、tail、export、clear
│   ├── storage.rs          # 日志目录、段文件、路径安全
│   └── tests.rs
├── level.rs
└── redact.rs
```

实时捕获路径必须保持轻量：捕获线程只做脱敏和发送消息；文件写入、分析器调用、事件聚合继续由 worker 执行。`query.rs` 必须保持流式读取，不把整个日志文件加载到内存。

### 4.6 Operation Log

```text
core/
└── operation_log/
    ├── mod.rs              # 公共类型和 re-export
    ├── model.rs            # OperationLog*、Undo* DTO
    ├── record.rs           # snapshot、record_operation
    ├── query.rs            # 分页和详情查询
    ├── undo_plan.rs        # Undo 计划和预览
    ├── undo_execute.rs     # Undo 执行和工作区状态保护
    └── tests.rs
```

Undo 计划和执行应继续分离。预览不能修改 Git；执行前必须重新检查当前 HEAD、分支和工作区状态，防止操作记录对应的状态已经被用户改变。

### 4.7 Watch

```text
runtime/
└── watch/
    ├── mod.rs              # RuntimeWatchEngine 和线程装配
    ├── debounce.rs         # notify 事件收集和去抖
    ├── classify.rs         # 路径分类和 ignore_path
    ├── impact.rs           # 受影响模块、下游传播和 Closure 限制
    ├── submit.rs           # RebuildRestart/Resolve 任务提交
    └── tests.rs
```

`impact.rs` 只负责纯的影响分析，不直接提交 Task；这样可以单独验证：

- 变更模块只扩散到允许的下游模块。
- 外部依赖不会被当成本地源码模块传播。
- Closure 之外的模块不会被错误加入重建集合。
- 同一批事件能够收敛为一个任务。

### 4.8 Runtime Config

```text
runtime/config/
├── mod.rs                  # 公共配置 API
├── model.rs                # RuntimeApplicationConfig、请求和摘要
├── repository.rs           # SQLite 元数据和配置生命周期
├── environment.rs          # Global/Workspace/Runtime/Service 合并
├── storage.rs              # JSON 读写、原子写和 schema 默认值
├── validation.rs           # 名称、路径、符号链接和敏感字段校验
└── tests.rs
```

配置的持久化边界必须保持现有设计：SQLite 保存元数据索引，`.gitworkspace/runtimes/*.json` 保存用户配置。拆分不能把完整秘密值重新暴露到 IPC。

### 4.9 Git Operations

```text
core/
└── git_ops/
    ├── mod.rs              # GitOps 门面和 TaskType 分发
    ├── remote.rs           # fetch、pull、push、clone
    ├── commit.rs           # normal commit、amend、index-only、identity
    ├── safety.rs           # secret、large-file、forbidden-file 扫描
    ├── shell.rs            # ShellCommand、超时和输出尾部
    └── tests.rs
```

远程 Git 继续使用系统 `git` CLI，Local commit/status/diff 继续使用 git2。拆分不能改变这个有意的适配策略。

## 5. Rust 模块迁移规则

### 5.1 文件改目录

Rust 不应同时保留以下两种同名模块：

```text
runtime/service.rs
runtime/service/mod.rs
```

迁移时应将前者移动为后者：

```text
runtime/service.rs  ->  runtime/service/mod.rs
```

外部路径仍然是：

```rust
crate::runtime::service::RuntimeService
```

因此，只要公共类型没有改名，通常不需要修改前端 IPC 或大范围调用方。

### 5.2 子模块访问权限

共享结构和跨子模块辅助函数放在 `mod.rs`，实现文件使用 `pub(super)`：

```rust
// service/mod.rs
pub struct RuntimeService {
    db: Arc<Mutex<Connection>>,
}

pub(super) fn workspace_root(&self, id: i64) -> AppResult<PathBuf> {
    // ...
}
```

原则：

- 对外 IPC 或其他领域需要的 API 使用 `pub`。
- 只供 `runtime::service` 子模块使用的 API 使用 `pub(super)`。
- 不为了方便拆分而把所有字段改成 `pub`。
- 领域内部实现不要通过 `pub(crate)` 扩大暴露范围。

### 5.3 公共入口和 re-export

保持以下兼容性：

- `runtime::RuntimeService`
- `runtime::RuntimeProcessManager`
- `runtime::RuntimeOperationRequest`
- `maven::sync_workspace_index`
- `maven::query_dependency_graph`
- `maven::DependencyGraphCache`
- `runtime::logs::RuntimeLogEngine`

内部文件可以变化，但 `runtime/mod.rs` 和 `maven/mod.rs` 继续作为稳定门面，避免调用方到处依赖内部路径。

### 5.4 测试拆分

第一轮采用同一父模块下的测试文件：

```rust
// service/mod.rs
#[cfg(test)]
mod tests;
```

对应：

```text
runtime/service/tests.rs
```

这样可以继续访问父模块私有类型，减少为测试修改生产可见性的需要。

测试进一步按性质分组：

- 纯函数测试：放在所属实现模块附近，例如生命周期迁移、路径归一化、影响分析。
- 服务闭环测试：放在 `service/tests.rs` 或 `manager/tests.rs`。
- 真 Maven/Java 测试：保留环境探测，缺失 Maven/JDK 时按项目规则 skip 并打印原因。
- IPC golden 测试：从 `models/ipc_golden_tests.rs` 按领域拆到 `models/ipc_golden/`。
- Benchmark：保留在 `benchmark/`，不要混入生产模块。

## 6. 分阶段实施计划

### Phase 0：基线和测试外移

目标：只调整文件组织，不改变生产逻辑。

任务：

1. 固定当前 Git 提交、测试结果和 `cargo fmt` 结果。
2. 将 `service.rs`、`manager.rs`、`pipeline.rs`、`index.rs`、`logs/engine.rs` 等测试移到同模块 `tests.rs`。
3. 将 `ipc_golden_tests.rs` 按 Runtime、Git、Task、Common 拆分。
4. 抽取重复测试 fixture，但不改变正式模块 API。

验收：

- `cargo test --manifest-path src-tauri/Cargo.toml` 通过。
- 测试数量不减少。
- 生产代码没有因为测试外移而增加 `pub` 字段或暴露秘密。
- Windows 相关测试仍保留编译期 `cfg` 分支。

### Phase 1：拆 RuntimeService

目标：降低 Runtime 应用层的单文件复杂度。

顺序：

1. 先创建 `runtime/service/mod.rs`，保持原结构和公共 re-export。
2. 移动 DTO 与 Scheduler 配置。
3. 移动查询方法。
4. 移动单服务操作。
5. 移动 Environment 编排。
6. 最后移动 `RuntimeTaskHandler` 和取消 watcher。

每一步只移动一个职责组，移动后立即编译和测试。

验收重点：

- `commands/runtime.rs` 不需要改变业务调用方式。
- Build、Start、Restart、Resolve 仍然通过 TaskManager 执行。
- CancelWatch 仍覆盖“取消早于 start 注册句柄”的竞态。
- 事件名称和 payload 不变。

### Phase 2：拆 RuntimeProcessManager

目标：把状态机、控制、监控和指标分离。

顺序：

1. 先移动纯类型和退出分类。
2. 移动 metrics sampler。
3. 移动 output sink 和启动探测。
4. 移动 monitor/finalize。
5. 移动 stop/kill/reconcile。
6. 最后移动 start/build 准备流程。

原因：先拆纯逻辑和低耦合代码，再处理同时涉及 DB、线程和进程的高风险启动流程。

验收重点：

- 生命周期仍然是 `Created → Preparing → Resolving → Building → Starting → Running → Stopping → Stopped/Failed`。
- reader 关闭后 monitor 仍能轮询取消和超时，不阻塞在 `child.wait()`。
- Windows 仍使用 `terminate_process` 和进程树终止策略。
- PID 与 start_time 校验逻辑没有移动到不安全的公共层。
- 指标采样不会为采样创建额外子进程。

### Phase 3：拆 Maven Index 和日志引擎

目标：隔离 SQLite 事务、Graph 查询、日志 worker 和历史查询。

顺序：

1. Maven index：先拆 `types`、`path`、`query`，再拆事务同步。
2. 日志 engine：先拆查询和 storage，再拆 worker/session。
3. 保持 `maven/mod.rs` 和 `runtime/logs/mod.rs` 的公共 re-export。

验收重点：

- Maven 索引同步保持原子性。
- Cache fingerprint 和失效时机不变。
- 路径比较经过统一归一化，特别是 Windows 混合分隔符和 `\\?\` 前缀。
- 日志搜索、导出和 tail 仍为流式读取。
- 日志在落盘前脱敏，应用日志仍然只读。

### Phase 4：拆 Config、Watch、GitOps、Operation Log

目标：降低后续功能迭代的交叉修改范围。

重点：

- Config：分离 JSON 文件存储、SQLite 元数据、环境合并和安全验证。
- Watch：分离 notify/debounce、路径分类、影响分析和任务提交。
- GitOps：分离远程 CLI、local commit、安全扫描和 Shell 执行。
- Operation Log（§4.6）：分离记录、查询、Undo 计划和 Undo 执行；Undo 预览不修改 Git，执行前必须重新校验 HEAD、分支和工作区状态。

### Phase 5：按需要引入 Port/Adapter

只有在出现以下需求时才引入 trait：

- 需要多个 Build Engine，例如 Maven 和 Gradle。
- 需要 Fake Process Supervisor 进行纯单元测试。
- 需要替换事件出口，例如 Tauri event、日志事件或测试 recorder。
- 需要降低业务模块直接持有 SQLite 连接的范围。

建议优先引入的接口：

```rust
trait BuildExecutor {
    fn execute(&self, request: BuildRequest) -> AppResult<BuildOutcome>;
}

trait ProcessSupervisor {
    fn start(&self, request: StartRequest) -> AppResult<RuntimeProcessInfo>;
    fn stop(&self, process_id: i64) -> AppResult<Option<RuntimeProcessInfo>>;
}

trait RuntimeRepository {
    // Runtime metadata and process rows only.
}
```

不建议一开始为每个 DAO 函数、每个路径工具和每个 serde DTO 都增加 trait。

## 7. 数据库边界和并发注意事项

当前多个模块共享 `Arc<Mutex<rusqlite::Connection>>`。文件拆分不会自动解决数据库耦合，因此必须明确以下原则：

- 生产代码可以暂时继续共享单连接，保持现有 WAL、busy timeout 和事务策略。
- 不要让长时间 Maven/Java 进程执行期间持有 DB 锁。
- 进程状态、任务状态和索引同步的写操作必须短事务完成。
- 查询服务不应直接修改缓存失效状态，缓存失效由同步用例统一负责。
- 后续如引入 repository，优先按领域拆：`RuntimeRepository`、`MavenIndexRepository`、`TaskRepository`，不要建立一个包揽所有表的 `DatabaseService`。

推荐的未来边界：

```text
runtime/application  -> RuntimeRepository
maven/index           -> MavenIndexRepository
task                  -> TaskRepository
commands              -> application service only
```

## 8. 跨平台约束

拆分时必须继续遵守仓库的 Windows/macOS/Linux 约束：

- 路径比较不能使用未经归一化的裸字符串相等比较。
- 路径拼接使用 `Path::join`。
- Windows 可执行文件检测走 `find_in_path`，候选顺序为 `.exe`、`.cmd`、`.bat`、裸名。
- `.cmd`/`.bat` 通过 `cmd /C` 执行，Unix 脚本通过 `sh -c` 执行。
- 子进程使用 Windows `CREATE_NO_WINDOW` 规则。
- 超长 Java classpath 继续使用 pathing jar 和长度阈值判断。
- 进程树终止统一经过 `process/kill_tree.rs`。
- 含临时目录的测试使用 `std::env::temp_dir()`，不硬编码 `/tmp` 或盘符。

拆分 `manager` 和 `pipeline` 时尤其不能把平台分支散落到多个业务模块；平台差异应留在 `process/`、`launcher.rs` 或专门的 adapter 中。

## 9. 验证和回滚策略

每个小步骤都应执行：

```text
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

环境相关测试如果缺少 Maven、JDK 或网络条件，应按现有规则 skip 并打印原因；产品逻辑错误不能通过 skip 掩盖。

重构每一批代码前应完成：

1. 对要移动的公共符号进行 impact 分析，确认直接调用方和执行流。
2. 按“接口/公共类型 → 实现 → 调用方 → 测试”的顺序迁移。
3. 使用源码搜索检查字符串形式的命令、事件名、配置字段和模块路径。
4. 每个阶段结束后检查 Git diff，只允许出现预期文件和符号变化。
5. 提交前执行 GitNexus `detect_changes()`，确认没有超出预期的执行流影响。
6. 符号搬家后，同步更新引用其路径的文档——根 `AGENTS.md` 平台规范的「参照实现」（如 `runtime/service.rs::find_project`、`launch/manager.rs::infer_main_class`、`build/pipeline.rs::find_root_project`、`maven/index.rs::path_key`）、`docs/tasks-ai/` 及其他引用这些路径的任务文档，避免文档指向已不存在的文件。

推荐回滚粒度为一个职责组，而不是整个后端大回滚。例如 `RuntimeService` 的 Environment 拆分出现问题时，只回退 `service/environment.rs` 这一组，不回退已经验证过的查询拆分。

## 10. 不建议的方案

### 10.1 仅按行数硬切

例如每 500 行切一个文件，会把生命周期状态、数据库写入和监控逻辑拆散，增加跨模块状态访问，实际降低可维护性。

### 10.2 立即拆成多个 crate

当前后端是单桌面进程，Runtime、Task、DB、Tauri State 共享较多。过早拆 crate 会制造大量 `pub` API 和类型转换，不能解决核心的职责耦合。

### 10.3 立即全面改成 Hexagonal Architecture

端口/适配器适合解决真实替换需求，不适合把所有简单函数都包成 trait。应先拆职责，再从最难测试或最可能多实现的边界引入接口。

### 10.4 将所有数据库逻辑集中到一个大 Repository

这会把当前文件级 God Object 变成数据库级 God Object。Repository 应按领域能力拆分，并保持事务边界清晰。

### 10.5 把测试全部改成 integration test

很多当前测试验证了私有状态机和失败收尾逻辑。全部迁移到 integration test 会迫使生产代码扩大可见性。优先使用同父模块下的 `tests.rs`，只有真正面向公共契约的测试才放到 `src-tauri/tests/`。

## 11. 完成标准

本设计全部落地后，后端应满足：

- `mod.rs` 不超过约 400 行，且主要是公共类型、装配和 re-export。
- 单个生产子模块通常控制在约 300 到 800 行；算法复杂模块可以例外。
- 测试不再占据核心生产实现文件的一半以上。
- RuntimeService 不直接承担所有 Runtime 子域实现。
- RuntimeProcessManager 的状态机、监控、控制和指标职责可分别测试。
- Maven Index 的查询、事务同步、映射和路径处理具有独立边界。
- IPC 调用路径、事件名、serde 字段、数据库 schema 和用户配置格式保持兼容。
- Windows、macOS、Linux 的进程、路径和命令行为保持不变。

最终判断标准不是文件数量，而是一次常见修改的影响范围：

```text
修改日志查询      -> logs/engine/query.rs + 对应测试
修改 Maven 索引    -> maven/index/sync.rs + 事务测试
修改 Stop 行为     -> launch/manager/control.rs + 生命周期测试
修改环境编排       -> service/environment.rs + Environment 测试
修改 IPC 字段      -> service/dto.rs + golden 测试
```

如果一个小功能仍然需要同时修改 RuntimeService、ProcessManager、Maven Index、commands 和多个无关测试，说明边界还没有真正收敛。
