//! 最小 SSE（Server-Sent Events）解码器。
//!
//! 三种协议的流式接口都用 SSE 分帧（§7.2）：
//! - OpenAI Chat Completions：`data: {...}` 行，`data: [DONE]` 结束；
//! - OpenAI Responses：`event:` + `data:`，事件类型也在 data JSON 的 `type` 里；
//! - Anthropic Messages：`event:` + `data:`，事件类型在 data JSON 的 `type` 里。
//!
//! 只解析协议真正用到的字段（`event` / `data`），注释与 id/retry 忽略；
//! 容忍 `\r\n` 与分块任意切分（跨 chunk 缓冲）。

/// 一条已分帧的 SSE 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// `event:` 字段（可缺省）。
    pub event: Option<String>,
    /// `data:` 载荷（同一事件内多行 data 以 `\n` 连接）。
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    current_event: Option<String>,
    current_data: Vec<String>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入新字节，返回由此完整分帧出的事件。
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        // 逐行消费完整行（\n 结尾；\r 剥离）。
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=pos).collect();
            let mut line = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(event) = self.feed_line(&line) {
                events.push(event);
            }
        }
        events
    }

    /// 流结束：冲刷未以空行结尾的最后一个事件（宽容处理）。
    pub fn finish(&mut self) -> Vec<SseEvent> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let tail = String::from_utf8_lossy(&self.buffer).into_owned();
        self.buffer.clear();
        self.feed_line(&tail).into_iter().collect()
    }

    /// 喂入一行；返回 `Some(event)` 表示事件分帧完成（空行触发派发）。
    fn feed_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            // 空行 = 事件边界；仅当该事件携带 data 时派发。
            if self.current_data.is_empty() {
                self.current_event = None;
                return None;
            }
            let event = SseEvent {
                event: self.current_event.take(),
                data: self.current_data.join("\n"),
            };
            self.current_data.clear();
            return Some(event);
        }
        if let Some(field) = line.strip_prefix(':') {
            let _ = field; // 注释行，忽略
            return None;
        }
        let (name, value) = match line.split_once(':') {
            Some((n, v)) => (n, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match name {
            "event" => self.current_event = Some(value.to_string()),
            "data" => self.current_data.push(value.to_string()),
            _ => {} // id / retry 及未知字段忽略
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(input: &[u8]) -> Vec<SseEvent> {
        let mut d = SseDecoder::new();
        let mut out = d.push(input);
        out.extend(d.finish());
        out
    }

    #[test]
    fn parses_data_lines_and_done_sentinel() {
        let evs = events(b"data: {\"a\":1}\n\ndata: [DONE]\n\n");
        assert_eq!(
            evs,
            vec![
                SseEvent {
                    event: None,
                    data: "{\"a\":1}".into()
                },
                SseEvent {
                    event: None,
                    data: "[DONE]".into()
                },
            ]
        );
    }

    #[test]
    fn parses_event_and_data_fields() {
        let evs = events(b"event: content_block_delta\ndata: {\"x\":true}\n\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: Some("content_block_delta".into()),
                data: "{\"x\":true}".into()
            }]
        );
    }

    #[test]
    fn tolerates_crlf_comments_and_split_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"data: par").is_empty());
        let evs = d.push(b"tial\r\n: comment\r\n\r\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: None,
                data: "partial".into()
            }]
        );
    }

    #[test]
    fn multi_line_data_joined_with_newline() {
        let evs = events(b"data: a\ndata: b\n\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: None,
                data: "a\nb".into()
            }]
        );
    }

    #[test]
    fn dataless_events_are_not_dispatched() {
        assert!(events(b"event: ping\n\n").is_empty());
    }
}
