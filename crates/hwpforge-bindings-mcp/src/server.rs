//! MCP server definition and handler implementation.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::*;
use rmcp::{tool_router, ServerHandler};

/// HwpForge MCP server.
///
/// Exposes document lifecycle tools: Create, Read, Update, Discover.
/// All tools use the 3-layer output format: `{ data, summary, next }`.
#[derive(Clone)]
pub struct HwpForgeServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl HwpForgeServer {
    /// Create a new HwpForge MCP server with all tools registered.
    pub fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    // Tools will be added in subsequent tasks (4-8).
}

impl ServerHandler for HwpForgeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "hwpforge-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                description: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "HwpForge MCP server for Korean HWPX document generation and editing. \
                 Converts Markdown to HWPX, inspects document structure, and supports \
                 JSON round-trip editing. Use hwpforge_templates to discover available \
                 style templates before creating documents."
                    .into(),
            ),
        }
    }
}
