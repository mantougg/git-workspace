//! 提交热力图聚合（F-01b）：当前用户（git config `user.email` / `user.name`）
//! 在 workspace 内所有仓库的提交按天计数。
//!
//! 全程只读本地仓库（git2 revwalk，TIME 排序，遇到早于截止日期的提交即停），
//! 无网络、无子进程（全局约束 §10 / 平台规范 §3）。

use std::collections::BTreeMap;
use std::path::Path;

use rayon::prelude::*;
use serde::Serialize;

/// 热力图响应：按天计数 + 用于展示的身份标识。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitHeatmap {
    /// 匹配到的提交者标识（email 优先，其次 name）；未配置 git 身份时为 None。
    pub identity: Option<String>,
    pub days: Vec<HeatmapDay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapDay {
    /// `YYYY-MM-DD`（按提交作者时区）。
    pub date: String,
    pub count: u32,
}

/// 读取本机 git 身份（user.email / user.name，global config）。
fn user_identity() -> (Option<String>, Option<String>) {
    let config = git2::Config::open_default().ok();
    let email = config
        .as_ref()
        .and_then(|c| c.get_string("user.email").ok())
        .filter(|v| !v.trim().is_empty());
    let name = config
        .as_ref()
        .and_then(|c| c.get_string("user.name").ok())
        .filter(|v| !v.trim().is_empty());
    (name, email)
}

/// 汇总一组仓库在 `since`（Unix 秒）之后、当前用户的按天提交数。
pub fn workspace_heatmap(repo_paths: &[String], since: i64) -> CommitHeatmap {
    let (name, email) = user_identity();
    let identity = email.clone().or_else(|| name.clone());
    if identity.is_none() {
        return CommitHeatmap {
            identity: None,
            days: vec![],
        };
    }

    let per_repo: Vec<BTreeMap<String, u32>> = repo_paths
        .par_iter()
        .map(|path| repo_commit_counts(Path::new(path), name.as_deref(), email.as_deref(), since))
        .collect();

    let mut totals: BTreeMap<String, u32> = BTreeMap::new();
    for counts in per_repo {
        for (date, count) in counts {
            *totals.entry(date).or_insert(0) += count;
        }
    }
    CommitHeatmap {
        identity,
        days: totals
            .into_iter()
            .map(|(date, count)| HeatmapDay { date, count })
            .collect(),
    }
}

/// 单仓库按天提交数。email 优先匹配（大小写不敏感），无 email 时按 name。
fn repo_commit_counts(
    repo_path: &Path,
    name: Option<&str>,
    email: Option<&str>,
    since: i64,
) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    let Ok(repo) = git2::Repository::open(repo_path) else {
        return counts;
    };
    let Ok(mut walk) = repo.revwalk() else {
        return counts;
    };
    let _ = walk.set_sorting(git2::Sort::TIME);
    if walk.push_head().is_err() {
        return counts;
    }
    for oid in walk.flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let time = commit.time();
        if time.seconds() < since {
            // TIME 排序：最新在前，遇到早于截止日期的即可停止。
            break;
        }
        let author = commit.author();
        let matches = match email {
            Some(email) => author
                .email()
                .map(|candidate| candidate.eq_ignore_ascii_case(email))
                .unwrap_or(false),
            None => name
                .and_then(|name| author.name().map(|candidate| candidate == name))
                .unwrap_or(false),
        };
        if !matches {
            continue;
        }
        // 按作者本地时区取日期（提交时间自带 offset）。
        let offset = chrono::FixedOffset::east_opt(time.offset_minutes() * 60)
            .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
        let date = chrono::DateTime::from_timestamp(time.seconds(), 0)
            .map(|dt| dt.with_timezone(&offset).date_naive().to_string());
        if let Some(date) = date {
            *counts.entry(date).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(repo: &git2::Repository, email: &str, name: &str, when: i64) {
        let sig = git2::Signature::new(name, email, &git2::Time::new(when, 8 * 60)).unwrap();
        let mut index = repo.index().unwrap();
        let tree_oid = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        let parents: Vec<git2::Commit> = match repo.head() {
            Ok(head) => vec![head.peel_to_commit().unwrap()],
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "commit",
            &tree,
            &parent_refs,
        )
        .unwrap();
    }

    #[test]
    fn counts_only_matching_author_and_recent_commits() {
        let dir = std::env::temp_dir().join(format!(
            "gw_heatmap_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let repo = git2::Repository::init(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();

        let now = chrono::Utc::now().timestamp();
        // 当前用户两笔今天提交（+08:00 时区）、别人一笔、一年前一笔。
        commit(&repo, "me@example.com", "me", now);
        commit(&repo, "me@example.com", "me", now);
        commit(&repo, "other@example.com", "other", now);
        commit(&repo, "me@example.com", "me", now - 400 * 24 * 3600);

        let since = now - 365 * 24 * 3600;
        let counts = repo_commit_counts(&dir, None, Some("me@example.com"), since);
        let total: u32 = counts.values().sum();
        assert_eq!(total, 2, "只统计最近一年且 email 匹配的提交: {counts:?}");
        assert_eq!(counts.len(), 1, "两笔提交在同一天: {counts:?}");

        // name 兜底匹配
        let by_name = repo_commit_counts(&dir, Some("other"), None, since);
        assert_eq!(by_name.values().sum::<u32>(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
