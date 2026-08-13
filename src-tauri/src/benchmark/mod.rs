//! Standalone benchmark: generate synthetic repositories and measure scan /
//! status timings. Run via `cargo run --release --example benchmark -- <count>`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::core::git_status;
use crate::core::scanner::RepoScanner;

/// Generate `count` synthetic Git repositories under `root`.
/// Each has 3 commits and a handful of source files.
pub fn generate_repos(root: &Path, count: usize) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(root)?;
    let mut paths = Vec::with_capacity(count);

    for i in 0..count {
        let dir = root.join(format!("repo_{:04}", i));
        let repo = git2::Repository::init(&dir).map_err(io_err)?;

        for c in 0..3 {
            let rel = format!("src/file_{}.txt", c);
            let file = dir.join(&rel);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, format!("content {} commit {}\n", c, c))?;

            let mut index = repo.index().map_err(io_err)?;
            index.add_path(Path::new(&rel)).map_err(io_err)?;
            index.write().map_err(io_err)?;
            let tree_oid = index.write_tree().map_err(io_err)?;
            let tree = repo.find_tree(tree_oid).map_err(io_err)?;
            let sig = git2::Signature::now("bench", "bench@example.com").map_err(io_err)?;

            let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
            let parents: Vec<git2::Commit> = head_commit.into_iter().collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();

            repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("commit {}", c),
                &tree,
                &parent_refs,
            )
            .map_err(io_err)?;
        }
        paths.push(dir);
    }
    Ok(paths)
}

fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

/// Run the benchmark and return a text report.
pub fn run(count: usize) -> String {
    let tmp = std::env::temp_dir().join(format!("gw_bench_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    let gen_start = Instant::now();
    let repos = match generate_repos(&tmp, count) {
        Ok(r) => r,
        Err(e) => return format!("generate_repos failed: {}", e),
    };
    let gen_ms = gen_start.elapsed().as_millis();

    // Initial scan (depth 5)
    let scanner = RepoScanner::new(5);
    let scan_start = Instant::now();
    let found = scanner.scan(&tmp);
    let scan_ms = scan_start.elapsed().as_millis();

    // Status refresh (sequential baseline)
    let status_start = Instant::now();
    let mut ok = 0usize;
    for r in &repos {
        if git_status::get_repo_status(r).is_ok() {
            ok += 1;
        }
    }
    let status_ms = status_start.elapsed().as_millis();

    let mut report = String::new();
    report.push_str(&format!("## Benchmark: {} repositories\n\n", count));
    report.push_str(&format!("- Generate: {} ms\n", gen_ms));
    report.push_str(&format!(
        "- Initial scan (depth 5): {} ms (found {})\n",
        scan_ms,
        found.len()
    ));
    report.push_str(&format!(
        "- Status refresh (sequential): {} ms ({} ok)\n",
        status_ms, ok
    ));
    report.push_str(&format!(
        "- Per-repo status avg: {:.2} ms\n",
        status_ms as f64 / count.max(1) as f64
    ));

    let _ = std::fs::remove_dir_all(&tmp);
    report
}
