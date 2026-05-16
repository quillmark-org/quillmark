use crate::commands::load_quill;
use crate::errors::{CliError, Result};
use crate::output::{derive_output_path, OutputWriter};
use clap::Parser;
use quillmark::Document;
use quillmark_core::{OutputFormat, RenderOptions};
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
pub struct RenderArgs {
    /// Path to quill directory
    #[arg(value_name = "QUILL_PATH")]
    quill: PathBuf,

    /// Path to markdown file with YAML frontmatter
    #[arg(value_name = "MARKDOWN_FILE")]
    markdown_file: Option<PathBuf>,

    /// Output file path (default: derived from input filename)
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Output format: pdf, svg, png, txt
    #[arg(short, long, value_name = "FORMAT", default_value = "pdf")]
    format: String,

    /// Write output to stdout instead of file
    #[arg(long)]
    stdout: bool,

    /// Show detailed processing information
    #[arg(short, long)]
    verbose: bool,

    /// Suppress all non-error output
    #[arg(long)]
    quiet: bool,

    /// Output intermediate JSON data to file
    #[arg(long, value_name = "DATA_FILE")]
    output_data: Option<PathBuf>,
}

pub fn execute(args: RenderArgs) -> Result<()> {
    if args.verbose {
        println!("Loading quill from: {}", args.quill.display());
    }

    // Load quill
    let quill = load_quill(&args.quill)?;

    if args.verbose {
        println!("Quill loaded: {}", quill.source().name());
    }

    // Determine if we have a markdown file or need to use the generated blueprint
    let (parse_output, markdown_path_for_output) =
        if let Some(ref markdown_path) = args.markdown_file {
            // Validate markdown file exists
            if !markdown_path.exists() {
                return Err(CliError::InvalidArgument(format!(
                    "Markdown file not found: {}",
                    markdown_path.display()
                )));
            }

            if args.verbose {
                println!("Reading markdown from: {}", markdown_path.display());
            }

            // Read markdown file
            let markdown = fs::read_to_string(markdown_path)?;

            // Parse markdown
            let output = Document::from_markdown_with_warnings(&markdown)?;

            if args.verbose {
                println!("Markdown parsed successfully");
            }
            (output, Some(markdown_path.clone()))
        } else {
            // Fall back to the quill's generated blueprint.
            let markdown = quill.source().config().blueprint();

            if args.verbose {
                println!("Using generated blueprint from quill");
            }

            // Parse markdown
            let output = Document::from_markdown_with_warnings(&markdown)?;

            if args.verbose {
                println!("Blueprint parsed successfully");
            }

            (output, None)
        };
    let (parsed, parse_warnings) = (parse_output.document, parse_output.warnings);

    if args.verbose {
        println!("Render-ready quill for backend: {}", quill.backend_id());
    }

    // Parse output format
    let output_format = match args.format.to_lowercase().as_str() {
        "pdf" => OutputFormat::Pdf,
        "svg" => OutputFormat::Svg,
        "png" => OutputFormat::Png,
        "txt" => OutputFormat::Txt,
        _ => {
            return Err(CliError::InvalidArgument(format!(
                "Invalid output format: {}. Must be one of: pdf, svg, png, txt",
                args.format
            )));
        }
    };

    if args.verbose {
        println!("Rendering to format: {:?}", output_format);
    }

    // Handle output-data
    if let Some(data_path) = args.output_data {
        let json_data = quill
            .compile_data(&parsed)
            .map_err(|e| CliError::Render(e))?;
        let f = std::fs::File::create(&data_path).map_err(|e| {
            CliError::Io(std::io::Error::new(
                e.kind(),
                format!(
                    "Failed to create data output file '{}': {}",
                    data_path.display(),
                    e
                ),
            ))
        })?;
        serde_json::to_writer_pretty(f, &json_data).map_err(|e| {
            CliError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to write JSON data: {}", e),
            ))
        })?;
        if args.verbose && !args.quiet {
            println!("JSON data written to: {}", data_path.display());
        }
    }

    // Render
    let mut result = quill.render(
        &parsed,
        &RenderOptions {
            output_format: Some(output_format),
            ..Default::default()
        },
    )?;

    // Merge parse-time warnings into the render result so downstream tooling
    // sees them in a single channel.
    result.warnings.splice(0..0, parse_warnings);

    // Display warnings if any
    if !result.warnings.is_empty() && !args.quiet {
        crate::errors::print_warnings(&result.warnings);
    }

    // Get the first artifact (there should only be one for single format render)
    let artifact = result.artifacts.first().ok_or_else(|| {
        CliError::InvalidArgument("No artifacts produced from rendering".to_string())
    })?;

    // Determine output path
    let output_path = if args.stdout {
        None
    } else {
        Some(args.output.unwrap_or_else(|| {
            if let Some(ref path) = markdown_path_for_output {
                derive_output_path(path, &args.format)
            } else {
                PathBuf::from(format!("blueprint.{}", args.format))
            }
        }))
    };

    let writer = OutputWriter::new(args.stdout, output_path, args.quiet);
    writer.write(&artifact.bytes)?;

    if args.verbose && !args.quiet {
        println!("Rendering completed successfully");
    }

    Ok(())
}
