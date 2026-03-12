//! HwpForge CLI — AI-first document generation and editing.

mod commands;
mod error;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
        Commands::ToMd { input, output } => {
            commands::to_md::run(&input, &output, cli.json);
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
