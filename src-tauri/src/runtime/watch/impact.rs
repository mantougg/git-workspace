//! 变更影响分析（R-17，B-07 拆分）——**纯逻辑，不直接提交 Task**（§4.7）。
//!
//! 变更路径 → 所属闭包模块 → 反向依赖传播（闭包内）→ 受影响模块 GA 集合。
//! 只扩散到闭包内的下游模块；远程/外部来源边（无 `source_project_id`）
//! 不参与传播。传播到不动点为止，天然收敛。

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use crate::maven::closure::RuntimeClosure;
use crate::maven::index::{DependencyEdge, DependencyGraph};

use super::classify::{module_dir, normalize_path, path_in_module_dir};

/// §72 变更影响分析（纯函数）：变更路径 → 所属模块 → 反向依赖传播。
/// 返回受影响模块 GA 集合（含变更模块自身）；无任何路径归属闭包内模块
/// 时返回 `None`。
pub(super) fn affected_modules(
    closure: &RuntimeClosure,
    graph: &DependencyGraph,
    changed_paths: &BTreeSet<PathBuf>,
) -> Option<BTreeSet<String>> {
    let mut changed_ids: BTreeSet<i64> = BTreeSet::new();
    for path in changed_paths {
        let normalized = normalize_path(path);
        for project in &closure.projects {
            if path_in_module_dir(&normalized, &module_dir(&project.path)) {
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

/// §72 变更影响分析的传播纯函数（单测覆盖）：`affected`（初始含变更模块）
/// 沿 workspace 内依赖边（`from` 依赖 `source_project_id`）向下游扩散至
/// 不动点；只扩散到 `closure_ids` 内的模块，远程/外部来源边（无
/// `source_project_id`）不参与。
pub(super) fn propagate_downstream(
    affected: &mut BTreeSet<i64>,
    dependencies: &[DependencyEdge],
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
