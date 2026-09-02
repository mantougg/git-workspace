//! Terminal / IDE 集成命令（T-31，Roadmap §56 / §57）。
//!
//! - 打开终端：系统默认终端 + Windows 专属（PowerShell / CMD / Git Bash /
//!   Windows Terminal）；macOS / Linux 走平台默认终端。
//! - 打开 IDE：VS Code / IntelliJ IDEA / Cursor / Zed（仓库目录或文件）。
//! - 可执行检测一律走 `java::detect::find_in_path`（Windows PATHEXT 语义）；
//!   `code` / `cursor` / `zed` 在 Windows 上是 `.cmd` shim，必须经 `cmd /C`
//!   执行（`needs_cmd_c`，AGENTS.md 平台规范 §2）。
//! - 终端窗口要「可见」：这里禁止沿用 `process/streaming.rs` 的
//!   `CREATE_NO_WINDOW`，保持默认 spawn（GUI 父进程给控制台子进程新开可见窗口）。
//! - 命令行构造为纯函数（`SpawnPlan`），可注入目录单测；系统调用只剩两个
//!   command 入口（与 `process/port.rs` 的纯函数约定一致）。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::java::detect::find_in_path;
use crate::maven::detect_exec::needs_cmd_c;

/// 终端类型；kebab-case 与 TS 侧 `TerminalKind` 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalKind {
    /// 平台默认终端（自动探测链）。
    System,
    PowerShell,
    Cmd,
    GitBash,
    WindowsTerminal,
}

impl TerminalKind {
    fn id(self) -> &'static str {
        match self {
            TerminalKind::System => "system",
            TerminalKind::PowerShell => "powershell",
            TerminalKind::Cmd => "cmd",
            TerminalKind::GitBash => "git-bash",
            TerminalKind::WindowsTerminal => "windows-terminal",
        }
    }
}

/// IDE 类型；kebab-case 与 TS 侧 `IdeKind` 对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdeKind {
    VsCode,
    Idea,
    Cursor,
    Zed,
}

impl IdeKind {
    fn id(self) -> &'static str {
        match self {
            IdeKind::VsCode => "vscode",
            IdeKind::Idea => "idea",
            IdeKind::Cursor => "cursor",
            IdeKind::Zed => "zed",
        }
    }
}

/// 平台无关的子进程启动计划（纯数据，单测友好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnPlan {
    pub program: String,
    pub args: Vec<String>,
    /// 个别终端没有「打开指定目录」的参数，只能靠子进程 cwd 传递。
    pub cwd: Option<String>,
}

impl SpawnPlan {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            cwd: None,
        }
    }

    fn with_cwd(mut self, cwd: &str) -> Self {
        self.cwd = Some(cwd.to_string());
        self
    }
}

// ---------------------------------------------------------------------------
// 终端命令行构造（纯函数）
// ---------------------------------------------------------------------------

/// PowerShell：`-NoExit -Command "Set-Location -LiteralPath '<dir>'"`。
/// 路径内单引号按 PowerShell 字面量规则转义为两个单引号。
#[cfg(any(windows, test))]
pub(crate) fn powershell_plan(exe: &Path, dir: &str) -> SpawnPlan {
    let escaped = dir.replace('\'', "''");
    SpawnPlan::new(
        exe.to_string_lossy().to_string(),
        vec![
            "-NoExit".to_string(),
            "-Command".to_string(),
            format!("Set-Location -LiteralPath '{escaped}'"),
        ],
    )
}

/// CMD：`/K cd /d <dir>`（`/d` 允许跨盘符切换）。
#[cfg(any(windows, test))]
pub(crate) fn cmd_plan(dir: &str) -> SpawnPlan {
    SpawnPlan::new(
        "cmd",
        vec!["/K".to_string(), "cd".to_string(), "/d".to_string(), dir.to_string()],
    )
}

/// Windows Terminal：`wt -d <dir>`。
#[cfg(any(windows, test))]
pub(crate) fn windows_terminal_plan(exe: &Path, dir: &str) -> SpawnPlan {
    SpawnPlan::new(exe.to_string_lossy().to_string(), vec!["-d".to_string(), dir.to_string()])
}

/// Git Bash：`git-bash.exe --cd=<dir>`。
#[cfg(any(windows, test))]
pub(crate) fn git_bash_plan(exe: &Path, dir: &str) -> SpawnPlan {
    SpawnPlan::new(
        exe.to_string_lossy().to_string(),
        vec![format!("--cd={dir}")],
    )
}

/// 由 `git.exe` 路径推导 `git-bash.exe`（`<Git>\cmd\git.exe` → `<Git>\git-bash.exe`）。
#[cfg(any(windows, test))]
pub(crate) fn git_bash_from_git_exe(git_exe: &Path) -> Option<PathBuf> {
    // `<Git>\cmd\git.exe`：上跳两级是 Git 安装根；git-bash.exe 固定在根下。
    let root = git_exe.parent()?.parent()?;
    let bash = root.join("git-bash.exe");
    bash.is_file().then_some(bash)
}

/// Linux 按探测链构造终端启动计划（`dirs` 注入便于单测）。
/// 顺序：gnome-terminal → konsole → xfce4-terminal → alacritty → kitty →
/// wezterm → x-terminal-emulator → xterm。
pub(crate) fn linux_terminal_plan_in_dirs(dir: &str, dirs: &[PathBuf]) -> Option<SpawnPlan> {
    use crate::java::detect::find_executable_in_dirs;

    let (name, plan): (&str, fn(&Path, &str) -> SpawnPlan) = if find_executable_in_dirs("gnome-terminal", dirs).is_some() {
        ("gnome-terminal", |exe, d| {
            SpawnPlan::new(exe.to_string_lossy().to_string(), vec![format!("--working-directory={d}")])
        })
    } else if find_executable_in_dirs("konsole", dirs).is_some() {
        ("konsole", |exe, d| {
            SpawnPlan::new(exe.to_string_lossy().to_string(), vec!["--workdir".to_string(), d.to_string()])
        })
    } else if find_executable_in_dirs("xfce4-terminal", dirs).is_some() {
        ("xfce4-terminal", |exe, d| {
            SpawnPlan::new(exe.to_string_lossy().to_string(), vec![format!("--working-directory={d}")])
        })
    } else if find_executable_in_dirs("alacritty", dirs).is_some() {
        ("alacritty", |exe, d| {
            SpawnPlan::new(exe.to_string_lossy().to_string(), vec!["--working-directory".to_string(), d.to_string()])
        })
    } else if find_executable_in_dirs("kitty", dirs).is_some() {
        ("kitty", |exe, d| {
            SpawnPlan::new(exe.to_string_lossy().to_string(), vec!["--directory".to_string(), d.to_string()])
        })
    } else if find_executable_in_dirs("wezterm", dirs).is_some() {
        ("wezterm", |exe, d| {
            SpawnPlan::new(
                exe.to_string_lossy().to_string(),
                vec!["start".to_string(), "--cwd".to_string(), d.to_string()],
            )
        })
    } else if find_executable_in_dirs("x-terminal-emulator", dirs).is_some()
        || find_executable_in_dirs("xterm", dirs).is_some()
    {
        // 通用 X 终端无统一目录参数，退化为子进程 cwd。
        ("x-terminal-emulator", |exe, d| {
            SpawnPlan::new(exe.to_string_lossy().to_string(), vec![]).with_cwd(d)
        })
    } else {
        return None;
    };
    let exe = find_executable_in_dirs(name, dirs)?;
    Some(plan(&exe, dir))
}

// ---------------------------------------------------------------------------
// IDE 命令行构造（纯函数）
// ---------------------------------------------------------------------------

/// VS Code / Cursor / Zed：单二进制 CLI（Windows 上可能是 `.cmd` shim）。
/// `needs_cmd_c` 命中时包一层 `cmd /C`。
pub(crate) fn cli_ide_plan(exe: &Path, target: &str) -> SpawnPlan {
    let exe_str = exe.to_string_lossy().to_string();
    if needs_cmd_c(exe) {
        SpawnPlan::new(
            "cmd",
            vec!["/C".to_string(), exe_str, target.to_string()],
        )
    } else {
        SpawnPlan::new(exe_str, vec![target.to_string()])
    }
}

/// 在给定目录列表中定位 IntelliJ IDEA 可执行（`candidates` 注入便于单测）。
/// 命中规则：目录下任一候选名存在即返回（如 `bin/idea64.exe`、`MacOS/idea`）。
pub(crate) fn find_idea_in_dirs(
    install_dirs: &[PathBuf],
    candidate_suffixes: &[&str],
) -> Option<PathBuf> {
    for dir in install_dirs {
        for suffix in candidate_suffixes {
            let candidate = dir.join(suffix);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 当前平台的 IDEA 默认安装目录列表（真实探测用，单测不依赖）。
fn idea_install_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        // 官方安装器：%ProgramFiles%\JetBrains\IntelliJ IDEA <ver>
        // Toolbox：%LOCALAPPDATA%\Programs（装到带版本号的子目录）。
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            dirs.push(PathBuf::from(pf).join("JetBrains"));
        }
        if let Some(lad) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(lad).join("Programs"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Applications"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // snap / 官方 tarball 一般已入 PATH（find_in_path 命中），
        // 这里只补常见手动解压位置。
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            dirs.push(home.join("opt"));
            dirs.push(home.join(".local"));
        }
    }
    dirs
}

/// 当前平台的 IDEA 候选相对路径（真实探测用）。
fn idea_candidate_suffixes() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec!["bin\\idea64.exe", "bin\\idea.exe"]
    }
    #[cfg(target_os = "macos")]
    {
        // /Applications 直下即可命中；子目录扫描是带版本目录的兜底。
        vec![
            "IntelliJ IDEA.app/Contents/MacOS/idea",
            "IntelliJ IDEA CE.app/Contents/MacOS/idea",
        ]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // `~/opt/idea/bin/idea.sh`；带版本号目录靠子目录扫描覆盖。
        vec!["idea/bin/idea.sh", "bin/idea.sh"]
    }
}

/// 定位 IntelliJ IDEA 可执行：先 PATH（`idea` 命令 / snap 链接），
/// 再平台默认安装目录（含一层带版本号的子目录，如
/// `%ProgramFiles%\JetBrains\IntelliJ IDEA 2024.2\bin\idea64.exe`）。
pub(crate) fn locate_idea() -> Option<PathBuf> {
    if let Some(exe) = find_in_path("idea") {
        return Some(exe);
    }
    let dirs = idea_install_dirs();
    let suffixes = idea_candidate_suffixes();
    if let Some(hit) = find_idea_in_dirs(&dirs, &suffixes) {
        return Some(hit);
    }
    for dir in &dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        let mut subs: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        subs.sort();
        for sub in subs {
            if let Some(hit) = find_idea_in_dirs(&[sub], &suffixes) {
                return Some(hit);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 平台终端计划（cfg 编译期分支）
// ---------------------------------------------------------------------------

fn terminal_plan(kind: TerminalKind, dir: &str) -> AppResult<SpawnPlan> {
    #[cfg(windows)]
    {
        return windows_terminal_plan(kind, dir);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_terminal_plan(kind, dir);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return linux_terminal_plan(kind, dir);
    }
}

#[cfg(windows)]
fn windows_terminal_plan(kind: TerminalKind, dir: &str) -> AppResult<SpawnPlan> {
    let plan = match kind {
        TerminalKind::PowerShell => {
            let exe = find_in_path("powershell").ok_or_else(|| {
                AppError::NotFound("未找到 powershell.exe（Windows 自带组件，请检查 PATH）".to_string())
            })?;
            powershell_plan(&exe, dir)
        }
        TerminalKind::Cmd => cmd_plan(dir),
        TerminalKind::GitBash => {
            let exe = locate_git_bash().ok_or_else(|| {
                AppError::NotFound(
                    "未找到 Git Bash。请确认已安装 Git for Windows（含 git-bash.exe）".to_string(),
                )
            })?;
            git_bash_plan(&exe, dir)
        }
        TerminalKind::WindowsTerminal => {
            let exe = find_in_path("wt").ok_or_else(|| {
                AppError::NotFound(
                    "未找到 Windows Terminal（wt.exe）。可从 Microsoft Store 安装".to_string(),
                )
            })?;
            windows_terminal_plan(&exe, dir)
        }
        // Windows 下的「系统默认」：WT → Git Bash → PowerShell 依次回退。
        TerminalKind::System => {
            if let Some(exe) = find_in_path("wt") {
                windows_terminal_plan(&exe, dir)
            } else if let Some(exe) = locate_git_bash() {
                git_bash_plan(&exe, dir)
            } else if let Some(exe) = find_in_path("powershell") {
                powershell_plan(&exe, dir)
            } else {
                return Err(AppError::NotFound(
                    "未找到任何可用终端（wt / git-bash / powershell 均不可用）".to_string(),
                ));
            }
        }
    };
    Ok(plan)
}

#[cfg(windows)]
fn locate_git_bash() -> Option<PathBuf> {
    if let Some(bash) = find_in_path("git-bash") {
        return Some(bash);
    }
    let git = find_in_path("git")?;
    git_bash_from_git_exe(&git)
}

#[cfg(target_os = "macos")]
fn macos_terminal_plan(kind: TerminalKind, dir: &str) -> AppResult<SpawnPlan> {
    // macOS 只区分「系统终端」；`open -a Terminal <dir>` 由 LaunchServices
    // 拉起 Terminal.app 并切目录。PowerShell/CMD 等专有类型按 System 处理。
    let _ = kind;
    Ok(SpawnPlan::new(
        "open",
        vec!["-a".to_string(), "Terminal".to_string(), dir.to_string()],
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_terminal_plan(kind: TerminalKind, dir: &str) -> AppResult<SpawnPlan> {
    let _ = kind;
    linux_terminal_plan_in_dirs(dir, &path_dirs()).ok_or_else(|| {
        AppError::NotFound(
            "未找到可用的终端模拟器（gnome-terminal / konsole / xfce4-terminal / alacritty / kitty / wezterm / xterm）"
                .to_string(),
        )
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| {
            p.to_string_lossy()
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn ide_plan(kind: IdeKind, target: &str) -> AppResult<SpawnPlan> {
    let (name, label, hint) = match kind {
        IdeKind::VsCode => ("code", "VS Code", "请安装 VS Code 并把 code 加入 PATH"),
        IdeKind::Cursor => ("cursor", "Cursor", "请安装 Cursor 并把 cursor 加入 PATH"),
        IdeKind::Zed => ("zed", "Zed", "请安装 Zed 并把 zed 加入 PATH"),
        IdeKind::Idea => ("idea", "IntelliJ IDEA", "请安装 IntelliJ IDEA（命令行启动器或默认安装路径）"),
    };
    let exe = if kind == IdeKind::Idea {
        locate_idea()
    } else {
        find_in_path(name)
    };
    let exe = exe.ok_or_else(|| {
        AppError::NotFound(format!("未找到 {label} 可执行文件。{hint}"))
    })?;
    Ok(cli_ide_plan(&exe, target))
}

// ---------------------------------------------------------------------------
// spawn（唯一系统调用入口）
// ---------------------------------------------------------------------------

fn spawn_plan(plan: &SpawnPlan) -> AppResult<()> {
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    if let Some(cwd) = &plan.cwd {
        command.current_dir(cwd);
    }
    // 注意：不要套 CREATE_NO_WINDOW——终端 / IDE 窗口必须可见。
    command.spawn()?;
    Ok(())
}

/// 校验目录存在（终端要求目录；IDE 目录/文件皆可）。
fn ensure_dir(path: &str) -> AppResult<()> {
    if Path::new(path).is_dir() {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("目录不存在：{path}")))
    }
}

fn ensure_exists(path: &str) -> AppResult<()> {
    if Path::new(path).exists() {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("路径不存在：{path}")))
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// 在指定终端打开目录（`kind` 缺省为平台默认终端）。
#[tauri::command]
pub fn open_in_terminal(path: String, kind: Option<TerminalKind>) -> AppResult<()> {
    ensure_dir(&path)?;
    let plan = terminal_plan(kind.unwrap_or(TerminalKind::System), &path)?;
    spawn_plan(&plan)
}

/// 在指定 IDE 打开仓库目录 / 文件 / worktree 目录。
#[tauri::command]
pub fn open_in_ide(path: String, ide: IdeKind) -> AppResult<()> {
    ensure_exists(&path)?;
    let plan = ide_plan(ide, &path)?;
    spawn_plan(&plan)
}

/// 当前平台可用的终端 / IDE（前端据此渲染菜单，避免展示必失败项）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationTargets {
    pub terminals: Vec<String>,
    pub ides: Vec<String>,
}

#[tauri::command]
pub fn list_integration_targets() -> IntegrationTargets {
    let terminals = integration_terminals();
    let ides = [
        (IdeKind::VsCode, "code"),
        (IdeKind::Cursor, "cursor"),
        (IdeKind::Zed, "zed"),
    ]
    .into_iter()
    .filter(|(_, name)| find_in_path(name).is_some())
    .map(|(kind, _)| kind.id().to_string())
    .collect::<Vec<_>>();
    let ides = if locate_idea().is_some() {
        let mut v = ides;
        v.push(IdeKind::Idea.id().to_string());
        v
    } else {
        ides
    };
    IntegrationTargets { terminals, ides }
}

#[allow(unused_mut)]
fn integration_terminals() -> Vec<String> {
    let mut terminals = vec![TerminalKind::System.id().to_string()];
    #[cfg(windows)]
    {
        terminals.push(TerminalKind::PowerShell.id().to_string());
        terminals.push(TerminalKind::Cmd.id().to_string());
        if locate_git_bash().is_some() {
            terminals.push(TerminalKind::GitBash.id().to_string());
        }
        if find_in_path("wt").is_some() {
            terminals.push(TerminalKind::WindowsTerminal.id().to_string());
        }
    }
    terminals
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::env;

    fn temp_dir(name: &str) -> PathBuf {
        let base = env::temp_dir().join(format!("gw-integration-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn powershell_plan_escapes_single_quotes() {
        let plan = powershell_plan(Path::new("powershell.exe"), r"C:\o'brien's dir");
        assert_eq!(plan.program, "powershell.exe");
        assert_eq!(plan.args[2], r"Set-Location -LiteralPath 'C:\o''brien''s dir'");
    }

    #[test]
    fn cmd_plan_uses_k_cd_d() {
        let plan = cmd_plan(r"C:\My Dir");
        assert_eq!(plan.program, "cmd");
        assert_eq!(plan.args, vec!["/K", "cd", "/d", r"C:\My Dir"]);
    }

    #[test]
    fn git_bash_derived_from_git_exe_location() {
        let base = temp_dir("gitbash");
        let git_cmd = base.join("Git").join("cmd");
        fs::create_dir_all(&git_cmd).unwrap();
        let git_exe = git_cmd.join("git.exe");
        fs::write(&git_exe, b"").unwrap();
        let bash = base.join("Git").join("git-bash.exe");
        fs::write(&bash, b"").unwrap();
        assert_eq!(git_bash_from_git_exe(&git_exe), Some(bash));
        // 缺 git-bash.exe 时返回 None。
        let base2 = temp_dir("gitbash-missing");
        let git_cmd2 = base2.join("Git").join("cmd");
        fs::create_dir_all(&git_cmd2).unwrap();
        let git_exe2 = git_cmd2.join("git.exe");
        fs::write(&git_exe2, b"").unwrap();
        assert_eq!(git_bash_from_git_exe(&git_exe2), None);
    }

    #[test]
    fn cli_ide_plan_wraps_cmd_shim() {
        // Windows 上 code / cursor / zed 的实体可能是 .cmd shim。
        let plan = cli_ide_plan(Path::new(r"C:\Tools\code.cmd"), r"C:\repo");
        assert_eq!(plan.program, "cmd");
        assert_eq!(plan.args, vec!["/C", r"C:\Tools\code.cmd", r"C:\repo"]);

        let plan2 = cli_ide_plan(Path::new("/usr/bin/zed"), "/home/u/repo");
        assert_eq!(plan2.program, "/usr/bin/zed");
        assert_eq!(plan2.args, vec!["/home/u/repo"]);
    }

    #[test]
    fn idea_detection_scans_install_dirs() {
        let base = temp_dir("idea");
        let bin = base.join("JetBrains").join("IntelliJ IDEA 2024.2").join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("idea64.exe"), b"").unwrap();
        let found = find_idea_in_dirs(
            &[base.join("JetBrains")],
            &["IntelliJ IDEA 2024.2/bin/idea64.exe"],
        );
        assert_eq!(found, Some(bin.join("idea64.exe")));
        // 候选相对路径不匹配时返回 None。
        assert_eq!(
            find_idea_in_dirs(&[base.join("JetBrains")], &["bin/idea64.exe"]),
            None
        );
    }

    #[test]
    fn linux_terminal_chain_prefers_gnome() {
        let base = temp_dir("term");
        for name in ["konsole", "xterm"] {
            fs::write(base.join(name), b"").unwrap();
        }
        let plan = linux_terminal_plan_in_dirs("/repo", &[base.clone()]);
        assert!(plan.is_some());
        let plan = plan.unwrap();
        assert_eq!(plan.program, base.join("konsole").to_string_lossy().to_string());
        assert_eq!(plan.args, vec!["--workdir", "/repo"]);
        assert_eq!(plan.cwd, None);
    }

    #[test]
    fn linux_terminal_fallback_uses_cwd() {
        let base = temp_dir("term2");
        fs::write(base.join("x-terminal-emulator"), b"").unwrap();
        let plan = linux_terminal_plan_in_dirs("/repo with space", &[base]).unwrap();
        assert_eq!(plan.args, Vec::<String>::new());
        assert_eq!(plan.cwd.as_deref(), Some("/repo with space"));
    }

    #[test]
    fn linux_terminal_none_when_no_emulator() {
        let base = temp_dir("term3");
        assert!(linux_terminal_plan_in_dirs("/repo", &[base]).is_none());
    }
}
