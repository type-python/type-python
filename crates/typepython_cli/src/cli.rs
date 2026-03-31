use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "typepython", version, about = "Rust compiler and tooling for TypePython")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Create a starter TypePython config and source tree.
    Init(InitArgs),
    /// Load the project and run the TypePython checking pipeline.
    Check(RunArgs),
    /// Build Python output, stubs, and cache artifacts for the project.
    Build(RunArgs),
    /// Watch project inputs and rebuild/check when files change.
    Watch(RunArgs),
    /// Remove configured build and cache directories.
    Clean(CleanArgs),
    /// Start the TypePython language server.
    Lsp(RunArgs),
    /// Verify emitted artifacts and incremental state.
    Verify(VerifyArgs),
    /// Analyze migration coverage and dynamic boundaries.
    Migrate(MigrateArgs),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    /// Human-readable output.
    Text,
    /// Machine-readable JSON output.
    Json,
}

#[derive(Debug, Args)]
pub(crate) struct RunArgs {
    /// Project directory to search from.
    #[arg(long, value_name = "PATH")]
    pub(crate) project: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// Target directory for generated files.
    #[arg(long, value_name = "PATH", default_value = ".")]
    pub(crate) dir: PathBuf,
    /// Overwrite existing generated files.
    #[arg(long)]
    pub(crate) force: bool,
    #[arg(long)]
    pub(crate) embed_pyproject: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CleanArgs {
    /// Project directory to search from.
    #[arg(long, value_name = "PATH")]
    pub(crate) project: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyArgs {
    #[command(flatten)]
    pub(crate) run: RunArgs,
    #[arg(
        long = "wheel",
        value_name = "PATH",
        help = "Verify a published wheel artifact against the build output"
    )]
    pub(crate) wheels: Vec<PathBuf>,
    #[arg(
        long = "sdist",
        value_name = "PATH",
        help = "Verify a published source distribution against the build output"
    )]
    pub(crate) sdists: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateArgs {
    #[command(flatten)]
    pub(crate) run: RunArgs,
    /// Emit the migration coverage report.
    #[arg(long)]
    pub(crate) report: bool,
    /// Generate inferred `.pyi` stubs for the selected `.py` files or directories.
    #[arg(long = "emit-stubs", value_name = "PATH")]
    pub(crate) emit_stubs: Vec<PathBuf>,
    /// Output directory for generated stubs. Defaults to writing alongside the source `.py` files.
    #[arg(long = "stub-out-dir", value_name = "PATH")]
    pub(crate) stub_out_dir: Option<PathBuf>,
}
