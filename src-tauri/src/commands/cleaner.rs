//! 工具箱「工作区清理」命令（T-36）：扫描会话 + 目录大小异步补算 + 确认删除。
//!
//! 安全模型（同 Go 版 GUI，见 docs/tasks/T-36-toolbox-cleaner.md）：
//! - 扫描结果保存在服务端内存会话（token 标识），删除只认会话内路径，
//!   不接受前端提交的任意路径；
//! - `cleaner_execute` 强制 `confirmed=true`（同 toolbox_route_apply 的
//!   二次确认约束），前端需先让用户输入 DELETE；
//! - 会话一次性：执行后清除，后续删除需重新扫描（磁盘状态已变化）。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::cleaner::{self, CleanerItemInfo, CleanerScanRequest, DeleteItemResult, ScannedItem};
use crate::error::{AppError, AppResult};

/// 扫描进度事件节流间隔（全局约束 §2：高频事件聚合推送）。
const PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

/// 清理工具状态：仅保存最近一次扫描会话（同 Go GUI 的单会话语义）。
#[derive(Default)]
pub struct CleanerState {
    inner: Arc<Mutex<CleanerInner>>,
}

#[derive(Default)]
struct CleanerInner {
    session: Option<CleanerSession>,
}

struct CleanerSession {
    token: String,
    items: Vec<ScannedItem>,
    /// 扫描时 canonical 化的排除路径快照，删除阶段复检复用。
    exclude_paths: Vec<PathBuf>,
    /// 已补算的大小（display path → bytes）；文件在扫描时已预填。
    sizes: HashMap<String, u64>,
}

impl CleanerState {
    fn lock(&self) -> AppResult<MutexGuard<'_, CleanerInner>> {
        self.inner
            .lock()
            .map_err(|e| AppError::Other(format!("cleaner state lock error: {e}")))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerScanResult {
    pub token: String,
    pub items: Vec<CleanerItemInfo>,
    pub visited: u64,
    pub matched: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerExecuteResult {
    pub deleted: u32,
    pub failed: u32,
    pub items: Vec<DeleteItemResult>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgressPayload {
    token: String,
    visited: u64,
    matched: u64,
    done: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SizeProgressPayload {
    token: String,
    /// done 帧的 path 为空串。
    path: String,
    size: u64,
    done: bool,
}

/// 受保护路径：应用 exe（及其所在目录）与 app data 目录（DB 所在）。
/// 存在即 canonicalize，与扫描侧的路径键同形态。
fn protected_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.to_path_buf());
        }
        out.push(exe);
    }
    out.push(crate::get_app_data_dir());
    out.iter()
        .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect()
}

/// 扫描预览（只读，不删除任何东西）。进度经 `cleaner_scan_progress` 事件推送。
#[tauri::command]
pub fn cleaner_scan(
    req: CleanerScanRequest,
    app: AppHandle,
    state: State<'_, CleanerState>,
) -> AppResult<CleanerScanResult> {
    let token = uuid::Uuid::new_v4().simple().to_string();
    let protected = protected_paths();
    let mut last_emit = Instant::now() - PROGRESS_THROTTLE;
    let outcome = {
        let token = token.clone();
        let app = app.clone();
        cleaner::scan(&req, &protected, move |visited, matched| {
            if last_emit.elapsed() >= PROGRESS_THROTTLE {
                last_emit = Instant::now();
                let _ = app.emit(
                    "cleaner_scan_progress",
                    ScanProgressPayload {
                        token: token.clone(),
                        visited,
                        matched,
                        done: false,
                    },
                );
            }
        })?
    };
    let _ = app.emit(
        "cleaner_scan_progress",
        ScanProgressPayload {
            token: token.clone(),
            visited: outcome.visited,
            matched: outcome.matched,
            done: true,
        },
    );

    let mut sizes = HashMap::new();
    let items: Vec<CleanerItemInfo> = outcome
        .items
        .iter()
        .map(|item| {
            if let Some(size) = item.size {
                sizes.insert(item.display_path.clone(), size);
            }
            CleanerItemInfo {
                path: item.display_path.clone(),
                is_dir: item.is_dir,
                size: item.size,
            }
        })
        .collect();

    let mut inner = state.lock()?;
    inner.session = Some(CleanerSession {
        token: token.clone(),
        items: outcome.items,
        exclude_paths: outcome.exclude_paths,
        sizes,
    });

    log::info!(
        "cleaner scan: visited={} matched={} candidates={}",
        outcome.visited,
        outcome.matched,
        items.len()
    );
    Ok(CleanerScanResult {
        token,
        items,
        visited: outcome.visited,
        matched: outcome.matched,
        warnings: outcome.warnings,
    })
}

/// 后台补算目录大小（扫描本身不递归统计，保证预览速度）。
/// 每项完成推送 `cleaner_size_progress`（带 token，前端据此丢弃过期事件）。
#[tauri::command]
pub fn cleaner_compute_sizes(
    token: String,
    app: AppHandle,
    state: State<'_, CleanerState>,
) -> AppResult<()> {
    let inner_arc = state.inner.clone();
    let targets: Vec<(String, PathBuf)> = {
        let inner = state.lock()?;
        let session = inner
            .session
            .as_ref()
            .ok_or_else(|| AppError::Other("扫描会话不存在或已过期，请重新扫描".into()))?;
        if session.token != token {
            return Err(AppError::Conflict(
                "扫描会话已更新，请基于最新扫描结果操作".into(),
            ));
        }
        session
            .items
            .iter()
            .filter(|i| i.is_dir)
            .map(|i| (i.display_path.clone(), i.io_path.clone()))
            .collect()
    };

    std::thread::spawn(move || {
        for (display, io_path) in targets {
            let size = cleaner::compute_size(&io_path, true);
            {
                let Ok(mut inner) = inner_arc.lock() else {
                    return;
                };
                match inner.session.as_mut() {
                    Some(s) if s.token == token => {
                        s.sizes.insert(display.clone(), size);
                    }
                    // 会话已更换/清除：旧线程直接退出，不再推送事件。
                    _ => return,
                }
            }
            let _ = app.emit(
                "cleaner_size_progress",
                SizeProgressPayload {
                    token: token.clone(),
                    path: display,
                    size,
                    done: false,
                },
            );
        }
        let still_current = inner_arc
            .lock()
            .map(|i| i.session.as_ref().is_some_and(|s| s.token == token))
            .unwrap_or(false);
        if still_current {
            let _ = app.emit(
                "cleaner_size_progress",
                SizeProgressPayload {
                    token,
                    path: String::new(),
                    size: 0,
                    done: true,
                },
            );
        }
    });
    Ok(())
}

/// 执行删除（**危险操作**）。
///
/// 三重门禁：会话 token（路径必须来自最近扫描会话）+ `confirmed=true`
/// （后端强制）+ 前端 DELETE 输入确认。执行后清除会话（一次性）。
#[tauri::command]
pub fn cleaner_execute(
    token: String,
    paths: Vec<String>,
    confirmed: bool,
    state: State<'_, CleanerState>,
) -> AppResult<CleanerExecuteResult> {
    if !confirmed {
        return Err(AppError::Permission(
            "删除为高危操作：请在前端完成 DELETE 确认后以 confirmed=true 调用".into(),
        ));
    }
    if paths.is_empty() {
        return Err(AppError::Other("未选择任何待删除项".into()));
    }
    let (items, exclude_paths) = {
        let inner = state.lock()?;
        let session = inner
            .session
            .as_ref()
            .ok_or_else(|| AppError::Other("扫描会话不存在或已过期，请重新扫描".into()))?;
        if session.token != token {
            return Err(AppError::Conflict(
                "扫描会话已更新，请基于最新扫描结果操作".into(),
            ));
        }
        let mut selected = Vec::with_capacity(paths.len());
        for p in &paths {
            let item = session
                .items
                .iter()
                .find(|i| &i.display_path == p)
                .ok_or_else(|| {
                    AppError::Permission(format!("路径不在本次扫描清单内，已拒绝：{p}"))
                })?;
            selected.push(item.clone());
        }
        (selected, session.exclude_paths.clone())
    };

    let protected = protected_paths();
    let results = cleaner::execute(&items, &exclude_paths, &protected);
    let deleted = results.iter().filter(|r| r.ok).count() as u32;
    let failed = results.len() as u32 - deleted;

    {
        let mut inner = state.lock()?;
        if inner.session.as_ref().is_some_and(|s| s.token == token) {
            inner.session = None;
        }
    }
    log::info!("cleaner execute: deleted={deleted} failed={failed}");
    Ok(CleanerExecuteResult {
        deleted,
        failed,
        items: results,
    })
}
