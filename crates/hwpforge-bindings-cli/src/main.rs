//! HwpForge CLI — AI-first document generation and editing.

mod analysis;
mod commands;
mod error;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Markdown conversion mode.
#[derive(Clone, Debug, ValueEnum)]
pub enum MdMode {
    /// Style-aware conversion with images (default).
    Styled,
    /// Readable markdown without style information.
    Lossy,
    /// Round-trip-safe markdown with YAML frontmatter.
    Lossless,
}

/// AI-first CLI for Korean HWP/HWPX document generation.
#[derive(Parser)]
#[command(name = "hwpforge", version, about)]
struct Cli {
    /// Output in JSON format (machine-readable).
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Audit structural/semantic parity between HWP5 source and converted HWPX output.
    #[command(
        long_about = "Audit structural and semantic parity between an HWP5 source and a converted HWPX result.\n\nThis command compares parser-backed semantic metrics and recursive package analysis. It does not guarantee pixel-perfect visual or pagination parity; use the checklist in the report for follow-up visual verification."
    )]
    AuditHwp5 {
        /// Original HWP5 (`.hwp`) file.
        source: PathBuf,

        /// Converted HWPX (`.hwpx`) result to compare against.
        result: PathBuf,
    },

    /// Build a raw fixture census for an HWP5 file and optional HWPX companion.
    CensusHwp5 {
        /// Input HWP5 (`.hwp`) file.
        input: PathBuf,

        /// Optional HWPX companion fixture for nested XML/path census.
        #[arg(long)]
        companion: Option<PathBuf>,

        /// Optional JSON output path for saving the census dataset.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Convert an HWP5 file to HWPX.
    ConvertHwp5 {
        /// Input HWP5 file.
        input: PathBuf,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Convert Markdown to HWPX.
    Convert {
        /// Input markdown file (use '-' for stdin).
        input: String,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Style preset name.
        #[arg(long, default_value = "default")]
        preset: String,
    },

    /// Inspect HWPX document structure.
    #[command(
        long_about = "Inspect decoded HWPX document structure.\n\nThis command accepts HWPX files only. For HWP5 sources, use `convert-hwp5` to generate HWPX first, or `audit-hwp5` to compare a converted HWPX result against the original HWP5 source."
    )]
    Inspect {
        /// HWPX file to inspect.
        file: PathBuf,

        /// Include style details (char/para shapes).
        #[arg(long)]
        styles: bool,
    },

    /// Export HWPX to editable JSON.
    ToJson {
        /// HWPX file to export.
        file: PathBuf,

        /// Output JSON file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Extract only a specific section (0-based index).
        #[arg(long)]
        section: Option<usize>,

        /// Exclude style information from output.
        #[arg(long)]
        no_styles: bool,
    },

    /// Convert JSON back to HWPX.
    FromJson {
        /// Input JSON file.
        input: PathBuf,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Base HWPX file to inherit images from (for round-trip fidelity).
        #[arg(long)]
        base: Option<PathBuf>,
    },

    /// List named click-here fields (누름틀) and whether each is fillable.
    Fields {
        /// HWPX file to inspect.
        file: PathBuf,
    },

    /// Show the document navigation map: headings, tables, fields, bookmarks.
    #[command(
        long_about = "Show the document navigation map — headings (shared outline classification), tables (ordinal + logical grid dims), named click-here fields, and bookmarks.\n\nName anchors (heading text, table ordinal, field/bookmark name) are the primary keys; {section, para} locators are secondary and go stale after structural edits. Fetch this once before targeted reads or edits."
    )]
    Outline {
        /// HWPX file to inspect.
        file: PathBuf,
    },

    /// Fill named click-here fields (누름틀) with values, preserving everything else.
    #[command(
        long_about = "Fill named click-here fields (누름틀) by name, byte-preserving every untouched package entry.\n\nAll requested values are validated first (unknown/duplicate/unfillable names, empty values) and nothing is written unless every one passes. Use `fields` to discover names."
    )]
    Fill {
        /// HWPX file to fill.
        file: PathBuf,

        /// name=value pair to fill (repeatable).
        #[arg(long = "set", value_name = "NAME=VALUE")]
        sets: Vec<String>,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Edit table cells by logical grid address (E3).
    #[command(
        name = "set-cell",
        long_about = "Edit table cells by logical grid address, all-or-nothing behind the fail-closed admission gate.\n\nAddress a cell with --table N (to-json export order) plus one of --at \"r,c\" (covered positions resolve to their merge anchor), --right-of LABEL, or --below LABEL (normalized exact match). --text \"\" clears the cell. Batch edits go through --map (JSON array of CellSpec)."
    )]
    SetCell {
        /// HWPX file to edit.
        file: PathBuf,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Table ordinal (document order, 0-based).
        #[arg(long)]
        table: Option<usize>,

        /// Grid coordinate "row,col" (covered positions resolve to anchor).
        #[arg(long, value_name = "R,C")]
        at: Option<String>,

        /// Target the cell right of this uniquely labeled cell.
        #[arg(long, value_name = "LABEL")]
        right_of: Option<String>,

        /// Target the cell below this uniquely labeled cell.
        #[arg(long, value_name = "LABEL")]
        below: Option<String>,

        /// Replacement text (empty string clears the cell).
        #[arg(long)]
        text: Option<String>,

        /// Batch spec map (JSON array of CellSpec); exclusive with the flags above.
        #[arg(long, value_name = "MAP_JSON")]
        map: Option<PathBuf>,
    },

    /// Discover prose placeholder candidates for stamping (E6, phase 1).
    #[command(
        name = "stamp-plan",
        long_about = "Discover class-A placeholder candidates (checkbox/paren-blank/date-blank/standalone-@/seal tokens) for template stamping.\n\nAuthor a spec map from the output: every unguarded candidate must get an action — {\"field\":{\"name\":\"…\"}} or \"ignore\" — then run `stamp --map`."
    )]
    StampPlan {
        /// HWPX file to inspect.
        file: PathBuf,
    },

    /// Promote placeholders to named click-here fields (E6, phase 2).
    #[command(
        long_about = "Apply a stamp spec map all-or-nothing behind a fail-closed admission gate (no-op round-trip + ZIP closed-world), then write the stamped HWPX and a manifest.\n\nThe map is a JSON array of StampSpec (from `stamp-plan --json` candidates plus an action per candidate). The stamped output is immediately usable with `fields`/`fill`."
    )]
    Stamp {
        /// HWPX file to stamp.
        file: PathBuf,

        /// JSON spec map (array of StampSpec with actions).
        #[arg(long, value_name = "MAP_JSON")]
        map: PathBuf,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,

        /// Manifest JSON path (default: <output>.manifest.json).
        #[arg(long)]
        manifest: Option<PathBuf>,
    },

    /// Patch a section in an existing HWPX file.
    Patch {
        /// Base HWPX file.
        base: PathBuf,

        /// Section index to replace (0-based).
        #[arg(long)]
        section: usize,

        /// JSON file containing the replacement section.
        section_json: PathBuf,

        /// Output HWPX file path.
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Manage style presets.
    Templates {
        #[command(subcommand)]
        action: TemplateAction,
    },

    /// Output JSON Schema for document/style types.
    Schema {
        /// Type to output schema for: document, exported-document, exported-section.
        #[arg(default_value = "document")]
        type_name: String,
    },

    /// Convert an HWPX file to Markdown.
    ToMd {
        /// Path to the input HWPX file.
        input: PathBuf,

        /// Output directory (defaults to same directory as input).
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Conversion mode: styled (default), lossy, or lossless.
        #[arg(long, value_enum, default_value_t = MdMode::Styled)]
        mode: MdMode,
    },
}

#[derive(Subcommand)]
enum TemplateAction {
    /// List available presets.
    List,
    /// Show preset details.
    Show {
        /// Preset name.
        name: String,
    },
}

fn main() {
    // Exit cleanly on broken pipe (e.g., `hwpforge schema document | head`).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.to_string();
        if msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        default_hook(info);
    }));

    let cli = Cli::parse();

    match cli.command {
        Commands::AuditHwp5 { source, result } => {
            commands::audit_hwp5::run(&source, &result, cli.json);
        }
        Commands::CensusHwp5 { input, companion, output } => {
            commands::census_hwp5::run(&input, &companion, &output, cli.json);
        }
        Commands::ConvertHwp5 { input, output } => {
            commands::convert_hwp5::run(&input, &output, cli.json);
        }
        Commands::Convert { input, output, preset } => {
            commands::convert::run(&input, &output, &preset, cli.json);
        }
        Commands::Inspect { file, styles } => {
            commands::inspect::run(&file, styles, cli.json);
        }
        Commands::ToJson { file, output, section, no_styles } => {
            commands::to_json::run(&file, &output, section, no_styles, cli.json);
        }
        Commands::FromJson { input, output, base } => {
            commands::from_json::run(&input, &output, &base, cli.json);
        }
        Commands::Outline { file } => {
            commands::outline::run(&file, cli.json);
        }
        Commands::Fields { file } => {
            commands::fields::run(&file, cli.json);
        }
        Commands::Fill { file, sets, output } => {
            commands::fill::run(&file, &sets, &output, cli.json);
        }
        Commands::SetCell { file, output, table, at, right_of, below, text, map } => {
            commands::set_cell::run(
                &file,
                &output,
                commands::set_cell::SingleTarget {
                    table,
                    at: at.as_deref(),
                    right_of: right_of.as_deref(),
                    below: below.as_deref(),
                    text: text.as_deref(),
                },
                map.as_ref(),
                cli.json,
            );
        }
        Commands::StampPlan { file } => {
            commands::stamp::run_plan(&file, cli.json);
        }
        Commands::Stamp { file, map, output, manifest } => {
            commands::stamp::run(&file, &map, &output, manifest.as_deref(), cli.json);
        }
        Commands::Patch { base, section, section_json, output } => {
            commands::patch::run(&base, section, &section_json, &output, cli.json);
        }
        Commands::Templates { action } => match action {
            TemplateAction::List => commands::templates::run_list(cli.json),
            TemplateAction::Show { name } => commands::templates::run_show(&name, cli.json),
        },
        Commands::Schema { type_name } => {
            commands::schema::run(&type_name, cli.json);
        }
        Commands::ToMd { input, output, mode } => {
            commands::to_md::run(&input, &output, &mode, cli.json);
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }
}
