//! 平台识别与 URL 构造（T-29，纯函数）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    GitHub,
    GitLab,
    Gitea,
    Gitee,
    Bitbucket,
}

impl Platform {
    pub fn id(&self) -> &'static str {
        match self {
            Platform::GitHub => "github",
            Platform::GitLab => "gitlab",
            Platform::Gitea => "gitea",
            Platform::Gitee => "gitee",
            Platform::Bitbucket => "bitbucket",
        }
    }
}

/// 解析出的远程仓库定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepo {
    pub platform: Platform,
    /// 站点主机（github.com / 自建 GitLab / Gitea 域名…）
    pub host: String,
    /// 归属路径（GitLab 支持子组，可能含 `/`）
    pub owner: String,
    /// 仓库名（不含 .git）
    pub repo: String,
}

/// Open 目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTarget {
    Repo,
    Issues,
    Pulls,
    /// 新建 PR 页（source/target 分支）
    NewPullRequest {
        source: String,
        target: String,
    },
    /// 具体第 n 号 PR
    Pull(i64),
    /// 具体第 n 号 Issue
    Issue(i64),
    /// CI / Actions / Pipelines
    Ci,
}

impl OpenTarget {
    pub fn id(&self) -> String {
        match self {
            OpenTarget::Repo => "repo".to_string(),
            OpenTarget::Issues => "issues".to_string(),
            OpenTarget::Pulls => "pulls".to_string(),
            OpenTarget::NewPullRequest { .. } => "new-pr".to_string(),
            OpenTarget::Pull(n) => format!("pull:{n}"),
            OpenTarget::Issue(n) => format!("issue:{n}"),
            OpenTarget::Ci => "ci".to_string(),
        }
    }
}

/// 解析 remote URL：HTTPS 与 SSH（git@host:owner/repo.git）形态。
/// 已知主机映射平台；未知主机按 Gitea 处理（自建实例的常见形态）。
pub fn parse_remote_url(url: &str) -> Option<RemoteRepo> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        let (h, p) = rest.split_once('/')?;
        (h.to_string(), p.to_string())
    } else if let Some(rest) = url.strip_prefix("git@") {
        let (h, p) = rest.split_once(':')?;
        (h.to_string(), p.to_string())
    } else if let Some(rest) = url.strip_prefix("ssh://git@") {
        let (h, p) = rest.split_once('/')?;
        (h.to_string(), p.to_string())
    } else {
        return None;
    };

    let host = host.split('@').next_back().unwrap_or(&host).to_string();
    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string();
    // SSH 形态可能带端口：host:port/path 已被切在冒号处，https 带端口需剥离
    let host = host.split(':').next().unwrap_or(&host).to_string();

    let platform = match host.as_str() {
        "github.com" => Platform::GitHub,
        "gitlab.com" => Platform::GitLab,
        "gitee.com" => Platform::Gitee,
        "bitbucket.org" => Platform::Bitbucket,
        // 其余按自建 Gitea / GitLab 处理（GitLab 自建路径常含 /gitlab 前缀，
        // 简化起见统一按 Gitea 命名空间；自建 GitLab 用户可改用 ssh 直连判断）
        _ => Platform::Gitea,
    };

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }

    let repo = segments.last().unwrap().to_string();
    let owner = segments[..segments.len() - 1].join("/");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(RemoteRepo {
        platform,
        host,
        owner,
        repo,
    })
}

impl RemoteRepo {
    /// 平台 Web 根：https://{host}/{owner}/{repo}
    fn web_base(&self) -> String {
        format!("https://{}/{}/{}", self.host, self.owner, self.repo)
    }

    /// 构造 Open URL（纯函数）。
    pub fn open_url(&self, target: &OpenTarget) -> String {
        let base = self.web_base();
        match self.platform {
            Platform::GitHub => match target {
                OpenTarget::Repo => base,
                OpenTarget::Issues => format!("{base}/issues"),
                OpenTarget::Pulls => format!("{base}/pulls"),
                OpenTarget::NewPullRequest { source, target } => {
                    format!("{base}/compare/{target}...{source}")
                }
                OpenTarget::Pull(n) => format!("{base}/pull/{n}"),
                OpenTarget::Issue(n) => format!("{base}/issues/{n}"),
                OpenTarget::Ci => format!("{base}/actions"),
            },
            Platform::GitLab => match target {
                OpenTarget::Repo => base,
                OpenTarget::Issues => format!("{base}/-/issues"),
                OpenTarget::Pulls => format!("{base}/-/merge_requests"),
                OpenTarget::NewPullRequest { source, target } => {
                    format!(
                        "{base}/-/merge_requests/new?merge_request%5Bsource_branch%5D={source}\
                         &merge_request%5Btarget_branch%5D={target}"
                    )
                }
                OpenTarget::Pull(n) => format!("{base}/-/merge_requests/{n}"),
                OpenTarget::Issue(n) => format!("{base}/-/issues/{n}"),
                OpenTarget::Ci => format!("{base}/-/pipelines"),
            },
            Platform::Gitea => match target {
                OpenTarget::Repo => base,
                OpenTarget::Issues => format!("{base}/issues"),
                OpenTarget::Pulls => format!("{base}/pulls"),
                OpenTarget::NewPullRequest { source, target } => {
                    format!("{base}/compare/{target}...{source}")
                }
                OpenTarget::Pull(n) => format!("{base}/pulls/{n}"),
                OpenTarget::Issue(n) => format!("{base}/issues/{n}"),
                OpenTarget::Ci => format!("{base}/actions"),
            },
            Platform::Gitee => match target {
                OpenTarget::Repo => base,
                OpenTarget::Issues => format!("{base}/issues"),
                OpenTarget::Pulls => format!("{base}/pulls"),
                OpenTarget::NewPullRequest { source, target } => {
                    format!("{base}/pull/new/{target}...{source}")
                }
                OpenTarget::Pull(n) => format!("{base}/pulls/{n}"),
                OpenTarget::Issue(n) => format!("{base}/issues/{n}"),
                OpenTarget::Ci => format!("{base}/pipeline"),
            },
            Platform::Bitbucket => match target {
                OpenTarget::Repo => base,
                OpenTarget::Issues => format!("{base}/issues"),
                OpenTarget::Pulls => format!("{base}/pull-requests"),
                OpenTarget::NewPullRequest { source, target } => {
                    format!("{base}/pull-requests/new?source={source}&dest={target}")
                }
                OpenTarget::Pull(n) => format!("{base}/pull-requests/{n}"),
                OpenTarget::Issue(n) => format!("{base}/issues/{n}"),
                OpenTarget::Ci => format!("{base}/pipelines"),
            },
        }
    }

    /// 平台 API 根。
    pub fn api_base(&self) -> String {
        match self.platform {
            Platform::GitHub => format!(
                "https://api.{}/repos/{}/{}",
                self.host, self.owner, self.repo
            ),
            Platform::GitLab => format!(
                "https://{}/api/v4/projects/{}",
                self.host,
                urlencoding(&format!("{}/{}", self.owner, self.repo))
            ),
            Platform::Gitea => format!(
                "https://{}/api/v1/repos/{}/{}",
                self.host, self.owner, self.repo
            ),
            Platform::Gitee => format!(
                "https://{}/api/v5/repos/{}/{}",
                self.host, self.owner, self.repo
            ),
            Platform::Bitbucket => format!(
                "https://api.bitbucket.org/2.0/repositories/{}/{}",
                self.owner, self.repo
            ),
        }
    }
}

/// GitLab 项目路径 URL 编码（`a/b` → `a%2Fb`）。
fn urlencoding(path: &str) -> String {
    path.replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_and_ssh() {
        let r = parse_remote_url("https://github.com/mantougg/git-workspace.git").unwrap();
        assert_eq!(r.platform, Platform::GitHub);
        assert_eq!(r.owner, "mantougg");
        assert_eq!(r.repo, "git-workspace");

        let r = parse_remote_url("git@github.com:mantougg/git-workspace.git").unwrap();
        assert_eq!(r.platform, Platform::GitHub);
        assert_eq!(r.repo, "git-workspace");

        let r = parse_remote_url("ssh://git@gitlab.com/group/sub/repo.git").unwrap();
        assert_eq!(r.platform, Platform::GitLab);
        assert_eq!(r.owner, "group/sub");
        assert_eq!(r.repo, "repo");

        let r = parse_remote_url("https://git.self.host/team/app/").unwrap();
        assert_eq!(r.platform, Platform::Gitea);
        assert_eq!(r.host, "git.self.host");

        assert!(parse_remote_url("/local/path").is_none());
        assert!(parse_remote_url("https://github.com/only-owner").is_none());
    }

    #[test]
    fn github_urls() {
        let r = parse_remote_url("https://github.com/o/r.git").unwrap();
        assert_eq!(r.open_url(&OpenTarget::Repo), "https://github.com/o/r");
        assert_eq!(
            r.open_url(&OpenTarget::Issues),
            "https://github.com/o/r/issues"
        );
        assert_eq!(
            r.open_url(&OpenTarget::Pulls),
            "https://github.com/o/r/pulls"
        );
        assert_eq!(
            r.open_url(&OpenTarget::Ci),
            "https://github.com/o/r/actions"
        );
        assert_eq!(
            r.open_url(&OpenTarget::NewPullRequest {
                source: "feat".into(),
                target: "main".into()
            }),
            "https://github.com/o/r/compare/main...feat"
        );
        assert_eq!(
            r.open_url(&OpenTarget::Pull(7)),
            "https://github.com/o/r/pull/7"
        );
        assert_eq!(
            r.open_url(&OpenTarget::Issue(3)),
            "https://github.com/o/r/issues/3"
        );
    }

    #[test]
    fn gitlab_urls() {
        let r = parse_remote_url("https://gitlab.com/group/sub/repo.git").unwrap();
        assert_eq!(
            r.open_url(&OpenTarget::Pulls),
            "https://gitlab.com/group/sub/repo/-/merge_requests"
        );
        assert_eq!(
            r.open_url(&OpenTarget::Ci),
            "https://gitlab.com/group/sub/repo/-/pipelines"
        );
        assert!(r
            .open_url(&OpenTarget::NewPullRequest {
                source: "f".into(),
                target: "m".into()
            })
            .contains("merge_requests/new"));
        assert_eq!(
            r.api_base(),
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Frepo"
        );
    }

    #[test]
    fn gitea_gitee_bitbucket_urls() {
        let g = parse_remote_url("https://git.self.host/o/r.git").unwrap();
        assert_eq!(
            g.open_url(&OpenTarget::Pulls),
            "https://git.self.host/o/r/pulls"
        );
        assert_eq!(g.api_base(), "https://git.self.host/api/v1/repos/o/r");

        let e = parse_remote_url("https://gitee.com/o/r.git").unwrap();
        assert_eq!(
            e.open_url(&OpenTarget::Ci),
            "https://gitee.com/o/r/pipeline"
        );

        let b = parse_remote_url("https://bitbucket.org/o/r.git").unwrap();
        assert_eq!(
            b.open_url(&OpenTarget::Pulls),
            "https://bitbucket.org/o/r/pull-requests"
        );
        assert_eq!(
            b.api_base(),
            "https://api.bitbucket.org/2.0/repositories/o/r"
        );
    }
}
