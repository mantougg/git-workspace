//! 平台 REST 调用（T-29）：Create PR + CI 状态。
//!
//! - 单次 HTTP 调用、无轮询循环（速率限制友好，§27）；
//! - 请求体构造与响应解析均为纯函数（可直接单测，不依赖网络）；
//! - 401/403/404 映射为可行动错误（带平台与操作语义）。

use serde::Serialize;

use crate::error::{AppError, AppResult};

use super::platform::{Platform, RemoteRepo};

#[derive(Debug, Clone)]
pub struct CreatePrInput {
    pub source: String,
    pub target: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrResult {
    pub number: i64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CiStatus {
    /// success / failure / pending / running / unknown
    pub state: String,
    pub url: String,
}

fn http_error(platform: Platform, status: u16, body: &str) -> AppError {
    let hint = match status {
        401 | 403 => "令牌缺失或权限不足（仓库 push / PR 写权限）",
        404 => "仓库不存在或令牌不可见",
        422 => "请求被拒绝（分支已存在同目标 PR 或参数不合法）",
        _ => "远程平台返回错误",
    };
    AppError::Other(format!(
        "{platform:?} API {status}：{hint}。响应片段：{}",
        body.chars().take(200).collect::<String>()
    ))
}

// ---------------------------------------------------------------------------
// 请求体构造（纯函数）
// ---------------------------------------------------------------------------

/// 各平台 Create PR 的 (url, json_body)。
pub fn build_create_pr_request(r: &RemoteRepo, input: &CreatePrInput) -> (String, String) {
    match r.platform {
        Platform::GitHub => {
            let body = serde_json::json!({
                "title": input.title,
                "head": input.source,
                "base": input.target,
                "body": input.body,
                "draft": input.draft,
            });
            (format!("{}/pulls", r.api_base()), body.to_string())
        }
        Platform::GitLab => {
            let body = serde_json::json!({
                "source_branch": input.source,
                "target_branch": input.target,
                "title": input.title,
                "description": input.body,
                "draft": input.draft,
            });
            (format!("{}/merge_requests", r.api_base()), body.to_string())
        }
        Platform::Gitea => {
            let body = serde_json::json!({
                "title": input.title,
                "head": input.source,
                "base": input.target,
                "body": input.body,
            });
            (format!("{}/pulls", r.api_base()), body.to_string())
        }
        Platform::Gitee => {
            let body = serde_json::json!({
                "title": input.title,
                "head": input.source,
                "base": input.target,
                "body": input.body,
            });
            (format!("{}/pulls", r.api_base()), body.to_string())
        }
        Platform::Bitbucket => {
            // Bitbucket 的分支结构是 {branch: {name}}
            let body = serde_json::json!({
                "title": input.title,
                "source": { "branch": { "name": input.source } },
                "destination": { "branch": { "name": input.target } },
                "description": input.body,
            });
            (format!("{}/pullrequests", r.api_base()), body.to_string())
        }
    }
}

/// 各平台认证头 (name, value)。
pub fn auth_header(r: &RemoteRepo, token: &str) -> (String, String) {
    match r.platform {
        Platform::GitLab => ("PRIVATE-TOKEN".to_string(), token.to_string()),
        _ => ("Authorization".to_string(), format!("Bearer {token}")),
    }
}

// ---------------------------------------------------------------------------
// 响应解析（纯函数）
// ---------------------------------------------------------------------------

/// 从 Create PR 响应提取 (number, html_url)。各平台字段名不同。
pub fn parse_create_pr_response(platform: Platform, body: &str) -> AppResult<PrResult> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AppError::Other(format!("{platform:?} 响应解析失败：{e}")))?;
    let number = match platform {
        // GitLab 的 MR 用 iid 作为页面编号
        Platform::GitLab => v.get("iid").and_then(|x| x.as_i64()),
        Platform::Bitbucket => v.get("id").and_then(|x| x.as_i64()),
        _ => v.get("number").and_then(|x| x.as_i64()),
    }
    .ok_or_else(|| AppError::Other(format!("{platform:?} 响应缺少 PR 编号")))?;
    let url = v
        .get("html_url")
        .or_else(|| v.get("web_url"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(PrResult { number, url })
}

/// 从 CI 状态响应提取 (state, url)。
pub fn parse_ci_status(platform: Platform, body: &str) -> AppResult<CiStatus> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| AppError::Other(format!("{platform:?} 响应解析失败：{e}")))?;
    let (state, url) = match platform {
        // GitHub commit status：state + statuses[].target_url（取最新）
        Platform::GitHub | Platform::Gitee => {
            let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("unknown").to_string();
            let url = v
                .get("statuses")
                .and_then(|x| x.as_array())
                .and_then(|a| a.first())
                .and_then(|s| s.get("target_url"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (state, url)
        }
        // GitLab commit statuses：数组，取第一条的 status
        Platform::GitLab => {
            let first = v.as_array().and_then(|a| a.first());
            let state = first
                .and_then(|s| s.get("status"))
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            let url = first
                .and_then(|s| s.get("target_url"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            (state, url)
        }
        // Gitea / Bitbucket：单对象 status
        _ => {
            let state = v
                .get("state")
                .or_else(|| v.get("status"))
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            (state, String::new())
        }
    };
    Ok(CiStatus { state, url })
}

/// CI 查询 URL。
pub fn build_ci_url(r: &RemoteRepo, git_ref: &str) -> String {
    match r.platform {
        Platform::GitHub => format!("{}/commits/{}/status", r.api_base(), git_ref),
        Platform::GitLab => format!("{}/repository/commits/{}/statuses", r.api_base(), git_ref),
        Platform::Gitee => format!("{}/commits/{}/status", r.api_base(), git_ref),
        Platform::Gitea => format!("{}/commits/{}/status", r.api_base(), git_ref),
        Platform::Bitbucket => format!("{}/commit/{}/status", r.api_base(), git_ref),
    }
}

// ---------------------------------------------------------------------------
// HTTP（唯一副作用入口）
// ---------------------------------------------------------------------------

/// Create PR（异步，单次调用）。
pub async fn create_pull_request(r: &RemoteRepo, token: Option<&str>, input: &CreatePrInput) -> AppResult<PrResult> {
    let (url, body) = build_create_pr_request(r, input);
    let client = reqwest::Client::new();
    let mut req = client
        .post(&url)
        .header("User-Agent", "git-workspace")
        .header("Accept", "application/json")
        .body(body);
    if let Some(token) = token {
        let (name, value) = auth_header(r, token);
        req = req.header(&name, value);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Other(format!("{:?} 请求失败：{e}", r.platform)))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(http_error(r.platform, status, &text));
    }
    parse_create_pr_response(r.platform, &text)
}

/// CI 状态（异步，单次调用）。
pub async fn fetch_ci_status(r: &RemoteRepo, git_ref: &str, token: Option<&str>) -> AppResult<CiStatus> {
    let url = build_ci_url(r, git_ref);
    let client = reqwest::Client::new();
    let mut req = client
        .get(&url)
        .header("User-Agent", "git-workspace")
        .header("Accept", "application/json");
    if let Some(token) = token {
        let (name, value) = auth_header(r, token);
        req = req.header(&name, value);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Other(format!("{:?} 请求失败：{e}", r.platform)))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(http_error(r.platform, status, &text));
    }
    parse_ci_status(r.platform, &text)
}

// ---------------------------------------------------------------------------
// tests（无网络：构造与解析）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::platform::parse_remote_url;

    fn github() -> RemoteRepo {
        parse_remote_url("https://github.com/o/r.git").unwrap()
    }

    #[test]
    fn create_pr_request_bodies() {
        let input = CreatePrInput {
            source: "feat".into(),
            target: "main".into(),
            title: "T".into(),
            body: "B".into(),
            draft: true,
        };
        let (url, body) = build_create_pr_request(&github(), &input);
        assert_eq!(url, "https://api.github.com/repos/o/r/pulls");
        assert!(body.contains("\"head\":\"feat\"") || body.contains("\"head\": \"feat\""));

        let gl = parse_remote_url("https://gitlab.com/g/r.git").unwrap();
        let (url, body) = build_create_pr_request(&gl, &input);
        assert_eq!(url, "https://gitlab.com/api/v4/projects/g%2Fr/merge_requests");
        assert!(body.contains("source_branch"));

        let bb = parse_remote_url("https://bitbucket.org/o/r.git").unwrap();
        let (_, body) = build_create_pr_request(&bb, &input);
        assert!(body.contains("destination"));
    }

    #[test]
    fn parse_pr_responses() {
        let gh = parse_create_pr_response(
            Platform::GitHub,
            r#"{"number": 12, "html_url": "https://github.com/o/r/pull/12"}"#,
        )
        .unwrap();
        assert_eq!(gh.number, 12);
        assert_eq!(gh.url, "https://github.com/o/r/pull/12");

        let gl = parse_create_pr_response(
            Platform::GitLab,
            r#"{"iid": 7, "web_url": "https://gitlab.com/g/r/-/merge_requests/7"}"#,
        )
        .unwrap();
        assert_eq!(gl.number, 7);

        assert!(parse_create_pr_response(Platform::GitHub, "{}").is_err());
    }

    #[test]
    fn parse_ci_responses() {
        let gh = parse_ci_status(
            Platform::GitHub,
            r#"{"state": "success", "statuses": [{"target_url": "https://ci/x"}]}"#,
        )
        .unwrap();
        assert_eq!(gh.state, "success");
        assert_eq!(gh.url, "https://ci/x");

        let gl = parse_ci_status(
            Platform::GitLab,
            r#"[{"status": "failed", "target_url": "https://gl/1"}]"#,
        )
        .unwrap();
        assert_eq!(gl.state, "failed");

        let ci_url = build_ci_url(&github(), "abc123");
        assert!(ci_url.ends_with("/commits/abc123/status"));
    }

    #[test]
    fn auth_headers() {
        let gh = auth_header(&github(), "tok");
        assert_eq!(gh.0, "Authorization");
        assert_eq!(gh.1, "Bearer tok");

        let gl = parse_remote_url("https://gitlab.com/g/r.git").unwrap();
        let h = auth_header(&gl, "tok");
        assert_eq!(h.0, "PRIVATE-TOKEN");
    }
}
