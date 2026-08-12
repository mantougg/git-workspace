use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::error::AppResult;

/// Commit information for the Git Graph view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub oid: String,
    pub short_oid: String,
    pub message: String,
    pub author: String,
    pub email: String,
    pub time: String,
    pub parents: Vec<String>,
    /// Branch and tag names pointing to this commit.
    pub refs: Vec<String>,
}

/// Read commit history from HEAD, sorted topologically.
///
/// `max_count` limits the number of commits returned (pagination).
/// Each commit includes its parent OIDs and any refs pointing to it.
pub fn get_commit_history(repo_path: &Path, max_count: usize) -> AppResult<Vec<CommitInfo>> {
    let repo = git2::Repository::open(repo_path)?;

    // Build a map of OID -> refs for quick lookup
    let ref_map = build_ref_map(&repo);

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

    let commits: Vec<CommitInfo> = revwalk
        .take(max_count)
        .filter_map(|oid_result| {
            let oid = oid_result.ok()?;
            let commit = repo.find_commit(oid).ok()?;

            let oid_str = oid.to_string();
            let short_oid = if oid_str.len() >= 7 {
                oid_str[..7].to_string()
            } else {
                oid_str.clone()
            };

            let message = commit.message().unwrap_or("").trim_end().to_string();

            let author = commit.author();
            let time = commit.time();
            let timestamp = time.seconds();
            let offset = time.offset_minutes();

            // Format time as ISO 8601 with timezone offset
            let dt = chrono::DateTime::from_timestamp(timestamp, 0)
                .unwrap_or_default()
                .naive_utc();
            let tz_sign = if offset >= 0 { '+' } else { '-' };
            let tz_hours = (offset / 60).abs();
            let tz_mins = (offset % 60).abs();
            let time_str = format!(
                "{} {}{:02}:{:02}",
                dt.format("%Y-%m-%d %H:%M:%S"),
                tz_sign,
                tz_hours,
                tz_mins
            );

            let parents: Vec<String> = commit.parent_ids().map(|p| p.to_string()).collect();

            let refs = ref_map.get(&oid_str).cloned().unwrap_or_default();

            Some(CommitInfo {
                oid: oid_str,
                short_oid,
                message,
                author: author.name().unwrap_or("").to_string(),
                email: author.email().unwrap_or("").to_string(),
                time: time_str,
                parents,
                refs,
            })
        })
        .collect();

    Ok(commits)
}

/// Build a map from commit OID to list of ref names (branches, tags).
fn build_ref_map(repo: &git2::Repository) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    // Local branches
    if let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) {
        for branch in branches.flatten() {
            let (branch_ref, _bt) = branch;
            if let Some(name) = branch_ref.name().ok().flatten() {
                if let Some(oid) = branch_ref.get().target() {
                    map.entry(oid.to_string())
                        .or_default()
                        .push(name.to_string());
                }
            }
        }
    }

    // Tags
    if let Ok(tag_names) = repo.tag_names(None) {
        for name_opt in tag_names.iter() {
            if let Some(name) = name_opt {
                if let Ok(ref_ref) = repo.find_reference(&format!("refs/tags/{}", name)) {
                    // For annotated tags, peel to the target commit
                    if let Ok(commit) = ref_ref.peel_to_commit() {
                        let oid = commit.id().to_string();
                        map.entry(oid).or_default().push(name.to_string());
                    } else if let Some(oid) = ref_ref.target() {
                        // Lightweight tag
                        map.entry(oid.to_string())
                            .or_default()
                            .push(name.to_string());
                    }
                }
            }
        }
    }

    // Remote branches
    if let Ok(branches) = repo.branches(Some(git2::BranchType::Remote)) {
        for branch in branches.flatten() {
            let (branch_ref, _bt) = branch;
            if let Some(name) = branch_ref.name().ok().flatten() {
                if let Some(oid) = branch_ref.get().target() {
                    map.entry(oid.to_string())
                        .or_default()
                        .push(name.to_string());
                }
            }
        }
    }

    map
}

/// Get all branches (local and remote) for a repository.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub is_remote: bool,
    pub is_current: bool,
    pub last_commit_oid: String,
    pub last_commit_message: String,
}

pub fn get_branches(repo_path: &Path) -> AppResult<Vec<BranchInfo>> {
    let repo = git2::Repository::open(repo_path)?;

    let current_branch: Option<String> = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()));

    let mut branches = Vec::new();

    // Local branches
    if let Ok(local_branches) = repo.branches(Some(git2::BranchType::Local)) {
        for branch in local_branches.flatten() {
            let (b, _branch_type) = branch;
            let name = b.name().ok().flatten().unwrap_or("").to_string();
            let is_current = b.is_head();
            let (oid, message) = if let Some(oid) = b.get().target() {
                let msg = repo
                    .find_commit(oid)
                    .ok()
                    .and_then(|c| c.summary().map(|s| s.to_string()))
                    .unwrap_or_default();
                (oid.to_string(), msg)
            } else {
                (String::new(), String::new())
            };

            branches.push(BranchInfo {
                name,
                is_remote: false,
                is_current,
                last_commit_oid: oid,
                last_commit_message: message,
            });
        }
    }

    // Remote branches
    if let Ok(remote_branches) = repo.branches(Some(git2::BranchType::Remote)) {
        for branch in remote_branches.flatten() {
            let (b, _branch_type) = branch;
            let name = b.name().ok().flatten().unwrap_or("").to_string();
            let is_current = current_branch
                .as_ref()
                .map(|cb| name.contains(cb.as_str()))
                .unwrap_or(false);
            let (oid, message) = if let Some(oid) = b.get().target() {
                let msg = repo
                    .find_commit(oid)
                    .ok()
                    .and_then(|c| c.summary().map(|s| s.to_string()))
                    .unwrap_or_default();
                (oid.to_string(), msg)
            } else {
                (String::new(), String::new())
            };

            branches.push(BranchInfo {
                name,
                is_remote: true,
                is_current,
                last_commit_oid: oid,
                last_commit_message: message,
            });
        }
    }

    Ok(branches)
}
