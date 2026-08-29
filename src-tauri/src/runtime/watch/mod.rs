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

use crate::maven::closure::RuntimeClosureCache;
use crate::maven::index::DependencyGraphCache;
use crate::runtime::config;
use crate::runtime::events::{
    FileChangedPayload, RuntimeEmission, RuntimeEventEmitter, EVENT_FILE_CHANGED,
};

mod classify;
mod debounce;
mod impact;
mod submit;

pub use classify::ignore_path;
use classify::{module_dir, normalize_path, path_in_module_dir};
use debounce::{collect_batch, collect_event_paths};
use impact::affected_modules;
pub use submit::WatchTaskSubmitter;

/// 静默窗口：最后一次事件后等待该时长再判定（File Change → Detection
/// < 300ms 预算的主要组成，§99）。
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);
/// 应用同步周期：对账运行中应用与监听集（挂载/卸载、自动重启归队）。
pub const SYNC_INTERVAL: Duration = Duration::from_secs(5);
/// 重启任务完成后到下次重启的最小间隔（防抖兜底）。
const RESTART_RESUBMIT_COOLDOWN: Duration = Duration::from_millis(500);

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
                    let paths = collect_event_paths(event);
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
                            p.runtime_name == *runtime_name
                                && p.status.is_active()
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
            .map(|project| project.path.parent().unwrap_or(Path::new("")).to_path_buf())
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
            let paths = collect_batch(&rx, first, DEBOUNCE_WINDOW);
            self.handle_events(&paths);
        }
    }

    /// 分类 + 影响分析（§71/§72）。路径 → 模块 → 应用闭包 → 受影响子集。
    fn handle_events(&self, paths: &[PathBuf]) {
        // 路径分类：按「监听中的应用闭包」归属（未监听路径直接忽略——
        // 不监听整个 Workspace，目标/构建产物等在挂载前已排除大半）。
        let apps = self
            .apps
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
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
                let normalized = normalize_path(path);
                if ignore_path(&normalized) {
                    continue;
                }
                // 归属判定：路径必须落在某个闭包模块目录内（前缀匹配）。
                let owned = closure
                    .projects
                    .iter()
                    .any(|project| path_in_module_dir(&normalized, &module_dir(&project.path)));
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
            let modules: Vec<String> = std::mem::take(&mut state.pending_modules)
                .into_iter()
                .collect();
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

    /// §72 变更影响分析（数据装配）：取闭包与依赖图，纯分析在
    /// `impact::affected_modules`。返回受影响模块 GA 集合（含变更模块自身）。
    fn affected_modules(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        changed_paths: &BTreeSet<PathBuf>,
    ) -> Option<BTreeSet<String>> {
        let closure = self.runtime_closure(workspace_id, runtime_name).ok()?;
        let conn = self.db.lock().unwrap();
        let graph = self
            .graph_cache
            .get_or_load(&conn, workspace_id)
            .ok()?
            .graph;
        drop(conn);
        affected_modules(&closure, &graph, changed_paths)
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

#[cfg(test)]
mod tests;
