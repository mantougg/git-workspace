//! File Watch / 增量构建 / 自动重启（R-17，§42/§43/§44/§71/§72）。
//!
//! 事件链路（§71）：OS File Event → Debounce → Path Classification →
//! Affected Project → Affected Dependency Closure → Incremental Build。
//!
//! 边界与预算（任务文档「架构/性能注意点」）：
//! - **只监听参与运行中应用 Closure 的模块目录**（watcher 句柄预算），
//!   不监听整个 Workspace；不触发全量扫描/全量构建；
//! - 复用 T-06 的 notify 设施与增量 mount/unmount 约定，不新建全局 watcher
//!   （全局约束 §7）；T-06 管仓库状态刷新，本引擎管 Runtime 源码监听，
//!   两者互不接管；
//! - 自动重启默认关（每应用 `autoRestart` 开关）；连续变化合并（restart
//!   防抖：构建中收到新变化只入队，不打断进行中的构建导致半产物）；
//! - `pom.xml` 变化 → 触发依赖模型失效重算（提交 ResolveDependencies），
//!   **而非直接构建**（联动 R-02/R-03 缓存失效路径）。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;

use crate::models::task::{RuntimeOp, RuntimeTaskOptions, TaskRequest, TaskType};
use crate::maven::index::DependencyGraphCache;
use crate::maven::closure::RuntimeClosureCache;
use crate::runtime::config;
use crate::runtime::events::{
    FileChangedPayload, RuntimeEmission, RuntimeEventEmitter, EVENT_FILE_CHANGED,
};

/// 静默窗口：最后一次事件后等待该时长再判定（File Change → Detection
/// < 300ms 预算的主要组成，§99）。
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);
/// 应用同步周期：对账运行中应用与监听集（挂载/卸载、自动重启归队）。
pub const SYNC_INTERVAL: Duration = Duration::from_secs(5);
/// 重启任务完成后到下次重启的最小间隔（防抖兜底）。
const RESTART_RESUBMIT_COOLDOWN: Duration = Duration::from_millis(500);

/// 自动重启任务提交端（解耦 TaskManager，测试注入假提交器）。
pub trait WatchTaskSubmitter: Send + Sync {
    fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String>;
}

impl WatchTaskSubmitter for crate::task::manager::TaskManager {
    fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String> {
        let mut ids = self.submit(&[request])?;
        ids.pop().ok_or_else(|| {
            crate::error::AppError::Task("任务提交失败：未返回任务 id".into())
        })
    }
}

/// 监听的应用键（workspace_id + runtime_name）。
type AppKey = (i64, String);

/// 单个受监听应用的状态。
#[derive(Default)]
struct WatchedApp {
    /// 防抖等待期间积累的变更（模块目录 → 变更文件相对路径）。
    pending_changes: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    /// 待重建的模块 GA 集合（合并语义）。
    pending_modules: BTreeSet<String>,
    /// RebuildRestart 任务在途（进程不在 Running）。
    in_flight: bool,
    /// 上次提交时刻（归队冷却）。
    last_submitted: Option<std::time::Instant>,
}

/// Runtime File Watch 引擎。
pub struct RuntimeWatchEngine {
    db: Arc<Mutex<Connection>>,
    graph_cache: Arc<DependencyGraphCache>,
    closure_cache: Arc<RuntimeClosureCache>,
    emitter: Arc<dyn RuntimeEventEmitter>,
    processes: Arc<crate::runtime::launch::RuntimeProcessManager>,
    task_manager: Mutex<Option<Arc<dyn WatchTaskSubmitter>>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// 当前挂载的模块目录。
    watched: Mutex<HashSet<PathBuf>>,
    /// 受监听应用（autoRestart 开启且进程活跃）。
    apps: Mutex<HashMap<AppKey, WatchedApp>>,
    /// 原始 FS 事件通道（watcher 回调 → 防抖 worker）。
    event_tx: std::sync::mpsc::Sender<Vec<PathBuf>>,
    stop: Arc<AtomicBool>,
}

impl RuntimeWatchEngine {
    /// 装配引擎并启动防抖 worker 与同步循环线程。
    pub fn spawn(
        db: Arc<Mutex<Connection>>,
        graph_cache: Arc<DependencyGraphCache>,
        closure_cache: Arc<RuntimeClosureCache>,
        emitter: Arc<dyn RuntimeEventEmitter>,
        processes: Arc<crate::runtime::launch::RuntimeProcessManager>,
    ) -> Arc<Self> {
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
        let engine = Arc::new(Self {
            db,
            graph_cache,
            closure_cache,
            emitter,
            processes,
            task_manager: Mutex::new(None),
            watcher: Mutex::new(None),
            watched: Mutex::new(HashSet::new()),
            apps: Mutex::new(HashMap::new()),
            event_tx,
            stop: Arc::new(AtomicBool::new(false)),
        });

        // watcher 回调：只把路径送进通道（OS 回调线程内不做任何重活）。
        let tx = engine.event_tx.clone();
        let watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let paths: Vec<PathBuf> = event
                        .paths
                        .into_iter()
                        .filter(|p| p.is_file() || p.extension().is_some())
                        .collect();
                    if !paths.is_empty() {
                        let _ = tx.send(paths);
                    }
                }
            },
            Config::default(),
        );
        if let Ok(watcher) = watcher {
            *engine.watcher.lock().unwrap() = Some(watcher);
        } else {
            log::warn!("R-17: failed to boot notify watcher; file watch disabled");
        }

        // 防抖 worker。
        {
            let engine = Arc::clone(&engine);
            std::thread::Builder::new()
                .name("runtime-watch-debounce".into())
                .spawn(move || engine.debounce_loop(event_rx))
                .ok();
        }
        // 同步循环。
        {
            let engine = Arc::clone(&engine);
            std::thread::Builder::new()
                .name("runtime-watch-sync".into())
                .spawn(move || loop {
                    if engine.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    engine.sync_running_apps();
                    std::thread::sleep(SYNC_INTERVAL);
                })
                .ok();
        }
        engine
    }

    /// 注入任务提交端（lib.rs 在 TaskManager 装配后调用，解构环依赖）。
    pub fn attach_task_manager(&self, task_manager: Arc<dyn WatchTaskSubmitter>) {
        *self.task_manager.lock().unwrap() = Some(task_manager);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    // ------------------------------------------------------------------
    // 同步：运行中应用 ↔ 监听集
    // ------------------------------------------------------------------

    /// 对账一次（公开供测试直接调用）：为 autoRestart 开启且进程活跃的
    /// 应用挂载闭包模块目录；应用退出/关闭开关则卸载；归队待提交的重启。
    pub fn sync_running_apps(&self) {
        let workspaces = {
            let conn = self.db.lock().unwrap();
            crate::db::dao::list_workspaces(&conn)
        };
        let workspaces = match workspaces {
            Ok(ws) => ws,
            Err(e) => {
                log::debug!("R-17: sync skipped, cannot list workspaces: {e}");
                return;
            }
        };

        // 期望监听集：app → 闭包模块目录。
        let mut desired_apps: HashMap<AppKey, Vec<PathBuf>> = HashMap::new();
        for ws in &workspaces {
            let processes = match self.processes.list_processes(ws.id) {
                Ok(list) => list,
                Err(_) => continue,
            };
            for process in processes {
                if !process.status.is_active() {
                    continue;
                }
                let auto_restart = {
                    let conn = self.db.lock().unwrap();
                    config::load_config_unredacted(&conn, ws.id, &process.runtime_name)
                        .ok()
                        .and_then(|config| config.auto_restart)
                };
                if auto_restart != Some(true) {
                    continue;
                }
                let dirs = self.closure_module_dirs(ws.id, &process.runtime_name);
                if dirs.is_empty() {
                    continue;
                }
                desired_apps.insert((ws.id, process.runtime_name.clone()), dirs);
            }
        }

        {
            let mut apps = self.apps.lock().unwrap();
            // 移除不再监听的应用。
            apps.retain(|key, _| desired_apps.contains_key(key));
            // 新增应用。
            for key in desired_apps.keys() {
                apps.entry(key.clone()).or_default();
            }
        }

        // 挂载/卸载（增量 diff，复用 T-06 约定）。
        let mut desired_dirs: HashSet<PathBuf> = HashSet::new();
        for dirs in desired_apps.values() {
            desired_dirs.extend(dirs.iter().cloned());
        }
        let (to_add, to_remove) = {
            let mut watched = self.watched.lock().unwrap();
            let to_add: Vec<PathBuf> = desired_dirs.difference(&watched).cloned().collect();
            let to_remove: Vec<PathBuf> = watched.difference(&desired_dirs).cloned().collect();
            for dir in &to_add {
                watched.insert(dir.clone());
            }
            for dir in &to_remove {
                watched.remove(dir);
            }
            (to_add, to_remove)
        };
        if !to_add.is_empty() || !to_remove.is_empty() {
            let mut watcher_guard = self.watcher.lock().unwrap();
            if let Some(watcher) = watcher_guard.as_mut() {
                for dir in &to_add {
                    if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                        log::warn!("R-17: failed to watch module dir {:?}: {e}", dir);
                    }
                }
                for dir in &to_remove {
                    let _ = watcher.unwatch(dir);
                }
                if !to_add.is_empty() {
                    log::info!(
                        "R-17: watching {} module dir(s) (+{} / -{}), apps: {}",
                        self.watched.lock().unwrap().len(),
                        to_add.len(),
                        to_remove.len(),
                        desired_apps.len()
                    );
                }
            }
        }

        // 归队：在途任务完成（进程回到 Running）且有待处理变更 → 再次提交。
        // 取走 pending_modules 并刷新提交时刻，防止下一轮 sync 重复归队。
        let resubmits: Vec<(i64, String, Vec<String>)> = {
            let mut apps = self.apps.lock().unwrap();
            let mut result = Vec::new();
            for ((workspace_id, runtime_name), state) in apps.iter_mut() {
                if !state.in_flight || state.pending_modules.is_empty() {
                    continue;
                }
                let running = self
                    .processes
                    .list_processes(*workspace_id)
                    .ok()
                    .map(|list| {
                        list.iter().any(|p| {
                            p.runtime_name == *runtime_name && p.status.is_active()
                                && p.status == crate::runtime::launch::LifecycleStatus::Running
                        })
                    })
                    .unwrap_or(false);
                let cooled = state
                    .last_submitted
                    .map(|t| t.elapsed() >= RESTART_RESUBMIT_COOLDOWN)
                    .unwrap_or(true);
                if running && cooled {
                    let modules = std::mem::take(&mut state.pending_modules)
                        .into_iter()
                        .collect();
                    state.last_submitted = Some(std::time::Instant::now());
                    result.push((*workspace_id, runtime_name.clone(), modules));
                }
            }
            result
        };
        for (workspace_id, runtime_name, modules) in resubmits {
            if !self.submit_rebuild(workspace_id, &runtime_name, &modules) {
                // 提交失败：模块放回 pending，下一轮 sync 重试。
                let mut apps = self.apps.lock().unwrap();
                if let Some(state) = apps.get_mut(&(workspace_id, runtime_name.clone())) {
                    state.pending_modules.extend(modules);
                }
            }
        }
    }

    /// 应用闭包内的模块目录（graph/closure 缓存热路径）。
    fn closure_module_dirs(&self, workspace_id: i64, runtime_name: &str) -> Vec<PathBuf> {
        let closure = match self.runtime_closure(workspace_id, runtime_name) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        closure
            .projects
            .iter()
            .map(|project| {
                project
                    .path
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_path_buf()
            })
            .collect()
    }

    /// 当前 Scope 下的 Runtime Closure（经 R-03 缓存）。
    fn runtime_closure(
        &self,
        workspace_id: i64,
        runtime_name: &str,
    ) -> crate::error::AppResult<crate::maven::RuntimeClosure> {
        let conn = self.db.lock().unwrap();
        let cfg = config::load_config_unredacted(&conn, workspace_id, runtime_name)?;
        let graph = self.graph_cache.get_or_load(&conn, workspace_id)?.graph;
        let needle = cfg.project.replace('\\', "/");
        let root = graph
            .projects
            .iter()
            .find(|p| {
                let path = p.path.to_string_lossy().replace('\\', "/");
                path == needle
                    || path.ends_with(&needle)
                    || p.coordinates.artifact_id == cfg.project
            })
            .ok_or_else(|| {
                crate::error::AppError::ProjectNotFound(format!(
                    "项目 '{0}' 不在依赖图中（R-17 watch）",
                    cfg.project
                ))
            })?;
        let lookup = self
            .closure_cache
            .get_or_compute(&graph, root.project_id, &cfg.scope)?;
        Ok(lookup.closure)
    }

    // ------------------------------------------------------------------
    // 防抖与分类
    // ------------------------------------------------------------------

    fn debounce_loop(&self, rx: std::sync::mpsc::Receiver<Vec<PathBuf>>) {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(first) = rx.recv() else { break };
            // 静默窗口：收集到不再有新事件为止（≤ 窗口上限）。
            let mut paths = first;
            let deadline = std::time::Instant::now() + DEBOUNCE_WINDOW;
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match rx.recv_timeout(remaining) {
                    Ok(more) => paths.extend(more),
                    Err(_) => break,
                }
            }
            self.handle_events(&paths);
        }
    }

    /// 分类 + 影响分析（§71/§72）。路径 → 模块 → 应用闭包 → 受影响子集。
    fn handle_events(&self, paths: &[PathBuf]) {
        // 路径分类：按「监听中的应用闭包」归属（未监听路径直接忽略——
        // 不监听整个 Workspace，目标/构建产物等在挂载前已排除大半）。
        let apps = self.apps.lock().unwrap().keys().cloned().collect::<Vec<_>>();
        if apps.is_empty() {
            return;
        }

        // 每个应用：变更文件 → 受影响模块 GA / pom 标记。
        let mut per_app: BTreeMap<AppKey, (BTreeSet<PathBuf>, bool)> = BTreeMap::new();
        for (workspace_id, runtime_name) in &apps {
            let closure = match self.runtime_closure(*workspace_id, runtime_name) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for path in paths {
                let normalized = path.to_string_lossy().replace('\\', "/");
                if ignore_path(&normalized) {
                    continue;
                }
                // 归属判定：路径必须落在某个闭包模块目录内（前缀匹配）。
                let owned = closure.projects.iter().any(|project| {
                    let dir = project
                        .path
                        .parent()
                        .unwrap_or(Path::new(""))
                        .to_string_lossy()
                        .replace('\\', "/");
                    normalized.starts_with(&format!("{dir}/"))
                });
                if !owned {
                    continue;
                }
                let is_pom = normalized.ends_with("/pom.xml");
                let entry = per_app
                    .entry((*workspace_id, runtime_name.clone()))
                    .or_default();
                entry.0.insert(path.clone());
                if is_pom {
                    entry.1 = true;
                }
            }
        }
        for ((workspace_id, runtime_name), (changed_paths, has_pom)) in per_app {
            // §64 file_changed 事件（通知语义：UI 收到后按需拉取）。
            self.emitter.emit(RuntimeEmission::new(
                EVENT_FILE_CHANGED,
                &FileChangedPayload {
                    workspace_id,
                    paths: changed_paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect(),
                    at: chrono::Utc::now().to_rfc3339(),
                },
            ));

            if has_pom {
                // pom.xml 变化：依赖模型失效重算（R-02/R-03 缓存在
                // exec_resolve 内 invalidate），不直接构建。
                log::info!(
                    "R-17: pom.xml changed in '{runtime_name}' closure → resubmitting dependency resolve"
                );
                self.submit_resolve(workspace_id);
                continue;
            }

            // 源码变化：计算受影响子集并入队（合并连续保存）。
            let Some(affected) = self.affected_modules(workspace_id, &runtime_name, &changed_paths)
            else {
                continue;
            };
            if affected.is_empty() {
                continue;
            }
            let mut apps = self.apps.lock().unwrap();
            let Some(state) = apps.get_mut(&(workspace_id, runtime_name.clone())) else {
                continue;
            };
            state.pending_modules.extend(affected);
            for path in &changed_paths {
                if let Some(dir) = path.parent() {
                    state
                        .pending_changes
                        .entry(dir.to_path_buf())
                        .or_default()
                        .insert(path.clone());
                }
            }
            if state.in_flight {
                log::debug!(
                    "R-17: '{runtime_name}' rebuild in flight; queued {} module change(s)",
                    state.pending_modules.len()
                );
                continue;
            }
            let modules: Vec<String> = std::mem::take(&mut state.pending_modules).into_iter().collect();
            drop(apps);
            if self.submit_rebuild(workspace_id, &runtime_name, &modules) {
                let mut apps = self.apps.lock().unwrap();
                if let Some(state) = apps.get_mut(&(workspace_id, runtime_name.clone())) {
                    state.in_flight = true;
                    state.last_submitted = Some(std::time::Instant::now());
                }
            } else {
                // 提交失败：变更放回 pending，下轮 sync / 下次事件重试。
                let mut apps = self.apps.lock().unwrap();
                if let Some(state) = apps.get_mut(&(workspace_id, runtime_name.clone())) {
                    state.pending_modules.extend(modules);
                }
            }
        }
    }

    /// §72 变更影响分析：变更路径 → 所属模块 → 反向依赖传播（闭包内）。
    /// 返回受影响模块 GA 集合（含变更模块自身）。
    fn affected_modules(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        changed_paths: &BTreeSet<PathBuf>,
    ) -> Option<BTreeSet<String>> {
        let closure = self.runtime_closure(workspace_id, runtime_name).ok()?;
        let conn = self.db.lock().unwrap();
        let graph = self.graph_cache.get_or_load(&conn, workspace_id).ok()?.graph;
        drop(conn);

        let mut changed_ids: BTreeSet<i64> = BTreeSet::new();
        for path in changed_paths {
            let normalized = path.to_string_lossy().replace('\\', "/");
            for project in &closure.projects {
                let dir = project
                    .path
                    .parent()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .replace('\\', "/");
                if normalized.starts_with(&format!("{dir}/")) {
                    changed_ids.insert(project.project_id);
                    break;
                }
            }
        }
        if changed_ids.is_empty() {
            return None;
        }

        // 反向传播：edge.from 依赖 edge.source_project_id → 上游变更传播到下游。
        let closure_ids: HashSet<i64> = closure.projects.iter().map(|p| p.project_id).collect();
        let mut affected = changed_ids.clone();
        propagate_downstream(&mut affected, &graph.dependencies, &closure_ids);

        let ga_of = |id: i64| {
            closure
                .projects
                .iter()
                .find(|p| p.project_id == id)
                .map(|p| format!("{}:{}", p.coordinates.group_id, p.coordinates.artifact_id))
        };
        Some(affected.iter().filter_map(|id| ga_of(*id)).collect())
    }

    // ------------------------------------------------------------------
    // 提交
    // ------------------------------------------------------------------

    /// 提交 RebuildRestart 任务。返回是否成功提交；失败时调用方负责把
    /// `modules` 放回 pending（本方法不再动 in_flight 状态）。
    fn submit_rebuild(&self, workspace_id: i64, runtime_name: &str, modules: &[String]) -> bool {
        let Some(submitter) = self.task_manager.lock().unwrap().clone() else {
            log::debug!("R-17: task manager not attached yet; rebuild for '{runtime_name}' kept pending");
            return false;
        };
        let mut options = RuntimeTaskOptions::default();
        options.affected_modules = modules.to_vec();
        let request = TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::RebuildRestart,
                workspace_id,
                runtime_name: runtime_name.to_string(),
                options,
            },
            repo_path: String::new(),
            repo_name: format!("自动重建重启 · {runtime_name}"),
        };
        match submitter.submit(request) {
            Ok(task_id) => {
                log::info!(
                    "R-17: auto rebuild+restart submitted for '{runtime_name}' (task {task_id})"
                );
                true
            }
            Err(e) => {
                log::warn!("R-17: failed to submit auto rebuild for '{runtime_name}': {e}");
                false
            }
        }
    }

    fn submit_resolve(&self, workspace_id: i64) {
        let Some(submitter) = self.task_manager.lock().unwrap().clone() else {
            return;
        };
        let request = TaskRequest {
            task_type: TaskType::Runtime {
                op: RuntimeOp::ResolveDependencies,
                workspace_id,
                runtime_name: String::new(),
                options: RuntimeTaskOptions::default(),
            },
            repo_path: String::new(),
            repo_name: format!("watch: workspace #{workspace_id} pom 变化 → 依赖重算"),
        };
        if let Err(e) = submitter.submit(request) {
            log::warn!("R-17: failed to submit resolve after pom change: {e}");
        }
    }

    // ------------------------------------------------------------------
    // 测试钩子
    // ------------------------------------------------------------------

    /// 直接注入一个受监听应用（测试用；生产路径由 sync_running_apps 对账）。
    #[cfg(test)]
    pub fn register_app_for_test(&self, workspace_id: i64, runtime_name: &str, dirs: Vec<PathBuf>) {
        self.apps
            .lock()
            .unwrap()
            .entry((workspace_id, runtime_name.to_string()))
            .or_default();
        let mut watcher_guard = self.watcher.lock().unwrap();
        if let Some(watcher) = watcher_guard.as_mut() {
            let mut watched = self.watched.lock().unwrap();
            for dir in dirs {
                if watched.insert(dir.clone()) {
                    let _ = watcher.watch(&dir, RecursiveMode::Recursive);
                }
            }
        }
    }

    /// 等待防抖窗口结束并处理已到达事件（测试用）。
    #[cfg(test)]
    pub fn flush_for_test(&self) {
        std::thread::sleep(DEBOUNCE_WINDOW + Duration::from_millis(150));
    }
}

/// 路径忽略规则（§43）：`target/` / `.git/` / `node_modules/` 等构建产物与
/// 元数据。纯函数（单测覆盖）。
pub fn ignore_path(normalized_path: &str) -> bool {
    const IGNORED_SEGMENTS: &[&str] = &[
        "/target/",
        "/.git/",
        "/node_modules/",
        "/.gitworkspace/",
    ];
    for segment in IGNORED_SEGMENTS {
        if normalized_path.contains(segment) {
            return true;
        }
    }
    // .class 文件（增量编译产物落 target 已排除；防御双扩展名等）。
    normalized_path.ends_with(".class")
}

/// §72 变更影响分析的传播纯函数（单测覆盖）：`affected`（初始含变更模块）
/// 沿 workspace 内依赖边（`from` 依赖 `source_project_id`）向下游扩散至
/// 不动点；只扩散到 `closure_ids` 内的模块，远程/外部来源边（无
/// `source_project_id`）不参与。
fn propagate_downstream(
    affected: &mut BTreeSet<i64>,
    dependencies: &[crate::maven::index::DependencyEdge],
    closure_ids: &HashSet<i64>,
) {
    loop {
        let mut propagated = BTreeSet::new();
        for edge in dependencies {
            let Some(upstream) = edge.source_project_id else {
                continue;
            };
            if !affected.contains(&upstream) || affected.contains(&edge.from_project_id) {
                continue;
            }
            if closure_ids.contains(&edge.from_project_id) {
                propagated.insert(edge.from_project_id);
            }
        }
        if propagated.is_empty() {
            break;
        }
        affected.extend(propagated);
    }
}

/// 测试用假任务提交端：只记录提交的请求。
struct RecordingSubmitter {
    requests: Mutex<Vec<TaskRequest>>,
}

impl RecordingSubmitter {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
        })
    }

    fn ops(&self) -> Vec<(RuntimeOp, i64, String, Vec<String>)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| match &r.task_type {
                TaskType::Runtime {
                    op,
                    workspace_id,
                    runtime_name,
                    options,
                } => (
                    *op,
                    *workspace_id,
                    runtime_name.clone(),
                    options.affected_modules.clone(),
                ),
                _ => panic!("unexpected non-runtime task"),
            })
            .collect()
    }
}

impl WatchTaskSubmitter for RecordingSubmitter {
    fn submit(&self, request: TaskRequest) -> crate::error::AppResult<String> {
        self.requests.lock().unwrap().push(request);
        Ok(format!("task-{}", self.requests.lock().unwrap().len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::repository::ScannedRepo;
    use crate::runtime::config::CreateRuntimeConfigRequest;
    use crate::runtime::events::VecEmitter;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn ignore_path_filters_build_outputs() {
        assert!(ignore_path("/ws/repo/app/target/classes/App.class"));
        assert!(ignore_path("/ws/repo/.git/refs/main"));
        assert!(ignore_path("/ws/repo/app/node_modules/vue/index.js"));
        assert!(!ignore_path("/ws/repo/app/src/main/java/App.java"));
        assert!(!ignore_path("/ws/repo/app/pom.xml"));
    }

    // ------------------------------------------------------------------
    // §72 反向传播（纯函数）
    // ------------------------------------------------------------------

    fn edge(from: i64, source: i64) -> crate::maven::index::DependencyEdge {
        crate::maven::index::DependencyEdge {
            dependency_id: 0,
            from_project_id: from,
            dependency: crate::maven::model::MavenDependency {
                group_id: "com.example".into(),
                artifact_id: "x".into(),
                version: Some("1.0.0".into()),
                scope: crate::maven::model::DependencyScope::Compile,
                optional: false,
                dep_type: "jar".into(),
                classifier: None,
                exclusions: Vec::new(),
            },
            source: crate::maven::resolver::DependencySource::WorkspaceSource,
            source_project_id: Some(source),
            resolved_path: None,
            reason: crate::maven::resolver::ResolutionReason::WorkspaceExactMatch,
        }
    }

    /// 链 common ← core ← auth ← boot：改 common 传播到全部；改 auth 只到
    /// auth/boot；闭包外模块不扩散。
    #[test]
    fn propagate_downstream_spreads_to_closure_descendants_only() {
        let edges = vec![edge(2, 1), edge(3, 2), edge(4, 3)];
        let closure: HashSet<i64> = [1, 2, 3, 4].into_iter().collect();

        let mut affected: BTreeSet<i64> = [1].into_iter().collect();
        propagate_downstream(&mut affected, &edges, &closure);
        assert_eq!(affected, [1, 2, 3, 4].into_iter().collect());

        let mut affected: BTreeSet<i64> = [3].into_iter().collect();
        propagate_downstream(&mut affected, &edges, &closure);
        assert_eq!(affected, [3, 4].into_iter().collect());

        // 闭包外下游不扩散（edge 指向 closure 外）。
        let closure_small: HashSet<i64> = [1, 2].into_iter().collect();
        let mut affected: BTreeSet<i64> = [1].into_iter().collect();
        propagate_downstream(&mut affected, &edges, &closure_small);
        assert_eq!(affected, [1, 2].into_iter().collect());
    }

    /// 无 source_project_id 的边（Remote Maven）不参与传播；环收敛。
    #[test]
    fn propagate_downstream_ignores_external_sources_and_converges() {
        let mut cyclic = edge(1, 4);
        let external = crate::maven::index::DependencyEdge {
            source_project_id: None,
            ..cyclic.clone()
        };
        cyclic.dependency_id = 1;
        let edges = vec![cyclic, external];
        let closure: HashSet<i64> = [1, 4].into_iter().collect();
        let mut affected: BTreeSet<i64> = [4].into_iter().collect();
        propagate_downstream(&mut affected, &edges, &closure);
        assert_eq!(affected, [1, 4].into_iter().collect());
    }

    // ------------------------------------------------------------------
    // 端到端（真 DB 索引 + 假提交端）：parent/lib/app 三模块
    // ------------------------------------------------------------------

    struct Fixture {
        root: PathBuf,
        db: Arc<Mutex<Connection>>,
        workspace_id: i64,
    }

    /// 单仓 parent(pom) + lib(jar) + app(jar→lib)，同步依赖图索引 + 配置。
    fn watch_fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "gw_r17_watch_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root.join("repo/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version><packaging>pom</packaging>\
             <modules><module>lib</module><module>app</module></modules></project>",
        );
        write(
            &root.join("repo/lib/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>lib</artifactId></project>",
        );
        write(
            &root.join("repo/app/pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion><parent><groupId>com.example</groupId>\
             <artifactId>parent</artifactId><version>1.0.0</version></parent>\
             <artifactId>app</artifactId><dependencies><dependency><groupId>com.example</groupId>\
             <artifactId>lib</artifactId><version>1.0.0</version></dependency></dependencies></project>",
        );
        git2::Repository::init(root.join("repo")).unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (name, path, created_at, updated_at) VALUES ('w', ?1, 't', 't')",
            [root.to_string_lossy().to_string()],
        )
        .unwrap();
        let workspace_id = conn.last_insert_rowid();
        crate::db::dao::upsert_repositories_batch(
            &mut conn,
            workspace_id,
            &[ScannedRepo {
                path: root.join("repo").to_string_lossy().to_string(),
                name: "repo".into(),
                relative_path: "repo".into(),
                git_dir_mtime: None,
            }],
        )
        .unwrap();
        let discovery = crate::maven::discover_poms(&root, 5, None, None);
        assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
        crate::maven::sync_workspace_index(&mut conn, workspace_id, &discovery, &root.join("m2"))
            .unwrap();

        crate::runtime::config::create_config(
            &conn,
            &CreateRuntimeConfigRequest {
                workspace_id,
                config: crate::runtime::config::RuntimeApplicationConfig {
                    name: "app".into(),
                    project: root.join("repo/app/pom.xml").to_string_lossy().to_string(),
                    main_class: Some("com.example.app.Application".into()),
                    auto_restart: Some(true),
                    ..Default::default()
                },
            },
        )
        .unwrap();

        Fixture {
            root,
            db: Arc::new(Mutex::new(conn)),
            workspace_id,
        }
    }

    /// 直接构造引擎（不走 spawn：不起防抖/同步线程，注入假提交端）。
    fn test_engine(
        fixture: &Fixture,
        emitter: Arc<VecEmitter>,
    ) -> (RuntimeWatchEngine, Arc<RecordingSubmitter>) {
        let submitter = RecordingSubmitter::new();
        let (event_tx, _event_rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
        let engine = RuntimeWatchEngine {
            db: Arc::clone(&fixture.db),
            graph_cache: Arc::new(DependencyGraphCache::new()),
            closure_cache: Arc::new(RuntimeClosureCache::new()),
            emitter,
            processes: Arc::new(crate::runtime::launch::RuntimeProcessManager::new(
                Arc::clone(&fixture.db),
            )),
            task_manager: Mutex::new(Some(Arc::clone(&submitter) as Arc<dyn WatchTaskSubmitter>)),
            watcher: Mutex::new(None),
            watched: Mutex::new(HashSet::new()),
            apps: Mutex::new(HashMap::new()),
            event_tx,
            stop: Arc::new(AtomicBool::new(false)),
        };
        (engine, submitter)
    }

    /// §43 忽略规则 + §72 影响分析 + RebuildRestart 提交 + file_changed 事件。
    #[test]
    fn source_change_submits_rebuild_restart_with_affected_subset() {
        let fixture = watch_fixture("rebuild");
        let emitter = Arc::new(VecEmitter::default());
        let (engine, submitter) = test_engine(&fixture, Arc::clone(&emitter));
        engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

        // 改 lib 源码 → 影响分析 = lib + app（反向传播）。
        let lib_source = fixture.root.join("repo/lib/src/main/java/Demo.java");
        engine.handle_events(&[lib_source]);

        let ops = submitter.ops();
        assert_eq!(ops.len(), 1, "one rebuild submitted: {ops:?}");
        let (op, ws, name, modules) = &ops[0];
        assert_eq!(*op, RuntimeOp::RebuildRestart);
        assert_eq!(*ws, fixture.workspace_id);
        assert_eq!(name, "app");
        assert_eq!(
            modules,
            &vec![
                "com.example:app".to_string(),
                "com.example:lib".to_string()
            ]
        );
        assert!(emitter.names().contains(&EVENT_FILE_CHANGED));

        // target/ 产物变化被忽略：不提交、不发事件。
        let before = submitter.ops().len();
        engine.handle_events(&[fixture
            .root
            .join("repo/lib/target/classes/Demo.class")]);
        assert_eq!(submitter.ops().len(), before);
        assert_eq!(emitter.names().len(), 1);
    }

    /// 改 app 自身 → 只重建 app（不扩大到无关模块）。
    #[test]
    fn app_own_change_rebuilds_only_app() {
        let fixture = watch_fixture("app-only");
        let emitter = Arc::new(VecEmitter::default());
        let (engine, submitter) = test_engine(&fixture, emitter);
        engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

        engine.handle_events(&[fixture
            .root
            .join("repo/app/src/main/java/Application.java")]);

        let ops = submitter.ops();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, RuntimeOp::RebuildRestart);
        assert_eq!(ops[0].3, vec!["com.example:app".to_string()]);
    }

    /// pom.xml 变化 → 提交依赖模型重算（ResolveDependencies），不直接构建。
    #[test]
    fn pom_change_submits_dependency_resolve_not_build() {
        let fixture = watch_fixture("pom");
        let emitter = Arc::new(VecEmitter::default());
        let (engine, submitter) = test_engine(&fixture, emitter);
        engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

        engine.handle_events(&[fixture.root.join("repo/lib/pom.xml")]);

        let ops = submitter.ops();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, RuntimeOp::ResolveDependencies);
        assert!(ops[0].2.is_empty(), "resolve 任务不带 runtime 名");
    }

    /// 构建中收到新变化 → 排队合并（in_flight 合并语义），不重复提交。
    #[test]
    fn changes_during_in_flight_rebuild_are_queued_and_merged() {
        let fixture = watch_fixture("merge");
        let emitter = Arc::new(VecEmitter::default());
        let (engine, submitter) = test_engine(&fixture, emitter);
        engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

        let lib_a = fixture.root.join("repo/lib/src/main/java/A.java");
        let lib_b = fixture.root.join("repo/lib/src/main/java/B.java");
        engine.handle_events(&[lib_a.clone()]);
        // 在途期间第二波变化：入队，不重复提交。
        engine.handle_events(&[lib_b]);
        assert_eq!(submitter.ops().len(), 1, "merged into one submission");

        let apps = engine.apps.lock().unwrap();
        let state = apps
            .get(&(fixture.workspace_id, "app".to_string()))
            .expect("watched app state");
        assert!(state.in_flight);
        assert!(
            state.pending_modules.contains("com.example:lib"),
            "queued module for the next rebuild: {:?}",
            state.pending_modules
        );
    }

    /// §42 归队：在途任务完成后（进程回到 running）有待处理变更 → 再提交。
    #[test]
    fn queued_changes_resubmit_after_process_returns_to_running() {
        let fixture = watch_fixture("resubmit");
        let emitter = Arc::new(VecEmitter::default());
        let (engine, submitter) = test_engine(&fixture, emitter);
        engine.register_app_for_test(fixture.workspace_id, "app", Vec::new());

        let lib_a = fixture.root.join("repo/lib/src/main/java/A.java");
        engine.handle_events(&[lib_a]);
        assert_eq!(submitter.ops().len(), 1);
        // 在途期间又改一处 → 排队。
        engine.handle_events(&[fixture.root.join("repo/lib/src/main/java/B.java")]);
        assert_eq!(submitter.ops().len(), 1);

        // 模拟重启完成：进程行回到 running；等待归队冷却窗口。
        {
            let conn = fixture.db.lock().unwrap();
            conn.execute(
                "INSERT INTO runtime_processes (workspace_id, runtime_name, status, started_at, updated_at)
                 VALUES (?1, 'app', 'running', 't', 't')",
                [fixture.workspace_id],
            )
            .unwrap();
        }
        std::thread::sleep(RESTART_RESUBMIT_COOLDOWN + Duration::from_millis(100));
        engine.sync_running_apps();

        let ops = submitter.ops();
        assert_eq!(ops.len(), 2, "queued change resubmitted: {ops:?}");
        assert_eq!(ops[1].0, RuntimeOp::RebuildRestart);
        assert!(ops[1].3.contains(&"com.example:lib".to_string()));
    }

    /// watch 引擎把受影响 GA 子集写进 RuntimeTaskOptions.affectedModules，
    /// 经 IPC serde round-trip 保持（契约防回归）。
    #[test]
    fn affected_modules_survive_ipc_serde() {
        let options = RuntimeTaskOptions {
            affected_modules: vec!["com.example:lib".into(), "com.example:app".into()],
            ..Default::default()
        };
        let json = serde_json::to_value(&options).unwrap();
        assert_eq!(
            json["affectedModules"],
            serde_json::json!(["com.example:lib", "com.example:app"])
        );
        let back: RuntimeTaskOptions = serde_json::from_value(json).unwrap();
        assert_eq!(back, options);

        // 旧客户端不带该字段 → 缺省空（向后兼容）。
        let legacy: RuntimeTaskOptions =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(legacy.affected_modules.is_empty());
    }
}
