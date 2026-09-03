use std::path::Path;

use serde::Deserialize;
use tauri::State;

use crate::core::diff::{self, DiffConfig, FileDiff};
use crate::core::stage;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, DiffCacheKey};

/// Diff rendering options from the UI (Roadmap §9 diff settings).
#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DiffOptionsParam {
    pub ignore_whitespace: bool,
    pub ignore_whitespace_eol: bool,
    pub ignore_case: bool,
}

impl DiffOptionsParam {
    fn to_config(&self) -> DiffConfig {
        DiffConfig {
            ignore_whitespace: self.ignore_whitespace,
            ignore_whitespace_eol: self.ignore_whitespace_eol,
            ignore_case: self.ignore_case,
        }
    }
}

/// Get the working directory diff for a repository.
///
/// Computes the diff between HEAD and the working tree (including index).
/// Returns a list of changed files, each with their hunks and line-level details.
///
/// For repositories with no commits, all files appear as "added".
#[tauri::command]
pub fn get_diff(repo_path: String, options: Option<DiffOptionsParam>) -> AppResult<Vec<FileDiff>> {
    let opt = options.unwrap_or_default();
    diff::get_workdir_diff_with_config(Path::new(&repo_path), &opt.to_config())
}

/// Get unstaged changes only (index → workdir, includes untracked).
///
/// This is the diff that "Stage hunk/line" operates on (T-12): hunk/line
/// indices returned here are valid inputs to `stage_hunk` / `stage_lines`
/// when the default diff options are used.
#[tauri::command]
pub fn get_unstaged_diff(repo_path: String, options: Option<DiffOptionsParam>) -> AppResult<Vec<FileDiff>> {
    let opt = options.unwrap_or_default();
    diff::get_unstaged_diff_with_config(Path::new(&repo_path), &opt.to_config())
}

/// Get staged changes only (HEAD tree → index), matching `git diff --cached`.
///
/// Hunk/line indices returned here are valid inputs to `unstage_hunk` /
/// `unstage_lines` when the default diff options are used.
#[tauri::command]
pub fn get_staged_diff(repo_path: String, options: Option<DiffOptionsParam>) -> AppResult<Vec<FileDiff>> {
    let opt = options.unwrap_or_default();
    diff::get_staged_diff_with_config(Path::new(&repo_path), &opt.to_config())
}

/// Diff between two arbitrary revisions (T-12 双点 Diff): branch / tag /
/// commit specs, e.g. `main` vs `feature`, `v1.0` vs `v1.1`, or two oids.
///
/// Results are cached in a bounded LRU keyed by resolved tree oids
/// (T-04: same pair viewed twice is served from cache).
#[tauri::command]
pub fn get_revision_diff(
    repo_path: String,
    base: String,
    other: String,
    options: Option<DiffOptionsParam>,
    state: State<'_, AppState>,
) -> AppResult<Vec<FileDiff>> {
    let config = options.unwrap_or_default().to_config();
    let repo = git2::Repository::open(&repo_path)?;
    let old_tree = repo.revparse_single(&base)?.peel_to_tree()?;
    let new_tree = repo.revparse_single(&other)?.peel_to_tree()?;
    cached_tree_diff(&state.diff_cache, &repo, &repo_path, &old_tree, &new_tree, &config)
}

/// Diff of a single commit (T-12 Commit Diff): first parent → commit
/// (empty tree → commit for a root commit). Cached like `get_revision_diff`.
#[tauri::command]
pub fn get_commit_diff(
    repo_path: String,
    oid: String,
    options: Option<DiffOptionsParam>,
    state: State<'_, AppState>,
) -> AppResult<Vec<FileDiff>> {
    let config = options.unwrap_or_default().to_config();
    let repo = git2::Repository::open(&repo_path)?;
    let commit = repo.revparse_single(&oid)?.peel_to_commit()?;
    let new_tree = commit.tree()?;
    let old_tree = if commit.parent_count() > 0 {
        commit.parent(0)?.tree()?
    } else {
        let empty = repo.treebuilder(None)?.write()?;
        repo.find_tree(empty)?
    };
    cached_tree_diff(&state.diff_cache, &repo, &repo_path, &old_tree, &new_tree, &config)
}

/// Compute a tree↔tree diff, serving repeated views from the bounded LRU
/// revision-diff cache (T-04 acceptance: second view of the same pair is a
/// cache hit; measured by T-07 benchmark).
///
/// Takes the cache directly (instead of `AppState`) so the T-07 benchmark
/// harness can exercise the exact command-path logic without a Tauri runtime.
pub(crate) fn cached_tree_diff(
    cache: &moka::sync::Cache<DiffCacheKey, Vec<FileDiff>>,
    repo: &git2::Repository,
    repo_path: &str,
    old_tree: &git2::Tree,
    new_tree: &git2::Tree,
    config: &DiffConfig,
) -> AppResult<Vec<FileDiff>> {
    let key = DiffCacheKey {
        repo_path: repo_path.to_string(),
        old_oid: old_tree.id().to_string(),
        new_oid: new_tree.id().to_string(),
        flags: config.bits(),
    };
    if let Some(hit) = cache.get(&key) {
        return Ok(hit);
    }
    let files = diff::diff_trees_with_config(repo, old_tree, new_tree, config)?;
    cache.insert(key, files.clone());
    Ok(files)
}

/// Stage one hunk of a file's unstaged changes (T-12; `git add -p` hunk
/// semantics). `hunk_index` refers to `get_unstaged_diff` with default
/// options. Untracked files have no hunks — stage them whole via batch_add.
#[tauri::command]
pub fn stage_hunk(repo_path: String, file_path: String, hunk_index: u32) -> AppResult<()> {
    stage::stage_hunk(Path::new(&repo_path), &file_path, hunk_index as usize)
}

/// Unstage one hunk of a file's staged changes (index moves back towards
/// HEAD). `hunk_index` refers to `get_staged_diff` with default options.
#[tauri::command]
pub fn unstage_hunk(repo_path: String, file_path: String, hunk_index: u32) -> AppResult<()> {
    stage::unstage_hunk(Path::new(&repo_path), &file_path, hunk_index as usize)
}

/// Stage only the selected lines of one hunk (indices into the hunk's line
/// list as returned by `get_unstaged_diff` with default options).
#[tauri::command]
pub fn stage_lines(repo_path: String, file_path: String, hunk_index: u32, line_indices: Vec<u32>) -> AppResult<()> {
    if line_indices.is_empty() {
        return Err(AppError::Other("未选择任何行".to_string()));
    }
    stage::stage_lines(Path::new(&repo_path), &file_path, hunk_index as usize, &line_indices)
}

/// Unstage only the selected lines of one staged hunk (indices into the
/// hunk's line list as returned by `get_staged_diff` with default options).
#[tauri::command]
pub fn unstage_lines(repo_path: String, file_path: String, hunk_index: u32, line_indices: Vec<u32>) -> AppResult<()> {
    if line_indices.is_empty() {
        return Err(AppError::Other("未选择任何行".to_string()));
    }
    stage::unstage_lines(Path::new(&repo_path), &file_path, hunk_index as usize, &line_indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::build_diff_cache;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_diffcmd_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn commit_file(repo: &git2::Repository, dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        match &parent {
            Some(p) => repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[p]).unwrap(),
            None => repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[]).unwrap(),
        };
    }

    /// The revision-diff cache must serve a repeated (old, new, flags) view
    /// without recomputing (T-04: second view < 50 ms via cache hit).
    #[test]
    fn revision_diff_cache_hit_returns_identical_result() {
        let dir = tmpdir("cache");
        let repo = git2::Repository::init(&dir).unwrap();
        commit_file(&repo, &dir, "a.txt", "one\ntwo\n", "c1");
        commit_file(&repo, &dir, "a.txt", "one\nTWO\n", "c2");

        let old_tree = repo.revparse_single("HEAD~1").unwrap().peel_to_tree().unwrap();
        let new_tree = repo.revparse_single("HEAD").unwrap().peel_to_tree().unwrap();
        let config = DiffConfig::default();
        let cache = build_diff_cache();

        let key = DiffCacheKey {
            repo_path: dir.to_string_lossy().to_string(),
            old_oid: old_tree.id().to_string(),
            new_oid: new_tree.id().to_string(),
            flags: config.bits(),
        };

        let first = diff::diff_trees_with_config(&repo, &old_tree, &new_tree, &config).unwrap();
        cache.insert(key.clone(), first.clone());

        let hit = cache.get(&key).expect("second view must be a cache hit");
        assert_eq!(hit.len(), first.len());
        assert_eq!(hit[0].new_path, "a.txt");
        assert!(hit[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.line_type == "add" && l.content == "TWO"));

        // T-04 acceptance proxy: a cache hit must be far below the 50 ms
        // "second view" budget. 1000 hits (each cloning the payload) in well
        // under 50 ms proves the hit path; the full T-07 harness measurement
        // tracks the end-to-end number separately.
        let start = std::time::Instant::now();
        for _ in 0..1000 {
            std::hint::black_box(cache.get(&key));
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 50,
            "1000 cache hits took {elapsed:?}, expected < 50ms"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Commit diff of a root commit uses the empty tree as the old side, so
    /// every file shows up as added with real hunks.
    #[test]
    fn commit_diff_handles_root_commit() {
        let dir = tmpdir("root");
        let repo = git2::Repository::init(&dir).unwrap();
        commit_file(&repo, &dir, "a.txt", "hello\n", "root");

        let files = diff::diff_commit(&repo, "HEAD").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "added");
        assert!(files[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.line_type == "add" && l.content == "hello"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Commit diff of a regular commit equals parent→commit.
    #[test]
    fn commit_diff_matches_parent_to_commit() {
        let dir = tmpdir("commit");
        let repo = git2::Repository::init(&dir).unwrap();
        commit_file(&repo, &dir, "a.txt", "one\n", "c1");
        commit_file(&repo, &dir, "a.txt", "one\ntwo\n", "c2");

        let files = diff::diff_commit(&repo, "HEAD").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "modified");
        let adds: Vec<_> = files[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == "add")
            .collect();
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0].content, "two");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Staged diff shows index changes; unstaged diff shows the rest —
    /// together they must partition the combined workdir diff (T-12 split
    /// view, matching `git diff --cached` / `git diff`).
    #[test]
    fn staged_and_unstaged_diffs_partition_changes() {
        let dir = tmpdir("split");
        let repo = git2::Repository::init(&dir).unwrap();
        commit_file(&repo, &dir, "a.txt", "one\ntwo\nthree\n", "c1");
        drop(repo);

        // Stage one change, leave another unstaged.
        std::fs::write(dir.join("a.txt"), "ONE\ntwo\nthree\n").unwrap();
        {
            let repo = git2::Repository::open(&dir).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
        }
        std::fs::write(dir.join("a.txt"), "ONE\ntwo\nTHREE\n").unwrap();

        let staged = diff::get_staged_diff_with_config(&dir, &DiffConfig::default()).unwrap();
        assert_eq!(staged.len(), 1);
        let staged_adds: Vec<_> = staged[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == "add")
            .map(|l| l.content.clone())
            .collect();
        assert_eq!(staged_adds, vec!["ONE"]);

        let unstaged = diff::get_unstaged_diff_with_config(&dir, &DiffConfig::default()).unwrap();
        assert_eq!(unstaged.len(), 1);
        let unstaged_adds: Vec<_> = unstaged[0]
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.line_type == "add")
            .map(|l| l.content.clone())
            .collect();
        assert_eq!(unstaged_adds, vec!["THREE"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
