//! XChaCha20-Poly1305 AEAD（设计文档 §7/§8）：
//! 每条消息独立随机 24 字节 nonce；nonce 不需要保密，随 Envelope 明文传输。

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

/// XChaCha20-Poly1305 nonce 长度（字节）。
pub const NONCE_LEN: usize = 24;

/// 生成随机 nonce（§8：每条消息独立；nonce 无需保密）。
pub fn generate_nonce() -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    nonce
}

/// AEAD 加密。返回随机 nonce + 密文（含 Poly1305 tag）。
pub fn encrypt(key: &Zeroizing<[u8; 32]>, plaintext: &[u8]) -> AppResult<(Vec<u8>, Vec<u8>)> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&**key));
    let nonce = generate_nonce();
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| AppError::LanChat("消息加密失败".into()))?;
    Ok((nonce.to_vec(), ciphertext))
}

/// AEAD 解密。失败（密钥错误 / 密文被篡改）只返回通用错误（§50），
/// 不泄露是认证失败还是格式错误。
pub fn decrypt(key: &Zeroizing<[u8; 32]>, nonce: &[u8], ciphertext: &[u8]) -> AppResult<Vec<u8>> {
    if nonce.len() != NONCE_LEN {
        return Err(AppError::LanChat("消息解密失败".into()));
    }
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&**key));
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| AppError::LanChat("消息解密失败".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn test_key(byte: u8) -> Zeroizing<[u8; 32]> {
        Zeroizing::new([byte; 32])
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = test_key(7);
        let plaintext = "你好，LAN Chat".as_bytes();
        let (nonce, ciphertext) = encrypt(&key, plaintext).unwrap();
        assert_eq!(nonce.len(), NONCE_LEN);
        assert_ne!(ciphertext, plaintext);
        let decrypted = decrypt(&key, &nonce, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let (nonce, ciphertext) = encrypt(&test_key(1), b"secret message").unwrap();
        let result = decrypt(&test_key(2), &nonce, &ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let key = test_key(3);
        let (nonce, mut ciphertext) = encrypt(&key, b"integrity check").unwrap();
        // 翻转密文中部一个字节，AEAD 必须检测出来。
        let mid = ciphertext.len() / 2;
        ciphertext[mid] ^= 0xFF;
        assert!(decrypt(&key, &nonce, &ciphertext).is_err());
    }

    #[test]
    fn tampered_nonce_fails_authentication() {
        let key = test_key(4);
        let (mut nonce, ciphertext) = encrypt(&key, b"nonce check").unwrap();
        nonce[0] ^= 0x01;
        assert!(decrypt(&key, &nonce, &ciphertext).is_err());
    }

    #[test]
    fn bad_nonce_length_rejected() {
        let key = test_key(5);
        assert!(decrypt(&key, &[0u8; 12], b"x").is_err());
    }

    #[test]
    fn nonce_uniqueness_over_100k() {
        let mut seen = HashSet::with_capacity(100_000);
        for _ in 0..100_000 {
            assert!(seen.insert(generate_nonce()), "nonce collision detected");
        }
    }
}
