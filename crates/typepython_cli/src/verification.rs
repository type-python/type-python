use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode},
};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde::Deserialize;
use tar::Archive as TarArchive;
use typepython_config::ConfigHandle;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_emit::{EmitArtifact, TypePythonStubContext, generate_typepython_stub_source};
use typepython_incremental::decode_snapshot;
use typepython_lowering::LoweredModule;
use typepython_syntax::{SourceFile, SourceKind};
use zip::ZipArchive;

use crate::cli::VerifyArgs;
use crate::discovery::normalize_glob_path;
use crate::pipeline::{
    ensure_output_dirs, materialize_build_outputs, py_typed_package_roots, run_pipeline,
};
use crate::{
    CommandSummary, RUNTIME_PUBLIC_NAMES_SCRIPT, STATIC_ALL_NAMES_SCRIPT, bytecode_path_for,
    exit_code, load_project, print_summary, resolve_python_executable,
};

#[derive(Debug, Clone)]
pub(crate) struct SuppliedVerifyArtifact {
    pub(crate) kind: SuppliedArtifactKind,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SuppliedArtifactKind {
    Wheel,
    Sdist,
}

impl SuppliedArtifactKind {
    fn label(self) -> &'static str {
        match self {
            Self::Wheel => "wheel",
            Self::Sdist => "sdist",
        }
    }
}

pub(crate) fn supplied_verify_artifacts(args: &VerifyArgs) -> Vec<SuppliedVerifyArtifact> {
    let mut artifacts = args
        .wheels
        .iter()
        .cloned()
        .map(|path| SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Wheel, path })
        .collect::<Vec<_>>();
    artifacts.extend(
        args.sdists
            .iter()
            .cloned()
            .map(|path| SuppliedVerifyArtifact { kind: SuppliedArtifactKind::Sdist, path }),
    );
    artifacts
}

pub(crate) fn run_verify(args: VerifyArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    ensure_output_dirs(&config)?;
    let snapshot = run_pipeline(&config)?;
    let mut notes = vec![String::from(
        "verifies current runtime artifacts, emitted stubs, and `py.typed` in the build tree",
    )];
    let diagnostics = if snapshot.diagnostics.has_errors() {
        snapshot.diagnostics.clone()
    } else {
        let mut diagnostics = DiagnosticReport::default();
        match materialize_build_outputs(&config, &snapshot) {
            Ok(materialize_notes) => notes.extend(materialize_notes),
            Err(error) if error.to_string().contains("TPY5001") => {
                diagnostics.push(Diagnostic::error("TPY5001", error.to_string()));
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "unable to write runtime artifacts under {}",
                        config.resolve_relative_path(&config.config.project.out_dir).display()
                    )
                });
            }
        }
        if !diagnostics.has_errors() {
            diagnostics = verify_build_artifacts(&config, &snapshot.emit_plan);
        }
        if !diagnostics.has_errors() {
            diagnostics.diagnostics.extend(
                verify_runtime_public_name_parity(&config, &snapshot.emit_plan).diagnostics,
            );
        }
        if !diagnostics.has_errors() {
            diagnostics.diagnostics.extend(
                verify_packaged_artifacts(
                    &config,
                    &snapshot.emit_plan,
                    &supplied_verify_artifacts(&args),
                )
                .diagnostics,
            );
        }
        diagnostics
    };

    let supplied_artifact_count = args.wheels.len() + args.sdists.len();
    if supplied_artifact_count > 0 {
        notes.push(format!(
            "verified {} supplied wheel/sdist artifact(s) against the authoritative build tree",
            supplied_artifact_count
        ));
    }

    let summary = CommandSummary {
        command: String::from("verify"),
        config_path: config.config_path.display().to_string(),
        config_source: config.source,
        discovered_sources: snapshot.discovered_sources,
        lowered_modules: snapshot.lowered_modules.len(),
        planned_artifacts: snapshot.emit_plan.len(),
        tracked_modules: snapshot.tracked_modules,
        notes,
    };

    print_summary(args.run.format, &summary, &diagnostics)?;
    Ok(exit_code(&diagnostics))
}

pub(crate) fn verify_build_artifacts(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);

    for artifact in artifacts {
        if let Some(runtime_path) = &artifact.runtime_path {
            if !runtime_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing runtime artifact `{}`", runtime_path.display()),
                ));
            } else if let Some(diagnostic) = verify_emitted_text_artifact(runtime_path) {
                diagnostics.push(diagnostic);
            }
            if config.config.emit.emit_pyc {
                let bytecode_path = match bytecode_path_for(runtime_path) {
                    Ok(path) => path,
                    Err(error) => {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "unable to determine bytecode path for `{}`: {error}",
                                runtime_path.display()
                            ),
                        ));
                        continue;
                    }
                };
                if !bytecode_path.exists() {
                    diagnostics.push(Diagnostic::error(
                        "TPY5003",
                        format!("missing bytecode artifact `{}`", bytecode_path.display()),
                    ));
                }
            }
        }

        if let Some(stub_path) = &artifact.stub_path {
            if !stub_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing stub artifact `{}`", stub_path.display()),
                ));
            } else if let Some(diagnostic) = verify_emitted_text_artifact(stub_path) {
                diagnostics.push(diagnostic);
            }
        }

        if let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path)
        {
            if runtime_path.exists() && stub_path.exists() {
                if let Some(diagnostic) =
                    verify_emitted_declaration_surface(runtime_path, stub_path)
                {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }

    if config.config.emit.write_py_typed {
        for package_root in py_typed_package_roots(&out_root, artifacts) {
            let marker_path = package_root.join("py.typed");
            if !marker_path.exists() {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!("missing package marker `{}`", marker_path.display()),
                ));
            }
        }
    }

    let snapshot_path =
        config.resolve_relative_path(&config.config.project.cache_dir).join("snapshot.json");
    if !snapshot_path.exists() {
        diagnostics.push(Diagnostic::error(
            "TPY5003",
            format!("missing incremental snapshot `{}`", snapshot_path.display()),
        ));
    } else if let Err(error) = verify_incremental_snapshot(&snapshot_path) {
        diagnostics.push(Diagnostic::error(
            "TPY6001",
            format!(
                "incremental snapshot `{}` is incompatible or corrupt: {}",
                snapshot_path.display(),
                error
            ),
        ));
    }

    diagnostics
}

pub(crate) fn verify_packaged_artifacts(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
    supplied_artifacts: &[SuppliedVerifyArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    if supplied_artifacts.is_empty() {
        return diagnostics;
    }

    let expected_files = match expected_published_files(config, artifacts) {
        Ok(files) => files,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!("unable to collect authoritative build artifacts for publication verification: {error}"),
            ));
            return diagnostics;
        }
    };

    for artifact in supplied_artifacts {
        match read_supplied_artifact_entries(artifact) {
            Ok(entries) => {
                for (relative_path, expected_bytes) in &expected_files {
                    match entries.get(relative_path) {
                        None => diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "{} artifact `{}` is missing published file `{relative_path}`",
                                artifact.kind.label(),
                                artifact.path.display(),
                            ),
                        )),
                        Some(actual_bytes) if actual_bytes != expected_bytes => {
                            diagnostics.push(Diagnostic::error(
                                "TPY5003",
                                format!(
                                    "{} artifact `{}` contains `{relative_path}` that diverges from the authoritative build output",
                                    artifact.kind.label(),
                                    artifact.path.display(),
                                ),
                            ));
                        }
                        Some(_) => {}
                    }
                }
                for relative_path in
                    entries.keys().filter(|path| is_authoritative_publication_file(path))
                {
                    if !expected_files.contains_key(relative_path) {
                        diagnostics.push(Diagnostic::error(
                            "TPY5003",
                            format!(
                                "{} artifact `{}` contains unexpected published file `{relative_path}`",
                                artifact.kind.label(),
                                artifact.path.display(),
                            ),
                        ));
                    }
                }
            }
            Err(error) => diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "unable to inspect {} artifact `{}`: {error}",
                    artifact.kind.label(),
                    artifact.path.display(),
                ),
            )),
        }
    }

    diagnostics
}

pub(crate) fn verify_runtime_public_name_parity(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);

    for artifact in artifacts {
        let (Some(runtime_path), Some(stub_path)) = (&artifact.runtime_path, &artifact.stub_path)
        else {
            continue;
        };
        if !(runtime_path.exists() && stub_path.exists()) {
            continue;
        }
        let Some(module_name) = logical_module_name_from_runtime_path(&out_root, runtime_path)
        else {
            continue;
        };
        let runtime_names = match runtime_public_names(config, &out_root, &module_name) {
            Ok(names) => names,
            Err(error) => {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "unable to inspect runtime public names for `{}` from `{}`: {error}",
                        module_name,
                        runtime_path.display(),
                    ),
                ));
                continue;
            }
        };
        let authoritative_names = match authoritative_public_names(config, stub_path) {
            Ok(names) => names,
            Err(error) => {
                diagnostics.push(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "unable to determine authoritative public names for `{}` from `{}`: {error}",
                        module_name,
                        stub_path.display(),
                    ),
                ));
                continue;
            }
        };

        let missing_from_runtime =
            authoritative_names.difference(&runtime_names).cloned().collect::<Vec<_>>();
        let missing_from_type_surface =
            runtime_names.difference(&authoritative_names).cloned().collect::<Vec<_>>();

        if !missing_from_runtime.is_empty() {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "runtime module `{}` is missing public names declared by the authoritative type surface: {}",
                    module_name,
                    missing_from_runtime.join(", "),
                ),
            ));
        }
        if !missing_from_type_surface.is_empty() {
            diagnostics.push(Diagnostic::error(
                "TPY5003",
                format!(
                    "authoritative type surface for `{}` is missing runtime public names: {}",
                    module_name,
                    missing_from_type_surface.join(", "),
                ),
            ));
        }
    }

    diagnostics
}

fn logical_module_name_from_runtime_path(out_root: &Path, runtime_path: &Path) -> Option<String> {
    let relative = runtime_path.strip_prefix(out_root).ok()?;
    let stem = runtime_path.file_stem()?.to_str()?;
    let mut components = relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;

    if stem == "__init__" {
        return (!components.is_empty()).then(|| components.join("."));
    }

    components.push(stem.to_owned());
    Some(components.join("."))
}

#[derive(Debug, Deserialize)]
struct RuntimePublicNameResult {
    importable: bool,
    names: Option<Vec<String>>,
    error: Option<String>,
}

fn runtime_public_names(
    config: &ConfigHandle,
    out_root: &Path,
    module_name: &str,
) -> std::result::Result<BTreeSet<String>, String> {
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .args(["-c", RUNTIME_PUBLIC_NAMES_SCRIPT])
        .arg(out_root)
        .arg(module_name)
        .output()
        .map_err(|error| {
            format!(
                "unable to run runtime public-name probe with `{}`: {error}",
                interpreter.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_suffix =
            if stderr.trim().is_empty() { String::new() } else { format!(": {}", stderr.trim()) };
        return Err(format!(
            "runtime public-name probe exited with status {}{}",
            output.status, stderr_suffix
        ));
    }
    let result = serde_json::from_slice::<RuntimePublicNameResult>(&output.stdout)
        .map_err(|error| format!("unable to parse runtime public-name output: {error}"))?;
    if !result.importable {
        return Err(result
            .error
            .unwrap_or_else(|| format!("module `{module_name}` could not be imported")));
    }
    Ok(result.names.unwrap_or_default().into_iter().collect::<BTreeSet<_>>())
}

fn authoritative_public_names(
    config: &ConfigHandle,
    path: &Path,
) -> std::result::Result<BTreeSet<String>, String> {
    if let Some(names) = static_all_names(config, path)? {
        return Ok(names);
    }

    let syntax = emitted_syntax(path)
        .ok_or_else(|| format!("`{}` could not be parsed as a Python module", path.display()))?;
    Ok(module_level_surface_names(&syntax)
        .into_iter()
        .filter(|name| !name.starts_with('_'))
        .collect())
}

fn static_all_names(
    config: &ConfigHandle,
    path: &Path,
) -> std::result::Result<Option<BTreeSet<String>>, String> {
    let interpreter = resolve_python_executable(config);
    let output = ProcessCommand::new(&interpreter)
        .args(["-c", STATIC_ALL_NAMES_SCRIPT])
        .arg(path)
        .output()
        .map_err(|error| {
            format!("unable to run Python parser `{}`: {error}", interpreter.display())
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_suffix =
            if stderr.trim().is_empty() { String::new() } else { format!(": {}", stderr.trim()) };
        return Err(format!("Python parser exited with status {}{}", output.status, stderr_suffix));
    }
    let names = serde_json::from_slice::<Option<Vec<String>>>(&output.stdout)
        .map_err(|error| format!("unable to parse `__all__` output: {error}"))?;
    Ok(names.map(|names| names.into_iter().collect()))
}

fn expected_published_files(
    config: &ConfigHandle,
    artifacts: &[EmitArtifact],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let out_root = config.resolve_relative_path(&config.config.project.out_dir);
    let mut expected_files = BTreeMap::new();
    let package_roots = py_typed_package_roots(&out_root, artifacts);

    for artifact in artifacts {
        if let Some(runtime_path) = &artifact.runtime_path {
            expected_files
                .insert(relative_publish_path(&out_root, runtime_path)?, fs::read(runtime_path)?);
        }
        if let Some(stub_path) = &artifact.stub_path {
            expected_files
                .insert(relative_publish_path(&out_root, stub_path)?, fs::read(stub_path)?);
        }
    }

    if config.config.emit.write_py_typed {
        for package_root in package_roots {
            let marker_path = package_root.join("py.typed");
            expected_files
                .insert(relative_publish_path(&out_root, &marker_path)?, fs::read(marker_path)?);
        }
    }

    Ok(expected_files)
}

fn relative_publish_path(out_root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(out_root)
        .with_context(|| format!("{} is not inside {}", path.display(), out_root.display()))?;
    Ok(normalize_glob_path(relative))
}

fn is_authoritative_publication_file(path: &str) -> bool {
    path.ends_with(".py") || path.ends_with(".pyi")
}

fn read_supplied_artifact_entries(
    artifact: &SuppliedVerifyArtifact,
) -> std::result::Result<BTreeMap<String, Vec<u8>>, String> {
    let path_text = artifact.path.to_string_lossy().to_ascii_lowercase();
    match artifact.kind {
        SuppliedArtifactKind::Wheel => {
            if !(path_text.ends_with(".whl") || path_text.ends_with(".zip")) {
                return Err(String::from("expected a .whl or .zip file"));
            }
            Ok(read_zip_entries(&artifact.path)?.into_iter().collect())
        }
        SuppliedArtifactKind::Sdist => {
            let entries = if path_text.ends_with(".tar.gz") || path_text.ends_with(".tgz") {
                read_tar_gz_entries(&artifact.path)?
            } else if path_text.ends_with(".zip") {
                read_zip_entries(&artifact.path)?
            } else {
                return Err(String::from("expected a .tar.gz, .tgz, or .zip file"));
            };
            Ok(strip_common_archive_root(entries))
        }
    }
}

fn read_zip_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("unable to read zip archive: {error}"))?;
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("unable to read zip entry {index}: {error}"))?;
        if file.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read zip entry `{}`: {error}", file.name()))?;
        entries.push((normalize_archive_path(file.name()), bytes));
    }

    Ok(entries)
}

fn read_tar_gz_entries(path: &Path) -> std::result::Result<Vec<(String, Vec<u8>)>, String> {
    let file = fs::File::open(path).map_err(|error| format!("unable to open archive: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = TarArchive::new(decoder);
    let mut entries = Vec::new();

    for entry in
        archive.entries().map_err(|error| format!("unable to read tar archive: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("unable to read tar entry: {error}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry
            .path()
            .map_err(|error| format!("unable to read tar entry path: {error}"))?
            .display()
            .to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| format!("unable to read tar entry `{entry_path}`: {error}"))?;
        entries.push((normalize_archive_path(&entry_path), bytes));
    }

    Ok(entries)
}

fn strip_common_archive_root(entries: Vec<(String, Vec<u8>)>) -> BTreeMap<String, Vec<u8>> {
    let Some(common_root) = common_archive_root(&entries) else {
        return entries.into_iter().collect();
    };

    entries
        .into_iter()
        .map(|(path, bytes)| {
            let normalized =
                path.strip_prefix(&format!("{common_root}/")).map(str::to_owned).unwrap_or(path);
            (normalized, bytes)
        })
        .collect()
}

fn common_archive_root(entries: &[(String, Vec<u8>)]) -> Option<String> {
    let mut root: Option<&str> = None;

    for (path, _) in entries {
        let mut components = path.split('/').filter(|component| !component.is_empty());
        let first = components.next()?;
        components.next()?;
        match root {
            Some(existing) if existing != first => return None,
            Some(_) => {}
            None => root = Some(first),
        }
    }

    root.map(str::to_owned)
}

fn normalize_archive_path(path: &str) -> String {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn verify_incremental_snapshot(path: &Path) -> Result<(), String> {
    let rendered = fs::read_to_string(path)
        .map_err(|error| format!("unable to read incremental snapshot: {error}"))?;
    decode_snapshot(&rendered).map(|_| ()).map_err(|error| error.to_string())
}

fn verify_emitted_text_artifact(path: &Path) -> Option<Diagnostic> {
    let source = match SourceFile::from_path(path) {
        Ok(source) => source,
        Err(error) => {
            return Some(Diagnostic::error(
                "TPY5003",
                format!("unable to read emitted artifact `{}`: {error}", path.display()),
            ));
        }
    };
    let syntax = typepython_syntax::parse(source);
    if syntax.diagnostics.has_errors() {
        Some(Diagnostic::error(
            "TPY5003",
            format!("emitted artifact `{}` is not valid Python syntax", path.display()),
        ))
    } else if path.extension().is_some_and(|extension| extension == "pyi") {
        verify_stub_syntax_rules(path, &syntax)
    } else {
        None
    }
}

fn verify_emitted_declaration_surface(runtime_path: &Path, stub_path: &Path) -> Option<Diagnostic> {
    let runtime_syntax = emitted_syntax(runtime_path)?;
    let stub_syntax = emitted_syntax(stub_path)?;

    let runtime_surface = declaration_surface(&runtime_syntax)
        .into_iter()
        .filter(is_public_surface_entry)
        .collect::<BTreeSet<_>>();
    let stub_surface = declaration_surface(&stub_syntax)
        .into_iter()
        .filter(is_public_surface_entry)
        .collect::<BTreeSet<_>>();

    if runtime_surface == stub_surface {
        None
    } else {
        Some(Diagnostic::error(
            "TPY5003",
            format!(
                "emitted runtime/stub declaration surface differs between `{}` and `{}`",
                runtime_path.display(),
                stub_path.display()
            ),
        ))
    }
}

fn verify_stub_syntax_rules(
    path: &Path,
    syntax: &typepython_syntax::SyntaxTree,
) -> Option<Diagnostic> {
    if syntax.statements.iter().any(stub_statement_is_runtime) {
        return Some(Diagnostic::error(
            "TPY5003",
            format!("emitted stub artifact `{}` contains runtime statements", path.display()),
        ));
    }

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.owner_name.is_some() =>
            {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains executable assignments",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::Value(statement)
                if statement.owner_name.is_none() && statement.annotation.is_none() =>
            {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains value declarations without annotations",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::Interface(_)
            | typepython_syntax::SyntaxStatement::DataClass(_)
            | typepython_syntax::SyntaxStatement::SealedClass(_)
            | typepython_syntax::SyntaxStatement::Unsafe(_) => {
                return Some(Diagnostic::error(
                    "TPY5003",
                    format!(
                        "emitted stub artifact `{}` contains TypePython-only syntax",
                        path.display()
                    ),
                ));
            }
            typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                if statement.members.iter().any(|member| {
                    member.kind == typepython_syntax::ClassMemberKind::Field
                        && member.annotation.is_none()
                }) {
                    return Some(Diagnostic::error(
                        "TPY5003",
                        format!(
                            "emitted stub artifact `{}` contains class fields without annotations",
                            path.display()
                        ),
                    ));
                }
            }
            _ => {}
        }
    }

    None
}

fn stub_statement_is_runtime(statement: &typepython_syntax::SyntaxStatement) -> bool {
    matches!(
        statement,
        typepython_syntax::SyntaxStatement::Call(_)
            | typepython_syntax::SyntaxStatement::MemberAccess(_)
            | typepython_syntax::SyntaxStatement::MethodCall(_)
            | typepython_syntax::SyntaxStatement::Return(_)
            | typepython_syntax::SyntaxStatement::Yield(_)
            | typepython_syntax::SyntaxStatement::If(_)
            | typepython_syntax::SyntaxStatement::Assert(_)
            | typepython_syntax::SyntaxStatement::Invalidate(_)
            | typepython_syntax::SyntaxStatement::Match(_)
            | typepython_syntax::SyntaxStatement::For(_)
            | typepython_syntax::SyntaxStatement::With(_)
            | typepython_syntax::SyntaxStatement::ExceptHandler(_)
            | typepython_syntax::SyntaxStatement::Unsafe(_)
    )
}

fn emitted_syntax(path: &Path) -> Option<typepython_syntax::SyntaxTree> {
    let source = SourceFile::from_path(path).ok()?;
    let syntax = typepython_syntax::parse(source);
    if syntax.diagnostics.has_errors() { None } else { Some(syntax) }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SurfaceEntry {
    owner: Option<String>,
    kind: &'static str,
    name: String,
    detail: String,
}

fn declaration_surface(
    syntax: &typepython_syntax::SyntaxTree,
) -> std::collections::BTreeSet<SurfaceEntry> {
    let mut surface = std::collections::BTreeSet::new();

    for statement in &syntax.statements {
        match statement {
            typepython_syntax::SyntaxStatement::TypeAlias(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "typealias",
                    name: statement.name.clone(),
                    detail: statement.value.clone(),
                });
            }
            typepython_syntax::SyntaxStatement::Interface(statement)
            | typepython_syntax::SyntaxStatement::DataClass(statement)
            | typepython_syntax::SyntaxStatement::SealedClass(statement)
            | typepython_syntax::SyntaxStatement::ClassDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "class",
                    name: statement.name.clone(),
                    detail: format!(
                        "bases=[{}];final={}",
                        statement.bases.join(","),
                        statement.is_final_decorator
                    ),
                });
                for member in &statement.members {
                    surface.insert(SurfaceEntry {
                        owner: Some(statement.name.clone()),
                        kind: match member.kind {
                            typepython_syntax::ClassMemberKind::Field => "field",
                            typepython_syntax::ClassMemberKind::Method => "method",
                            typepython_syntax::ClassMemberKind::Overload => "overload",
                        },
                        name: member.name.clone(),
                        detail: match member.kind {
                            typepython_syntax::ClassMemberKind::Field => format!(
                                "annotation={};final={};classvar={}",
                                member.annotation.clone().unwrap_or_default(),
                                member.is_final_decorator,
                                member.is_class_var
                            ),
                            typepython_syntax::ClassMemberKind::Method
                            | typepython_syntax::ClassMemberKind::Overload => format!(
                                "kind={:?};final={};sig={}",
                                member
                                    .method_kind
                                    .unwrap_or(typepython_syntax::MethodKind::Instance),
                                member.is_final_decorator,
                                format_signature(&member.params, member.returns.as_deref())
                            ),
                        },
                    });
                }
            }
            typepython_syntax::SyntaxStatement::OverloadDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "overload",
                    name: statement.name.clone(),
                    detail: format_signature(&statement.params, statement.returns.as_deref()),
                });
            }
            typepython_syntax::SyntaxStatement::FunctionDef(statement) => {
                surface.insert(SurfaceEntry {
                    owner: None,
                    kind: "function",
                    name: statement.name.clone(),
                    detail: format_signature(&statement.params, statement.returns.as_deref()),
                });
            }
            typepython_syntax::SyntaxStatement::Import(statement) => {
                for binding in &statement.bindings {
                    surface.insert(SurfaceEntry {
                        owner: None,
                        kind: "import",
                        name: binding.local_name.clone(),
                        detail: binding.source_path.clone(),
                    });
                }
            }
            typepython_syntax::SyntaxStatement::Value(statement) => {
                for name in &statement.names {
                    surface.insert(SurfaceEntry {
                        owner: None,
                        kind: "value",
                        name: name.clone(),
                        detail: statement.annotation.clone().unwrap_or_default(),
                    });
                }
            }
            typepython_syntax::SyntaxStatement::Call(_) => {}
            typepython_syntax::SyntaxStatement::MethodCall(_) => {}
            typepython_syntax::SyntaxStatement::MemberAccess(_) => {}
            typepython_syntax::SyntaxStatement::Return(_) => {}
            typepython_syntax::SyntaxStatement::Yield(_) => {}
            typepython_syntax::SyntaxStatement::If(_) => {}
            typepython_syntax::SyntaxStatement::Assert(_) => {}
            typepython_syntax::SyntaxStatement::Invalidate(_) => {}
            typepython_syntax::SyntaxStatement::Match(_) => {}
            typepython_syntax::SyntaxStatement::For(_) => {}
            typepython_syntax::SyntaxStatement::With(_) => {}
            typepython_syntax::SyntaxStatement::ExceptHandler(_) => {}
            typepython_syntax::SyntaxStatement::Unsafe(_) => {}
        }
    }

    surface
}

fn module_level_surface_names(syntax: &typepython_syntax::SyntaxTree) -> BTreeSet<String> {
    declaration_surface(syntax)
        .into_iter()
        .filter(|entry| entry.owner.is_none())
        .map(|entry| entry.name)
        .collect()
}

pub(crate) fn public_surface_completeness_diagnostics(
    config: &ConfigHandle,
    syntax_trees: &[typepython_syntax::SyntaxTree],
    lowered_modules: &[LoweredModule],
    stub_contexts: &BTreeMap<PathBuf, TypePythonStubContext>,
) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    if !config.config.typing.require_known_public_types {
        return diagnostics;
    }

    let lowered_by_source = lowered_modules
        .iter()
        .map(|module| (module.source_path.clone(), module))
        .collect::<BTreeMap<_, _>>();

    for syntax in syntax_trees {
        let surface_syntax = if syntax.source.kind == SourceKind::TypePython {
            let Some(module) = lowered_by_source.get(&syntax.source.path) else {
                continue;
            };
            let context = stub_contexts.get(&syntax.source.path).cloned().unwrap_or_default();
            let Ok(stub_source) = generate_typepython_stub_source(module, &context) else {
                continue;
            };
            let stub_file = SourceFile {
                path: syntax.source.path.with_extension("pyi"),
                kind: SourceKind::Stub,
                logical_module: syntax.source.logical_module.clone(),
                text: stub_source,
            };
            typepython_syntax::parse(stub_file)
        } else {
            syntax.clone()
        };

        for entry in declaration_surface(&surface_syntax)
            .into_iter()
            .filter(is_public_surface_entry)
            .filter(|entry| entry.kind != "import")
        {
            if !surface_detail_is_incomplete(&entry.detail) {
                continue;
            }

            diagnostics.push(Diagnostic::error(
                "TPY4015",
                format!(
                    "module `{}` exports incomplete type surface for `{}`",
                    syntax.source.path.display(),
                    display_surface_entry(&entry)
                ),
            ));
        }
    }

    diagnostics
}

fn is_public_surface_entry(entry: &SurfaceEntry) -> bool {
    if entry.name.starts_with('_') {
        return false;
    }

    match &entry.owner {
        Some(owner) => !owner.starts_with('_'),
        None => true,
    }
}

fn display_surface_entry(entry: &SurfaceEntry) -> String {
    match &entry.owner {
        Some(owner) => format!("{owner}.{}", entry.name),
        None => entry.name.clone(),
    }
}

fn surface_detail_is_incomplete(detail: &str) -> bool {
    let mut token = String::new();

    for ch in detail.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }

        if matches!(token.as_str(), "dynamic" | "unknown") {
            return true;
        }
        token.clear();
    }

    matches!(token.as_str(), "dynamic" | "unknown")
}

fn format_signature(params: &[typepython_syntax::FunctionParam], returns: Option<&str>) -> String {
    format!(
        "({})->{}",
        params
            .iter()
            .map(|param| match &param.annotation {
                Some(annotation) => format!("{}:{}", param.name, annotation),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(","),
        returns.unwrap_or("")
    )
}
