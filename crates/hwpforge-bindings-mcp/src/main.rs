//! HwpForge MCP Server — AI-first Korean document tools.
//!
//! Communicates via stdio (JSON-RPC 2.0). All logging goes to stderr.

#![deny(missing_docs)]

mod output;
mod prompts;
mod resources;
mod server;
mod tools;

use rmcp::ServiceExt;
use server::HwpForgeServer;

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

    tracing::info!("HwpForge MCP Server v{}", env!("CARGO_PKG_VERSION"));

    let server = HwpForgeServer::new();
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
