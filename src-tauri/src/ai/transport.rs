//! HTTP 传输抽象（§7.2 / 全局约束 §11）。
//!
//! Gateway/Adapter 的唯一网络入口：协议细节（URL、认证头、请求体）由
//! Adapter 组装，实际收发经 [`HttpTransport`] 完成。生产实现是
//! [`ReqwestTransport`]（reqwest + rustls）；测试注入 fake transport，
//! 以「调用计数 + 预设响应」覆盖超时/取消/429/5xx/非法 JSON 等场景
//! （§18.2），不发真实网络请求。
//!
//! 取消语义：请求发起与响应体读取都可被 [`CancelToken`] 协作式中断
//! （tokio CancellationToken 的最小等价实现，避免引入额外依赖）。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

/// 手工装箱的 async trait 返回类型（保持 dyn 兼容，不引入 async-trait）。
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

// ---------------------------------------------------------------------------
// CancelToken：协作式取消
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CancelInner {
    flag: AtomicBool,
    notified: Notify,
}

/// 协作式取消令牌。`cancel()` 立即置位并唤醒所有等待者；请求循环在
/// select 分支中等待 `cancelled()`，把取消传播到进行中的网络读取。
#[derive(Clone, Default)]
pub struct CancelToken {
    inner: Arc<CancelInner>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.flag.store(true, Ordering::SeqCst);
        self.inner.notified.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.flag.load(Ordering::SeqCst)
    }

    /// 等待取消。注册等待与标志检查的顺序保证不丢信号（enable 先于复查）。
    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.inner.notified.notified();
            let mut pinned = std::pin::pin!(notified);
            pinned.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            pinned.await;
        }
    }
}

// ---------------------------------------------------------------------------
// 请求 / 响应模型
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        }
    }
}

/// 传输层请求。头与体由 Adapter 组装；Key 只出现在头里，内存流经。
#[derive(Debug, Clone)]
pub struct TransportRequest {
    pub method: HttpMethod,
    pub url: reqwest::Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// 响应体字节流。真实实现包装 `reqwest::Response::chunk()`；fake 实现
/// 按预设分块吐出。
pub trait ByteStream: Send {
    /// 返回下一块字节；`Ok(None)` 表示流结束。
    fn next_chunk<'a>(&'a mut self) -> BoxFuture<'a, std::io::Result<Option<Vec<u8>>>>;
}

/// 传输错误（§16.2：网络与 TLS 错误归一化，不依赖 shell；消息只含类别，
/// 不含 URL/头——头里可能有 Key）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// 临时网络错误（连接失败/读中断），可自动重试（§7.4）。
    Transient(String),
    /// 超时（不可自动重试——长请求翻倍等待只会更糟）。
    Timeout,
    /// 请求被取消。
    Cancelled,
    /// 传输层配置错误（如 TLS 初始化失败），不可重试。
    Invalid(String),
}

/// 响应。`status` 为 HTTP 状态码；头名统一小写。
pub struct TransportResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Box<dyn ByteStream>,
}

impl TransportResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

// ---------------------------------------------------------------------------
// Transport trait + reqwest 实现
// ---------------------------------------------------------------------------

/// HTTP 传输抽象。`timeout` 由调用方按请求配置传入：
/// 非流式 = 整请求上限；流式 = 到响应头为止（体读取的超时由泵任务按
/// 块间空闲控制，见 gateway）。
pub trait HttpTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
        cancel: &'a CancelToken,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<TransportResponse, TransportError>>;
}

/// 生产实现：reqwest（rustls TLS）。
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| TransportError::Invalid(format!("HTTP 客户端初始化失败: {}", e)))?;
        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestTransport {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
        cancel: &'a CancelToken,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<TransportResponse, TransportError>> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_str().as_bytes())
                .map_err(|e| TransportError::Invalid(format!("非法 HTTP 方法: {}", e)))?;
            let mut req = self.client.request(method, request.url.clone()).timeout(timeout);
            for (name, value) in &request.headers {
                req = req.header(name, value);
            }
            if let Some(body) = request.body {
                req = req.body(body);
            }

            let response = tokio::select! {
                r = req.send() => r,
                _ = cancel.cancelled() => return Err(TransportError::Cancelled),
            }
            .map_err(map_reqwest_error)?;

            let status = response.status().as_u16();
            let mut headers = BTreeMap::new();
            for (name, value) in response.headers() {
                if let Ok(v) = value.to_str() {
                    headers.insert(name.as_str().to_ascii_lowercase(), v.to_string());
                }
            }
            let body = ReqwestByteStream { response };
            Ok(TransportResponse {
                status,
                headers,
                body: Box::new(body),
            })
        })
    }
}

fn map_reqwest_error(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else if e.is_connect() || e.is_body() || e.is_decode() {
        TransportError::Transient(normalize_network_label(&e))
    } else {
        TransportError::Transient(normalize_network_label(&e))
    }
}

/// 网络错误只保留类别标签（不含 URL/头，可能带 Key）。
fn normalize_network_label(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "无法建立连接（检查网络与 baseUrl）".to_string()
    } else if e.is_body() {
        "响应读取中断".to_string()
    } else if e.is_decode() {
        "响应解码失败".to_string()
    } else {
        "网络错误".to_string()
    }
}

struct ReqwestByteStream {
    response: reqwest::Response,
}

impl ByteStream for ReqwestByteStream {
    fn next_chunk<'a>(&'a mut self) -> BoxFuture<'a, std::io::Result<Option<Vec<u8>>>> {
        Box::pin(async move {
            match self.response.chunk().await {
                Ok(Some(bytes)) => Ok(Some(bytes.to_vec())),
                Ok(None) => Ok(None),
                Err(_) => Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "响应流读取中断",
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_token_wakes_waiters() {
        let token = CancelToken::new();
        let t2 = token.clone();
        let waiter = tokio::spawn(async move {
            t2.cancelled().await;
        });
        token.cancel();
        waiter.await.unwrap();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_before_wait_returns_immediately() {
        let token = CancelToken::new();
        token.cancel();
        // 已取消时 cancelled() 应立即返回（不挂起）。
        tokio::time::timeout(std::time::Duration::from_millis(50), token.cancelled())
            .await
            .expect("cancelled() must resolve immediately when already cancelled");
    }
}
