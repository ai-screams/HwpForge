//! MCP server definition and handler implementation.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::output::{ToolErrorInfo, ToolOutput};
use crate::tools::{convert, inspect, patch, templates, to_json};

// ── MCP Request Types ────────────────────────────────────────────────────────

/// Request parameters for `hwpforge_convert`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertRequest {
    /// Markdown file path or inline content.
    pub markdown: String,
    /// Whether `markdown` is a file path (true) or inline content (false). Default: true.
    #[serde(default = "default_true")]
    pub is_file: bool,
    /// Output HWPX file path. Must end with `.hwpx`.
    pub output_path: String,
    /// Style preset name. Default: "default".
    #[serde(default = "default_preset")]
    pub preset: String,
}

/// Request parameters for `hwpforge_inspect`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct InspectRequest {
    /// Path to the HWPX file to inspect.
    pub file_path: String,
    /// Include style details (fonts, char shapes, para shapes). Default: false.
    #[serde(default)]
    pub styles: bool,
}

/// Request parameters for `hwpforge_to_json`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ToJsonRequest {
    /// Path to the HWPX file to export.
    pub file_path: String,
    /// Extract only a specific section (0-based index). Omit for full document.
    #[serde(default)]
    pub section: Option<usize>,
    /// Output JSON file path. If omitted, returns JSON inline.
    #[serde(default)]
    pub output_path: Option<String>,
}

/// Request parameters for `hwpforge_patch`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PatchRequest {
    /// Path to the base HWPX file.
    pub base_path: String,
    /// Section index to replace (0-based).
    pub section: usize,
    /// Path to the JSON file containing the replacement section.
    pub section_json_path: String,
    /// Output HWPX file path.
    pub output_path: String,
}

/// Request parameters for `hwpforge_templates`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TemplatesRequest {
    /// Filter by preset name. Omit to list all presets.
    #[serde(default)]
    pub name: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_preset() -> String {
    "default".to_string()
}

// ── Helper ───────────────────────────────────────────────────────────────────

/// Convert a `ToolErrorInfo` into an MCP error response (non-fatal, returns content).
fn tool_error_response(err: ToolErrorInfo) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_json_string())])
}

// ── Server ───────────────────────────────────────────────────────────────────

/// HwpForge MCP server.
///
/// Exposes document lifecycle tools: Create, Read, Update, Discover.
/// All tools use the 3-layer output format: `{ data, summary, next }`.
#[derive(Clone)]
pub struct HwpForgeServer {
    #[allow(dead_code)] // Read by rmcp macro-generated code
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl HwpForgeServer {
    /// Create a new HwpForge MCP server with all tools registered.
    pub fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    /// Convert Markdown to a Korean HWPX document (KS X 6101 standard).
    /// Use when the user wants to create a .hwpx file from markdown content.
    /// Supports GFM tables, images, headings, and Korean typography.
    #[tool(
        name = "hwpforge_convert",
        description = "Convert Markdown to a Korean HWPX document (KS X 6101 standard). Supports GFM tables, images, headings, and Korean typography. Returns the output file path and document summary."
    )]
    async fn hwpforge_convert(
        &self,
        Parameters(req): Parameters<ConvertRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            convert::run_convert(&req.markdown, req.is_file, &req.output_path, &req.preset)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Generated {} ({} bytes, {} sections, {} paragraphs)",
                        data.output_path, data.size_bytes, data.sections, data.paragraphs,
                    ),
                    vec![
                        "Use hwpforge_inspect to verify the output",
                        "Use hwpforge_to_json + hwpforge_patch to edit",
                    ],
                );
                Ok(CallToolResult::success(vec![Content::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Inspect an HWPX document and return its structure summary.
    /// Use to understand document layout before editing.
    #[tool(
        name = "hwpforge_inspect",
        description = "Inspect an HWPX document structure. Returns section count, paragraph counts, tables, images, charts, headers, footers, and page numbers per section."
    )]
    async fn hwpforge_inspect(
        &self,
        Parameters(req): Parameters<InspectRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result =
            tokio::task::spawn_blocking(move || inspect::run_inspect(&req.file_path, req.styles))
                .await
                .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "{} sections, {} paragraphs, {} tables, {} images, {} charts",
                        data.sections,
                        data.total_paragraphs,
                        data.total_tables,
                        data.total_images,
                        data.total_charts,
                    ),
                    vec![
                        "Use hwpforge_to_json to export for editing",
                        "Use hwpforge_convert to create new documents",
                    ],
                );
                Ok(CallToolResult::success(vec![Content::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Export an HWPX document to JSON for AI-driven editing.
    /// Use `section` parameter to extract a single section (token-efficient).
    #[tool(
        name = "hwpforge_to_json",
        description = "Export HWPX to JSON for editing. Use section parameter (0-based) to extract a single section for token efficiency. Returns JSON inline or writes to file."
    )]
    async fn hwpforge_to_json(
        &self,
        Parameters(req): Parameters<ToJsonRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            to_json::run_to_json(&req.file_path, req.section, req.output_path.as_deref())
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let summary = if let Some(ref path) = data.output_path {
                    format!(
                        "Exported to {} ({} bytes{})",
                        path,
                        data.size_bytes,
                        if data.section_only { ", section only" } else { "" }
                    )
                } else {
                    format!(
                        "Exported JSON ({} bytes{})",
                        data.size_bytes,
                        if data.section_only { ", section only" } else { "" }
                    )
                };
                let output = ToolOutput::new(
                    &data,
                    summary,
                    vec![
                        "Edit the JSON and use hwpforge_patch to apply changes",
                        "Use hwpforge_inspect to understand structure first",
                    ],
                );
                Ok(CallToolResult::success(vec![Content::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Replace a section in an existing HWPX file with edited JSON.
    /// Preserves images, styles, and binary content from the base file.
    #[tool(
        name = "hwpforge_patch",
        description = "Replace a section in an existing HWPX file with edited JSON data. Preserves images and styles from the base file. Use after hwpforge_to_json for surgical edits."
    )]
    async fn hwpforge_patch(
        &self,
        Parameters(req): Parameters<PatchRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            patch::run_patch(&req.base_path, req.section, &req.section_json_path, &req.output_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Patched section {} → {} ({} bytes, {} sections)",
                        data.patched_section, data.output_path, data.size_bytes, data.sections,
                    ),
                    vec!["Use hwpforge_inspect to verify the patched output"],
                );
                Ok(CallToolResult::success(vec![Content::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// List available style presets for document generation.
    /// Call this before hwpforge_convert to discover formatting options.
    #[tool(
        name = "hwpforge_templates",
        description = "List available style presets (templates) for HWPX document generation. Returns preset names, descriptions, fonts, and page sizes. Call before hwpforge_convert to choose a preset."
    )]
    async fn hwpforge_templates(
        &self,
        Parameters(req): Parameters<TemplatesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result =
            tokio::task::spawn_blocking(move || templates::run_templates(req.name.as_deref()))
                .await
                .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let names: Vec<&str> = data.templates.iter().map(|t| t.name.as_str()).collect();
                let output = ToolOutput::new(
                    &data,
                    format!("Available presets: {}", names.join(", ")),
                    vec!["Use --preset <name> with hwpforge_convert"],
                );
                Ok(CallToolResult::success(vec![Content::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }
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
