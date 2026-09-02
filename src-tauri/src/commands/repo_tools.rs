//! Submodule / Git LFS / Git Hooks 仓库工具（T-30，Roadmap §29/§30/§31）。
//!
//! - Submodule / LFS 走 git CLI（网络类操作按全局约束 §3 归 CLI）；
//!   解析均为纯函数可单测。
//! - Hooks 直接读写 `.git/hooks` 文件：启停用 `<name>.disabled` 重命名
//!   （Windows 无执行位，跨平台语义一致）；运行 unix 直跑、Windows 经
//!   Git Bash（hook 本身是 shell 脚本）。
//! - git 子进程统一 `CREATE_NO_WINDOW`（F-27 教训：GUI 进程 spawn 控制台
//!   程序会闪窗）+ 超时保护。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// git 命令超时。
const GIT_TIMEOUT: Duration = Duration::from_secs(60);
/// 网络类子模块操作超时。
const SUBMODULE_NET_TIMEOUT: Duration = Duration::from_secs(120);
/// hook 运行超时。
const HOOK_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ---------------------------------------------------------------------------
// git 子进程（唯一系统调用入口）
// ---------------------------------------------------------------------------

fn git_command(repo: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 子进程输出（分离流 + 退出状态）。
struct ProcOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// 等待子进程结束并收集输出（读线程分离，避免管道写满阻塞）；
/// 超时 kill。不能复用 `maven::detect_exec::wait_with_timeout`——它合并
/// 双流且不暴露退出码，为版本探测设计。
fn wait_with_streams(mut child: std::process::Child, timeout: Duration) -> AppResult<ProcOutput> {
    fn read_all<R: std::io::Read + Send + 'static>(
        stream: Option<R>,
    ) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut s) = stream {
                use std::io::Read;
                let mut raw = Vec::new();
                let _ = s.read_to_end(&mut raw);
                buf.push_str(&String::from_utf8_lossy(&raw));
            }
            buf
        })
    }
    let out_t = read_all(child.stdout.take());
    let err_t = read_all(child.stderr.take());

    let start = std::time::Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Other(format!(
                        "命令超时（{}s），已终止",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(AppError::Other(format!("等待子进程失败：{err}"))),
        }
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Ok(ProcOutput {
        code,
        stdout,
        stderr,
    })
}

/// 运行 git 命令并返回 stdout（非零退出 → 可行动错误，带 stderr）。
fn run_git(repo: &str, args: &[&str], timeout: Duration) -> AppResult<String> {
    let child = git_command(repo, args)
        .spawn()
        .map_err(|e| AppError::Other(format!("git 启动失败（{}）：{e}", args.join(" "))))?;
    let output = wait_with_streams(child, timeout)?;
    if output.code != Some(0) {
        return Err(AppError::Other(format!(
            "git {} 失败：{}",
            args.join(" "),
            output.stderr.trim().chars().take(300).collect::<String>()
        )));
    }
    Ok(output.stdout)
}

// ---------------------------------------------------------------------------
// Submodule
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleEntry {
    pub path: String,
    pub sha: String,
    /// synced / modified / uninitialized / conflict
    pub status: String,
    pub url: Option<String>,
    pub branch: Option<String>,
}

/// `git submodule status --recursive` 行解析（纯函数）。
/// 形态：`+<sha> <path> (<describe>)`；前缀空格 / `+` / `-` / `U`。
pub fn parse_submodule_status(out: &str) -> Vec<(char, String, String)> {
    out.lines()
        .filter_map(|line| {
            let line = line.trim_end();
            if line.len() < 42 {
                return None;
            }
            let mut chars = line.chars();
            let prefix = chars.next()?;
            let rest = chars.as_str();
            let sha = rest.get(..40)?;
            let path = rest[41..].trim();
            let path = path.split(" (").next().unwrap_or(path).trim();
            if path.is_empty() {
                return None;
            }
            Some((prefix, sha.to_string(), path.to_string()))
        })
        .collect()
}

/// `.gitmodules` 的 `submodule.<name>.<key>=<value>` 行解析（纯函数）。
pub fn parse_gitmodules(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            let rest = key.strip_prefix("submodule.")?;
            let (name, prop) = rest.split_once('.')?;
            Some((name.to_string(), prop.to_string(), value.trim().to_string()))
        })
        .collect()
}

fn submodule_meta(
    repo: &str,
) -> AppResult<std::collections::HashMap<String, (String, Option<String>)>> {
    // name → (path, url)
    let out = run_git(
        repo,
        &["config", "-f", ".gitmodules", "--list"],
        GIT_TIMEOUT,
    )?;
    let mut map: std::collections::HashMap<String, (String, Option<String>)> =
        std::collections::HashMap::new();
    for (name, prop, value) in parse_gitmodules(&out) {
        let entry = map.entry(name).or_insert_with(|| (String::new(), None));
        match prop.as_str() {
            "path" => entry.0 = value,
            "url" => entry.1 = Some(value),
            _ => {}
        }
    }
    Ok(map)
}

/// 列出子模块（状态前缀映射 + .gitmodules 元数据）。
#[tauri::command]
pub fn list_submodules(repo_path: String) -> AppResult<Vec<SubmoduleEntry>> {
    let status_out = match run_git(
        &repo_path,
        &["submodule", "status", "--recursive"],
        GIT_TIMEOUT,
    ) {
        Ok(out) => out,
        Err(_) => String::new(), // 无子模块时 git 以非零退出且无输出
    };
    if status_out.trim().is_empty() {
        return Ok(Vec::new());
    }
    let meta = submodule_meta(&repo_path)?;
    // 递归状态的 path 是「父路径 + 子路径」，meta 以直接子模块 name/path 记录，
    // URL 取最长路径前缀匹配。
    let mut metas: Vec<(String, String, Option<String>)> = meta
        .into_iter()
        .map(|(_, (path, url))| {
            let len = path.len();
            (path.clone(), path, url)
        })
        .collect();
    metas.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    Ok(parse_submodule_status(&status_out)
        .into_iter()
        .map(|(prefix, sha, path)| {
            let status = match prefix {
                ' ' => "synced",
                '+' => "modified",
                '-' => "uninitialized",
                'U' => "conflict",
                _ => "unknown",
            };
            let url = metas
                .iter()
                .find(|(p, _, _)| path == p.as_str() || path.starts_with(&format!("{p}/")))
                .and_then(|(_, _, url)| url.clone());
            SubmoduleEntry {
                path,
                sha,
                status: status.to_string(),
                url,
                branch: None,
            }
        })
        .collect())
}

/// 子模块操作：init / update / sync / add / remove。
#[tauri::command]
pub fn submodule_op(
    repo_path: String,
    op: String,
    path: Option<String>,
    url: Option<String>,
) -> AppResult<String> {
    let path_args: Vec<&str> = path.as_deref().map(|p| vec![p]).unwrap_or_default();
    match op.as_str() {
        "init" => run_git(
            &repo_path,
            &[&["submodule", "init"], path_args.as_slice()].concat(),
            GIT_TIMEOUT,
        ),
        "update" => run_git(
            &repo_path,
            &[
                &["submodule", "update", "--init", "--recursive"],
                path_args.as_slice(),
            ]
            .concat(),
            SUBMODULE_NET_TIMEOUT,
        ),
        "sync" => run_git(
            &repo_path,
            &[&["submodule", "sync", "--recursive"], path_args.as_slice()].concat(),
            GIT_TIMEOUT,
        ),
        "add" => {
            let url = url.ok_or_else(|| AppError::Other("add 需要 url 参数".to_string()))?;
            let target = path.ok_or_else(|| AppError::Other("add 需要 path 参数".to_string()))?;
            run_git(
                &repo_path,
                &["submodule", "add", &url, &target],
                SUBMODULE_NET_TIMEOUT,
            )
        }
        "remove" => {
            let target =
                path.ok_or_else(|| AppError::Other("remove 需要 path 参数".to_string()))?;
            run_git(
                &repo_path,
                &["submodule", "deinit", "-f", &target],
                GIT_TIMEOUT,
            )?;
            run_git(&repo_path, &["rm", "-f", &target], GIT_TIMEOUT)
        }
        other => Err(AppError::Other(format!("未知的子模块操作：{other}"))),
    }
}

// ---------------------------------------------------------------------------
// Git LFS
// ---------------------------------------------------------------------------

fn ensure_lfs() -> AppResult<()> {
    let probe = run_git(
        &std::env::temp_dir().to_string_lossy(),
        &["lfs", "version"],
        GIT_TIMEOUT,
    );
    match probe {
        Ok(_) => Ok(()),
        Err(_) => Err(AppError::NotFound(
            "Git LFS 不可用。请安装 Git LFS（git lfs install）后重试".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsFile {
    pub path: String,
    /// synced / pointer / dirty
    pub state: String,
}

/// `git lfs ls-files` 行解析（纯函数）：`<oid-trimmed> <marker> <path>`，
/// `*` = 已检出同步，`-` = 指针未同步；`--long` 形态的 ` (...)` 尺寸后缀剥离。
pub fn parse_lfs_ls_files(out: &str) -> Vec<LfsFile> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, char::is_whitespace);
            let _oid = parts.next()?.trim();
            let marker = parts.next()?.trim();
            let path = parts.next()?.trim();
            if path.is_empty() {
                return None;
            }
            let path = path.split(" (").next().unwrap_or(path).trim();
            let state = match marker {
                "*" => "synced",
                "-" => "pointer",
                _ => "dirty",
            };
            Some(LfsFile {
                path: path.to_string(),
                state: state.to_string(),
            })
        })
        .collect()
}

/// LFS 文件清单（状态标记）。
#[tauri::command]
pub fn lfs_list(repo_path: String) -> AppResult<Vec<LfsFile>> {
    ensure_lfs()?;
    let out = run_git(&repo_path, &["lfs", "ls-files", "--long"], GIT_TIMEOUT)?;
    Ok(parse_lfs_ls_files(&out))
}

/// LFS 网络操作：fetch / pull / push。
#[tauri::command]
pub fn lfs_op(repo_path: String, op: String, include: Option<String>) -> AppResult<String> {
    ensure_lfs()?;
    let include_pattern = include;
    let mut args: Vec<String> = match op.as_str() {
        "fetch" => vec!["lfs".into(), "fetch".into(), "origin".into()],
        "pull" => vec!["lfs".into(), "pull".into()],
        "push" => vec!["lfs".into(), "push".into(), "origin".into(), "--all".into()],
        other => return Err(AppError::Other(format!("未知的 LFS 操作：{other}"))),
    };
    if let Some(pattern) = include_pattern {
        if op == "fetch" {
            args.push("--include".into());
            args.push(pattern);
        }
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_git(&repo_path, &refs, SUBMODULE_NET_TIMEOUT)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LfsLock {
    pub id: String,
    pub path: String,
    pub owner: Option<String>,
}

/// `git lfs locks` 行解析（纯函数）：`<id> <path> <owner>`（tab / 多空格分隔）。
pub fn parse_lfs_locks(out: &str) -> Vec<LfsLock> {
    out.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let mut parts = line.split_whitespace();
            let id = parts.next()?.to_string();
            let path = parts.next()?.to_string();
            let owner = parts
                .next()
                .map(|s| s.trim_matches('(').trim_matches(')').to_string());
            Some(LfsLock { id, path, owner })
        })
        .collect()
}

/// LFS 锁列表。
#[tauri::command]
pub fn lfs_locks(repo_path: String) -> AppResult<Vec<LfsLock>> {
    ensure_lfs()?;
    let out = run_git(&repo_path, &["lfs", "locks"], GIT_TIMEOUT)?;
    Ok(parse_lfs_locks(&out))
}

/// 创建 / 解除 LFS 锁（op = lock / unlock）。
#[tauri::command]
pub fn lfs_lock_op(repo_path: String, op: String, path: String) -> AppResult<String> {
    ensure_lfs()?;
    match op.as_str() {
        "lock" => run_git(&repo_path, &["lfs", "lock", &path], GIT_TIMEOUT),
        "unlock" => run_git(&repo_path, &["lfs", "unlock", &path], GIT_TIMEOUT),
        other => Err(AppError::Other(format!("未知的锁操作：{other}"))),
    }
}

// ---------------------------------------------------------------------------
// Git Hooks
// ---------------------------------------------------------------------------

/// 已支持的 hook 名（§29）。
pub const KNOWN_HOOKS: &[&str] = &[
    "pre-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-push",
    "post-checkout",
    "post-merge",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInfo {
    pub name: String,
    /// 实体文件存在（含 .disabled）
    pub exists: bool,
    /// 处于启用状态（存在且未被 disable）
    pub enabled: bool,
}

/// worktree 形态的 .git 目录（`.git` 目录或 `gitdir:` 文件）。
pub(crate) fn resolve_git_dir(repo_path: &str) -> Option<PathBuf> {
    let dotgit = Path::new(repo_path).join(".git");
    if dotgit.is_dir() {
        return Some(dotgit);
    }
    let content = std::fs::read_to_string(&dotgit).ok()?;
    let target = content.trim().strip_prefix("gitdir:")?.trim();
    let p = Path::new(target);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(repo_path).join(p)
    })
}

fn hook_paths(repo_path: &str, name: &str) -> Option<(PathBuf, PathBuf)> {
    let git_dir = resolve_git_dir(repo_path)?;
    let active = git_dir.join("hooks").join(name);
    let disabled = git_dir.join("hooks").join(format!("{name}.disabled"));
    Some((active, disabled))
}

/// hook 状态列表（纯枚举，无子进程）。
#[tauri::command]
pub fn list_hooks(repo_path: String) -> AppResult<Vec<HookInfo>> {
    Ok(KNOWN_HOOKS
        .iter()
        .map(|name| {
            let info = match hook_paths(&repo_path, name) {
                Some((active, disabled)) => HookInfo {
                    name: name.to_string(),
                    exists: active.exists() || disabled.exists(),
                    enabled: active.exists(),
                },
                None => HookInfo {
                    name: name.to_string(),
                    exists: false,
                    enabled: false,
                },
            };
            info
        })
        .collect())
}

/// 读取 hook 内容（未创建返回空串）。
#[tauri::command]
pub fn get_hook(repo_path: String, name: String) -> AppResult<String> {
    if !KNOWN_HOOKS.contains(&name.as_str()) {
        return Err(AppError::Other(format!("不支持的 hook：{name}")));
    }
    let (active, disabled) = hook_paths(&repo_path, &name)
        .ok_or_else(|| AppError::Other("无法定位 .git 目录".to_string()))?;
    let file = if active.exists() { active } else { disabled };
    Ok(std::fs::read_to_string(file).unwrap_or_default())
}

/// 保存 hook 内容（保存即创建并启用；unix 补执行位）。
#[tauri::command]
pub fn save_hook(repo_path: String, name: String, content: String) -> AppResult<()> {
    if !KNOWN_HOOKS.contains(&name.as_str()) {
        return Err(AppError::Other(format!("不支持的 hook：{name}")));
    }
    let (active, disabled) = hook_paths(&repo_path, &name)
        .ok_or_else(|| AppError::Other("无法定位 .git 目录".to_string()))?;
    std::fs::create_dir_all(active.parent().unwrap())?;
    // 旧 disabled 文件先清掉
    if disabled.exists() {
        std::fs::remove_file(&disabled)?;
    }
    std::fs::write(&active, &content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&active)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&active, perms)?;
    }
    Ok(())
}

/// 启停 hook：停用 = 重命名为 `<name>.disabled`；启用 = 改回原名。
#[tauri::command]
pub fn set_hook_enabled(repo_path: String, name: String, enabled: bool) -> AppResult<()> {
    if !KNOWN_HOOKS.contains(&name.as_str()) {
        return Err(AppError::Other(format!("不支持的 hook：{name}")));
    }
    let (active, disabled) = hook_paths(&repo_path, &name)
        .ok_or_else(|| AppError::Other("无法定位 .git 目录".to_string()))?;
    if enabled {
        if disabled.exists() {
            std::fs::rename(&disabled, &active)?;
        }
    } else if active.exists() {
        std::fs::rename(&active, &disabled)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRunResult {
    pub exit_code: Option<i32>,
    pub output: String,
}

/// 手动运行 hook（repo 根为工作目录；hook 可能修改仓库，属用户显式动作）。
#[tauri::command]
pub fn run_hook(repo_path: String, name: String) -> AppResult<HookRunResult> {
    if !KNOWN_HOOKS.contains(&name.as_str()) {
        return Err(AppError::Other(format!("不支持的 hook：{name}")));
    }
    let (active, _) = hook_paths(&repo_path, &name)
        .ok_or_else(|| AppError::Other("无法定位 .git 目录".to_string()))?;
    if !active.exists() {
        return Err(AppError::NotFound(format!("hook {name} 未创建")));
    }

    let mut command = hook_command(&active);
    command.current_dir(&repo_path);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command
        .spawn()
        .map_err(|e| AppError::Other(format!("hook 运行失败：{e}")))?;
    let output = wait_with_streams(child, HOOK_TIMEOUT)?;
    let mut text = output.stdout;
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&output.stderr);
    }
    Ok(HookRunResult {
        exit_code: output.code,
        output: text
            .chars()
            .rev()
            .take(4000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    })
}

/// 平台 hook 执行器：unix 直跑（脚本自带 shebang + 执行位）；
/// Windows 经 Git Bash（hook 是 shell 脚本，cmd 无法执行）。
fn hook_command(script: &Path) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new(script);
        c
    }
    #[cfg(windows)]
    {
        let bash = crate::java::detect::find_in_path("bash")
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "bash".to_string());
        let mut c = Command::new(bash);
        c.arg(script);
        c
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submodule_status_lines() {
        // 注意：首行前缀空格是状态语义，不能用 "\` 续行（会剥前导空白）。
        let out = concat!(
            " e4f9a1c0a1b2c3d4e5f60718293a4b5c6d7e8f90 libs/core (v1.2.0)\n",
            "+9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b libs/util\n",
            "-b0a9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1 libs/missing\n"
        );
        let parsed = parse_submodule_status(out);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].0, ' ');
        assert_eq!(parsed[0].2, "libs/core");
        assert_eq!(parsed[1].0, '+');
        assert_eq!(parsed[2].0, '-');
        assert_eq!(parsed[2].1.len(), 40);
    }

    #[test]
    fn gitmodules_lines() {
        let out = "\
submodule.core.path=libs/core
submodule.core.url=https://github.com/o/core.git
submodule.core.branch=stable
submodule.util.path=libs/util
submodule.util.url=git@host:t/util.git
";
        let parsed = parse_gitmodules(out);
        assert_eq!(parsed.len(), 5);
        let core_url = parsed.iter().find(|(n, p, _)| n == "core" && p == "url");
        assert_eq!(core_url.unwrap().2, "https://github.com/o/core.git");
    }

    #[test]
    fn lfs_ls_files_lines() {
        let out = "\
abcdef12 * media/big.mp4 (10.2 MB)
12345678 - media/pending.png
";
        let parsed = parse_lfs_ls_files(out);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].state, "synced");
        assert_eq!(parsed[0].path, "media/big.mp4");
        assert_eq!(parsed[1].state, "pointer");
    }

    #[test]
    fn lfs_locks_lines() {
        let out = "\
1\tmedia/big.mp4\talice
2\tmedia/other.mp4\tbob
";
        let parsed = parse_lfs_locks(out);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].path, "media/big.mp4");
        assert_eq!(parsed[1].owner.as_deref(), Some("bob"));
    }

    #[test]
    fn hooks_crud_lifecycle() {
        let base = std::env::temp_dir().join(format!("gw-hooks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join(".git/hooks")).unwrap();
        let repo_str = repo.to_string_lossy().to_string();

        let infos = list_hooks(repo_str.clone()).unwrap();
        assert_eq!(infos.len(), KNOWN_HOOKS.len());
        assert!(infos.iter().all(|i| !i.exists));

        save_hook(
            repo_str.clone(),
            "pre-commit".into(),
            "#!/bin/sh\necho hi\n".into(),
        )
        .unwrap();
        assert_eq!(
            get_hook(repo_str.clone(), "pre-commit".into()).unwrap(),
            "#!/bin/sh\necho hi\n"
        );

        // 启停切换
        set_hook_enabled(repo_str.clone(), "pre-commit".into(), false).unwrap();
        let infos = list_hooks(repo_str.clone()).unwrap();
        let pre = infos.iter().find(|i| i.name == "pre-commit").unwrap();
        assert!(pre.exists && !pre.enabled);
        // 内容仍可读（disabled 文件）
        assert!(get_hook(repo_str.clone(), "pre-commit".into())
            .unwrap()
            .contains("hi"));

        set_hook_enabled(repo_str.clone(), "pre-commit".into(), true).unwrap();
        let infos = list_hooks(repo_str.clone()).unwrap();
        let pre = infos.iter().find(|i| i.name == "pre-commit").unwrap();
        assert!(pre.enabled);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(repo.join(".git/hooks/pre-commit"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }

        // 不支持的 hook 名拒绝
        assert!(save_hook(repo_str.clone(), "evil".into(), "".into()).is_err());

        // run_hook：真实跑一个脚本（unix 直跑）
        #[cfg(unix)]
        {
            let result = run_hook(repo_str.clone(), "pre-commit".into()).unwrap();
            assert_eq!(result.exit_code, Some(0));
            assert!(result.output.contains("hi"));
        }
    }
}
