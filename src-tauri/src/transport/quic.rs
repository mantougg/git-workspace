//! QUIC 传输（设计文档 §19/§20）。
//!
//! 安全模型说明（对应 §20）：
//! - 服务端使用 rcgen 生成的**临时自签证书**（每次启动随机生成，不持久化）；
//! - 客户端通过自定义 `ServerCertVerifier` **跳过证书校验**——真正的端到端
//!   认证在应用层完成：只有持有正确 Shared Secret 的 peer 才能通过
//!   XChaCha20-Poly1305 AEAD 解密/认证消息（无法解密的消息直接丢弃，§50）。
//!   QUIC/TLS 层在这里只提供传输加密与可靠有序字节流，不承担身份认证。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use quinn::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::error::{AppError, AppResult};

/// 应用层 ALPN 标识：握手时双方必须一致，避免误连其他 QUIC 服务。
const ALPN: &[u8] = b"gitworkspace-lan-chat/1";

/// 对端空闲多久判定断线（配合 keep-alive 探测）。
const MAX_IDLE_TIMEOUT_MS: u32 = 60_000;
/// QUIC keep-alive 间隔，保证 NAT / 防火墙映射存活并及时发现死连接。
const KEEP_ALIVE_INTERVAL_SECS: u64 = 15;

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut config = quinn::TransportConfig::default();
    config.keep_alive_interval(Some(Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS)));
    config.max_idle_timeout(Some(
        quinn::VarInt::from_u32(MAX_IDLE_TIMEOUT_MS).into(),
    ));
    Arc::new(config)
}

/// 生成临时自签证书并构建 QUIC 服务端配置。
fn server_config() -> AppResult<quinn::ServerConfig> {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "gitworkspace-lan-chat".to_string()])
        .map_err(|e| AppError::LanChat(format!("生成临时证书失败: {e}")))?;
    let cert_der: CertificateDer<'static> = certified.cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()));

    let mut tls = quinn::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .map_err(|e| AppError::LanChat(format!("构建 TLS 配置失败: {e}")))?;
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .map_err(|e| AppError::LanChat(format!("构建 QUIC 配置失败: {e}")))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(transport_config());
    Ok(config)
}

/// 跳过服务端证书校验（见模块级安全模型注释）。
#[derive(Debug)]
struct SkipServerVerification;

impl quinn::rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &quinn::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: quinn::rustls::pki_types::UnixTime,
    ) -> Result<quinn::rustls::client::danger::ServerCertVerified, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<quinn::rustls::SignatureScheme> {
        use quinn::rustls::SignatureScheme::*;
        vec![
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ECDSA_NISTP521_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            ED25519,
        ]
    }
}

/// 客户端配置：跳过证书校验（认证由应用层 AEAD 承担，见模块注释）。
fn client_config() -> AppResult<quinn::ClientConfig> {
    let mut tls = quinn::rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| AppError::LanChat(format!("构建 QUIC 配置失败: {e}")))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport_config());
    Ok(config)
}

/// 创建同时支持监听与拨号的 QUIC Endpoint（绑定随机端口）。
/// 返回 (endpoint, 实际监听端口)。
pub fn bind_endpoint() -> AppResult<(quinn::Endpoint, u16)> {
    let mut endpoint = quinn::Endpoint::server(server_config()?, "0.0.0.0:0".parse().expect("valid bind addr"))
        .map_err(|e| AppError::LanChat(format!("启动聊天监听失败: {e}")))?;
    endpoint.set_default_client_config(client_config()?);
    let port = endpoint
        .local_addr()
        .map_err(|e| AppError::LanChat(format!("获取监听地址失败: {e}")))?
        .port();
    Ok((endpoint, port))
}

/// 解析 `ip:port` 形式的 bootstrap / peer 地址（支持 IPv6 `[::1]:8080`）。
pub fn parse_addr(input: &str) -> AppResult<SocketAddr> {
    input
        .trim()
        .parse::<SocketAddr>()
        .map_err(|_| AppError::LanChat(format!("地址格式无效: {input}（应为 ip:port，如 192.168.1.5:45678）")))
}
