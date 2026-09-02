//! Remote Platform 命令（T-29）。
//!
//! - `detect_remote`：读 origin → 平台识别
//! - `remote_open_url`：Open Repository/Issue/PR/CI 的 URL（前端经 shell
//!   plugin 打开浏览器，与现有 openPath 模式一致）
//! - `create_pull_request` / `get_ci_status`：平台 REST（异步，不阻塞）
//! - token：OS Credential Store（keyring，不落盘明文）→ 系统
//!   `git credential fill` 回退（系统 git 凭据助手）

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::ai::credentials::{CredentialStore, KeyringStore};
use crate::error::{AppError, AppResult};
use crate::remote::api::{self, CiStatus, CreatePrInput, PrResult};
use crate::remote::platform::{parse_remote_url, OpenTarget, RemoteRepo};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub platform: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
    pub url: String,
}

/// 读 origin 的 URL（git2 只读，不跨线程持有句柄）。
fn origin_url(repo_path: &str) -> AppResult<String> {
    let repo = git2::Repository::open(Path::new(repo_path)).map_err(|e| AppError::Git(e))?;
    let remote = repo
        .find_remote("origin")
        .map_err(|_| AppError::NotFound(format!("{repo_path} 未配置 origin 远程")))?;
    let url = remote
        .url()
        .map(|u| u.to_string())
        .or_else(|| remote.pushurl().map(|u| u.to_string()))
        .ok_or_else(|| AppError::NotFound("origin 远程没有配置 URL".to_string()))?;
    Ok(url)
}

fn detect(repo_path: &str) -> AppResult<RemoteRepo> {
    let url = origin_url(repo_path)?;
    parse_remote_url(&url)
        .ok_or_else(|| AppError::NotFound(format!("无法识别 origin 远程平台：{url}")))
}

/// 平台识别。
#[tauri::command]
pub fn detect_remote(repo_path: String) -> AppResult<RemoteInfo> {
    let r = detect(&repo_path)?;
    Ok(RemoteInfo {
        platform: r.platform.id().to_string(),
        host: r.host.clone(),
        owner: r.owner.clone(),
        repo: r.repo.clone(),
        url: r.open_url(&OpenTarget::Repo),
    })
}

/// 构造 Open URL；`target` 形如 `repo` / `issues` / `pulls` / `ci` /
/// `new-pr:source..target` / `pull:7` / `issue:3`。
#[tauri::command]
pub fn remote_open_url(repo_path: String, target: String) -> AppResult<String> {
    let r = detect(&repo_path)?;
    let open = if let Some(rest) = target.strip_prefix("new-pr:") {
        let (source, target_branch) = rest
            .split_once("..")
            .ok_or_else(|| AppError::Other("new-pr 目标格式应为 source..target".to_string()))?;
        OpenTarget::NewPullRequest {
            source: source.to_string(),
            target: target_branch.to_string(),
        }
    } else if let Some(n) = target.strip_prefix("pull:") {
        OpenTarget::Pull(
            n.parse()
                .map_err(|_| AppError::Other("PR 编号不合法".to_string()))?,
        )
    } else if let Some(n) = target.strip_prefix("issue:") {
        OpenTarget::Issue(
            n.parse()
                .map_err(|_| AppError::Other("Issue 编号不合法".to_string()))?,
        )
    } else {
        match target.as_str() {
            "repo" => OpenTarget::Repo,
            "issues" => OpenTarget::Issues,
            "pulls" => OpenTarget::Pulls,
            "ci" => OpenTarget::Ci,
            other => {
                return Err(AppError::Other(format!("未知的远程打开目标：{other}")));
            }
        }
    };
    Ok(r.open_url(&open))
}

// ---------------------------------------------------------------------------
// 凭据（OS Credential Store → git credential fill）
// ---------------------------------------------------------------------------

fn keyring_ref(platform: &str, host: &str) -> String {
    format!("remote:{platform}:{host}")
}

/// 平台 token 解析链：keyring（`remote:{platform}:{host}`）→ 系统 git 凭据。
#[tauri::command]
pub fn resolve_remote_token(platform: String, host: String) -> AppResult<Option<String>> {
    // 1) OS Credential Store
    let from_store = KeyringStore::new()
        .get(&keyring_ref(&platform, &host))
        .map_err(|e| AppError::Other(format!("凭证读取失败：{e}")))?;
    if from_store.is_some() {
        return Ok(from_store);
    }
    // 2) 系统 git 凭据助手（异步子进程，阻塞此处可接受——亚秒级）
    Ok(git_credential_fill(&host)?)
}

/// 保存 token 到 OS Credential Store（不落盘明文）。
#[tauri::command]
pub fn save_remote_token(platform: String, host: String, token: String) -> AppResult<()> {
    KeyringStore::new()
        .set(&keyring_ref(&platform, &host), &token)
        .map_err(|e| AppError::Other(format!("凭证保存失败：{e}")))
}

/// 删除已保存的 token。
#[tauri::command]
pub fn delete_remote_token(platform: String, host: String) -> AppResult<()> {
    KeyringStore::new()
        .delete(&keyring_ref(&platform, &host))
        .map_err(|e| AppError::Other(format!("凭证删除失败：{e}")))
}

/// `git credential fill`：交给系统凭据助手解析 host 的密码（= token）。
fn git_credential_fill(host: &str) -> AppResult<Option<String>> {
    let mut child = Command::new("git")
        .args(["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Other(format!("git credential 启动失败：{e}")))?;
    let input = format!("protocol=https\nhost={host}\n\n");
    child
        .stdin
        .take()
        .ok_or_else(|| AppError::Other("git credential stdin 不可用".to_string()))?
        .write_all(input.as_bytes())
        .map_err(|e| AppError::Other(format!("git credential 写入失败：{e}")))?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut username_none = false;
    for line in text.lines() {
        if let Some(password) = line.strip_prefix("password=") {
            if password.is_empty() {
                return Ok(None);
            }
            let _ = username_none;
            return Ok(Some(password.to_string()));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Create PR / CI
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_pull_request(
    repo_path: String,
    source: String,
    target: String,
    title: String,
    body: String,
    draft: bool,
) -> AppResult<PrResult> {
    // 平台识别与凭据解析不跨 await 持锁；HTTP 在线程池外发起。
    let (remote, token) = {
        let r = detect(&repo_path)?;
        let platform = r.platform.id().to_string();
        let host = r.host.clone();
        // spawn_blocking：git credential fill 可能拉起系统助手
        let token = tauri::async_runtime::spawn_blocking(move || {
            resolve_remote_token_inner(&platform, &host)
        })
        .await
        .map_err(|e| AppError::Other(format!("凭据解析任务失败：{e}")))??;
        (r, token)
    };
    api::create_pull_request(
        &remote,
        token.as_deref(),
        &CreatePrInput {
            source,
            target,
            title,
            body,
            draft,
        },
    )
    .await
}

fn resolve_remote_token_inner(platform: &str, host: &str) -> AppResult<Option<String>> {
    let from_store = KeyringStore::new()
        .get(&keyring_ref(platform, host))
        .map_err(|e| AppError::Other(format!("凭证读取失败：{e}")))?;
    if from_store.is_some() {
        return Ok(from_store);
    }
    git_credential_fill(host)
}

#[tauri::command]
pub async fn get_ci_status(repo_path: String, git_ref: String) -> AppResult<CiStatus> {
    let remote = detect(&repo_path)?;
    let platform = remote.platform.id().to_string();
    let host = remote.host.clone();
    let token =
        tauri::async_runtime::spawn_blocking(move || resolve_remote_token_inner(&platform, &host))
            .await
            .map_err(|e| AppError::Other(format!("凭据解析任务失败：{e}")))??;
    api::fetch_ci_status(&remote, &git_ref, token.as_deref()).await
}
