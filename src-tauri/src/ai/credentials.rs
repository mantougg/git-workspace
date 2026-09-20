//! API Key 凭证存取（设计文档 §6.4，全局约束 §4 硬规则）。
//!
//! - **持久存储固定使用加密文件**（F-50 用户决策）：XChaCha20-Poly1305 +
//!   Argon2id，落盘 `~/.gitworkspace/credentials/`（对齐 `.claude`/`.codex`
//!   的用户主目录习惯；不再使用 OS Credential Store——Windows Credential
//!   Manager 有写入失败/兼容性问题）。
//! - OS Credential Store 仅作**一次性迁移源**：文件未命中时试读 OS，命中即
//!   写入加密文件并删除 OS 条目（存量用户升级无感）。
//! - 加密文件也不可用时允许本次会话临时输入（内存保存，进程退出即清除），
//!   UI 侧标记「仅本次会话」。
//! - SQLite 只保存 `credential_ref`；Key 不进日志、错误信息、诊断导出、
//!   进程命令行、URL。
//!
//! 安全边界：加密密钥由代码内固定材料派生（防明文落盘），与 `.claude`/
//! `.codex` 同级——防不住同时拿到文件与程序本体的本机攻击者。

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

/// 会话级临时凭证（不落盘，进程退出即清除）。仅当用户明确选择「仅本次会话」
/// 时使用（§6.4）。
#[derive(Default)]
pub struct SessionStore {
    inner: Mutex<HashMap<String, String>>,
}

/// 恒不可用的 OS 迁移源（测试装配：模拟无 OS 凭证残留 / 无 Secret Service）。
struct UnavailableOsStore;

impl CredentialStore for UnavailableOsStore {
    fn name(&self) -> &'static str {
        "unavailable"
    }
    fn is_available(&self) -> bool {
        false
    }
    fn get(&self, _: &str) -> Result<Option<String>, CredentialError> {
        Err(CredentialError::Unavailable("no os credential store".into()))
    }
    fn set(&self, _: &str, _: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable("no os credential store".into()))
    }
    fn delete(&self, _: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable("no os credential store".into()))
    }
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

/// 加密文件凭证的规范落盘目录（F-50）：`~/.gitworkspace/credentials/`——
/// 对齐 `.claude`/`.codex` 的用户主目录习惯；凭证是全局的，不放工作区级
/// `.gitworkspace/`（后者已被 runtimes/environments 占用）。home 不可用时
/// 回退旧版 `<app_data_dir>/credentials/`。
pub fn canonical_file_store_dir() -> PathBuf {
    match dirs::home_dir() {
        Some(home) => home.join(".gitworkspace").join(CREDENTIALS_SUBDIR),
        None => legacy_file_store_dir(),
    }
}

/// 旧版文件凭证目录（F-50 前：`<app_data_dir>/credentials/`），启动时整体搬迁。
fn legacy_file_store_dir() -> PathBuf {
    crate::get_app_data_dir().join(CREDENTIALS_SUBDIR)
}

impl FileCredentialStore {
    /// 在指定 app_data_dir 下创建文件凭证后端。
    pub fn new(app_data_dir: &std::path::Path) -> Self {
        let dir = app_data_dir.join(CREDENTIALS_SUBDIR);
        let key = Self::derive_key();
        Self { dir, key }
    }

    /// 注入自定义目录（测试装配，避免污染生产目录）。
    pub fn with_dir(dir: PathBuf) -> Self {
        let key = Self::derive_key();
        Self { dir, key }
    }

    /// 把旧目录下已有的凭证文件搬迁到本目录（目标已存在则跳过；失败不
    /// 阻塞——旧文件原地保留，用户可自行清理）。
    pub fn migrate_from(&self, legacy_dir: &std::path::Path) {
        if legacy_dir == self.dir || !legacy_dir.is_dir() {
            return;
        }
        let entries = match fs::read_dir(legacy_dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let src = entry.path();
            if src.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let dest = self.dir.join(entry.file_name());
            if dest.exists() {
                continue;
            }
            if self.ensure_dir().is_err() || fs::copy(&src, &dest).is_err() {
                continue;
            }
            let _ = fs::remove_file(&src);
            log::info!(
                "credential: 已搬迁文件凭证 {} → {}",
                src.display(),
                dest.display()
            );
        }
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
    /// 加密文件（唯一持久落点，F-50）。
    FileStore,
    SessionOnly,
}

/// 凭证管理器（F-50）：加密文件为唯一持久落点，OS 凭证存储仅作一次性
/// 迁移源，会话内存兜底（§6.4）。
///
/// 一条 `credential_ref` 的 Key 只存在于一个落点：写入一处时会清除其余落点，
/// 避免「会话里改了 Key 但读取时命中旧值」的歧义。
pub struct CredentialManager {
    /// OS Credential Store：只读的存量迁移源，不再作为写入目标。
    legacy_os: Arc<dyn CredentialStore>,
    file: Arc<FileCredentialStore>,
    session: SessionStore,
}

impl CredentialManager {
    /// 生产装配：加密文件（`~/.gitworkspace/credentials/`）+ OS 迁移源 +
    /// 会话内存。启动时把旧版 app_data_dir 下的文件凭证整体搬迁到新目录。
    pub fn production() -> Self {
        let file = Arc::new(FileCredentialStore::new(&canonical_file_store_dir()));
        file.migrate_from(&legacy_file_store_dir());
        Self {
            legacy_os: Arc::new(AvailabilityCachedStore::new(Arc::new(KeyringStore::new()))),
            file,
            session: SessionStore::new(),
        }
    }

    /// 测试装配：注入文件凭证后端（OS 迁移源用恒不可用后端隔离）。
    #[cfg(test)]
    pub fn with_store(file: Arc<FileCredentialStore>) -> Self {
        Self {
            legacy_os: Arc::new(UnavailableOsStore),
            file,
            session: SessionStore::new(),
        }
    }

    /// 测试装配：注入文件后端与 OS 迁移源（控制迁移行为）。
    pub fn with_stores(
        file: Arc<FileCredentialStore>,
        legacy_os: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            legacy_os,
            file,
            session: SessionStore::new(),
        }
    }

    /// 持久存储（加密文件）当前是否可用。
    pub fn persistent_store_available(&self) -> bool {
        self.file.is_available()
    }

    /// 写入凭证（F-50：`persist = true` 固定写加密文件；`false` 只存会话内存）。
    pub fn set(
        &self,
        credential_ref: &str,
        secret: &str,
        persist: bool,
    ) -> Result<CredentialLocation, AiError> {
        if persist {
            self.file
                .set(credential_ref, secret)
                .map_err(|e| AiError::CredentialUnavailable {
                    message: format!("文件凭证写入失败（{}）: {}", self.file.dir.display(), e),
                })?;
            let _ = self.session.delete(credential_ref);
            Ok(CredentialLocation::FileStore)
        } else {
            self.session
                .set(credential_ref, secret)
                .map_err(|e| AiError::CredentialUnavailable {
                    message: format!("会话凭证写入失败: {}", e),
                })?;
            let _ = self.file.delete(credential_ref);
            Ok(CredentialLocation::SessionOnly)
        }
    }

    /// 读取凭证：加密文件优先；未命中时试 OS 存量并**一次性迁移**（命中即写
    /// 文件并删除 OS 条目）；会话内存兜底。OS 后端故障（`Err`）降级读会话副本
    /// 时打 warn 供排障。
    pub fn get(&self, credential_ref: &str) -> Option<String> {
        if let Ok(Some(secret)) = self.file.get(credential_ref) {
            return Some(secret);
        }
        match self.legacy_os.get(credential_ref) {
            Ok(Some(secret)) => {
                if self.file.set(credential_ref, &secret).is_ok() {
                    let _ = self.legacy_os.delete(credential_ref);
                    log::info!("credential: 存量 OS 凭证已迁移到加密文件: {credential_ref}");
                }
                Some(secret)
            }
            Ok(None) => self.session.get(credential_ref).ok().flatten(),
            Err(e) => {
                log::warn!(
                    "credential: OS 迁移源读取失败（{e}），降级读取会话副本: {credential_ref}"
                );
                self.session.get(credential_ref).ok().flatten()
            }
        }
    }

    pub fn has(&self, credential_ref: &str) -> bool {
        self.get(credential_ref).is_some()
    }

    /// 凭证是否仅存在于会话内存（UI 标记「仅本次会话」）。
    pub fn is_session_only(&self, credential_ref: &str) -> bool {
        !matches!(self.file.get(credential_ref), Ok(Some(_)))
            && !matches!(self.legacy_os.get(credential_ref), Ok(Some(_)))
            && matches!(self.session.get(credential_ref), Ok(Some(_)))
    }

    /// 删除凭证（三个落点都清；OS 迁移源条目删除失败不影响结果）。
    pub fn delete(&self, credential_ref: &str) -> Result<(), AiError> {
        let file_result = self.file.delete(credential_ref);
        let session_result = self.session.delete(credential_ref);
        if let Err(e) = self.legacy_os.delete(credential_ref) {
            log::warn!("credential: OS 迁移源条目删除失败（{e}）: {credential_ref}");
        }
        if let Err(e) = file_result {
            return Err(AiError::CredentialUnavailable {
                message: format!("文件凭证删除失败: {}", e),
            });
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

    /// 内存后端（模拟可用的 OS 迁移源）。
    fn memory_store() -> Arc<dyn CredentialStore> {
        Arc::new(SessionStore::new())
    }

    fn tmp_file_store(tag: &str) -> Arc<FileCredentialStore> {
        let dir = std::env::temp_dir()
            .join("gw-cred-file-test")
            .join(format!("{tag}{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Arc::new(FileCredentialStore::with_dir(dir))
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

    /// F-50：persist 固定写加密文件（不再尝试 OS 凭证存储）。
    #[test]
    fn persist_roundtrip_via_file_store() {
        let mgr = CredentialManager::with_store(tmp_file_store("round"));
        let loc = mgr.set("ai-provider:p1", "sk-test", true).unwrap();
        assert_eq!(loc, CredentialLocation::FileStore);
        assert!(mgr.has("ai-provider:p1"));
        assert!(!mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-test"));
        assert!(mgr.persistent_store_available());

        mgr.delete("ai-provider:p1").unwrap();
        assert!(!mgr.has("ai-provider:p1"));
    }

    /// F-50：存量 OS 凭证一次性迁移——文件未命中时读 OS，命中即写文件并删除
    /// OS 条目，后续读取走文件。
    #[test]
    fn legacy_os_credential_migrates_to_file() {
        let legacy: Arc<dyn CredentialStore> = memory_store();
        legacy.set("ai-provider:p1", "sk-os").unwrap();
        let file = tmp_file_store("migrate");
        let mgr = CredentialManager::with_stores(file, Arc::clone(&legacy));

        // 首次读取触发迁移。
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-os"));
        // 文件已落、OS 条目已清。
        assert_eq!(mgr.file.get("ai-provider:p1").unwrap().as_deref(), Some("sk-os"));
        assert_eq!(legacy.get("ai-provider:p1").unwrap(), None);
        assert!(!mgr.is_session_only("ai-provider:p1"));
    }

    /// OS 迁移源故障（Err）时降级读会话副本，不 panic；后端恢复后有条目即迁移。
    #[test]
    fn legacy_os_failure_degrades_to_session() {
        let unlocked = Arc::new(AtomicBool::new(false));
        let cached: Arc<dyn CredentialStore> =
            Arc::new(AvailabilityCachedStore::new(Arc::new(FlakyStore::new(Arc::clone(&unlocked)))));
        let mgr = CredentialManager::with_stores(tmp_file_store("flaky"), cached);
        mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        // OS 读取失败 → 降级会话值。
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-session"));
        assert!(mgr.is_session_only("ai-provider:p1"));

        // OS 恢复且有条目时，迁移到文件、不再是 session-only。
        unlocked.store(true, Ordering::Relaxed);
        mgr.legacy_os.set("ai-provider:p2", "sk-os").unwrap();
        assert_eq!(mgr.get("ai-provider:p2").as_deref(), Some("sk-os"));
        assert!(!mgr.is_session_only("ai-provider:p2"));
    }

    #[test]
    fn session_only_keeps_key_in_memory() {
        let mgr = CredentialManager::with_store(tmp_file_store("sonly"));
        let loc = mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        assert_eq!(loc, CredentialLocation::SessionOnly);
        assert!(mgr.has("ai-provider:p1"));
        assert!(mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.session_count(), 1);

        mgr.delete("ai-provider:p1").unwrap();
        assert_eq!(mgr.session_count(), 0);
    }

    /// 单落点不变式：写文件 → 改写会话-only → 文件副本清除；再写回文件 → 会话副本清除。
    #[test]
    fn single_location_invariant() {
        let mgr = CredentialManager::with_store(tmp_file_store("single"));
        mgr.set("ai-provider:p1", "sk-file", true).unwrap();
        mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        assert!(mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-session"));
        mgr.set("ai-provider:p1", "sk-file2", true).unwrap();
        assert!(!mgr.is_session_only("ai-provider:p1"));
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-file2"));
    }

    /// OS 迁移源冒烟：环境不可用（无 Secret Service / 无桌面会话的 CI）时
    /// skip 并打印原因，不硬失败（全局约束 §11）。OS 侧只读，不写入真实凭证。
    #[test]
    fn legacy_os_migration_source_smoke_or_skip() {
        let store = KeyringStore::new();
        if !store.is_available() {
            eprintln!("SKIP legacy_os_migration_source_smoke_or_skip: OS 凭证存储在当前环境不可用");
            return;
        }
        // 只读探测路径：NoEntry = 可用；其他错误打印原因后跳过。
        match store.get("ai-probe:smoke-test") {
            Ok(None) | Ok(Some(_)) => {}
            Err(e) => {
                eprintln!("SKIP legacy_os_migration_source_smoke_or_skip: 读取失败: {}", e);
            }
        }
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

    /// F-50：旧目录（app_data_dir/credentials）整体搬迁到新目录——
    /// 目标已存在则跳过，旧文件删除。
    #[test]
    fn file_store_migrate_from_legacy_dir() {
        let base = std::env::temp_dir()
            .join("gw-cred-file-test")
            .join(format!("legacy{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let legacy_dir = base.join("old");
        let target_dir = base.join("new");
        let legacy = FileCredentialStore::with_dir(legacy_dir.clone());
        legacy.set("ai-provider:p1", "sk-legacy").unwrap();
        assert!(legacy_dir.join("ai-provider_p1.json").exists());

        let target = FileCredentialStore::with_dir(target_dir.clone());
        target.migrate_from(&legacy_dir);
        assert!(target_dir.join("ai-provider_p1.json").exists());
        assert!(!legacy_dir.join("ai-provider_p1.json").exists(), "旧文件应已删除");
        assert_eq!(target.get("ai-provider:p1").unwrap().as_deref(), Some("sk-legacy"));

        // 目标已存在时不覆盖。
        legacy.set("ai-provider:p1", "sk-legacy").unwrap();
        target.set("ai-provider:p1", "sk-target").unwrap();
        target.migrate_from(&legacy_dir);
        assert_eq!(target.get("ai-provider:p1").unwrap().as_deref(), Some("sk-target"));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 规范目录落在 `~/.gitworkspace/credentials/`（home 不可用时回退
    /// app_data_dir，随平台分支）。
    #[test]
    fn canonical_dir_uses_home_gitworkspace() {
        let dir = canonical_file_store_dir();
        if dirs::home_dir().is_some() {
            assert!(dir.ends_with(".gitworkspace/credentials"), "{dir:?}");
        } else {
            assert!(dir.ends_with("credentials"), "{dir:?}");
        }
    }

    /// 文件后端作为唯一持久落点（OS 迁移源不可用时 persist 仍成功）。
    #[test]
    fn manager_uses_file_store_for_persist() {
        let mgr = CredentialManager::with_stores(tmp_file_store("mgr"), Arc::new(UnavailableOsStore));
        let loc = mgr.set("ai-provider:p1", "sk-file", true).unwrap();
        assert_eq!(loc, CredentialLocation::FileStore);
        assert_eq!(mgr.get("ai-provider:p1").as_deref(), Some("sk-file"));
        assert!(!mgr.is_session_only("ai-provider:p1"));
        mgr.delete("ai-provider:p1").unwrap();
        assert!(!mgr.has("ai-provider:p1"));
    }

    /// session-only 不影响文件后端。
    #[test]
    fn session_only_writes_only_to_session_not_file() {
        let file = tmp_file_store("sonly2");
        let mgr = CredentialManager::with_stores(Arc::clone(&file), Arc::new(UnavailableOsStore));
        let loc = mgr.set("ai-provider:p1", "sk-session", false).unwrap();
        assert_eq!(loc, CredentialLocation::SessionOnly);
        assert!(mgr.is_session_only("ai-provider:p1"));
        // 文件后端无该条目。
        assert_eq!(file.get("ai-provider:p1").unwrap(), None);
    }
}
