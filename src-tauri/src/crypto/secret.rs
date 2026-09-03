//! 随机密钥生成（设计文档 §39）：生成 128/192/256-bit 随机 Secret，
//! 输出 hex / base64 / base64url 三种格式，供 LAN 加密聊天等场景
//! 通过带外安全渠道分发（工具本身不负责分发）。

use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD as B64URL};
use base64::Engine;
use rand::RngCore;
use serde::Serialize;
use zeroize::Zeroize;

/// 生成的随机 Secret（三种编码同源，serde camelCase 对齐前端契约）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RandomSecret {
    pub hex: String,
    pub base64: String,
    pub base64_url: String,
}

/// 生成 `byte_len` 字节的加密安全随机 Secret（OsRng）。
pub fn random_secret(byte_len: usize) -> RandomSecret {
    let mut bytes = vec![0u8; byte_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let output = RandomSecret {
        hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
        base64: B64.encode(&bytes),
        base64_url: B64URL.encode(&bytes),
    };
    bytes.zeroize();
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_formats_decode_to_same_bytes() {
        let out = random_secret(32);
        assert_eq!(out.hex.len(), 64);
        assert!(out.hex.chars().all(|c| c.is_ascii_hexdigit()));
        let raw = B64.decode(&out.base64).unwrap();
        assert_eq!(raw.len(), 32);
        assert_eq!(B64URL.decode(&out.base64_url).unwrap(), raw);
        assert_eq!(out.hex, raw.iter().map(|b| format!("{b:02x}")).collect::<String>());
        assert!(!out.base64_url.contains(['+', '/', '=']));
    }

    #[test]
    fn different_lengths() {
        assert_eq!(B64.decode(random_secret(16).base64).unwrap().len(), 16);
        assert_eq!(B64.decode(random_secret(24).base64).unwrap().len(), 24);
    }

    #[test]
    fn successive_secrets_differ() {
        assert_ne!(random_secret(32).hex, random_secret(32).hex);
    }

    #[test]
    fn ipc_payload_is_camel_case() {
        let v = serde_json::to_value(random_secret(16)).unwrap();
        for key in ["hex", "base64", "base64Url"] {
            assert!(v.get(key).is_some(), "RandomSecret missing {key}");
        }
    }
}
