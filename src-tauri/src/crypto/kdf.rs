//! Argon2id KDF（设计文档 §6）：Shared Secret + 确定性 salt → 32 字节 Room Key。
//!
//! salt 必须对所有 peer 一致：`"gitworkspace-lan-chat-v1:" + room_id`。
//! 不能随机 salt，否则各 peer 推导出的 key 不同，无法互相解密。

use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

/// KDF salt 前缀：协议域分隔 + room_id，保证同 room 同 secret 推出同 key，
/// 不同 room 即使复用同一 secret 也互不相关。
const KDF_SALT_PREFIX: &str = "gitworkspace-lan-chat-v1:";

/// 由 Shared Secret 派生 32 字节 Room Key（Argon2id 默认参数：m=19MiB, t=2, p=1）。
///
/// 返回 [`Zeroizing`] 包裹的 key，离开作用域自动清零（§28/§29）。
pub fn derive_room_key(secret: &str, room_id: &str) -> AppResult<Zeroizing<[u8; 32]>> {
    let salt = format!("{KDF_SALT_PREFIX}{room_id}");
    let mut key = Zeroizing::new([0u8; 32]);
    // 面向用户的错误不暴露 KDF 内部细节（§50）。
    argon2::Argon2::default()
        .hash_password_into(secret.as_bytes(), salt.as_bytes(), &mut *key)
        .map_err(|_| AppError::LanChat("密钥派生失败，请重试".into()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_secret_and_room_derive_same_key() {
        let a = derive_room_key("s3cret", "room-1").unwrap();
        let b = derive_room_key("s3cret", "room-1").unwrap();
        assert_eq!(*a, *b);
    }

    #[test]
    fn different_room_derives_different_key() {
        let a = derive_room_key("s3cret", "room-1").unwrap();
        let b = derive_room_key("s3cret", "room-2").unwrap();
        assert_ne!(*a, *b);
    }

    #[test]
    fn different_secret_derives_different_key() {
        let a = derive_room_key("s3cret-a", "room-1").unwrap();
        let b = derive_room_key("s3cret-b", "room-1").unwrap();
        assert_ne!(*a, *b);
    }
}
