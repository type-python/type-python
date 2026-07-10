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
    /// Validate emitted artifacts across downstream Python type checkers.
    Compat(CompatArgs),
    /// Compare two public typing surfaces and report likely API drift.
    ApiDiff(ApiDiffArgs),
    /// Inspect dependency and stub metadata for typing health.
    TypeHealth(TypeHealthArgs),
    /// Analyze migration coverage and dynamic boundaries.
    Migrate(MigrateArgs),
    /// Validate framework adapter manifests.
    Adapter(AdapterArgs),
}

impl Command {
    pub(crate) fn output_format(&self) -> OutputFormat {
        match self {
            Self::Check(args) | Self::Build(args) | Self::Watch(args) | Self::Lsp(args) => {
                args.format
            }
            Self::Verify(args) => args.run.format,
            Self::Compat(args) => args.run.format,
            Self::ApiDiff(args) => args.format,
            Self::TypeHealth(args) => args.run.format,
            Self::Migrate(args) => args.run.format,
            Self::Adapter(AdapterArgs { command: AdapterCommand::Validate(args) }) => args.format,
            Self::Init(_) | Self::Clean(_) => OutputFormat::Text,
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct AdapterArgs {
    #[command(subcommand)]
    pub(crate) command: AdapterCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AdapterCommand {
    /// Validate a `typepython-framework.toml` adapter manifest.
    Validate(AdapterValidateArgs),
}

#[derive(Debug, Args)]
pub(crate) struct AdapterValidateArgs {
    /// Adapter manifest to validate.
    #[arg(value_name = "PATH")]
    pub(crate) manifest: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub(crate) format: OutputFormat,
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
    #[arg(
        long = "api-diff",
        value_name = "OLD_SURFACE",
        help = "Compare an old public typing surface against the current verified build output"
    )]
    pub(crate) api_diff_old: Option<PathBuf>,
    #[arg(
        long = "checker",
        value_name = "COMMAND",
        help = "Run an external type checker against the emitted build output (repeatable)"
    )]
    pub(crate) checkers: Vec<String>,
    #[arg(
        long = "checker-preset",
        value_name = "PRESET",
        help = "Run a named checker preset; currently supports `all` for mypy, pyright, and ty"
    )]
    pub(crate) checker_preset: Option<String>,
    #[arg(
        long = "checker-allowlist",
        value_name = "PATH",
        help = "TOML allowlist of known checker disagreements to report without failing verification"
    )]
    pub(crate) checker_allowlist: Option<PathBuf>,
    #[arg(
        long = "unsafe-runtime-imports",
        help = "Import emitted runtime modules during verification to compare runtime-visible public names; this executes project-controlled Python code"
    )]
    pub(crate) unsafe_runtime_imports: bool,
    #[arg(
        long = "publication-type-health",
        help = "Run package-maintainer type-health checks during verification and fail on type metadata debt"
    )]
    pub(crate) publication_type_health: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CompatArgs {
    #[command(flatten)]
    pub(crate) run: RunArgs,
    #[arg(
        long = "checkers",
        value_name = "LIST",
        default_value = "all",
        help = "Comma-separated checker list: all, mypy, pyright, ty, pyrefly, basedpyright, zuban"
    )]
    pub(crate) checkers: String,
    #[arg(
        long = "profile",
        value_name = "NAME",
        help = "Checker profile: library-portable, app-strict, pyright-first, mypy-compatible, experimental-checkers"
    )]
    pub(crate) profile: Option<String>,
    #[arg(
        long = "strict-portability",
        help = "Fail on any configured downstream checker rejection"
    )]
    pub(crate) strict_portability: bool,
    #[arg(
        long = "checker-allowlist",
        value_name = "PATH",
        help = "TOML allowlist of known checker disagreements to report without failing portability checks"
    )]
    pub(crate) checker_allowlist: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct ApiDiffArgs {
    /// Old source/build/wheel/sdist/.pyi tree. Prototype support accepts files or directories.
    #[arg(value_name = "OLD")]
    pub(crate) old: PathBuf,
    /// New source/build/wheel/sdist/.pyi tree. Prototype support accepts files or directories.
    #[arg(value_name = "NEW")]
    pub(crate) new: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub(crate) format: OutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct TypeHealthArgs {
    #[command(flatten)]
    pub(crate) run: RunArgs,
    /// Fail when the computed type-health score is below this threshold.
    #[arg(long = "fail-under", value_name = "SCORE")]
    pub(crate) fail_under: Option<u8>,
    /// Write `.typepython/type-lock.toml` with observed typing inputs.
    #[arg(long = "write-lock")]
    pub(crate) write_lock: bool,
}

#[derive(Debug, Args)]
pub(crate) struct MigrateArgs {
    #[command(flatten)]
    pub(crate) run: RunArgs,
    /// Emit the migration coverage report.
    #[arg(long)]
    pub(crate) report: bool,
    /// Read an existing migration diagnostic baseline for comparison.
    #[arg(long = "baseline", value_name = "PATH")]
    pub(crate) baseline: Option<PathBuf>,
    /// Write the current diagnostic baseline to the given JSON file.
    #[arg(long = "write-baseline", value_name = "PATH")]
    pub(crate) write_baseline: Option<PathBuf>,
    /// Fail when diagnostics appear that are not present in the baseline.
    #[arg(long = "no-new-diagnostics")]
    pub(crate) no_new_diagnostics: bool,
    /// Read an existing migration type budget baseline for comparison.
    #[arg(long = "budget-baseline", value_name = "PATH")]
    pub(crate) budget_baseline: Option<PathBuf>,
    /// Write the current migration type budget baseline to the given JSON file.
    #[arg(long = "write-budget-baseline", value_name = "PATH")]
    pub(crate) write_budget_baseline: Option<PathBuf>,
    /// Fail when public Any exports appear that are not present in the budget baseline.
    #[arg(long = "no-new-public-any")]
    pub(crate) no_new_public_any: bool,
    /// Fail when public Unknown exports appear that are not present in the budget baseline.
    #[arg(long = "no-new-public-unknown")]
    pub(crate) no_new_public_unknown: bool,
    /// Generate inferred `.pyi` stubs for the selected `.py` files or directories.
    #[arg(long = "emit-stubs", value_name = "PATH")]
    pub(crate) emit_stubs: Vec<PathBuf>,
    /// Output directory for generated stubs. Defaults to writing alongside the source `.py` files.
    #[arg(long = "stub-out-dir", value_name = "PATH")]
    pub(crate) stub_out_dir: Option<PathBuf>,
}
