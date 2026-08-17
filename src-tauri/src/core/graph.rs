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

/// Lazy newest-first commit walk from HEAD (T-04).
///
/// Replaces libgit2's revwalk for paginated history loads: measured on a
/// 10k-commit repo, libgit2's TIME/TOPOLOGICAL-sorted revwalk costs
/// O(whole history) before emitting the first commit (~2-3 s), which defeats
/// the < 1 s graph-first-screen budget; `Sort::NONE` is lazy but ignores
/// commit times. This walk pops the newest commit from a max-heap keyed by
/// commit time and pushes parents only when a commit is emitted, so loading
/// N commits touches ~2N commit objects (~30-40 ms per 100).
///
/// Ordering semantics match `git log`: newest commit time first; a commit is
/// only discovered through one of its children, so children always precede
/// parents. Ties on identical timestamps break towards earlier discovery
/// (`Reverse(seq)`), which keeps a parent from jumping ahead of the child
/// that discovered it.
struct CommitWalk<'r> {
    repo: &'r git2::Repository,
    heap: std::collections::BinaryHeap<(i64, std::cmp::Reverse<u64>, git2::Oid)>,
    seen: std::collections::HashSet<git2::Oid>,
    seq: u64,
}

impl<'r> CommitWalk<'r> {
    fn new(repo: &'r git2::Repository) -> AppResult<Self> {
        let head = repo.head()?.peel_to_commit()?;
        let mut walk = Self {
            repo,
            heap: std::collections::BinaryHeap::new(),
            seen: std::collections::HashSet::new(),
            seq: 0,
        };
        walk.push(head.id(), head.time().seconds());
        Ok(walk)
    }

    fn push(&mut self, oid: git2::Oid, time: i64) {
        if self.seen.insert(oid) {
            self.seq += 1;
            self.heap
                .push((time, std::cmp::Reverse(self.seq), oid));
        }
    }

    /// Emit the next commit in newest-first order, queueing its parents.
    fn next_commit(&mut self) -> AppResult<Option<git2::Commit<'r>>> {
        let Some((_, _, oid)) = self.heap.pop() else {
            return Ok(None);
        };
        let commit = self.repo.find_commit(oid)?;
        let parents: Vec<git2::Oid> = commit
            .parent_ids()
            .filter(|pid| !self.seen.contains(pid))
            .collect();
        for pid in parents {
            let time = self.repo.find_commit(pid)?.time().seconds();
            self.push(pid, time);
        }
        Ok(Some(commit))
    }
}

/// Read commit history from HEAD, newest first.
///
/// `max_count` limits the number of commits returned (pagination).
/// Each commit includes its parent OIDs and any refs pointing to it.
pub fn get_commit_history(repo_path: &Path, max_count: usize) -> AppResult<Vec<CommitInfo>> {
    let repo = git2::Repository::open(repo_path)?;

    // Build a map of OID -> refs for quick lookup
    let ref_map = ref_map(&repo);

    let mut walk = CommitWalk::new(&repo)?;
    let mut commits: Vec<CommitInfo> = Vec::new();

    while commits.len() < max_count {
        let Some(commit) = walk.next_commit()? else {
            break;
        };

        let oid_str = commit.id().to_string();
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

        commits.push(CommitInfo {
            oid: oid_str,
            short_oid,
            message,
            author: author.name().unwrap_or("").to_string(),
            email: author.email().unwrap_or("").to_string(),
            time: time_str,
            parents,
            refs,
        });
    }

    Ok(commits)
}

/// Walk HEAD and return up to `max_count` commit OIDs, newest first.
/// Separated from commit parsing so the command layer can consult the DB cache
/// per OID and only parse commits that are not yet cached.
///
/// Uses the lazy heap walk (`CommitWalk`), not libgit2 revwalk — see the
/// `CommitWalk` docs for why (T-04: bounded pagination cost on big repos).
pub fn revwalk_oids(repo_path: &Path, max_count: usize) -> AppResult<Vec<String>> {
    let repo = git2::Repository::open(repo_path)?;
    let mut walk = CommitWalk::new(&repo)?;
    let mut oids: Vec<String> = Vec::new();

    while oids.len() < max_count {
        let Some(commit) = walk.next_commit()? else {
            break;
        };
        oids.push(commit.id().to_string());
    }

    Ok(oids)
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

    /// Create a commit with explicit parents and commit time (no HEAD update).
    fn commit_at(
        repo: &git2::Repository,
        dir: &Path,
        name: &str,
        content: &str,
        msg: &str,
        secs: i64,
        parents: &[&git2::Commit],
    ) -> git2::Oid {
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::new("t", "t@e.c", &git2::Time::new(secs, 0)).unwrap();
        repo.commit(None, &sig, &sig, msg, &tree, parents).unwrap()
    }

    /// Build a merge diamond: c0 root, side+main fork off c0, merge on top.
    /// Returns (dir, [merge, main, side, c0]).
    fn merge_diamond(tag: &str, base_secs: i64, equal_times: bool) -> (std::path::PathBuf, Vec<git2::Oid>) {
        let dir = std::env::temp_dir().join(format!(
            "gw_graph_walk_{}_{}",
            tag,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let repo = git2::Repository::init(&dir).unwrap();

        let t = |i: i64| if equal_times { base_secs } else { base_secs + i };
        let c0 = commit_at(&repo, &dir, "f", "0", "c0", t(0), &[]);
        let c0c = repo.find_commit(c0).unwrap();
        let side = commit_at(&repo, &dir, "s", "s", "side", t(1), &[&c0c]);
        let main = commit_at(&repo, &dir, "m", "m", "main", t(2), &[&c0c]);
        let sidec = repo.find_commit(side).unwrap();
        let mainc = repo.find_commit(main).unwrap();
        let merge = commit_at(&repo, &dir, "x", "x", "merge", t(3), &[&mainc, &sidec]);
        repo.reference("refs/heads/master", merge, true, "test").unwrap();
        repo.set_head("refs/heads/master").unwrap();

        (dir, vec![merge, main, side, c0])
    }

    /// With distinct commit times the walk must be strictly newest-first
    /// (merge → main → side → c0 for the diamond).
    #[test]
    fn walk_orders_merge_diamond_newest_first() {
        let (dir, expected) = merge_diamond("timed", 1_700_000_000, false);
        let oids = revwalk_oids(&dir, 10).unwrap();
        let expected: Vec<String> = expected.iter().map(|o| o.to_string()).collect();
        assert_eq!(oids, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With identical commit times the walk must still emit every child
    /// before its parents (graph lane rendering depends on it).
    #[test]
    fn walk_keeps_children_before_parents_on_time_ties() {
        let (dir, order) = merge_diamond("ties", 1_700_000_000, true);
        let oids = revwalk_oids(&dir, 10).unwrap();
        assert_eq!(oids.len(), 4);

        let pos = |oid: &git2::Oid| {
            oids.iter().position(|o| o == &oid.to_string()).unwrap()
        };
        let (merge, main, side, c0) = (&order[0], &order[1], &order[2], &order[3]);
        assert!(pos(merge) < pos(main), "merge must precede main parent");
        assert!(pos(merge) < pos(side), "merge must precede side parent");
        assert!(pos(main) < pos(c0) && pos(side) < pos(c0), "c0 must be last");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
