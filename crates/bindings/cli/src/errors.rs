use quillmark_core::RenderError;
use std::fmt;

/// CLI-specific error type that wraps underlying errors
/// and provides user-friendly error messages
#[derive(Debug)]
pub enum CliError {
    Io(std::io::Error),
    Render(RenderError),
    Parse(quillmark_core::ParseError),
    InvalidArgument(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Io(e) => write!(f, "I/O error: {}", e),
            CliError::Render(e) => write!(f, "{}", e),
            CliError::Parse(e) => write!(f, "Parse error: {}", e),
            CliError::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Io(err)
    }
}

impl From<RenderError> for CliError {
    fn from(err: RenderError) -> Self {
        CliError::Render(err)
    }
}

impl From<quillmark_core::ParseError> for CliError {
    fn from(err: quillmark_core::ParseError) -> Self {
        CliError::Parse(err)
    }
}

pub type Result<T> = std::result::Result<T, CliError>;

/// Print detailed diagnostics for CLI errors
pub fn print_cli_error(err: &CliError) {
    match err {
        CliError::Render(render_err) => {
            // Use the core library's print_errors function for full diagnostics
            quillmark_core::error::print_errors(render_err);
        }
        CliError::Parse(parse_err) => {
            eprintln!("{}", parse_err.to_diagnostic().fmt_pretty());
        }
        CliError::Io(io_err) => {
            eprintln!("[ERROR] I/O error: {}", io_err);
        }
        CliError::InvalidArgument(msg) => {
            eprintln!("[ERROR] Invalid argument: {}", msg);
        }
    }
}

/// Print warnings with full diagnostic information
pub fn print_warnings(warnings: &[quillmark_core::Diagnostic]) {
    if warnings.is_empty() {
        return;
    }

    eprintln!("\nWarnings:");
    for warning in warnings {
        eprintln!("{}", warning.fmt_pretty());
    }
}
