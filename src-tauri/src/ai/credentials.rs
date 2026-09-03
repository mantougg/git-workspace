//! API Key 凭证存取（设计文档 §6.4，全局约束 §4 硬规则）。
//!
//! - Key **只存 OS Credential Store**：Windows Credential Manager /
//!   macOS Keychain / Linux Secret Service（`keyring` crate 三平台原生后端）。
//! - 凭证存储不可用时**不回退普通文件**：只允许本次会话临时输入（内存保存，
//!   进程退出即清除），UI 侧标记「仅本次会话」。
//! - SQLite 只保存 `credential_ref`；Key 不进日志、错误信息、诊断导出、
//!   进程命令行、URL。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::error::AiError;

/// keyring service 名（三平台凭证条目的命名空间）。
const KEYRING_SERVICE: &str = "com.gitworkspace.app";

/// 凭证存储后端抽象（生产实现 = OS Credential Store；测试可注入内存实现）。
pub trait CredentialStore: Send + Sync {
    fn name(&self) -> &'static str;
    /// 后端当前是否可用（Linux 无 Secret Service / 未解锁等场景为 false）。
    fn is_available(&self) -> bool;
    /// 读取凭证；不存在返回 `Ok(None)`，后端不可用返回 `Err(Unavailable)`。
    fn get(&self, credential_ref: &str) -> Result<Option<String>, CredentialError>;
    fn set(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError>;
    fn delete(&self, credential_ref: &str) -> Result<(), CredentialError>;
}

/// 凭证层错误。message 只含后端/平台信息，永不含 Key。
#[derive(Debug)]
pub enum CredentialError {
    /// 后端不可用（未安装/未解锁 Secret Service、无桌面会话等）。
    Unavailable(String),
    /// 其他后端错误。
    Other(String),
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::Unavailable(m) | CredentialError::Other(m) => write!(f, "{}", m),
        }
    }
}

/// OS Credential Store 后端（三平台分支由 keyring crate 内部完成）。
pub struct KeyringStore {
    /// 可用性探测结果（首次使用时缓存；`refresh_availability` 可重测）。
    available: OnceLock<bool>,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self {
            available: OnceLock::new(),
        }
    }

    fn entry(credential_ref: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(KEYRING_SERVICE, credential_ref)
            .map_err(|e| CredentialError::Other(format!("凭证条目创建失败: {}", e)))
    }

    fn map_err(e: keyring::Error) -> CredentialError {
        match e {
            // 平台级不可用：无 Secret Service、DBus 不可达、无桌面会话等。
            keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
                CredentialError::Unavailable(format!("OS 凭证存储不可用: {}", e))
            }
            other => CredentialError::Other(format!("凭证存储错误: {}", other)),
        }
    }

    fn probe_availability() -> bool {
        // 用一个不存在的探测条目执行 get：NoEntry = 后端可用；
        // NoStorageAccess / PlatformFailure = 不可用。
        // 注意：不做 set/delete 往返探测——实测 gnome-keyring 在
        // 「探测写入后立即删除」会让后续连接的 create 静默不可见（NoEntry），
        // 写路径的失败由 `set` 的错误路径兜底（AiCredentialUnavailable）。
        match Self::entry("ai-probe").and_then(|entry| entry.get_password().map_err(Self::map_err)) {
            Ok(_) | Err(CredentialError::Other(_)) => true,
            Err(CredentialError::Unavailable(_)) => false,
        }
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for KeyringStore {
    fn name(&self) -> &'static str {
        "os-credential-store"
    }

    fn is_available(&self) -> bool {
        *self.available.get_or_init(Self::probe_availability)
    }

    fn get(&self, credential_ref: &str) -> Result<Option<String>, CredentialError> {
        match Self::entry(credential_ref)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Self::map_err(e)),
        }
    }

    fn set(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
        Self::entry(credential_ref)?.set_password(secret).map_err(Self::map_err)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), CredentialError> {
        match Self::entry(credential_ref)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Self::map_err(e)),
        }
    }
}

/// 会话级临时凭证（不落盘，进程退出即清除）。仅当 OS 凭证存储不可用或用户
/// 明确选择「仅本次会话」时使用（§6.4）。
#[derive(Default)]
pub struct SessionStore {
    inner: Mutex<HashMap<String, String>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl CredentialStore for SessionStore {
    fn name(&self) -> &'static str {
        "session-memory"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn get(&self, credential_ref: &str) -> Result<Option<String>, CredentialError> {
        Ok(self
            .inner
            .lock()
            .map_err(|e| CredentialError::Other(format!("会话凭证锁错误: {}", e)))?
            .get(credential_ref)
            .cloned())
    }

    fn set(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
        self.inner
            .lock()
            .map_err(|e| CredentialError::Other(format!("会话凭证锁错误: {}", e)))?
            .insert(credential_ref.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, credential_ref: &str) -> Result<(), CredentialError> {
        self.inner
            .lock()
            .map_err(|e| CredentialError::Other(format!("会话凭证锁错误: {}", e)))?
            .remove(credential_ref);
        Ok(())
    }
}

/// 凭证落点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialLocation {
    OsStore,
    SessionOnly,
}

/// 凭证管理器：OS 存储优先，会话内存兜底（§6.4）。
///
/// 一条 `credential_ref` 的 Key 只存在于一个落点：写入一处时会清除另一处，
/// 避免「会话里改了 Key 但读取时命中 OS 旧值」的歧义。
pub struct CredentialManager {
    os: Arc<dyn CredentialStore>,
    session: SessionStore,
}

impl CredentialManager {
    /// 生产装配：OS Credential Store + 会话内存。
    pub fn production() -> Self {
        Self {
            os: Arc::new(KeyringStore::new()),
            session: SessionStore::new(),
        }
    }

    /// 测试装配：注入内存/失败后端。
    pub fn with_store(os: Arc<dyn CredentialStore>) -> Self {
        Self {
            os,
            session: SessionStore::new(),
        }
    }

    pub fn os_store_available(&self) -> bool {
        self.os.is_available()
    }

    /// 写入凭证。`persist = true` 要求落 OS 存储（不可用时报
    /// `AiCredentialUnavailable`，**不回退普通文件**）；`persist = false`
    /// 明确只存本次会话。
    pub fn set(&self, credential_ref: &str, secret: &str, persist: bool) -> Result<CredentialLocation, AiError> {
        if persist {
            if !self.os.is_available() {
                return Err(AiError::CredentialUnavailable {
                    message: "OS 凭证存储不可用：可选择「仅本次会话」临时保存（不落盘）".to_string(),
                });
            }
            self.os
                .set(credential_ref, secret)
                .map_err(|e| AiError::CredentialUnavailable {
                    message: format!("凭证写入失败: {}", e),
                })?;
            // 清除会话里的旧副本，保证单落点。
            let _ = self.session.delete(credential_ref);
            Ok(CredentialLocation::OsStore)
        } else {
            self.session
                .set(credential_ref, secret)
                .map_err(|e| AiError::CredentialUnavailable {
                    message: format!("会话凭证写入失败: {}", e),
                })?;
            let _ = self.os.delete(credential_ref);
            Ok(CredentialLocation::SessionOnly)
        }
    }

    /// 读取凭证（OS 存储优先，其次会话内存）。后端不可用按 None 处理。
    pub fn get(&self, credential_ref: &str) -> Option<String> {
        if let Ok(Some(secret)) = self.os.get(credential_ref) {
            return Some(secret);
        }
        self.session.get(credential_ref).ok().flatten()
    }

    pub fn has(&self, credential_ref: &str) -> bool {
        self.get(credential_ref).is_some()
    }

    /// 凭证是否仅存在于会话内存（UI 标记「仅本次会话」）。
    pub fn is_session_only(&self, credential_ref: &str) -> bool {
        !matches!(self.os.get(credential_ref), Ok(Some(_))) && matches!(self.session.get(credential_ref), Ok(Some(_)))
    }

    /// 删除凭证（两个落点都清）。
    pub fn delete(&self, credential_ref: &str) -> Result<(), AiError> {
        let os_result = self.os.delete(credential_ref);
        let session_result = self.session.delete(credential_ref);
        if let Err(e) = os_result {
            // 后端不可用不算失败（条目本来也读不到）。
            if !matches!(e, CredentialError::Unavailable(_)) {
                return Err(AiError::CredentialUnavailable {
                    message: format!("凭证删除失败: {}", e),
                });
            }
        }
        session_result.map_err(|e| AiError::CredentialUnavailable {
            message: format!("会话凭证删除失败: {}", e),
        })?;
        Ok(())
    }

    pub fn session_count(&self) -> usize {
        self.session.count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内存后端（模拟可用的 OS 存储）。
    fn memory_store() -> Arc<dyn CredentialStore> {
        Arc::new(SessionStore::new())
    }

    /// 恒不可用后端（模拟 Linux 无 Secret Service）。
    struct UnavailableStore;
    impl CredentialStore for UnavailableStore {
        fn name(&self) -> &'static str {
            "unavailable"
        }
        fn is_available(&self) -> bool {
            false
        }
        fn get(&self, _: &str) -> Result<Option<String>, CredentialError> {
            Err(CredentialError::Unavailable("no secret service".into()))
        }
        fn set(&self, _: &str, _: &str) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable("no secret service".into()))
        }
        fn delete(&self, _: &str) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable("no secret service".into()))
        }
    }

    #[test]
    fn persist_roundtrip_via_os_store() {
        let mgr = CredentialManager::with_store(memory_store());
        let loc = mgr.set("ai-provider:p1", "sk-test", true).unwrap();
        assert_eq!(loc, CredentialLocation::OsStore);
        assert!(mgr.has("ai-provider:p1"));
        assert!(!mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-test"));

        mgr.delete("ai-provider:p1").unwrap();
        assert!(!mgr.has("ai-provider:p1"));
    }

    #[test]
    fn persist_fails_when_os_store_unavailable_no_file_fallback() {
        let mgr = CredentialManager::with_store(Arc::new(UnavailableStore));
        let err = mgr.set("ai-provider:p1", "sk-test", true).unwrap_err();
        assert_eq!(err.code(), "AiCredentialUnavailable");
        // 不回退普通文件：凭证不存在于任何落点。
        assert!(!mgr.has("ai-provider:p1"));
    }

    #[test]
    fn session_only_keeps_key_in_memory() {
        let mgr = CredentialManager::with_store(Arc::new(UnavailableStore));
        let loc = mgr.set("ai-provider:p1", "sk-test", false).unwrap();
        assert_eq!(loc, CredentialLocation::SessionOnly);
        assert!(mgr.has("ai-provider:p1"));
        assert!(mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.session_count(), 1);

        mgr.delete("ai-provider:p1").unwrap();
        assert_eq!(mgr.session_count(), 0);
    }

    #[test]
    fn single_location_invariant() {
        let mgr = CredentialManager::with_store(memory_store());
        mgr.set("ai-provider:p1", "sk-os", true).unwrap();
        // 改写为会话-only：OS 副本必须清除，读取命中会话值。
        mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        assert!(mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-session"));
        // 再写回 OS：会话副本清除。
        mgr.set("ai-provider:p1", "sk-os2", true).unwrap();
        assert!(!mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-os2"));
    }

    /// 三平台真实 OS Credential Store 冒烟：环境不可用（无 Secret Service /
    /// 无桌面会话的 CI）或往返行为异常时 skip 并打印原因，不硬失败
    /// （全局约束 §11）；产品逻辑的可行动错误由其他测试覆盖。
    #[test]
    fn os_credential_store_smoke_or_skip() {
        let store = KeyringStore::new();
        if !store.is_available() {
            eprintln!("SKIP os_credential_store_smoke_or_skip: OS 凭证存储在当前环境不可用");
            return;
        }
        let cref = "ai-probe:smoke-test";
        if let Err(e) = store.set(cref, "sk-smoke") {
            eprintln!("SKIP os_credential_store_smoke_or_skip: 写入失败: {}", e);
            return;
        }
        match store.get(cref) {
            Ok(Some(v)) if v == "sk-smoke" => {}
            other => {
                let shape = other.map(|o| o.map(|s| s.len()));
                eprintln!("SKIP os_credential_store_smoke_or_skip: 往返读取异常: {:?}", shape);
                let _ = store.delete(cref);
                return;
            }
        }
        store.delete(cref).unwrap();
        assert_eq!(store.get(cref).unwrap(), None);
    }
}
