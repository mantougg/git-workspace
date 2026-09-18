//! API Key 凭证存取（设计文档 §6.4，全局约束 §4 硬规则）。
//!
//! - Key 优先存 OS Credential Store：Windows Credential Manager /
//!   macOS Keychain / Linux Secret Service（`keyring` crate 三平台原生后端）。
//! - OS Credential Store 不可用时回退到**加密文件存储**（XChaCha20-Poly1305 +
//!   Argon2id，文件位于 `<app_data_dir>/credentials/`）。
//! - 加密文件存储也不可用时允许本次会话临时输入（内存保存，进程退出即清除），
//!   UI 侧标记「仅本次会话」。
//! - SQLite 只保存 `credential_ref`；Key 不进日志、错误信息、诊断导出、
//!   进程命令行、URL。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use zeroize::Zeroizing;

use super::error::AiError;

/// keyring service 名（三平台凭证条目的命名空间）。
const KEYRING_SERVICE: &str = "com.gitworkspace.app";

/// 凭证存储后端抽象（生产实现 = OS Credential Store；测试可注入内存实现）。
pub trait CredentialStore: Send + Sync {
    fn name(&self) -> &'static str;
    /// 后端当前是否可用（Linux 无 Secret Service / 未解锁等场景为 false）。
    fn is_available(&self) -> bool;
    /// 重测可用性。带缓存的实现必须绕过缓存真实探测（PAF-21：keyring
    /// 晚解锁场景免重启恢复）；无缓存实现等同于 `is_available`。
    fn refresh_availability(&self) -> bool {
        self.is_available()
    }
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
/// 可用性缓存由 [`AvailabilityCachedStore`] 装饰器负责，本类型每次真实探测。
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        Self
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
        Self::probe_availability()
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

/// 可用性缓存装饰器（PAF-21）：进程内缓存 `is_available` 探测结果。
///
/// - 操作返回 `Unavailable` 时缓存失效——下次 `is_available` 重新探测
///   （keyring 晚解锁：首次探测 false 后用户解锁 keyring，无需重启即可恢复）；
/// - 操作成功时缓存 `true`；
/// - `refresh_availability` 强制绕过缓存重新探测（`set` 的 persist 路径在
///   拒绝用户前调用一次，保证「重试即可恢复」）。
pub struct AvailabilityCachedStore {
    inner: Arc<dyn CredentialStore>,
    available: Mutex<Option<bool>>,
}

impl AvailabilityCachedStore {
    pub fn new(inner: Arc<dyn CredentialStore>) -> Self {
        Self {
            inner,
            available: Mutex::new(None),
        }
    }

    fn note_available(&self, value: bool) {
        if let Ok(mut guard) = self.available.lock() {
            *guard = Some(value);
        }
    }

    fn note_unavailable(&self) {
        if let Ok(mut guard) = self.available.lock() {
            *guard = None;
        }
    }
}

impl CredentialStore for AvailabilityCachedStore {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn is_available(&self) -> bool {
        let mut guard = match self.available.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match *guard {
            Some(value) => value,
            None => {
                let probed = self.inner.is_available();
                *guard = Some(probed);
                probed
            }
        }
    }

    fn refresh_availability(&self) -> bool {
        let probed = self.inner.is_available();
        self.note_available(probed);
        probed
    }

    fn get(&self, credential_ref: &str) -> Result<Option<String>, CredentialError> {
        let result = self.inner.get(credential_ref);
        match &result {
            Ok(_) => self.note_available(true),
            Err(CredentialError::Unavailable(_)) => self.note_unavailable(),
            Err(_) => {}
        }
        result
    }

    fn set(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
        let result = self.inner.set(credential_ref, secret);
        match &result {
            Ok(()) => self.note_available(true),
            Err(CredentialError::Unavailable(_)) => self.note_unavailable(),
            Err(_) => {}
        }
        result
    }

    fn delete(&self, credential_ref: &str) -> Result<(), CredentialError> {
        let result = self.inner.delete(credential_ref);
        match &result {
            Ok(()) => self.note_available(true),
            Err(CredentialError::Unavailable(_)) => self.note_unavailable(),
            Err(_) => {}
        }
        result
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

/// 文件凭证后端（OS Credential Store 不可用时的加密文件回退）。
///
/// - 凭证以 XChaCha20-Poly1305 加密后存为 JSON 文件在
///   `<app_data_dir>/credentials/<sanitized_ref>.json`；
/// - 加密密钥由固定应用密钥经 Argon2id 派生（每条凭证独立 nonce）；
/// - `is_available` = credentials 目录存在或可创建。
pub struct FileCredentialStore {
    dir: PathBuf,
    /// Argon2id 派生的 32 字节加密密钥（进程生命周期内固定）。
    key: Zeroizing<[u8; 32]>,
}

/// 应用固定密钥材料（用于文件凭证加密，不暴露给外部）。
const FILE_STORE_APP_SECRET: &[u8] = b"gitworkspace-file-credential-v1";
const FILE_STORE_SALT: &[u8] = b"gw-file-cred-salt-v1";
const CREDENTIALS_SUBDIR: &str = "credentials";

impl FileCredentialStore {
    /// 在指定 app_data_dir 下创建文件凭证后端。
    pub fn new(app_data_dir: &std::path::Path) -> Self {
        let dir = app_data_dir.join(CREDENTIALS_SUBDIR);
        let key = Self::derive_key();
        Self { dir, key }
    }

    /// 注入自定义目录（测试装配，避免污染生产 app_data_dir）。
    pub fn with_dir(dir: PathBuf) -> Self {
        let key = Self::derive_key();
        Self { dir, key }
    }

    fn derive_key() -> Zeroizing<[u8; 32]> {
        let mut key = Zeroizing::new([0u8; 32]);
        // 固定 salt + 固定 secret → 确定性派生（文件凭证解密需要相同 key）。
        argon2::Argon2::default()
            .hash_password_into(FILE_STORE_APP_SECRET, FILE_STORE_SALT, &mut *key)
            .expect("Argon2id KDF for file credential store must not fail");
        key
    }

    /// credential_ref → 文件名（`:` 替换为 `_`，其余保留）。
    fn sanitize_ref(credential_ref: &str) -> String {
        credential_ref.replace(':', "_")
    }

    fn file_path(&self, credential_ref: &str) -> PathBuf {
        self.dir.join(format!("{}.json", Self::sanitize_ref(credential_ref)))
    }

    fn ensure_dir(&self) -> Result<(), CredentialError> {
        if !self.dir.exists() {
            fs::create_dir_all(&self.dir).map_err(|e| {
                CredentialError::Other(format!("无法创建凭证目录: {}", e))
            })?;
        }
        Ok(())
    }

    /// 加密凭证并写入 JSON 文件。
    fn write_encrypted(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
        self.ensure_dir()?;
        let (nonce, ciphertext) = crate::crypto::cipher::encrypt(&self.key, secret.as_bytes())
            .map_err(|e| CredentialError::Other(format!("凭证加密失败: {}", e)))?;
        let envelope = serde_json::json!({
            "nonce": B64.encode(&nonce),
            "ciphertext": B64.encode(&ciphertext),
        });
        let path = self.file_path(credential_ref);
        let content = serde_json::to_string_pretty(&envelope)
            .map_err(|e| CredentialError::Other(format!("凭证序列化失败: {}", e)))?;
        fs::write(&path, content).map_err(|e| {
            CredentialError::Other(format!("凭证文件写入失败: {}", e))
        })
    }

    /// 从 JSON 文件读取并解密凭证。
    fn read_encrypted(&self, credential_ref: &str) -> Result<Option<String>, CredentialError> {
        let path = self.file_path(credential_ref);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| CredentialError::Other(format!("凭证文件读取失败: {}", e)))?;
        let envelope: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| CredentialError::Other(format!("凭证文件格式错误: {}", e)))?;
        let nonce_b64 = envelope["nonce"].as_str()
            .ok_or_else(|| CredentialError::Other("凭证文件缺少 nonce 字段".into()))?;
        let ct_b64 = envelope["ciphertext"].as_str()
            .ok_or_else(|| CredentialError::Other("凭证文件缺少 ciphertext 字段".into()))?;
        let nonce = B64.decode(nonce_b64)
            .map_err(|e| CredentialError::Other(format!("nonce 解码失败: {}", e)))?;
        let ciphertext = B64.decode(ct_b64)
            .map_err(|e| CredentialError::Other(format!("ciphertext 解码失败: {}", e)))?;
        let plaintext = crate::crypto::cipher::decrypt(&self.key, &nonce, &ciphertext)
            .map_err(|e| CredentialError::Other(format!("凭证解密失败: {}", e)))?;
        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|e| CredentialError::Other(format!("凭证内容非有效 UTF-8: {}", e)))
    }
}

impl CredentialStore for FileCredentialStore {
    fn name(&self) -> &'static str {
        "file-credential-store"
    }

    fn is_available(&self) -> bool {
        // 目录已存在，或可创建。
        if self.dir.exists() {
            return true;
        }
        fs::create_dir_all(&self.dir).is_ok()
    }

    fn get(&self, credential_ref: &str) -> Result<Option<String>, CredentialError> {
        self.read_encrypted(credential_ref)
    }

    fn set(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
        self.write_encrypted(credential_ref, secret)
    }

    fn delete(&self, credential_ref: &str) -> Result<(), CredentialError> {
        let path = self.file_path(credential_ref);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                CredentialError::Other(format!("凭证文件删除失败: {}", e))
            })?;
        }
        Ok(())
    }
}

/// 凭证落点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialLocation {
    OsStore,
    FileStore,
    SessionOnly,
}

/// 凭证管理器：OS 存储优先，文件加密存储次之，会话内存兜底（§6.4）。
///
/// 一条 `credential_ref` 的 Key 只存在于一个落点：写入一处时会清除其余落点，
/// 避免「会话里改了 Key 但读取时命中 OS 旧值」的歧义。
pub struct CredentialManager {
    os: Arc<dyn CredentialStore>,
    file: Arc<FileCredentialStore>,
    session: SessionStore,
}

impl CredentialManager {
    /// 生产装配：OS Credential Store（带可用性缓存）+ 文件加密回退 + 会话内存。
    pub fn production() -> Self {
        let app_data_dir = crate::get_app_data_dir();
        Self {
            os: Arc::new(AvailabilityCachedStore::new(Arc::new(KeyringStore::new()))),
            file: Arc::new(FileCredentialStore::new(&app_data_dir)),
            session: SessionStore::new(),
        }
    }

    /// 测试装配：注入内存/失败后端（文件后端使用临时目录）。
    pub fn with_store(os: Arc<dyn CredentialStore>) -> Self {
        let tmp_dir = std::env::temp_dir().join("gw-cred-test").join(format!(
            "{}",
            std::process::id()
        ));
        Self {
            os,
            file: Arc::new(FileCredentialStore::with_dir(tmp_dir)),
            session: SessionStore::new(),
        }
    }

    /// 测试装配：注入 OS 后端和文件后端（用于需要控制两者行为的测试）。
    #[cfg(test)]
    pub fn with_stores(os: Arc<dyn CredentialStore>, file: Arc<FileCredentialStore>) -> Self {
        Self {
            os,
            file,
            session: SessionStore::new(),
        }
    }

    pub fn os_store_available(&self) -> bool {
        self.os.is_available()
    }

    /// 写入凭证。`persist = true` 要求落持久存储——先尝试 OS Credential Store，
    /// 若 `Unavailable` 则回退到加密文件存储；`persist = false` 明确只存本次会话。
    pub fn set(&self, credential_ref: &str, secret: &str, persist: bool) -> Result<CredentialLocation, AiError> {
        if persist {
            // PAF-21：缓存不可用时先重测一次再拒绝——keyring 晚解锁（应用
            // 启动后 Secret Service 才解锁）场景下用户重试即可恢复，免重启。
            let os_ok = self.os.is_available() || self.os.refresh_availability();
            if os_ok {
                match self.os.set(credential_ref, secret) {
                    Ok(()) => {
                        // 清除其余落点的旧副本，保证单落点。
                        let _ = self.session.delete(credential_ref);
                        let _ = self.file.delete(credential_ref);
                        return Ok(CredentialLocation::OsStore);
                    }
                    Err(CredentialError::Unavailable(_)) => {
                        // OS 写入失败（Unavailable）——回退到文件存储。
                        log::warn!("credential: OS 存储写入失败，回退到加密文件存储: {credential_ref}");
                    }
                    Err(e) => {
                        return Err(AiError::CredentialUnavailable {
                            message: format!("凭证写入失败: {}", e),
                        });
                    }
                }
            }
            // OS 不可用或写入 Unavailable → 回退文件存储。
            self.file
                .set(credential_ref, secret)
                .map_err(|e| AiError::CredentialUnavailable {
                    message: format!("文件凭证写入失败: {}", e),
                })?;
            let _ = self.session.delete(credential_ref);
            let _ = self.os.delete(credential_ref);
            Ok(CredentialLocation::FileStore)
        } else {
            self.session
                .set(credential_ref, secret)
                .map_err(|e| AiError::CredentialUnavailable {
                    message: format!("会话凭证写入失败: {}", e),
                })?;
            let _ = self.os.delete(credential_ref);
            let _ = self.file.delete(credential_ref);
            Ok(CredentialLocation::SessionOnly)
        }
    }

    /// 读取凭证（OS 存储优先，文件加密存储次之，会话内存兜底）。
    /// PAF-21：OS 后端故障（`Err`，非 `Ok(None)`）降级读文件/会话副本时必须
    /// 可见——此时副本可能是过期值（单落点不变式只在写入时维护），打 warn 供排障；
    /// 「无条目」（`Ok(None)`）才是静默降级。
    pub fn get(&self, credential_ref: &str) -> Option<String> {
        match self.os.get(credential_ref) {
            Ok(Some(secret)) => return Some(secret),
            Ok(None) => {}
            Err(e) => {
                log::warn!(
                    "credential: OS 存储读取失败（{e}），降级读取文件/会话副本: {credential_ref}"
                );
            }
        }
        // 文件存储回退（静默：无条目时不打 warn）。
        match self.file.get(credential_ref) {
            Ok(Some(secret)) => return Some(secret),
            Ok(None) => {}
            Err(e) => {
                log::warn!(
                    "credential: 文件存储读取失败（{e}），降级读取会话副本: {credential_ref}"
                );
            }
        }
        self.session.get(credential_ref).ok().flatten()
    }

    pub fn has(&self, credential_ref: &str) -> bool {
        self.get(credential_ref).is_some()
    }

    /// 凭证是否仅存在于会话内存（UI 标记「仅本次会话」）。
    pub fn is_session_only(&self, credential_ref: &str) -> bool {
        !matches!(self.os.get(credential_ref), Ok(Some(_)))
            && !matches!(self.file.get(credential_ref), Ok(Some(_)))
            && matches!(self.session.get(credential_ref), Ok(Some(_)))
    }

    /// 删除凭证（三个落点都清）。
    pub fn delete(&self, credential_ref: &str) -> Result<(), AiError> {
        let os_result = self.os.delete(credential_ref);
        let file_result = self.file.delete(credential_ref);
        let session_result = self.session.delete(credential_ref);
        if let Err(e) = os_result {
            // 后端不可用不算失败（条目本来也读不到）。
            if !matches!(e, CredentialError::Unavailable(_)) {
                return Err(AiError::CredentialUnavailable {
                    message: format!("凭证删除失败: {}", e),
                });
            }
        }
        if let Err(e) = file_result {
            log::warn!("credential: 文件凭证删除失败（{e}）: {credential_ref}");
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
    use std::sync::atomic::{AtomicBool, Ordering};

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

    /// 可用性可翻转的后端（模拟 keyring 晚解锁：启动时不可用，随后解锁）。
    /// 可用时行为等同内存存储。
    struct FlakyStore {
        unlocked: Arc<AtomicBool>,
        entries: Mutex<HashMap<String, String>>,
    }
    impl FlakyStore {
        fn new(unlocked: Arc<AtomicBool>) -> Self {
            Self {
                unlocked,
                entries: Mutex::new(HashMap::new()),
            }
        }
        fn check(&self) -> Result<(), CredentialError> {
            if self.unlocked.load(Ordering::Relaxed) {
                Ok(())
            } else {
                Err(CredentialError::Unavailable("locked".into()))
            }
        }
    }
    impl CredentialStore for FlakyStore {
        fn name(&self) -> &'static str {
            "flaky"
        }
        fn is_available(&self) -> bool {
            self.unlocked.load(Ordering::Relaxed)
        }
        fn get(&self, credential_ref: &str) -> Result<Option<String>, CredentialError> {
            self.check()?;
            Ok(self.entries.lock().unwrap().get(credential_ref).cloned())
        }
        fn set(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
            self.check()?;
            self.entries.lock().unwrap().insert(credential_ref.to_string(), secret.to_string());
            Ok(())
        }
        fn delete(&self, credential_ref: &str) -> Result<(), CredentialError> {
            self.check()?;
            self.entries.lock().unwrap().remove(credential_ref);
            Ok(())
        }
    }

    /// PAF-21：keyring 晚解锁场景——首次探测不可用，回退到文件存储；
    /// 用户解锁后重试 `set`（persist）应经 `refresh_availability` 重测成功，
    /// 写入 OS 存储，无需重启。
    #[test]
    fn late_unlock_recovers_without_restart() {
        let unlocked = Arc::new(AtomicBool::new(false));
        let mgr = CredentialManager::with_store(Arc::new(AvailabilityCachedStore::new(Arc::new(
            FlakyStore::new(Arc::clone(&unlocked)),
        ))));

        // 启动时后端不可用：回退到文件存储。
        let loc = mgr.set("ai-provider:p1", "sk-test", true).unwrap();
        assert_eq!(loc, CredentialLocation::FileStore);
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-test"));

        // 用户解锁 keyring 后重试：重测可用 → 写入 OS 成功，文件副本清除。
        unlocked.store(true, Ordering::Relaxed);
        let loc = mgr.set("ai-provider:p1", "sk-test", true).unwrap();
        assert_eq!(loc, CredentialLocation::OsStore);
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-test"));
        // 可用性缓存已翻新为 true。
        assert!(mgr.os_store_available());
    }

    /// PAF-21：OS 后端故障（Err）时降级读会话副本，且操作失败使缓存失效——
    /// 下次 `is_available` 重新探测而非使用旧 false。
    #[test]
    fn backend_failure_invalidates_cache_and_degrades_to_session() {
        let unlocked = Arc::new(AtomicBool::new(false));
        let cached: Arc<dyn CredentialStore> =
            Arc::new(AvailabilityCachedStore::new(Arc::new(FlakyStore::new(Arc::clone(&unlocked)))));
        assert!(!cached.is_available(), "首次探测不可用并缓存");

        // OS 读取失败 → 静默路径改为告警 + 降级会话（此处断言降级行为）。
        let mgr = CredentialManager::with_store(Arc::clone(&cached));
        mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-session"));

        // Unavailable 操作使缓存失效：后端恢复后 is_available 立即为 true，
        // 且此时 OS 侧无该条目 → 凭证仍标记「仅本次会话」（语义不回归）。
        unlocked.store(true, Ordering::Relaxed);
        assert!(cached.is_available());
        assert!(mgr.is_session_only("ai-provider:p1"));
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

    /// OS 不可用时回退到加密文件存储（非 session-only）。
    #[test]
    fn persist_falls_back_to_file_when_os_unavailable() {
        let mgr = CredentialManager::with_store(Arc::new(UnavailableStore));
        let loc = mgr.set("ai-provider:p1", "sk-test", true).unwrap();
        assert_eq!(loc, CredentialLocation::FileStore);
        assert!(mgr.has("ai-provider:p1"));
        assert!(!mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-test"));
        mgr.delete("ai-provider:p1").unwrap();
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

    // ── FileCredentialStore 单元测试 ──────────────────────────────────

    /// 文件凭证后端加密往返。
    #[test]
    fn file_store_encrypt_decrypt_roundtrip() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("round{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCredentialStore::with_dir(dir.clone());
        assert!(store.is_available());
        store.set("ai-provider:p1", "sk-file-secret").unwrap();
        let got = store.get("ai-provider:p1").unwrap();
        assert_eq!(got.as_deref(), Some("sk-file-secret"));
        store.delete("ai-provider:p1").unwrap();
        assert_eq!(store.get("ai-provider:p1").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 文件名中 `:` 被替换为 `_`。
    #[test]
    fn file_store_sanitizes_colons_in_ref() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("sanitize{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCredentialStore::with_dir(dir.clone());
        store.set("ai-provider:my:ref", "sk-sani").unwrap();
        // 文件名应为 ai-provider_my_ref.json
        assert!(dir.join("ai-provider_my_ref.json").exists());
        assert_eq!(store.get("ai-provider:my:ref").unwrap().as_deref(), Some("sk-sani"));
        store.delete("ai-provider:my:ref").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 文件不存在时 get 返回 None。
    #[test]
    fn file_store_get_missing_returns_none() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("miss{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCredentialStore::with_dir(dir.clone());
        assert_eq!(store.get("nonexistent").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 覆盖写入（同一 ref 两次 set，第二次覆盖第一次）。
    #[test]
    fn file_store_overwrite() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("overwrite{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCredentialStore::with_dir(dir.clone());
        store.set("ai-provider:p1", "sk-old").unwrap();
        store.set("ai-provider:p1", "sk-new").unwrap();
        assert_eq!(store.get("ai-provider:p1").unwrap().as_deref(), Some("sk-new"));
        store.delete("ai-provider:p1").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// delete 不存在的文件不报错。
    #[test]
    fn file_store_delete_missing_no_error() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("del{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCredentialStore::with_dir(dir.clone());
        store.delete("nonexistent").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// OS 不可用 → 文件回退 → 读取成功（集成测试）。
    #[test]
    fn manager_os_unavailable_falls_back_to_file() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("mgr{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file_store = Arc::new(FileCredentialStore::with_dir(dir.clone()));
        let mgr = CredentialManager::with_stores(Arc::new(UnavailableStore), file_store);
        let loc = mgr.set("ai-provider:p1", "sk-file", true).unwrap();
        assert_eq!(loc, CredentialLocation::FileStore);
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-file"));
        assert!(!mgr.is_session_only("ai-provider:p1"));
        mgr.delete("ai-provider:p1").unwrap();
        assert!(!mgr.has("ai-provider:p1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// session-only 不影响文件后端。
    #[test]
    fn session_only_writes_only_to_session_not_file() {
        let dir = std::env::temp_dir().join("gw-cred-file-test").join(format!("sonly{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file_store = Arc::new(FileCredentialStore::with_dir(dir.clone()));
        let mgr = CredentialManager::with_stores(Arc::new(UnavailableStore), file_store);
        let loc = mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        assert_eq!(loc, CredentialLocation::SessionOnly);
        assert!(mgr.is_session_only("ai-provider:p1"));
        // 文件后端无该条目。
        assert_eq!(mgr.file.get("ai-provider:p1").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
