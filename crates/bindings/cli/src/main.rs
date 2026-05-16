mod commands;
mod errors;
mod output;

use clap::{Parser, Subcommand};
use std::process;

#[derive(Parser)]
#[command(name = "quillmark")]
#[command(about = "Command-line interface for Quillmark", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Render markdown file to output format
    Render(commands::render::RenderArgs),

    /// Output the JSON schema for a quill
    Schema(commands::schema::SchemaArgs),

    /// Output the annotated Markdown blueprint for a quill
    Specs(commands::specs::SpecsArgs),

    /// Validate a quill's configuration (including defaults)
    Validate(commands::validate::ValidateArgs),

    /// Display metadata and information about a quill
    Info(commands::info::InfoArgs),
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Render(args) => commands::render::execute(args),
        Commands::Schema(args) => commands::schema::execute(args),
        Commands::Specs(args) => commands::specs::execute(args),
        Commands::Validate(args) => commands::validate::execute(args),
        Commands::Info(args) => commands::info::execute(args),
    };

    if let Err(e) = result {
        errors::print_cli_error(&e);
        process::exit(1);
    }
}
