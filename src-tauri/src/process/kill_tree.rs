//! 进程树终止（R-09 取消传播，任务文档「架构/性能注意点」）。
//!
//! 用 sysinfo 枚举全系统进程，沿 parent 链收集 root 的全部后代，按
//! post-order（先叶子后 root）发送 kill；随后再枚举一轮兜底「枚举期间
//! 新 fork 出来」的竞态后代。Windows 没有进程组（pgid）语义，sysinfo
//! 的 parent 链枚举在两个平台上行为一致。

use std::collections::{HashMap, HashSet};

use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

/// 终止 `root_pid` 及其全部后代进程。root 已退出时只清理残余后代。
///
/// 使用 SIGKILL 语义（sysinfo `Process::kill`），不等待退出——调用方负责
/// 对 root 做 `wait()` reap。
pub fn kill_process_tree(root_pid: u32) {
    let root = Pid::from_u32(root_pid);
    // 两轮：第一轮杀当前可见的整棵树；短暂停顿后第二轮兜底竞态中
    // 新 fork 的后代（例如 mvn 脚本在被杀前刚好 exec 出 java）。
    for round in 0..2 {
        if round > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let mut system =
            System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
        system.refresh_processes();

        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for process in system.processes().values() {
            if let Some(parent) = process.parent() {
                children.entry(parent).or_default().push(process.pid());
            }
        }

        // DFS 收集 root 与其全部后代。
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(pid) = stack.pop() {
            if !seen.insert(pid) {
                continue;
            }
            order.push(pid);
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }

        // post-order：先杀叶子再杀 root，避免父进程先死后子进程被
        // reparent 到 init 而逃出 parent 链。
        for pid in order.iter().rev() {
            if let Some(process) = system.process(*pid) {
                process.kill();
            }
        }
    }
}

/// 向单个进程发送优雅终止信号（R-10 Stop，任务文档 §34「先发 SIGTERM 等效」）。
///
/// 只发信号、不等待退出；grace 超时后的进程树升级由调用方走
/// [`kill_process_tree`]。返回 `false` 表示平台无优雅信号语义（Windows 无
/// SIGTERM）或进程已不存在——调用方应直接升级为强杀进程树。
pub fn terminate_process(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let system = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::new()),
        );
        system
            .process(Pid::from_u32(pid))
            .and_then(|process| process.kill_with(sysinfo::Signal::Term))
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// 进程当前是否存活；`expected_start_time`（sysinfo `Process::start_time`，
/// epoch 秒）用于防 PID 复用：给定且不匹配时视为「原进程已死，PID 被复用」。
pub fn process_alive(pid: u32, expected_start_time: Option<u64>) -> bool {
    process_start_time(pid).is_some_and(|start| match expected_start_time {
        Some(expected) => start == expected,
        None => true,
    })
}

/// 读取进程的 `start_time`（epoch 秒）；进程不存在返回 `None`。
/// spawn 后记录该值，后续存活核对可防 PID 复用误判。
pub fn process_start_time(pid: u32) -> Option<u64> {
    let mut system = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    system.refresh_processes();
    system
        .process(Pid::from_u32(pid))
        .map(|process| process.start_time())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn kills_shell_and_its_children() {
        use std::process::{Command, Stdio};

        // sh 起一个子进程 sleep；杀掉 sh 的树后 sleep 也不应存活。
        let mut child = Command::new("sh")
            .args(["-c", "sleep 300 & wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let root_pid = child.id();
        // 给 sh 一点时间来 fork sleep。
        std::thread::sleep(std::time::Duration::from_millis(300));

        super::kill_process_tree(root_pid);
        let _ = child.wait();

        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut system = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::new()),
        );
        system.refresh_processes();
        let survivors: Vec<_> = system
            .processes()
            .values()
            .filter(|p| {
                p.name() == "sleep" && p.cmd().iter().any(|arg| arg == "300")
            })
            .collect();
        assert!(survivors.is_empty(), "sleep 300 should be killed: {survivors:?}");
    }

    #[cfg(unix)]
    #[test]
    fn terminate_process_sends_sigterm_and_probes_guard_pid_reuse() {
        use std::process::{Command, Stdio};

        // trap SIGTERM 并以退出码 0 退出，模拟 Spring Boot 优雅关闭。
        let mut child = Command::new("sh")
            .args(["-c", "trap 'exit 0' TERM; while true; do sleep 0.1; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();

        let start = super::process_start_time(pid).expect("spawned process must be visible");
        assert!(super::process_alive(pid, Some(start)));
        // start_time 不匹配 → 视为 PID 复用，不算存活。
        assert!(!super::process_alive(pid, Some(start.wrapping_add(1))));

        assert!(super::terminate_process(pid), "SIGTERM should be delivered");
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(0), "trap must turn SIGTERM into exit 0");

        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!super::process_alive(pid, Some(start)), "exited process is gone");
        assert!(!super::terminate_process(pid), "no signal target after exit");
    }
}
