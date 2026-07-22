//! MCP server definition and handler implementation.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::output::{ToolErrorInfo, ToolOutput};
use crate::tools::{
    convert, fields, fill, from_json, inspect, outline, patch, restyle, set_cell, stamp, templates,
    to_json, to_md, validate,
};
use crate::{prompts, resources};

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
    /// Reserved for future use. When true, will include style details. Currently ignored.
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

/// Request parameters for `hwpforge_from_json`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FromJsonRequest {
    /// JSON string matching the ExportedDocument schema.
    /// Use hwpforge_to_json output as a reference for the structure.
    pub structure: String,
    /// Output HWPX file path. Must end with `.hwpx`.
    pub output_path: String,
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

/// Request parameters for `hwpforge_fields`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FieldsRequest {
    /// Path to the HWPX file to inspect.
    pub file_path: String,
}

/// Request parameters for `hwpforge_outline`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OutlineRequest {
    /// Path to the HWPX file to map.
    pub file_path: String,
}

/// Request parameters for `hwpforge_fill`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FillRequest {
    /// Path to the HWPX file to fill.
    pub file_path: String,
    /// Field name → value map. All values are validated before anything is
    /// written (all-or-nothing). Use hwpforge_fields to discover names.
    pub values: std::collections::BTreeMap<String, String>,
    /// Output HWPX file path. Must end with `.hwpx`.
    pub output_path: String,
}

/// Request parameters for `hwpforge_stamp_plan`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StampPlanRequest {
    /// Path to the HWPX file to inspect for placeholder candidates.
    pub file_path: String,
}

/// Request parameters for `hwpforge_stamp`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StampRequest {
    /// Path to the HWPX file to stamp.
    pub file_path: String,
    /// Approved text specs: one per candidate from hwpforge_stamp_plan,
    /// each with an action ({"field":{"name":"…"}} or "ignore"). Every
    /// unguarded candidate must be covered (all-or-nothing).
    #[serde(default)]
    pub specs: Vec<hwpforge_smithy_hwpx::stamp::StampSpec>,
    /// Approved cell specs (class-B, from hwpforge_stamp_plan `cells`).
    /// Field actions REQUIRE a non-blank hint; presence of any cell spec
    /// requires `source_sha256`.
    #[serde(default)]
    pub cells: Vec<hwpforge_smithy_hwpx::stamp::CellStampSpec>,
    /// SHA-256 of the input from hwpforge_stamp_plan — mandatory with
    /// `cells` (drift pinning); selects the v2 pipeline when present.
    pub source_sha256: Option<String>,
    /// Output HWPX file path. Must end with `.hwpx`.
    pub output_path: String,
    /// Manifest JSON path (default: `<output>.manifest.json`).
    pub manifest_path: Option<String>,
}

/// Request parameters for `hwpforge_set_cell`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetCellRequest {
    /// Path to the HWPX file to edit.
    pub file_path: String,
    /// Cell edits: each spec addresses a table by ordinal (to-json export
    /// order) plus one of `at` {row,col} (covered positions resolve to their
    /// merge anchor), `right_of` LABEL, or `below` LABEL (normalized exact
    /// match). `text` "" clears the cell. All-or-nothing.
    pub specs: Vec<hwpforge_smithy_hwpx::CellSpec>,
    /// Output HWPX file path. Must end with `.hwpx`.
    pub output_path: String,
}

/// Request parameters for `hwpforge_validate`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateRequest {
    /// Path to the HWPX file to validate.
    pub file_path: String,
}

/// Request parameters for `hwpforge_restyle`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RestyleRequest {
    /// Path to the source HWPX file.
    pub file_path: String,
    /// Style preset name to apply. Use hwpforge_templates to see available presets.
    pub preset: String,
    /// Output HWPX file path. Must end with `.hwpx`.
    pub output_path: String,
}

/// Request parameters for `hwpforge_templates`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TemplatesRequest {
    /// Filter by preset name. Omit to list all presets.
    #[serde(default)]
    pub name: Option<String>,
}

/// Request parameters for `hwpforge_to_md`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ToMdRequest {
    /// Path to the HWPX file to convert.
    pub file_path: String,
    /// Output directory for the generated Markdown and image files.
    /// Defaults to the same directory as the input file.
    #[serde(default)]
    pub output_dir: Option<String>,
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
    CallToolResult::error(vec![ContentBlock::text(err.to_json_string())])
}

// ── Server ───────────────────────────────────────────────────────────────────

/// HwpForge MCP server.
///
/// Exposes document lifecycle tools: Create, Read, Update, Verify, Discover.
/// Also provides style template resources and workflow prompts.
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
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
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
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
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
                let summary = if data.warnings.is_empty() {
                    summary
                } else {
                    format!("{summary}, {} warning(s)", data.warnings.len())
                };
                let mut next = vec![
                    "Edit the JSON and use hwpforge_patch to apply changes".to_string(),
                    "Use hwpforge_inspect to understand structure first".to_string(),
                ];
                if let Some(warning) = data.warnings.first() {
                    next.insert(0, format!("Warning: {}", warning.message));
                }
                let output = ToolOutput { data: &data, summary, next };
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Create an HWPX document directly from a JSON structure.
    /// Use when building documents programmatically without Markdown.
    #[tool(
        name = "hwpforge_from_json",
        description = "Create an HWPX document directly from a JSON structure (ExportedDocument schema). Use when building documents programmatically without Markdown. Get the schema from hwpforge_to_json output. For large documents, prefer hwpforge_to_json + hwpforge_patch workflow with file paths instead of inline JSON."
    )]
    async fn hwpforge_from_json(
        &self,
        Parameters(req): Parameters<FromJsonRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            from_json::run_from_json(&req.structure, &req.output_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Created {} ({} bytes, {} sections, {} paragraphs)",
                        data.output_path, data.size_bytes, data.sections, data.paragraphs,
                    ),
                    vec![
                        "Use hwpforge_inspect to verify the output",
                        "Use hwpforge_to_json + hwpforge_patch to edit",
                        "Note: images are NOT preserved in JSON round-trip; use hwpforge_patch with base_path to keep images",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
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
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Document navigation map: headings, tables, fields, bookmarks.
    #[tool(
        name = "hwpforge_outline",
        description = "Show the document navigation map for an HWPX file: headings (level + text), tables (ordinal + logical grid dims + addressable), named click-here fields, and bookmarks. Name anchors are the primary keys; {section, para} locators are secondary and go stale after structural edits. Fetch this once before targeted reads or edits."
    )]
    async fn hwpforge_outline(
        &self,
        Parameters(req): Parameters<OutlineRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || outline::run_outline(&req.file_path))
            .await
            .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "{} heading(s), {} table(s), {} field(s), {} bookmark(s)",
                        data.outline.headings.len(),
                        data.outline.tables.len(),
                        data.outline.fields.len(),
                        data.outline.bookmarks.len(),
                    ),
                    vec![
                        "Use hwpforge_fields + hwpforge_fill for named fields",
                        "Use hwpforge_set_cell with a table ordinal + addr to fill cells",
                        "Use hwpforge_to_json to export for structural editing",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// List named click-here fields (누름틀) for fill discoverability.
    #[tool(
        name = "hwpforge_fields",
        description = "List named click-here fields (누름틀) in an HWPX document: name, hint, current value, and whether each is fillable. Use before hwpforge_fill to discover field names."
    )]
    async fn hwpforge_fields(
        &self,
        Parameters(req): Parameters<FieldsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || fields::run_fields(&req.file_path))
            .await
            .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!("{} field(s), {} fillable", data.fields.len(), data.fillable_count),
                    vec!["Use hwpforge_fill with values {name: value} to fill fields"],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Fill named click-here fields (누름틀) with values — delta edit that
    /// byte-preserves every untouched package entry.
    #[tool(
        name = "hwpforge_fill",
        description = "Fill named click-here fields (누름틀) by name→value map, byte-preserving everything else. All values are validated first (all-or-nothing). Much cheaper than to_json+patch for template filling. Use hwpforge_fields to discover names."
    )]
    async fn hwpforge_fill(
        &self,
        Parameters(req): Parameters<FillRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            fill::run_fill(&req.file_path, &req.values, &req.output_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Filled {} field(s) → {} ({} bytes)",
                        data.filled.len(),
                        data.output_path,
                        data.size_bytes,
                    ),
                    vec!["Use hwpforge_inspect or hwpforge_to_md to verify the filled output"],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Discover prose placeholder candidates for template stamping (E6).
    #[tool(
        name = "hwpforge_stamp_plan",
        description = "Discover class-A placeholder candidates (checkbox/paren-blank/date-blank/standalone-@/seal tokens) in an HWPX for template stamping. Author one spec per candidate (field name or ignore) and pass them to hwpforge_stamp. Guarded candidates (instruction context) are never auto-applied."
    )]
    async fn hwpforge_stamp_plan(
        &self,
        Parameters(req): Parameters<StampPlanRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || stamp::run_stamp_plan(&req.file_path))
            .await
            .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "{} text + {} cell stamp candidate(s) found",
                        data.candidates.len(),
                        data.cells.len()
                    ),
                    vec![
                        "Author one spec per candidate: {\"field\":{\"name\":\"…\"}} or \"ignore\"",
                        "Cell specs need a non-blank hint and the plan's source_sha256",
                        "Then call hwpforge_stamp with the full spec list (all-or-nothing)",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Promote placeholders to named click-here fields (E6 stamping).
    #[tool(
        name = "hwpforge_stamp",
        description = "Promote prose placeholders to named click-here fields (누름틀) using the approved spec list from hwpforge_stamp_plan. Fail-closed admission gate (lossless round-trip + ZIP closed-world) + all-or-nothing preflight; writes the stamped HWPX and a manifest. The output is immediately usable with hwpforge_fields/hwpforge_fill."
    )]
    async fn hwpforge_stamp(
        &self,
        Parameters(req): Parameters<StampRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            stamp::run_stamp(
                &req.file_path,
                &req.specs,
                &req.cells,
                req.source_sha256.as_deref(),
                &req.output_path,
                req.manifest_path.as_deref(),
            )
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Stamped {} text + {} cell field(s) (ignored {}, guarded-skipped {}) → {}",
                        data.stamped.len(),
                        data.stamped_cells.len(),
                        data.ignored,
                        data.skipped_guarded,
                        data.output_path,
                    ),
                    vec![
                        "Use hwpforge_fields to list the stamped fields",
                        "Fill them with hwpforge_fill (name→value map)",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Edit table cells by logical grid address (E3).
    #[tool(
        name = "hwpforge_set_cell",
        description = "Edit table cells by logical grid address in an HWPX file. Address a cell with a table ordinal (to-json export order) plus at {row,col} (covered positions resolve to their merge anchor), right_of LABEL, or below LABEL (normalized exact match). Empty text clears the cell. All-or-nothing behind the fail-closed admission gate; cells containing tables/images/controls are rejected."
    )]
    async fn hwpforge_set_cell(
        &self,
        Parameters(req): Parameters<SetCellRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            set_cell::run_set_cell(&req.file_path, &req.specs, &req.output_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let covered = data
                    .cells
                    .iter()
                    .filter(|c| {
                        c.resolution == hwpforge_smithy_hwpx::CellResolution::CoveredToAnchor
                    })
                    .count();
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Set {} cell(s) ({} resolved to merge anchors) → {}",
                        data.cells.len(),
                        covered,
                        data.output_path,
                    ),
                    vec![
                        "Each result reports requested/anchor/resolution — covered coordinates were redirected to their merge anchor",
                        "Verify with hwpforge_to_json (cells carry addr {row,col})",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Validate an HWPX file structure and integrity.
    /// Returns validation status and any issues found.
    #[tool(
        name = "hwpforge_validate",
        description = "Validate an HWPX file structure and integrity. Returns validation status, section/paragraph counts, and any issues found. Use to verify files before editing or after generation."
    )]
    async fn hwpforge_validate(
        &self,
        Parameters(req): Parameters<ValidateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || validate::run_validate(&req.file_path))
            .await
            .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let summary = if data.valid {
                    format!(
                        "Valid HWPX: {} sections, {} paragraphs",
                        data.sections, data.paragraphs
                    )
                } else {
                    format!("Invalid HWPX: {} issues found", data.issues.len())
                };
                let next = if data.valid {
                    vec!["Use hwpforge_to_json to export for editing"]
                } else {
                    vec![
                        "Fix the issues and re-validate",
                        "Use hwpforge_convert to create a new valid document",
                    ]
                };
                let output = ToolOutput::new(&data, summary, next);
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Apply a different style template (preset) to an existing HWPX document.
    /// Replaces fonts and styles while preserving document content.
    #[tool(
        name = "hwpforge_restyle",
        description = "Apply a different style template (preset) to an existing HWPX document. Replaces fonts and paragraph styles while preserving document content and structure. Use hwpforge_templates to discover available presets."
    )]
    async fn hwpforge_restyle(
        &self,
        Parameters(req): Parameters<RestyleRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            restyle::run_restyle(&req.file_path, &req.preset, &req.output_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let output = ToolOutput::new(
                    &data,
                    format!(
                        "Restyled with '{}' → {} ({} bytes, {} sections)",
                        data.applied_preset, data.output_path, data.size_bytes, data.sections,
                    ),
                    vec![
                        "Use hwpforge_inspect to verify the output",
                        "Use hwpforge_validate to check integrity",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
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
                    vec![
                        "Set preset parameter in hwpforge_convert",
                        "Use hwpforge_restyle to change existing document styles",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }

    /// Convert an HWPX document to Markdown with extracted images.
    /// Use to read or review HWPX content, or to convert for further editing.
    #[tool(
        name = "hwpforge_to_md",
        description = "Convert an HWPX document to Markdown. Extracts text, headings, tables, and images from the HWPX file. Returns the path to the generated Markdown file and any extracted image files."
    )]
    async fn hwpforge_to_md(
        &self,
        Parameters(req): Parameters<ToMdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = tokio::task::spawn_blocking(move || {
            to_md::run_to_md(&req.file_path, req.output_dir.as_deref())
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        match result {
            Ok(data) => {
                let summary = if data.image_count > 0 {
                    format!(
                        "Converted to {} ({} bytes, {} images extracted)",
                        data.markdown_path, data.size_bytes, data.image_count,
                    )
                } else {
                    format!("Converted to {} ({} bytes)", data.markdown_path, data.size_bytes,)
                };
                let output = ToolOutput::new(
                    &data,
                    summary,
                    vec![
                        "Edit the Markdown and use hwpforge_convert to create a new HWPX",
                        "Use hwpforge_inspect to review the original document structure",
                    ],
                );
                Ok(CallToolResult::success(vec![ContentBlock::text(output.to_json_string())]))
            }
            Err(err) => Ok(tool_error_response(err)),
        }
    }
}

#[tool_handler]
impl ServerHandler for HwpForgeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::LATEST)
        .with_server_info(
            Implementation::new("hwpforge-mcp", env!("CARGO_PKG_VERSION"))
                .with_title("HwpForge MCP Server")
                .with_description("AI-first Korean HWPX document generation and editing tools")
                .with_website_url("https://github.com/ai-screams/HwpForge"),
        )
        .with_instructions(
            "HwpForge MCP server for Korean HWPX document generation and editing. \
             Converts Markdown to HWPX, inspects document structure, and supports \
             JSON round-trip editing. Use hwpforge_templates to discover available \
             style templates before creating documents. Resources provide style template \
             details. Prompts guide document creation workflows.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        resources::list_resources()
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        resources::read_resource(&request.uri)
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        prompts::list_prompts()
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        prompts::get_prompt(&request.name, request.arguments.as_ref())
    }
}
