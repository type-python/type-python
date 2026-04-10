//! Project discovery and configuration loading for TypePython.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt::{self, Display},
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const NULL_SENTINEL: &str = "__TYPEPYTHON_NULL__";

/// Resolved TypePython configuration source.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    /// Loaded from `typepython.toml`.
    TypePythonToml,
    /// Loaded from `[tool.typepython]` in `pyproject.toml`.
    PyProject,
}

impl Display for ConfigSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TypePythonToml => "typepython.toml",
            Self::PyProject => "pyproject.toml:[tool.typepython]",
        })
    }
}

/// Loaded project configuration together with its discovery context.
#[derive(Debug, Clone)]
pub struct ConfigHandle {
    /// Directory that owns the discovered config file.
    pub config_dir: PathBuf,
    /// Path to the discovered config file.
    pub config_path: PathBuf,
    /// Source kind of the discovered config.
    pub source: ConfigSource,
    /// Effective configuration after defaults and profile expansion.
    pub config: Config,
}

impl ConfigHandle {
    /// Resolves a project-relative path against the discovered config directory.
    #[must_use]
    pub fn resolve_relative_path(&self, relative: &str) -> PathBuf {
        self.config_dir.join(relative)
    }
}

/// Errors produced while discovering or loading TypePython config.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// No TypePython project could be found.
    #[error("unable to find `typepython.toml` or `[tool.typepython]` starting from {0}")]
    NotFound(PathBuf),
    /// Underlying filesystem error while reading config.
    #[error("TPY1001: unable to read configuration from {path}: {source}")]
    Io {
        /// Path that failed to load.
        path: PathBuf,
        /// Original IO error.
        #[source]
        source: io::Error,
    },
    /// TOML parse failure.
    #[error("TPY1001: unable to parse configuration from {path}: {source}")]
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Original TOML error.
        #[source]
        source: toml::de::Error,
    },
    #[error("TPY1002: invalid configuration value in {path}: {message}")]
    InvalidValue { path: PathBuf, message: String },
}

/// Effective TypePython configuration.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Project filesystem settings.
    pub project: ProjectConfig,
    /// Import resolution settings.
    pub resolution: ResolutionConfig,
    /// Formatting settings.
    pub format: FormatConfig,
    /// Emit settings.
    pub emit: EmitConfig,
    /// Typing settings.
    pub typing: TypingConfig,
    /// Watch settings.
    pub watch: WatchConfig,
}

/// Project-level settings.
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// Source roots.
    pub src: Vec<String>,
    /// Include globs.
    pub include: Vec<String>,
    /// Exclude globs.
    pub exclude: Vec<String>,
    /// Logical root for output projection.
    pub root_dir: String,
    /// Output directory.
    pub out_dir: String,
    /// Cache directory.
    pub cache_dir: String,
    /// Target Python version.
    pub target_python: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            src: vec![String::from("src")],
            include: vec![
                String::from("src/**/*.tpy"),
                String::from("src/**/*.py"),
                String::from("src/**/*.pyi"),
            ],
            exclude: vec![
                String::from(".typepython/**"),
                String::from("dist/**"),
                String::from(".venv/**"),
                String::from("venv/**"),
            ],
            root_dir: String::from("src"),
            out_dir: String::from(".typepython/build"),
            cache_dir: String::from(".typepython/cache"),
            target_python: String::from("3.10"),
        }
    }
}

/// Import resolution settings.
#[derive(Debug, Clone)]
pub struct ResolutionConfig {
    /// Base URL for non-relative resolution.
    pub base_url: String,
    /// Extra type roots.
    pub type_roots: Vec<String>,
    /// Configured Python executable.
    pub python_executable: Option<String>,
    /// Static alias map.
    pub paths: BTreeMap<String, Vec<String>>,
}

impl Default for ResolutionConfig {
    fn default() -> Self {
        Self {
            base_url: String::from("."),
            type_roots: Vec::new(),
            python_executable: None,
            paths: BTreeMap::new(),
        }
    }
}

/// Formatting settings.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Explicit formatter command to execute over stdin. The `{file}` placeholder expands to the
    /// current document path and `{workspace_root}` expands to the project root.
    pub command: Option<Vec<String>>,
    /// Preferred line length for auto-detected formatter backends.
    pub line_length: u16,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self { command: None, line_length: 1000 }
    }
}

/// Emit settings.
#[derive(Debug, Clone)]
pub struct EmitConfig {
    /// Emit `.pyi` files.
    pub emit_pyi: bool,
    /// Emit `.pyc` files.
    pub emit_pyc: bool,
    /// Emit `py.typed`.
    pub write_py_typed: bool,
    /// Preserve comments when possible.
    pub preserve_comments: bool,
    /// Stop emit on fatal diagnostics.
    pub no_emit_on_error: bool,
    /// Emit runtime validators.
    pub runtime_validators: bool,
}

impl Default for EmitConfig {
    fn default() -> Self {
        Self {
            emit_pyi: true,
            emit_pyc: false,
            write_py_typed: true,
            preserve_comments: true,
            no_emit_on_error: true,
            runtime_validators: false,
        }
    }
}

/// Import typing fallback.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFallback {
    /// Treat untyped imports as `unknown`.
    Unknown,
    /// Treat untyped imports as `dynamic`.
    Dynamic,
}

/// Diagnostic severity policy in config.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    /// Ignore the diagnostic.
    Ignore,
    /// Emit a warning.
    Warning,
    /// Emit an error.
    Error,
}

/// Reserved typing profile names.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TypingProfile {
    /// Library-oriented profile.
    Library,
    /// Application-oriented profile.
    Application,
    /// Migration-oriented profile.
    Migration,
}

/// Typing settings.
#[derive(Debug, Clone)]
pub struct TypingConfig {
    /// Selected profile, if any.
    pub profile: Option<TypingProfile>,
    /// Master strictness switch.
    pub strict: bool,
    /// Enforce strict nullability.
    pub strict_nulls: bool,
    /// Fallback behavior for untyped imports.
    pub imports: ImportFallback,
    /// Disallow implicit dynamic fallback.
    pub no_implicit_dynamic: bool,
    /// Warn on unsafe boundaries.
    pub warn_unsafe: bool,
    /// Enable sealed exhaustiveness.
    pub enable_sealed_exhaustiveness: bool,
    /// Severity for deprecated symbol reporting.
    pub report_deprecated: DiagnosticLevel,
    /// Require explicit override annotations.
    pub require_explicit_overrides: bool,
    /// Require known public surface types.
    pub require_known_public_types: bool,
    /// Enable pass-through inference.
    pub infer_passthrough: bool,
    pub conditional_returns: bool,
}

impl Default for TypingConfig {
    fn default() -> Self {
        Self {
            profile: None,
            strict: true,
            strict_nulls: true,
            imports: ImportFallback::Unknown,
            no_implicit_dynamic: true,
            warn_unsafe: true,
            enable_sealed_exhaustiveness: true,
            report_deprecated: DiagnosticLevel::Warning,
            require_explicit_overrides: false,
            require_known_public_types: false,
            infer_passthrough: false,
            conditional_returns: false,
        }
    }
}

/// Watch settings.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Debounce delay in milliseconds.
    pub debounce_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self { debounce_ms: 80 }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    project: Option<RawProjectConfig>,
    resolution: Option<RawResolutionConfig>,
    format: Option<RawFormatConfig>,
    emit: Option<RawEmitConfig>,
    typing: Option<RawTypingConfig>,
    watch: Option<RawWatchConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjectConfig {
    src: Option<Vec<String>>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    root_dir: Option<String>,
    out_dir: Option<String>,
    cache_dir: Option<String>,
    target_python: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResolutionConfig {
    base_url: Option<String>,
    type_roots: Option<Vec<String>>,
    python_executable: Option<String>,
    #[serde(default)]
    paths: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEmitConfig {
    emit_pyi: Option<bool>,
    emit_pyc: Option<bool>,
    write_py_typed: Option<bool>,
    preserve_comments: Option<bool>,
    no_emit_on_error: Option<bool>,
    runtime_validators: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFormatConfig {
    command: Option<Vec<String>>,
    line_length: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTypingConfig {
    profile: Option<TypingProfile>,
    strict: Option<bool>,
    strict_nulls: Option<bool>,
    imports: Option<ImportFallback>,
    no_implicit_dynamic: Option<bool>,
    warn_unsafe: Option<bool>,
    enable_sealed_exhaustiveness: Option<bool>,
    report_deprecated: Option<DiagnosticLevel>,
    require_explicit_overrides: Option<bool>,
    require_known_public_types: Option<bool>,
    infer_passthrough: Option<bool>,
    conditional_returns: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWatchConfig {
    debounce_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawPyProject {
    tool: Option<RawPyProjectTool>,
}

#[derive(Debug, Deserialize)]
struct RawPyProjectTool {
    typepython: Option<RawConfig>,
}

impl Config {
    fn validate(&self, config_path: &Path) -> Result<(), ConfigError> {
        let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

        validate_target_python(config_path, &self.project.target_python)?;
        validate_project_paths(
            config_path,
            config_dir,
            &self.project.out_dir,
            &self.project.cache_dir,
        )?;
        validate_resolution_base_url(config_path, config_dir, &self.resolution.base_url)?;
        validate_resolution_paths(config_path, &self.resolution.paths)?;

        if let Some(python_executable) = &self.resolution.python_executable {
            validate_python_executable(
                config_path,
                config_dir,
                python_executable,
                &self.project.target_python,
            )?;
        }

        validate_emit_preserve_comments(config_path, self.emit.preserve_comments)?;
        validate_formatter_config(config_path, &self.format)?;

        Ok(())
    }

    fn from_raw(raw: RawConfig) -> Self {
        let mut config = Self::default();

        if let Some(project) = raw.project {
            if let Some(src) = project.src {
                config.project.src = src;
            }
            if let Some(include) = project.include {
                config.project.include = include;
            }
            if let Some(exclude) = project.exclude {
                config.project.exclude = exclude;
            }
            if let Some(root_dir) = project.root_dir {
                config.project.root_dir = root_dir;
            }
            if let Some(out_dir) = project.out_dir {
                config.project.out_dir = out_dir;
            }
            if let Some(cache_dir) = project.cache_dir {
                config.project.cache_dir = cache_dir;
            }
            if let Some(target_python) = project.target_python {
                config.project.target_python = target_python;
            }
        }

        if let Some(resolution) = raw.resolution {
            if let Some(base_url) = resolution.base_url {
                config.resolution.base_url = base_url;
            }
            if let Some(type_roots) = resolution.type_roots {
                config.resolution.type_roots = type_roots;
            }
            if let Some(python_executable) = resolution.python_executable {
                config.resolution.python_executable =
                    if python_executable == NULL_SENTINEL { None } else { Some(python_executable) };
            }
            if !resolution.paths.is_empty() {
                config.resolution.paths = resolution.paths;
            }
        }

        if let Some(format) = raw.format {
            if let Some(command) = format.command {
                config.format.command = Some(command);
            }
            if let Some(line_length) = format.line_length {
                config.format.line_length = line_length;
            }
        }

        if let Some(emit) = raw.emit {
            if let Some(emit_pyi) = emit.emit_pyi {
                config.emit.emit_pyi = emit_pyi;
            }
            if let Some(emit_pyc) = emit.emit_pyc {
                config.emit.emit_pyc = emit_pyc;
            }
            if let Some(write_py_typed) = emit.write_py_typed {
                config.emit.write_py_typed = write_py_typed;
            }
            if let Some(preserve_comments) = emit.preserve_comments {
                config.emit.preserve_comments = preserve_comments;
            }
            if let Some(no_emit_on_error) = emit.no_emit_on_error {
                config.emit.no_emit_on_error = no_emit_on_error;
            }
            if let Some(runtime_validators) = emit.runtime_validators {
                config.emit.runtime_validators = runtime_validators;
            }
        }

        config.typing = TypingConfig::from_raw(raw.typing.unwrap_or_default());

        if let Some(watch) = raw.watch {
            if let Some(debounce_ms) = watch.debounce_ms {
                config.watch.debounce_ms = debounce_ms;
            }
        }

        config
    }
}

impl TypingConfig {
    fn from_raw(raw: RawTypingConfig) -> Self {
        let mut config = match raw.profile {
            Some(TypingProfile::Library) => Self {
                profile: Some(TypingProfile::Library),
                strict: true,
                strict_nulls: true,
                imports: ImportFallback::Unknown,
                no_implicit_dynamic: true,
                warn_unsafe: true,
                enable_sealed_exhaustiveness: true,
                report_deprecated: DiagnosticLevel::Warning,
                require_explicit_overrides: false,
                require_known_public_types: true,
                infer_passthrough: false,
                conditional_returns: false,
            },
            Some(TypingProfile::Application) => {
                Self { profile: Some(TypingProfile::Application), ..Self::default() }
            }
            Some(TypingProfile::Migration) => Self {
                profile: Some(TypingProfile::Migration),
                strict: false,
                strict_nulls: true,
                imports: ImportFallback::Dynamic,
                no_implicit_dynamic: false,
                warn_unsafe: true,
                enable_sealed_exhaustiveness: true,
                report_deprecated: DiagnosticLevel::Ignore,
                require_explicit_overrides: false,
                require_known_public_types: false,
                infer_passthrough: false,
                conditional_returns: false,
            },
            None => Self::default(),
        };

        if let Some(strict) = raw.strict {
            config.strict = strict;
        }
        if let Some(strict_nulls) = raw.strict_nulls {
            config.strict_nulls = strict_nulls;
        }
        if let Some(imports) = raw.imports {
            config.imports = imports;
        }
        if let Some(no_implicit_dynamic) = raw.no_implicit_dynamic {
            config.no_implicit_dynamic = no_implicit_dynamic;
        }
        if let Some(warn_unsafe) = raw.warn_unsafe {
            config.warn_unsafe = warn_unsafe;
        }
        if let Some(enable_sealed_exhaustiveness) = raw.enable_sealed_exhaustiveness {
            config.enable_sealed_exhaustiveness = enable_sealed_exhaustiveness;
        }
        if let Some(report_deprecated) = raw.report_deprecated {
            config.report_deprecated = report_deprecated;
        }
        if let Some(require_explicit_overrides) = raw.require_explicit_overrides {
            config.require_explicit_overrides = require_explicit_overrides;
        }
        if let Some(require_known_public_types) = raw.require_known_public_types {
            config.require_known_public_types = require_known_public_types;
        }
        if let Some(infer_passthrough) = raw.infer_passthrough {
            config.infer_passthrough = infer_passthrough;
        }
        if let Some(conditional_returns) = raw.conditional_returns {
            config.conditional_returns = conditional_returns;
        }

        config
    }
}

/// Discovers and loads TypePython configuration by searching upwards from a
/// starting directory.
pub fn load(start_dir: impl AsRef<Path>) -> Result<ConfigHandle, ConfigError> {
    let start_dir = start_dir.as_ref();

    for directory in start_dir.ancestors() {
        let typepython_path = directory.join("typepython.toml");
        if typepython_path.is_file() {
            let config = load_typepython_toml(&typepython_path)?;
            return Ok(ConfigHandle {
                config_dir: directory.to_path_buf(),
                config_path: typepython_path,
                source: ConfigSource::TypePythonToml,
                config,
            });
        }

        let pyproject_path = directory.join("pyproject.toml");
        if pyproject_path.is_file() {
            if let Some(config) = load_pyproject_toml(&pyproject_path)? {
                return Ok(ConfigHandle {
                    config_dir: directory.to_path_buf(),
                    config_path: pyproject_path,
                    source: ConfigSource::PyProject,
                    config,
                });
            }
        }
    }

    Err(ConfigError::NotFound(start_dir.to_path_buf()))
}

fn load_typepython_toml(path: &Path) -> Result<Config, ConfigError> {
    let raw = read_toml::<RawConfig>(path)?;
    let config = Config::from_raw(raw);
    config.validate(path)?;
    Ok(config)
}

fn load_pyproject_toml(path: &Path) -> Result<Option<Config>, ConfigError> {
    let raw = read_toml::<RawPyProject>(path)?;
    let maybe_config = raw.tool.and_then(|tool| tool.typepython);
    maybe_config
        .map(|raw_config| {
            let config = Config::from_raw(raw_config);
            config.validate(path)?;
            Ok(config)
        })
        .transpose()
}

fn read_toml<T>(path: &Path) -> Result<T, ConfigError>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(path)
        .map_err(|source| ConfigError::Io { path: path.to_path_buf(), source })?;
    let normalized = normalize_spec_toml(&content);

    toml::from_str(&normalized)
        .map_err(|source| ConfigError::Parse { path: path.to_path_buf(), source })
}

fn normalize_spec_toml(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("python_executable")
            && trimmed.split_once('=').is_some_and(|(_, value)| value.trim().starts_with("null"))
        {
            let indentation = line.len() - line.trim_start().len();
            normalized.push_str(&" ".repeat(indentation));
            normalized.push_str("python_executable = \"");
            normalized.push_str(NULL_SENTINEL);
            normalized.push('"');
        } else {
            normalized.push_str(line);
        }
        normalized.push('\n');
    }

    normalized
}

fn validate_project_paths(
    config_path: &Path,
    config_dir: &Path,
    out_dir: &str,
    cache_dir: &str,
) -> Result<(), ConfigError> {
    let normalized_out_dir = normalize_project_path(config_dir, out_dir);
    let normalized_cache_dir = normalize_project_path(config_dir, cache_dir);

    if normalized_out_dir == normalized_cache_dir {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "project.out_dir = `{out_dir}` and project.cache_dir = `{cache_dir}` resolve to the same path `{}`",
                normalized_out_dir.display()
            ),
        });
    }

    if normalized_out_dir.starts_with(&normalized_cache_dir)
        || normalized_cache_dir.starts_with(&normalized_out_dir)
    {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "project.out_dir = `{out_dir}` and project.cache_dir = `{cache_dir}` must not overlap; resolved paths `{}` and `{}` are nested",
                normalized_out_dir.display(),
                normalized_cache_dir.display()
            ),
        });
    }

    if let (Some(existing_out_dir), Some(existing_cache_dir)) = (
        best_effort_existing_project_path(&normalized_out_dir),
        best_effort_existing_project_path(&normalized_cache_dir),
    ) {
        if existing_out_dir == existing_cache_dir {
            return Err(ConfigError::InvalidValue {
                path: config_path.to_path_buf(),
                message: format!(
                    "project.out_dir = `{out_dir}` and project.cache_dir = `{cache_dir}` resolve through existing path aliases to the same path `{}`",
                    existing_out_dir.display()
                ),
            });
        }

        if existing_out_dir.starts_with(&existing_cache_dir)
            || existing_cache_dir.starts_with(&existing_out_dir)
        {
            return Err(ConfigError::InvalidValue {
                path: config_path.to_path_buf(),
                message: format!(
                    "project.out_dir = `{out_dir}` and project.cache_dir = `{cache_dir}` resolve through existing path aliases to overlapping paths `{}` and `{}`",
                    existing_out_dir.display(),
                    existing_cache_dir.display()
                ),
            });
        }
    }

    Ok(())
}

fn validate_resolution_base_url(
    config_path: &Path,
    config_dir: &Path,
    base_url: &str,
) -> Result<(), ConfigError> {
    let normalized_config_dir = normalize_lexical_path(config_dir);
    let normalized_base_url = normalize_project_path(config_dir, base_url);

    if normalized_base_url != normalized_config_dir {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "resolution.base_url = `{base_url}` is not implemented yet; only the project root (`.`) is currently supported"
            ),
        });
    }

    Ok(())
}

fn validate_resolution_paths(
    config_path: &Path,
    paths: &BTreeMap<String, Vec<String>>,
) -> Result<(), ConfigError> {
    if !paths.is_empty() {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: String::from(
                "resolution.paths is not implemented yet; remove path aliases until static path mapping support lands",
            ),
        });
    }

    Ok(())
}

fn validate_emit_preserve_comments(
    config_path: &Path,
    preserve_comments: bool,
) -> Result<(), ConfigError> {
    if !preserve_comments {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: String::from(
                "emit.preserve_comments = false is not implemented yet; lowered output currently always preserves comments when available",
            ),
        });
    }

    Ok(())
}

fn normalize_project_path(config_dir: &Path, configured_path: &str) -> PathBuf {
    let configured_path = Path::new(configured_path);
    let resolved = if configured_path.is_absolute() {
        configured_path.to_path_buf()
    } else {
        config_dir.join(configured_path)
    };

    normalize_lexical_path(&resolved)
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut has_root = false;

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => {
                normalized.push(component.as_os_str());
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = normalized.file_name().is_some_and(|name| name != OsStr::new(".."));
                if can_pop {
                    normalized.pop();
                } else if !has_root {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

fn best_effort_existing_project_path(path: &Path) -> Option<PathBuf> {
    let mut existing_prefix = normalize_lexical_path(path);
    let mut remainder = Vec::new();

    while !existing_prefix.exists() {
        let component = existing_prefix.file_name()?.to_os_string();
        remainder.push(component);
        existing_prefix.pop();
    }

    let mut resolved = fs::canonicalize(&existing_prefix).ok()?;
    for component in remainder.into_iter().rev() {
        resolved.push(component);
    }

    Some(normalize_lexical_path(&resolved))
}

fn validate_target_python(config_path: &Path, target_python: &str) -> Result<(), ConfigError> {
    match target_python {
        "3.10" | "3.11" | "3.12" => Ok(()),
        _ => Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "project.target_python = `{target_python}` is unsupported; expected one of `3.10`, `3.11`, or `3.12`"
            ),
        }),
    }
}

fn validate_python_executable(
    config_path: &Path,
    config_dir: &Path,
    python_executable: &str,
    target_python: &str,
) -> Result<(), ConfigError> {
    let executable_path = resolve_python_executable(config_dir, python_executable);
    let output = Command::new(&executable_path)
        .args([
            "-c",
            "import sys; print(f'{sys.version_info[0]}.{sys.version_info[1]}')",
        ])
        .output()
        .map_err(|error| ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "resolution.python_executable = `{python_executable}` could not be executed: {error}"
            ),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "resolution.python_executable = `{python_executable}` exited with status {} while probing its version{}",
                output.status,
                format_stderr_suffix(stderr.trim())
            ),
        });
    }

    let resolved_version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if resolved_version != target_python {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: format!(
                "resolution.python_executable = `{python_executable}` resolved to Python {resolved_version}, which is incompatible with project.target_python = `{target_python}`"
            ),
        });
    }

    Ok(())
}

fn validate_formatter_config(config_path: &Path, format: &FormatConfig) -> Result<(), ConfigError> {
    if format.line_length == 0 {
        return Err(ConfigError::InvalidValue {
            path: config_path.to_path_buf(),
            message: String::from("format.line_length must be greater than 0"),
        });
    }

    if let Some(command) = &format.command {
        if command.is_empty() {
            return Err(ConfigError::InvalidValue {
                path: config_path.to_path_buf(),
                message: String::from("format.command must contain at least one argument"),
            });
        }
        if command.iter().any(|part| part.trim().is_empty()) {
            return Err(ConfigError::InvalidValue {
                path: config_path.to_path_buf(),
                message: String::from("format.command entries must not be empty"),
            });
        }
    }

    Ok(())
}

fn resolve_python_executable(config_dir: &Path, python_executable: &str) -> PathBuf {
    let executable = Path::new(python_executable);
    if executable.is_absolute() || !python_executable.contains(std::path::MAIN_SEPARATOR) {
        return executable.to_path_buf();
    }

    config_dir.join(executable)
}

fn format_stderr_suffix(stderr: &str) -> String {
    if stderr.is_empty() { String::new() } else { format!(": {stderr}") }
}

#[cfg(test)]
mod tests {
    use super::{ConfigSource, load};
    use std::{
        env, fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn prefers_typepython_toml_over_pyproject() {
        let project_dir = temp_project_dir("prefers_typepython_toml_over_pyproject");
        fs::write(project_dir.join("typepython.toml"), "[project]\ntarget_python = \"3.11\"\n")
            .expect("typepython.toml should be written");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[tool.typepython.project]\ntarget_python = \"3.12\"\n",
        )
        .expect("pyproject.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected config discovery to succeed");
        assert_eq!(handle.source, ConfigSource::TypePythonToml);
        assert_eq!(handle.config.project.target_python, "3.11");
    }

    #[test]
    fn rejects_unsupported_target_python() {
        let project_dir = temp_project_dir("rejects_unsupported_target_python");
        fs::write(project_dir.join("typepython.toml"), "[project]\ntarget_python = \"3.9\"\n")
            .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected invalid target_python to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("project.target_python = `3.9`"));
    }

    #[test]
    fn rejects_unsupported_resolution_base_url() {
        let project_dir = temp_project_dir("rejects_unsupported_resolution_base_url");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[resolution]\nbase_url = \"src\"\n",
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected unsupported base_url to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("resolution.base_url = `src`"));
        assert!(message.contains("not implemented yet"));
    }

    #[test]
    fn rejects_unsupported_resolution_paths() {
        let project_dir = temp_project_dir("rejects_unsupported_resolution_paths");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\nsrc = [\"src\"]\n\n",
                "[resolution.paths]\n",
                "\"@app/*\" = [\"src/app/*\"]\n"
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected unsupported resolution.paths to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("resolution.paths"));
        assert!(message.contains("not implemented yet"));
    }

    #[test]
    fn rejects_unsupported_emit_preserve_comments_override() {
        let project_dir = temp_project_dir("rejects_unsupported_emit_preserve_comments_override");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[emit]\npreserve_comments = false\n",
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected unsupported preserve_comments override");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("emit.preserve_comments = false"));
        assert!(message.contains("not implemented yet"));
    }

    #[test]
    fn rejects_equivalent_out_dir_and_cache_dir_after_normalization() {
        let project_dir =
            temp_project_dir("rejects_equivalent_out_dir_and_cache_dir_after_normalization");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "out_dir = \".typepython/build\"\n",
                "cache_dir = \"./.typepython/build/../build\"\n"
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected identical resolved output paths to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("project.out_dir = `.typepython/build`"));
        assert!(message.contains("project.cache_dir = `./.typepython/build/../build`"));
        assert!(message.contains("resolve to the same path"));
    }

    #[test]
    fn rejects_nested_out_dir_and_cache_dir() {
        let project_dir = temp_project_dir("rejects_nested_out_dir_and_cache_dir");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "out_dir = \".typepython/build\"\n",
                "cache_dir = \".typepython/build/cache\"\n"
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected nested output paths to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("project.out_dir = `.typepython/build`"));
        assert!(message.contains("project.cache_dir = `.typepython/build/cache`"));
        assert!(message.contains("must not overlap"));
        assert!(message.contains("are nested"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_out_dir_and_cache_dir_with_existing_symlink_alias() {
        let project_dir =
            temp_project_dir("rejects_out_dir_and_cache_dir_with_existing_symlink_alias");
        fs::create_dir_all(project_dir.join("real-build"))
            .expect("real output directory should be created");
        symlink(project_dir.join("real-build"), project_dir.join("build-link"))
            .expect("symlink alias should be created");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!("[project]\n", "out_dir = \"real-build\"\n", "cache_dir = \"build-link\"\n"),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected symlink alias paths to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("resolve through existing path aliases"));
    }

    #[test]
    fn rejects_case_alias_out_dir_and_cache_dir_when_filesystem_collapses_them() {
        let project_dir = temp_project_dir(
            "rejects_case_alias_out_dir_and_cache_dir_when_filesystem_collapses_them",
        );
        fs::create_dir_all(project_dir.join("Build"))
            .expect("existing output directory should be created");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!("[project]\n", "out_dir = \"Build\"\n", "cache_dir = \"build\"\n"),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);
        let filesystem_collapses_case = project_dir.join("build").exists();

        remove_temp_project_dir(&project_dir);

        if filesystem_collapses_case {
            let error = load_result.expect_err("expected case-collapsed alias paths to fail");
            let message = error.to_string();
            assert!(message.contains("TPY1002"));
            assert!(message.contains("resolve through existing path aliases"));
        } else {
            load_result.expect("case-sensitive filesystems should keep paths distinct");
        }
    }

    #[test]
    fn rejects_python_executable_version_mismatch() {
        let project_dir = temp_project_dir("rejects_python_executable_version_mismatch");
        let executable = write_fake_python(&project_dir, "3.11");
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                concat!(
                    "[project]\n",
                    "target_python = \"3.10\"\n\n",
                    "[resolution]\n",
                    "python_executable = \"{}\"\n"
                ),
                executable.display()
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected python_executable mismatch to fail");
        let message = error.to_string();
        assert!(message.contains("TPY1002"));
        assert!(message.contains("resolved to Python 3.11"));
        assert!(message.contains("project.target_python = `3.10`"));
    }

    #[test]
    fn accepts_matching_python_executable_version() {
        let project_dir = temp_project_dir("accepts_matching_python_executable_version");
        let executable = write_fake_python(&project_dir, "3.11");
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                concat!(
                    "[project]\n",
                    "target_python = \"3.11\"\n\n",
                    "[resolution]\n",
                    "python_executable = \"{}\"\n"
                ),
                executable.display()
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected matching python_executable to succeed");
        assert_eq!(handle.config.project.target_python, "3.11");
        assert_eq!(
            handle.config.resolution.python_executable.as_deref(),
            Some(handle.resolve_relative_path("fake-python.sh").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn loads_embedded_pyproject_typepython_config() {
        let project_dir = temp_project_dir("loads_embedded_pyproject_typepython_config");
        fs::write(
            project_dir.join("pyproject.toml"),
            concat!(
                "[tool.typepython.project]\n",
                "target_python = \"3.12\"\n\n",
                "[tool.typepython.watch]\n",
                "debounce_ms = 125\n"
            ),
        )
        .expect("pyproject.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected embedded pyproject config to load");
        assert_eq!(handle.source, ConfigSource::PyProject);
        assert_eq!(handle.config.project.target_python, "3.12");
        assert_eq!(handle.config.watch.debounce_ms, 125);
    }

    #[test]
    fn library_profile_expands_required_defaults() {
        let project_dir = temp_project_dir("library_profile_expands_required_defaults");
        fs::write(project_dir.join("typepython.toml"), "[typing]\nprofile = \"library\"\n")
            .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected library profile to load");
        assert!(handle.config.typing.strict);
        assert!(handle.config.typing.strict_nulls);
        assert!(handle.config.typing.no_implicit_dynamic);
        assert!(handle.config.typing.warn_unsafe);
        assert!(handle.config.typing.require_known_public_types);
        assert_eq!(handle.config.typing.imports, super::ImportFallback::Unknown);
        assert_eq!(handle.config.typing.report_deprecated, super::DiagnosticLevel::Warning);
        assert!(!handle.config.typing.require_explicit_overrides);
    }

    #[test]
    fn explicit_typing_keys_override_profile_defaults() {
        let project_dir = temp_project_dir("explicit_typing_keys_override_profile_defaults");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[typing]\n",
                "profile = \"migration\"\n",
                "strict = true\n",
                "imports = \"unknown\"\n",
                "report_deprecated = \"error\"\n",
                "require_known_public_types = true\n"
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected explicit typing overrides to load");
        assert!(handle.config.typing.strict);
        assert_eq!(handle.config.typing.imports, super::ImportFallback::Unknown);
        assert_eq!(handle.config.typing.report_deprecated, super::DiagnosticLevel::Error);
        assert!(handle.config.typing.require_known_public_types);
    }

    #[test]
    fn accepts_null_python_executable_in_typepython_toml() {
        let project_dir = temp_project_dir("accepts_null_python_executable_in_typepython_toml");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!(
                "[project]\n",
                "target_python = \"3.10\"\n\n",
                "[resolution]\n",
                "python_executable = null\n"
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected null python_executable to load");
        assert_eq!(handle.config.resolution.python_executable, None);
    }

    #[test]
    fn accepts_null_python_executable_in_embedded_pyproject() {
        let project_dir = temp_project_dir("accepts_null_python_executable_in_embedded_pyproject");
        fs::write(
            project_dir.join("pyproject.toml"),
            concat!(
                "[tool.typepython.project]\n",
                "target_python = \"3.11\"\n\n",
                "[tool.typepython.resolution]\n",
                "python_executable = null\n"
            ),
        )
        .expect("pyproject.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected embedded null python_executable to load");
        assert_eq!(handle.source, ConfigSource::PyProject);
        assert_eq!(handle.config.resolution.python_executable, None);
    }

    #[test]
    fn loads_conditional_return_opt_in_from_typepython_toml() {
        let project_dir = temp_project_dir("loads_conditional_return_opt_in_from_typepython_toml");
        fs::write(project_dir.join("typepython.toml"), "[typing]\nconditional_returns = true\n")
            .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected conditional return opt-in to load");
        assert!(handle.config.typing.conditional_returns);
    }

    #[test]
    fn loads_infer_passthrough_opt_in_from_typepython_toml() {
        let project_dir = temp_project_dir("loads_infer_passthrough_opt_in_from_typepython_toml");
        fs::write(project_dir.join("typepython.toml"), "[typing]\ninfer_passthrough = true\n")
            .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected infer_passthrough opt-in to load");
        assert!(handle.config.typing.infer_passthrough);
    }

    #[test]
    fn loads_all_supported_typepython_toml_configuration_fields() {
        let project_dir = temp_project_dir("loads_all_supported_typepython_toml_configuration_fields");
        let executable = write_fake_python(&project_dir, "3.11");
        fs::write(
            project_dir.join("typepython.toml"),
            format!(
                concat!(
                    "[project]\n",
                    "src = [\"pkg\", \"vendor\"]\n",
                    "include = [\"pkg/**/*.tpy\", \"vendor/**/*.py\"]\n",
                    "exclude = [\"pkg/generated/**\"]\n",
                    "root_dir = \"pkg\"\n",
                    "out_dir = \".cache/build-out\"\n",
                    "cache_dir = \".cache/state\"\n",
                    "target_python = \"3.11\"\n\n",
                    "[resolution]\n",
                    "type_roots = [\"stubs\", \"more-stubs\"]\n",
                    "python_executable = \"{}\"\n\n",
                    "[format]\n",
                    "command = [\"python3\", \"{{workspace_root}}/tools/format.py\", \"{{file}}\"]\n",
                    "line_length = 88\n\n",
                    "[emit]\n",
                    "emit_pyi = false\n",
                    "emit_pyc = true\n",
                    "write_py_typed = false\n",
                    "no_emit_on_error = false\n",
                    "runtime_validators = true\n\n",
                    "[typing]\n",
                    "profile = \"migration\"\n",
                    "strict = true\n",
                    "strict_nulls = false\n",
                    "imports = \"dynamic\"\n",
                    "no_implicit_dynamic = false\n",
                    "warn_unsafe = false\n",
                    "enable_sealed_exhaustiveness = false\n",
                    "report_deprecated = \"error\"\n",
                    "require_explicit_overrides = true\n",
                    "require_known_public_types = true\n",
                    "infer_passthrough = true\n",
                    "conditional_returns = true\n\n",
                    "[watch]\n",
                    "debounce_ms = 125\n"
                ),
                executable.display()
            ),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected full typepython.toml config to load");
        assert_eq!(handle.config.project.src, vec![String::from("pkg"), String::from("vendor")]);
        assert_eq!(
            handle.config.project.include,
            vec![String::from("pkg/**/*.tpy"), String::from("vendor/**/*.py")]
        );
        assert_eq!(handle.config.project.exclude, vec![String::from("pkg/generated/**")]);
        assert_eq!(handle.config.project.root_dir, "pkg");
        assert_eq!(handle.config.project.out_dir, ".cache/build-out");
        assert_eq!(handle.config.project.cache_dir, ".cache/state");
        assert_eq!(handle.config.project.target_python, "3.11");
        assert_eq!(
            handle.config.resolution.type_roots,
            vec![String::from("stubs"), String::from("more-stubs")]
        );
        assert_eq!(
            handle.config.resolution.python_executable.as_deref(),
            Some(handle.resolve_relative_path("fake-python.sh").to_string_lossy().as_ref())
        );
        assert_eq!(
            handle.config.format.command,
            Some(vec![
                String::from("python3"),
                String::from("{workspace_root}/tools/format.py"),
                String::from("{file}"),
            ])
        );
        assert_eq!(handle.config.format.line_length, 88);
        assert!(!handle.config.emit.emit_pyi);
        assert!(handle.config.emit.emit_pyc);
        assert!(!handle.config.emit.write_py_typed);
        assert!(!handle.config.emit.no_emit_on_error);
        assert!(handle.config.emit.runtime_validators);
        assert_eq!(handle.config.typing.profile, Some(super::TypingProfile::Migration));
        assert!(handle.config.typing.strict);
        assert!(!handle.config.typing.strict_nulls);
        assert_eq!(handle.config.typing.imports, super::ImportFallback::Dynamic);
        assert!(!handle.config.typing.no_implicit_dynamic);
        assert!(!handle.config.typing.warn_unsafe);
        assert!(!handle.config.typing.enable_sealed_exhaustiveness);
        assert_eq!(handle.config.typing.report_deprecated, super::DiagnosticLevel::Error);
        assert!(handle.config.typing.require_explicit_overrides);
        assert!(handle.config.typing.require_known_public_types);
        assert!(handle.config.typing.infer_passthrough);
        assert!(handle.config.typing.conditional_returns);
        assert_eq!(handle.config.watch.debounce_ms, 125);
    }

    #[test]
    fn loads_all_supported_embedded_pyproject_configuration_fields() {
        let project_dir =
            temp_project_dir("loads_all_supported_embedded_pyproject_configuration_fields");
        let executable = write_fake_python(&project_dir, "3.12");
        fs::write(
            project_dir.join("pyproject.toml"),
            format!(
                concat!(
                    "[tool.typepython.project]\n",
                    "src = [\"src\"]\n",
                    "include = [\"src/**/*.tpy\"]\n",
                    "exclude = [\"dist/**\"]\n",
                    "root_dir = \"src\"\n",
                    "out_dir = \".typepython/out\"\n",
                    "cache_dir = \".typepython/cache-data\"\n",
                    "target_python = \"3.12\"\n\n",
                    "[tool.typepython.resolution]\n",
                    "type_roots = [\"stubs\"]\n",
                    "python_executable = \"{}\"\n\n",
                    "[tool.typepython.format]\n",
                    "command = [\"ruff\", \"format\", \"{{file}}\"]\n",
                    "line_length = 120\n\n",
                    "[tool.typepython.emit]\n",
                    "emit_pyi = true\n",
                    "emit_pyc = false\n",
                    "write_py_typed = true\n",
                    "no_emit_on_error = true\n",
                    "runtime_validators = false\n\n",
                    "[tool.typepython.typing]\n",
                    "profile = \"library\"\n",
                    "strict = false\n",
                    "strict_nulls = true\n",
                    "imports = \"unknown\"\n",
                    "no_implicit_dynamic = true\n",
                    "warn_unsafe = true\n",
                    "enable_sealed_exhaustiveness = true\n",
                    "report_deprecated = \"warning\"\n",
                    "require_explicit_overrides = false\n",
                    "require_known_public_types = false\n",
                    "infer_passthrough = false\n",
                    "conditional_returns = false\n\n",
                    "[tool.typepython.watch]\n",
                    "debounce_ms = 40\n"
                ),
                executable.display()
            ),
        )
        .expect("pyproject.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected embedded pyproject config to load");
        assert_eq!(handle.source, ConfigSource::PyProject);
        assert_eq!(handle.config.project.src, vec![String::from("src")]);
        assert_eq!(handle.config.project.include, vec![String::from("src/**/*.tpy")]);
        assert_eq!(handle.config.project.exclude, vec![String::from("dist/**")]);
        assert_eq!(handle.config.project.root_dir, "src");
        assert_eq!(handle.config.project.out_dir, ".typepython/out");
        assert_eq!(handle.config.project.cache_dir, ".typepython/cache-data");
        assert_eq!(handle.config.project.target_python, "3.12");
        assert_eq!(handle.config.resolution.type_roots, vec![String::from("stubs")]);
        assert_eq!(
            handle.config.resolution.python_executable.as_deref(),
            Some(handle.resolve_relative_path("fake-python.sh").to_string_lossy().as_ref())
        );
        assert_eq!(
            handle.config.format.command,
            Some(vec![String::from("ruff"), String::from("format"), String::from("{file}")])
        );
        assert_eq!(handle.config.format.line_length, 120);
        assert!(handle.config.emit.emit_pyi);
        assert!(!handle.config.emit.emit_pyc);
        assert!(handle.config.emit.write_py_typed);
        assert!(handle.config.emit.no_emit_on_error);
        assert!(!handle.config.emit.runtime_validators);
        assert_eq!(handle.config.typing.profile, Some(super::TypingProfile::Library));
        assert!(!handle.config.typing.strict);
        assert!(handle.config.typing.strict_nulls);
        assert_eq!(handle.config.typing.imports, super::ImportFallback::Unknown);
        assert!(handle.config.typing.no_implicit_dynamic);
        assert!(handle.config.typing.warn_unsafe);
        assert!(handle.config.typing.enable_sealed_exhaustiveness);
        assert_eq!(handle.config.typing.report_deprecated, super::DiagnosticLevel::Warning);
        assert!(!handle.config.typing.require_explicit_overrides);
        assert!(!handle.config.typing.require_known_public_types);
        assert!(!handle.config.typing.infer_passthrough);
        assert!(!handle.config.typing.conditional_returns);
        assert_eq!(handle.config.watch.debounce_ms, 40);
    }

    #[test]
    fn rejects_unknown_typepython_toml_keys_during_load() {
        let project_dir = temp_project_dir("rejects_unknown_typepython_toml_keys_during_load");
        fs::write(
            project_dir.join("typepython.toml"),
            concat!("[project]\n", "target_python = \"3.10\"\n", "mystery = true\n"),
        )
        .expect("typepython.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected unknown keys to fail loading");
        let message = error.to_string();
        assert!(message.contains("TPY1001"));
        assert!(message.contains("unknown field `mystery`"));
    }

    #[test]
    fn rejects_unknown_embedded_pyproject_keys_during_load() {
        let project_dir = temp_project_dir("rejects_unknown_embedded_pyproject_keys_during_load");
        fs::write(
            project_dir.join("pyproject.toml"),
            concat!(
                "[tool.typepython.project]\n",
                "target_python = \"3.10\"\n",
                "mystery = true\n"
            ),
        )
        .expect("pyproject.toml should be written");

        let load_result = load(&project_dir);

        remove_temp_project_dir(&project_dir);

        let error = load_result.expect_err("expected unknown embedded keys to fail loading");
        let message = error.to_string();
        assert!(message.contains("TPY1001"));
        assert!(message.contains("unknown field `mystery`"));
    }

    #[test]
    fn discovers_parent_typepython_toml_from_nested_directory() {
        let project_dir =
            temp_project_dir("discovers_parent_typepython_toml_from_nested_directory");
        let nested_dir = project_dir.join("packages/app/src");
        fs::create_dir_all(&nested_dir).expect("nested directory should be created");
        fs::write(project_dir.join("typepython.toml"), "[project]\ntarget_python = \"3.11\"\n")
            .expect("typepython.toml should be written");

        let load_result = load(&nested_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected parent typepython.toml to be discovered");
        assert_eq!(handle.source, ConfigSource::TypePythonToml);
        assert_eq!(handle.config.project.target_python, "3.11");
    }

    #[test]
    fn discovers_parent_embedded_pyproject_from_nested_directory() {
        let project_dir =
            temp_project_dir("discovers_parent_embedded_pyproject_from_nested_directory");
        let nested_dir = project_dir.join("packages/app/src");
        fs::create_dir_all(&nested_dir).expect("nested directory should be created");
        fs::write(
            project_dir.join("pyproject.toml"),
            "[tool.typepython.project]\ntarget_python = \"3.12\"\n",
        )
        .expect("pyproject.toml should be written");

        let load_result = load(&nested_dir);

        remove_temp_project_dir(&project_dir);

        let handle = load_result.expect("expected parent embedded pyproject to be discovered");
        assert_eq!(handle.source, ConfigSource::PyProject);
        assert_eq!(handle.config.project.target_python, "3.12");
    }

    fn temp_project_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let directory = env::temp_dir().join(format!("typepython-config-{test_name}-{unique}"));
        fs::create_dir_all(&directory).expect("temp project directory should be created");
        directory
    }

    fn remove_temp_project_dir(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("temp project directory should be removed");
        }
    }

    fn write_fake_python(project_dir: &Path, reported_version: &str) -> PathBuf {
        let executable = project_dir.join("fake-python.sh");
        fs::write(&executable, format!("#!/bin/sh\nprintf '%s\\n' '{reported_version}'\n"))
            .expect("fake python executable should be written");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&executable)
                .expect("fake python metadata should be readable")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions)
                .expect("fake python executable should be chmodded");
        }
        executable
    }
}
