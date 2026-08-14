use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::error::AppResult;
use crate::models::repository::CommitRecord;

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
    let ref_map = ref_map(&repo);

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
            let time_str = format_commit_time(time.seconds(), time.offset_minutes());

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

/// Walk HEAD and return up to `max_count` commit OIDs in topological order.
/// Separated from commit parsing so the command layer can consult the DB cache
/// per OID and only parse commits that are not yet cached.
pub fn revwalk_oids(repo_path: &Path, max_count: usize) -> AppResult<Vec<String>> {
    let repo = git2::Repository::open(repo_path)?;
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

    Ok(revwalk
        .take(max_count)
        .filter_map(|r| r.ok())
        .map(|oid| oid.to_string())
        .collect())
}

/// Build a persistable `CommitRecord` from a commit object (for the DB cache).
pub fn commit_record_from_oid(repo: &git2::Repository, oid: &git2::Oid) -> Option<CommitRecord> {
    let commit = repo.find_commit(*oid).ok()?;
    let author = commit.author();
    let committer = commit.committer();
    let committed_at = committer.when().seconds();
    Some(CommitRecord {
        oid: oid.to_string(),
        message: commit.message().unwrap_or("").trim_end().to_string(),
        author: format!(
            "{} <{}>",
            author.name().unwrap_or(""),
            author.email().unwrap_or("")
        ),
        committer: format!(
            "{} <{}>",
            committer.name().unwrap_or(""),
            committer.email().unwrap_or("")
        ),
        authored_at: commit.time().seconds(),
        committed_at,
        offset_minutes: commit.time().offset_minutes(),
        parents: commit.parent_ids().map(|p| p.to_string()).collect(),
    })
}

/// Format a unix timestamp (+ optional tz offset in minutes) as a readable string.
pub(crate) fn format_commit_time(seconds: i64, offset_minutes: i32) -> String {
    let dt = chrono::DateTime::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .naive_utc();
    let tz_sign = if offset_minutes >= 0 { '+' } else { '-' };
    let tz_hours = (offset_minutes / 60).abs();
    let tz_mins = (offset_minutes % 60).abs();
    format!(
        "{} {}{:02}:{:02}",
        dt.format("%Y-%m-%d %H:%M:%S"),
        tz_sign,
        tz_hours,
        tz_mins
    )
}

/// Split a git `Name <email>` author string into its two parts.
fn parse_author(author: &str) -> (String, String) {
    if let Some(lt) = author.rfind('<') {
        let name = author[..lt].trim().to_string();
        let email = author[lt + 1..].trim_end_matches('>').trim().to_string();
        (name, email)
    } else {
        (author.to_string(), String::new())
    }
}

/// Convert a cached `CommitRecord` back into a `CommitInfo` (refs supplied by
/// the caller, since branch/tag refs are dynamic and not cached).
pub fn commit_info_from_record(record: &CommitRecord, refs: Vec<String>) -> CommitInfo {
    let (author, email) = parse_author(&record.author);
    CommitInfo {
        oid: record.oid.clone(),
        short_oid: if record.oid.len() >= 7 {
            record.oid[..7].to_string()
        } else {
            record.oid.clone()
        },
        message: record.message.clone(),
        author,
        email,
        time: format_commit_time(record.authored_at, record.offset_minutes),
        parents: record.parents.clone(),
        refs,
    }
}

/// Build a map from commit OID to list of ref names (branches, tags).
pub fn ref_map(repo: &git2::Repository) -> HashMap<String, Vec<String>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        for c in 0..2 {
            let rel = format!("f{}.txt", c);
            std::fs::write(dir.join(&rel), format!("content {}", c)).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new(&rel)).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = git2::Signature::now("tester", "t@example.com").unwrap();
            let head = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
            let parents: Vec<git2::Commit> = head.into_iter().collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
            repo.commit(Some("HEAD"), &sig, &sig, &format!("msg {}", c), &tree, &parent_refs)
                .unwrap();
        }
        repo
    }

    #[test]
    fn commit_record_roundtrip_preserves_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "gw_graph_test_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let repo = init_repo(&dir);

        let oids = revwalk_oids(&dir, 10).unwrap();
        assert_eq!(oids.len(), 2);

        // Newest commit first.
        let oid = git2::Oid::from_str(&oids[0]).unwrap();
        let record = commit_record_from_oid(&repo, &oid).unwrap();
        let info = commit_info_from_record(&record, vec!["main".to_string()]);

        assert_eq!(info.oid, record.oid);
        assert_eq!(info.message, "msg 1");
        assert_eq!(info.author, "tester");
        assert_eq!(info.email, "t@example.com");
        assert_eq!(info.refs, vec!["main".to_string()]);
        assert_eq!(info.parents.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
