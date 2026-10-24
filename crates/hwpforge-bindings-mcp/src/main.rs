//! Vise — HwpForge MCP Server: AI-first Korean document tools.
//!
//! Communicates via stdio (JSON-RPC 2.0). All logging goes to stderr.

#![deny(missing_docs)]

mod compat;
mod output;
mod prompts;
mod resources;
mod server;
mod tools;

use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{RoleServer, ServiceExt};
use server::HwpForgeServer;
use tokio::io::{AsyncRead, AsyncWrite};

/// Longest accepted stdin line (32 MiB). The largest `from_json` request
/// measured over the stratified HWPX sample was 14.7 MiB once the document
/// JSON is escaped into the request string, so this keeps twice that room
/// while bounding what one line can hold. This is a line length limit, not a
/// cap on buffer allocation or process memory (the read buffer may grow past
/// it before the line is rejected). A longer line is dropped with an error log
/// and the connection stays open.
const MAX_LINE: usize = 32 * 1024 * 1024;

// Keep the two limits in the measured relation: an inline payload at the tool
// limit, escaped with the growth measured on real documents (at most 1.12x),
// fits one line. This checks the constants only; input that escapes heavier
// (for example text made mostly of newlines) can still exceed the line limit
// and be dropped by the transport. 이것을 실패시키는 것: MAX_INLINE_SIZE 를 MAX_LINE 에 가깝게 올리는 것(컴파일 실패).
const _: () = assert!(output::MAX_INLINE_SIZE / 8 * 9 < MAX_LINE);

fn bounded_transport<R, W>(read: R, write: W, max_line: usize) -> AsyncRwTransport<RoleServer, R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin + 'static,
{
    AsyncRwTransport::new(read, write).with_max_line_length(max_line)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // All logging to stderr (stdout is JSON-RPC only).
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("hwpforge=info".parse()?),
        )
        .init();

    tracing::info!("Vise — HwpForge MCP Server v{}", env!("CARGO_PKG_VERSION"));

    let server = HwpForgeServer::new();
    let service =
        server.serve(bounded_transport(tokio::io::stdin(), tokio::io::stdout(), MAX_LINE)).await?;
    service.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    /// Sends INIT, then a padded `ping` (id 2) of `pad` bytes, then a plain `ping` (id 3);
    /// returns the ids of the responses received before EOF.
    async fn response_ids(limit: usize, pad: usize) -> Vec<u64> {
        let (client, server_io) = tokio::io::duplex(1 << 20);
        let (sr, sw) = tokio::io::split(server_io);
        let svc = tokio::spawn(async move {
            let service =
                HwpForgeServer::new().serve(bounded_transport(sr, sw, limit)).await.unwrap();
            service.waiting().await.unwrap();
        });
        let (cr, mut cw) = tokio::io::split(client);
        let padded = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"ping","params":{{"pad":"{}"}}}}"#,
            "x".repeat(pad)
        );
        let ping = r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#;
        let writer = tokio::spawn(async move {
            for line in [INIT, INITIALIZED, padded.as_str(), ping] {
                cw.write_all(line.as_bytes()).await.unwrap();
                cw.write_all(b"\n").await.unwrap();
            }
            cw.shutdown().await.unwrap();
        });
        let mut lines = BufReader::new(cr).lines();
        let mut ids = Vec::new();
        while let Some(l) = lines.next_line().await.unwrap() {
            let v: serde_json::Value = serde_json::from_str(&l).unwrap();
            ids.push(v["id"].as_u64().unwrap());
        }
        writer.await.unwrap();
        svc.await.unwrap();
        ids
    }

    // 이것을 실패시키는 것: bounded_transport 가 max_line 을 적용하지 않고 기본값(16 MiB)을 쓰는 변경 (id 2 에 응답이 와 버림).
    #[tokio::test]
    async fn oversized_line_is_dropped_and_next_request_answered() {
        assert_eq!(response_ids(4096, 64 * 1024).await, vec![1, 3]);
    }

    // 이것을 실패시키는 것: `with_max_line_length(0)` 처럼 한도를 지나치게 작게 적용하는 변경 (정상 길이 줄까지 버려 id 2 응답이 사라짐).
    #[tokio::test]
    async fn line_within_limit_is_answered() {
        assert_eq!(response_ids(4096, 100).await, vec![1, 2, 3]);
    }
}
