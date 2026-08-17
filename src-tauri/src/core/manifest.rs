//! Workspace Manifest (T-33): a rebuildable environment description
//! (`gitworkspace.json`) holding every repository's remote URL / default
//! branch / group / tags, plus the import-side validation and clone planning.
//!
//! Boundaries (global constraints §3/§5):
//! - Remote URL and default branch are read via libgit2 locally — **no
//!   network access** anywhere in this module. The actual batch clone runs
//!   through `TaskType::Clone` (system git CLI, task-queue concurrency and
//!   Partial Success from T-05).
//! - The manifest stores plain data only; credentials are never written —
//!   authentication follows the user's system git credential helper.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Current manifest schema version. Bump on breaking changes; the importer
/// rejects any other version with an explicit error.
pub const MANIFEST_VERSION: u32 = 1;

/// Default file name offered by the export save-dialog.
pub const MANIFEST_FILE_NAME: &str = "gitworkspace.json";

/// One repository entry inside the manifest. `path` is relative to the
/// workspace root and always uses `/` separators (cross-platform).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestRepo {
    /// Path relative to the workspace root, `/` separators.
    pub path: String,
    pub name: String,
    /// Remote URL (origin, else first remote); `None` for local-only repos —
    /// such entries cannot be cloned on import and are flagged in the plan.
    pub remote_url: Option<String>,
    /// Default branch (remote HEAD when known, else current branch).
    pub default_branch: Option<String>,
    /// Group name (from `repo_groups`); grouping is metadata only.
    pub group: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The workspace manifest document (`gitworkspace.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceManifest {
    pub version: u32,
    /// Workspace display name.
    pub name: String,
    /// RFC 3339 export timestamp.
    pub exported_at: String,
    #[serde(default)]
    pub repositories: Vec<ManifestRepo>,
}

/// What the importer will do with one manifest entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloneAction {
    /// Has a remote URL and the destination does not exist yet — clone it.
    Clone,
    /// Destination path already exists — skipped (never overwritten).
    SkipExisting,
    /// No remote URL recorded — cannot be cloned automatically.
    NoUrl,
}

/// One row of the import preview / clone plan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClonePlanItem {
    /// Relative path from the manifest (`/` separators).
    pub path: String,
    pub name: String,
    pub remote_url: Option<String>,
    pub default_branch: Option<String>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    /// Absolute clone destination (`workspace_root` + relative path).
    pub dest_path: String,
    pub action: CloneAction,
}

/// Aggregate import preview: per-repo actions plus counts for the summary
/// ("将克隆 N / 已存在跳过 M / 无 URL 不可克隆 K").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClonePlan {
    pub workspace_root: String,
    pub to_clone: usize,
    pub skip_existing: usize,
    pub no_url: usize,
    pub items: Vec<ClonePlanItem>,
}

/// Read a repository's remote URL and default branch with libgit2 (local
/// only, no network). The remote is `origin` when present, else the first
/// configured remote; the default branch comes from the remote-tracking HEAD
/// (`refs/remotes/<remote>/HEAD`) when available, else the checked-out branch.
pub fn read_remote_info(repo_path: &Path) -> (Option<String>, Option<String>) {
    let Ok(repo) = git2::Repository::open(repo_path) else {
        return (None, None);
    };

    let remote_name = if repo.find_remote("origin").is_ok() {
        Some("origin".to_string())
    } else {
        repo.remotes()
            .ok()
            .and_then(|names| names.get(0).map(str::to_string))
    };

    let Some(remote_name) = remote_name else {
        // No remote at all: still report the current branch as a hint.
        return (None, head_branch(&repo));
    };

    let remote_url = repo
        .find_remote(&remote_name)
        .ok()
        .and_then(|r| r.url().map(str::to_string));

    // Remote default branch via the remote-tracking symref, e.g.
    // refs/remotes/origin/HEAD -> refs/remotes/origin/main.
    let remote_head_ref = format!("refs/remotes/{}/HEAD", remote_name);
    let default_branch = repo
        .find_reference(&remote_head_ref)
        .ok()
        .and_then(|r| r.symbolic_target().map(str::to_string))
        .and_then(|target| {
            target
                .strip_prefix(&format!("refs/remotes/{}/", remote_name))
                .map(str::to_string)
        })
        .or_else(|| head_branch(&repo));

    (remote_url, default_branch)
}

/// Current branch shorthand (None when HEAD is detached or unborn).
fn head_branch(repo: &git2::Repository) -> Option<String> {
    match repo.head() {
        Ok(h) if h.is_branch() => h.shorthand().map(str::to_string),
        _ => None,
    }
}

/// Validate and normalize a manifest-relative path: backslashes become `/`,
/// leading/trailing slashes are stripped, and absolute paths / drive letters /
/// `.` / `..` / empty segments are rejected (path-traversal guard — entries
/// must always land strictly inside the chosen target root).
pub fn normalize_rel_path(path: &str) -> AppResult<String> {
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(AppError::Other(format!(
            "Manifest 仓库路径必须是相对路径（绝对路径）: {}",
            path
        )));
    }
    let norm = path.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if trimmed.is_empty() {
        return Err(AppError::Other("Manifest 中存在空仓库路径".to_string()));
    }
    if trimmed.contains(':') {
        return Err(AppError::Other(format!(
            "Manifest 仓库路径必须是相对路径（含盘符）: {}",
            path
        )));
    }
    for seg in trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(AppError::Other(format!(
                "Manifest 仓库路径非法（含 '{}' 段）: {}",
                seg, path
            )));
        }
    }
    Ok(trimmed.to_string())
}

/// Parse and validate manifest JSON. Rejects unknown schema versions and
/// illegal repository paths with an explicit, user-readable error.
pub fn parse_manifest(content: &str) -> AppResult<WorkspaceManifest> {
    let manifest: WorkspaceManifest = serde_json::from_str(content)
        .map_err(|e| AppError::Other(format!("Manifest 文件解析失败（不是有效的 JSON）: {}", e)))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

/// Structural validation shared by file import and IPC-passed manifests.
pub fn validate_manifest(manifest: &WorkspaceManifest) -> AppResult<()> {
    if manifest.version != MANIFEST_VERSION {
        return Err(AppError::Other(format!(
            "不支持的 Manifest 版本 {}（当前支持版本 {}）",
            manifest.version, MANIFEST_VERSION
        )));
    }
    if manifest.name.trim().is_empty() {
        return Err(AppError::Other("Manifest 缺少 workspace 名称".to_string()));
    }
    for repo in &manifest.repositories {
        normalize_rel_path(&repo.path)?;
        if repo.name.trim().is_empty() {
            return Err(AppError::Other(format!(
                "Manifest 仓库 {} 缺少名称",
                repo.path
            )));
        }
        if let Some(url) = &repo.remote_url {
            let ok = url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("ssh://")
                || url.starts_with("git://")
                || url.contains('@'); // scp-like SSH syntax: git@host:org/repo.git
            if !ok {
                return Err(AppError::Other(format!(
                    "Manifest 仓库 {} 的 remote URL 无法识别: {}",
                    repo.path, url
                )));
            }
        }
    }
    Ok(())
}

/// Serialize the manifest to the on-disk JSON form (pretty, trailing newline).
pub fn serialize_manifest(manifest: &WorkspaceManifest) -> AppResult<String> {
    let mut s = serde_json::to_string_pretty(manifest)?;
    s.push('\n');
    Ok(s)
}

/// Build the import clone plan for a target workspace root. Entries whose
/// destination already exists are never touched (Safety First); entries
/// without a remote URL are reported as not cloneable. The manifest is
/// re-validated here because it may arrive over IPC, not only from
/// `parse_manifest`.
pub fn build_clone_plan(
    manifest: &WorkspaceManifest,
    workspace_root: &Path,
) -> AppResult<ClonePlan> {
    validate_manifest(manifest)?;

    let mut items = Vec::with_capacity(manifest.repositories.len());
    let (mut to_clone, mut skip_existing, mut no_url) = (0usize, 0usize, 0usize);

    for repo in &manifest.repositories {
        let rel = normalize_rel_path(&repo.path)?;
        let dest: PathBuf = rel
            .split('/')
            .fold(workspace_root.to_path_buf(), |acc, seg| acc.join(seg));

        let action = match &repo.remote_url {
            None => CloneAction::NoUrl,
            Some(_) if dest.exists() => CloneAction::SkipExisting,
            Some(_) => CloneAction::Clone,
        };
        match action {
            CloneAction::Clone => to_clone += 1,
            CloneAction::SkipExisting => skip_existing += 1,
            CloneAction::NoUrl => no_url += 1,
        }

        items.push(ClonePlanItem {
            path: rel,
            name: repo.name.clone(),
            remote_url: repo.remote_url.clone(),
            default_branch: repo.default_branch.clone(),
            group: repo.group.clone(),
            tags: repo.tags.clone(),
            dest_path: dest.to_string_lossy().to_string(),
            action,
        });
    }

    Ok(ClonePlan {
        workspace_root: workspace_root.to_string_lossy().to_string(),
        to_clone,
        skip_existing,
        no_url,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gw_manifest_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Init a repo with one commit on `main` and an `origin` remote whose
    /// remote-tracking HEAD points at `origin/main`.
    fn init_repo_with_remote(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let sig = git2::Signature::now("tester", "t@example.com").unwrap();
        let oid = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        repo.remote("origin", "https://example.com/org/repo.git")
            .unwrap();
        repo.reference("refs/remotes/origin/main", oid, false, "test")
            .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "test",
        )
        .unwrap();
    }

    fn sample_manifest() -> WorkspaceManifest {
        WorkspaceManifest {
            version: MANIFEST_VERSION,
            name: "ws".into(),
            exported_at: "2026-08-17T00:00:00Z".into(),
            repositories: vec![
                ManifestRepo {
                    path: "apps/web".into(),
                    name: "web".into(),
                    remote_url: Some("https://example.com/org/web.git".into()),
                    default_branch: Some("main".into()),
                    group: Some("前端".into()),
                    tags: vec!["vue".into()],
                },
                ManifestRepo {
                    path: "libs/core".into(),
                    name: "core".into(),
                    remote_url: None,
                    default_branch: None,
                    group: None,
                    tags: vec![],
                },
            ],
        }
    }

    #[test]
    fn manifest_json_roundtrip_uses_camel_case() {
        let m = sample_manifest();
        let json = serialize_manifest(&m).unwrap();
        assert!(json.contains("\"remoteUrl\""));
        assert!(json.contains("\"defaultBranch\""));
        assert!(json.contains("\"exportedAt\""));
        let back = parse_manifest(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn parse_rejects_unknown_version() {
        let mut m = sample_manifest();
        m.version = 99;
        let err = parse_manifest(&serialize_manifest(&m).unwrap()).unwrap_err();
        assert!(err.to_string().contains("不支持的 Manifest 版本"));
    }

    #[test]
    fn parse_rejects_traversal_and_absolute_paths() {
        for bad in ["../evil", "a/../../b", "C:/abs/repo", "/abs/repo", "a//b", ""] {
            let mut m = sample_manifest();
            m.repositories[0].path = bad.to_string();
            let err = parse_manifest(&serialize_manifest(&m).unwrap());
            assert!(err.is_err(), "path {:?} must be rejected", bad);
        }
    }

    #[test]
    fn parse_rejects_unrecognized_remote_url() {
        let mut m = sample_manifest();
        m.repositories[0].remote_url = Some("not a url".into());
        let err = parse_manifest(&serialize_manifest(&m).unwrap()).unwrap_err();
        assert!(err.to_string().contains("remote URL"));
        // scp-like SSH syntax is accepted.
        let mut m = sample_manifest();
        m.repositories[0].remote_url = Some("git@example.com:org/web.git".into());
        assert!(parse_manifest(&serialize_manifest(&m).unwrap()).is_ok());
    }

    #[test]
    fn normalize_rel_path_normalizes_separators() {
        assert_eq!(normalize_rel_path("apps\\web\\").unwrap(), "apps/web");
        assert_eq!(normalize_rel_path("apps/web/").unwrap(), "apps/web");
        assert!(normalize_rel_path("/abs/repo").is_err());
        assert!(normalize_rel_path("\\\\server\\share").is_err());
    }

    #[test]
    fn clone_plan_classifies_clone_skip_and_no_url() {
        let root = tmpdir("plan");
        // Pre-create the destination of a third entry to force SkipExisting.
        let existing = root.join("tools").join("cli");
        std::fs::create_dir_all(&existing).unwrap();

        let mut m = sample_manifest();
        m.repositories.push(ManifestRepo {
            path: "tools/cli".into(),
            name: "cli".into(),
            remote_url: Some("https://example.com/org/cli.git".into()),
            default_branch: Some("main".into()),
            group: None,
            tags: vec![],
        });

        let plan = build_clone_plan(&m, &root).unwrap();
        assert_eq!(plan.to_clone, 1);
        assert_eq!(plan.skip_existing, 1);
        assert_eq!(plan.no_url, 1);

        let by_name = |n: &str| plan.items.iter().find(|i| i.name == n).unwrap();
        assert_eq!(by_name("web").action, CloneAction::Clone);
        assert_eq!(by_name("core").action, CloneAction::NoUrl);
        assert_eq!(by_name("cli").action, CloneAction::SkipExisting);
        // Destination = root + relative path.
        assert_eq!(
            PathBuf::from(&by_name("web").dest_path),
            root.join("apps").join("web")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_remote_info_reads_url_and_default_branch_without_network() {
        let dir = tmpdir("remote");
        init_repo_with_remote(&dir);
        let (url, branch) = read_remote_info(&dir);
        assert_eq!(url.as_deref(), Some("https://example.com/org/repo.git"));
        assert_eq!(branch.as_deref(), Some("main"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_remote_info_handles_missing_remote_and_missing_repo() {
        // Repo without any remote: no URL, current branch still reported.
        let dir = tmpdir("noremote");
        {
            let repo = git2::Repository::init(&dir).unwrap();
            std::fs::write(dir.join("a.txt"), "x").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("a.txt")).unwrap();
            index.write().unwrap();
            let tree_oid = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = git2::Signature::now("tester", "t@example.com").unwrap();
            repo.commit(Some("refs/heads/trunk"), &sig, &sig, "init", &tree, &[])
                .unwrap();
            repo.set_head("refs/heads/trunk").unwrap();
        }

        let (url, branch) = read_remote_info(&dir);
        assert_eq!(url, None);
        assert_eq!(branch.as_deref(), Some("trunk"));

        // Non-repository path: both absent, no panic.
        let empty = tmpdir("empty");
        assert_eq!(read_remote_info(&empty), (None, None));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
