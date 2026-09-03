//! LAN Chat 加密层（设计文档 §3/§6-§8）：
//! Shared Secret → Argon2id KDF → 32 字节 Room Key → XChaCha20-Poly1305 AEAD。
//!
//! 安全约束（§49/§50）：
//! - 日志与错误信息禁止携带 secret / key / 明文 / 密文内容；
//! - 解密失败只暴露通用错误，不泄露内部细节；
//! - Room Key 用 [`zeroize::Zeroizing`] 包裹，离开房间 / 进程退出时清零。

pub mod cipher;
pub mod kdf;
