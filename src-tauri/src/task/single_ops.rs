//! GF-07：单仓网络操作的取消注册表。
//!
//! `sync_fetch/sync_pull/sync_push/smart_pull` 与 `push_branch` 是「即发即忘」
//! 的单仓命令，不走任务队列（生命周期讨论见 GF-14），但仍需要与队列
//! `cancel_flags` 同模式的取消通道：前端拿 `op_id`（由命令入参传入或后端
//! 生成并经 `git_op_started` 事件下发）调 `cancel_git_op` 置位；
//! `run_git_streaming` 每 50ms 轮询一次，置位即杀 git 进程树。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

/// op_id → 取消 flag。条目随操作结束（含 panic）经 [`SingleOpGuard`] 摘除，
/// 不会无限增长。
#[derive(Debug, Default)]
pub struct SingleOpRegistry {
    flags: DashMap<String, Arc<AtomicBool>>,
}

impl SingleOpRegistry {
    /// 登记一个 op，返回其取消 flag。同一 op_id 重复登记复用同一 flag
    /// （前端生成的 UUID 碰撞可忽略）。
    pub fn register(&self, op_id: &str) -> Arc<AtomicBool> {
        self.flags
            .entry(op_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// 取消一个进行中的 op：置位 flag。返回 `false` 表示 op 不存在——
    /// 通常已自然结束（取消入口应随之消失）。
    pub fn cancel(&self, op_id: &str) -> bool {
        match self.flags.get(op_id) {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// 提前摘除（命令收尾调用；`SingleOpGuard` drop 时也会调）。
    pub fn remove(&self, op_id: &str) {
        self.flags.remove(op_id);
    }

    /// 当前进行中的 op id 列表（诊断 / 测试用）。
    pub fn active_ids(&self) -> Vec<String> {
        self.flags.iter().map(|e| e.key().clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.flags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }
}

/// RAII 守卫：持表 + op_id，drop 时保证摘除——即使 `spawn_blocking` panic
/// （join error 路径）也不会把 flag 遗留在表里。
pub struct SingleOpGuard {
    registry: Arc<SingleOpRegistry>,
    op_id: String,
}

impl SingleOpGuard {
    pub fn new(registry: Arc<SingleOpRegistry>, op_id: String) -> Self {
        SingleOpGuard { registry, op_id }
    }
}

impl Drop for SingleOpGuard {
    fn drop(&mut self) {
        self.registry.remove(&self.op_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_remove_roundtrip() {
        let reg = SingleOpRegistry::default();
        let flag = reg.register("op-1");
        assert!(!flag.load(Ordering::Relaxed));
        assert_eq!(reg.active_ids(), vec!["op-1".to_string()]);
        assert_eq!(reg.len(), 1);

        assert!(reg.cancel("op-1"));
        assert!(flag.load(Ordering::Relaxed), "cancel must set the flag");

        reg.remove("op-1");
        assert!(reg.is_empty());
    }

    /// 取消不存在的 op（已结束 / 从未登记）返回 false——前端据此静默移除
    /// 取消入口，不当成错误打断用户。
    #[test]
    fn cancel_unknown_op_returns_false() {
        let reg = SingleOpRegistry::default();
        assert!(!reg.cancel("nope"));
    }

    /// 同一 op_id 重复登记复用同一 flag：先登记者置位后，后拿到的 flag
    /// 也必须看得到（entry or_insert_with 语义）。
    #[test]
    fn duplicate_register_reuses_same_flag() {
        let reg = SingleOpRegistry::default();
        let a = reg.register("op-1");
        let b = reg.register("op-1");
        a.store(true, Ordering::Relaxed);
        assert!(b.load(Ordering::Relaxed));
        assert_eq!(reg.len(), 1, "duplicate register must not add entries");
    }

    /// Guard drop（含 panic 路径的 unwind）必须摘除条目，表不能泄漏。
    #[test]
    fn guard_removes_entry_on_drop() {
        let reg = Arc::new(SingleOpRegistry::default());
        {
            let _guard = SingleOpGuard::new(Arc::clone(&reg), "op-1".to_string());
            reg.register("op-1");
            assert_eq!(reg.len(), 1);
        }
        assert!(reg.is_empty(), "guard drop must remove the entry");
    }
}
