use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command as ProcessCommand,
    process::ExitCode,
};

use anyhow::{Context, Result};
use ruff_python_ast::{
    Expr, Operator, Stmt,
    visitor::{self, Visitor},
};
use ruff_python_parser::parse_module;
use serde::Serialize;
use typepython_diagnostics::{Diagnostic, DiagnosticReport};
use typepython_project::bundled_stdlib_root;
use typepython_target::PythonTarget;

use crate::{
    CLI_JSON_SCHEMA_VERSION, CommandSummary,
    cli::{OutputFormat, TypeHealthArgs},
    exit_code, load_project, print_summary,
};

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct TypeHealthReport {
    pub(crate) score: u8,
    pub(crate) packages: Vec<TypePackageHealth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lock_inputs: Option<TypeLockInputs>,
    pub(crate) lock_path: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct TypeLockInputs {
    pub(crate) target_python: String,
    pub(crate) analysis_python: String,
    pub(crate) typing_extensions_version: Option<String>,
    pub(crate) typeshed_commit: Option<String>,
    pub(crate) checker_versions: Vec<CheckerVersion>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct CheckerVersion {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct TypePackageHealth {
    pub(crate) name: String,
    pub(crate) root: String,
    pub(crate) has_py_typed: bool,
    pub(crate) is_stub_only: bool,
    pub(crate) is_partial_stub: bool,
    pub(crate) public_any_returns: usize,
    pub(crate) public_any_attributes: usize,
    pub(crate) overload_any_fallbacks: usize,
    pub(crate) public_untyped_attributes: usize,
    pub(crate) unsupported_typing_extensions_imports: usize,
    pub(crate) precision_debt: usize,
    pub(crate) runtime_version: Option<String>,
    pub(crate) stub_version: Option<String>,
    pub(crate) stub_version_matches_runtime: Option<bool>,
}

pub(crate) fn run_type_health(args: TypeHealthArgs) -> Result<ExitCode> {
    let config = load_project(args.run.project.as_ref())?;
    let mut report = build_type_health_report_for_target(
        &config.config_dir,
        &config.config.resolution.type_roots,
        config.config.project.target_python,
    )?;
    let lock_inputs = collect_type_lock_inputs(&config.config_dir, &config.config)?;
    if args.write_lock {
        let lock_path = config.config_dir.join(".typepython/type-lock.toml");
        write_type_lock(&lock_path, &report, &lock_inputs)?;
        report.lock_path = Some(lock_path.display().to_string());
    }
    report.lock_inputs = Some(lock_inputs);

    let mut diagnostics = DiagnosticReport::default();
    if let Some(threshold) = args.fail_under
        && report.score < threshold
    {
        diagnostics.push(Diagnostic::error(
            "TPY7002",
            format!("type health score {} is below required threshold {threshold}", report.score),
        ));
    }

    if args.run.format == OutputFormat::Json {
        let payload = serde_json::json!({
            "schema_version": CLI_JSON_SCHEMA_VERSION,
            "summary": report,
            "diagnostics": diagnostics,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .context("unable to serialize type-health report")?
        );
    } else {
        print_type_health_text(&report);
        let summary = CommandSummary {
            command: String::from("type-health"),
            config_path: config.config_path.display().to_string(),
            config_source: config.source,
            discovered_sources: report.packages.len(),
            lowered_modules: 0,
            planned_artifacts: usize::from(args.write_lock),
            tracked_modules: report
                .packages
                .iter()
                .filter(|package| package.has_py_typed || package.is_stub_only)
                .count(),
            notes: vec![String::from(
                "inspected configured resolution.type_roots for PEP 561 typing metadata",
            )],
        };
        print_summary(args.run.format, &summary, &diagnostics)?;
    }
    Ok(exit_code(&diagnostics))
}

pub(crate) fn build_type_health_report_for_target(
    config_dir: &Path,
    type_roots: &[String],
    target_python: PythonTarget,
) -> Result<TypeHealthReport> {
    let mut packages = Vec::new();
    for root in type_roots {
        let root_path = config_dir.join(root);
        let metadata = fs::metadata(&root_path).with_context(|| {
            format!("configured type root `{}` is unavailable", root_path.display())
        })?;
        if !metadata.is_dir() {
            anyhow::bail!("configured type root `{}` is not a directory", root_path.display());
        }
        for entry in fs::read_dir(&root_path)
            .with_context(|| format!("unable to read {}", root_path.display()))?
        {
            let path = entry?.path();
            let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            if path.is_dir() && !file_name.ends_with(".dist-info") {
                packages.push(package_health(&path, target_python)?);
            }
        }
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    let score = type_health_score(&packages);
    Ok(TypeHealthReport { score, packages, lock_inputs: None, lock_path: None })
}

fn type_health_score(packages: &[TypePackageHealth]) -> u8 {
    if packages.is_empty() {
        return 100;
    }
    let total = packages.iter().map(package_health_score).sum::<usize>();
    (total / packages.len()) as u8
}

fn package_health_score(package: &TypePackageHealth) -> usize {
    if !package.has_py_typed && !package.is_stub_only {
        return 0;
    }
    100usize.saturating_sub(package.precision_debt.saturating_mul(10).min(50))
}

pub(crate) fn collect_type_lock_inputs(
    config_dir: &Path,
    config: &typepython_config::Config,
) -> Result<TypeLockInputs> {
    Ok(TypeLockInputs {
        target_python: config.project.target_python_text.clone(),
        analysis_python: config
            .resolution
            .analysis_python_text
            .clone()
            .unwrap_or_else(|| config.project.target_python_text.clone()),
        typing_extensions_version: typing_extensions_version(
            config_dir,
            &config.resolution.type_roots,
        )?,
        typeshed_commit: bundled_typeshed_commit()?,
        checker_versions: ["mypy", "pyright", "ty"]
            .into_iter()
            .map(|checker| CheckerVersion {
                name: checker.to_owned(),
                version: checker_version(checker),
            })
            .collect(),
    })
}

fn typing_extensions_version(config_dir: &Path, type_roots: &[String]) -> Result<Option<String>> {
    for root in type_roots {
        let root_path = config_dir.join(root);
        let version = match distribution_version(&root_path, "typing-extensions")? {
            Some(version) => Some(version),
            None => distribution_version(&root_path, "typing_extensions")?,
        };
        if version.is_some() {
            return Ok(version);
        }
    }
    Ok(None)
}

fn bundled_typeshed_commit() -> Result<Option<String>> {
    let baseline_path = bundled_stdlib_root()?.join("BASELINE.toml");
    let baseline = fs::read_to_string(&baseline_path)
        .with_context(|| format!("unable to read {}", baseline_path.display()))?;
    Ok(baseline.lines().find_map(|line| {
        let stripped = line.trim();
        stripped
            .strip_prefix("typeshed_commit = ")
            .and_then(|value| value.trim_matches('"').split_once('"').map(|(commit, _)| commit))
            .or_else(|| {
                stripped.strip_prefix("typeshed_commit = ").map(|value| value.trim_matches('"'))
            })
            .map(str::to_owned)
    }))
}

fn checker_version(checker: &str) -> Option<String> {
    let output = ProcessCommand::new(checker).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stdout.is_empty() { (!stderr.is_empty()).then_some(stderr) } else { Some(stdout) }
}

fn package_health(path: &Path, target_python: PythonTarget) -> Result<TypePackageHealth> {
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let py_typed = path.join("py.typed");
    let marker = read_py_typed_marker(&py_typed)?;
    let package_name = file_name.trim_end_matches("-stubs").to_owned();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let runtime_version = distribution_version(parent, &package_name)?;
    let stub_version = distribution_version(parent, &format!("{package_name}-stubs"))?;
    let stub_version_matches_runtime =
        runtime_version.as_ref().zip(stub_version.as_ref()).map(|(runtime, stub)| runtime == stub);
    let public_any = public_any_counts(path, target_python)?;
    let precision_debt = public_any.precision_debt();
    Ok(TypePackageHealth {
        name: package_name,
        root: path.display().to_string(),
        has_py_typed: marker.is_some() && !file_name.ends_with("-stubs"),
        is_stub_only: file_name.ends_with("-stubs"),
        is_partial_stub: marker
            .as_deref()
            .is_some_and(|marker| marker.lines().any(|line| line.trim() == "partial")),
        public_any_returns: public_any.returns,
        public_any_attributes: public_any.attributes,
        overload_any_fallbacks: public_any.overload_fallbacks,
        public_untyped_attributes: public_any.untyped_attributes,
        unsupported_typing_extensions_imports: public_any.unsupported_typing_extensions_imports,
        precision_debt,
        runtime_version,
        stub_version,
        stub_version_matches_runtime,
    })
}

/// Reads a package-owned PEP 561 marker without following symlinks.
///
/// Installed wheel metadata should materialize `py.typed` as a regular file. Treating a directory,
/// device, or symlink as a marker would let filesystem layout accidentally claim typing support.
fn read_py_typed_marker(path: &Path) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("unable to inspect PEP 561 marker {}", path.display()));
        }
    };
    if !metadata.file_type().is_file() {
        return Ok(None);
    }
    fs::read_to_string(path)
        .with_context(|| format!("unable to read PEP 561 marker {}", path.display()))
        .map(Some)
}

#[derive(Debug, Default)]
struct PublicAnyCounts {
    returns: usize,
    attributes: usize,
    overload_fallbacks: usize,
    untyped_attributes: usize,
    unsupported_typing_extensions_imports: usize,
}

impl PublicAnyCounts {
    fn precision_debt(&self) -> usize {
        self.returns
            + self.attributes
            + self.overload_fallbacks
            + self.untyped_attributes
            + self.unsupported_typing_extensions_imports
    }
}

fn public_any_counts(path: &Path, target_python: PythonTarget) -> Result<PublicAnyCounts> {
    let mut counts = PublicAnyCounts::default();
    collect_public_any_counts(path, target_python, &mut counts)?;
    Ok(counts)
}

fn collect_public_any_counts(
    path: &Path,
    target_python: PythonTarget,
    counts: &mut PublicAnyCounts,
) -> Result<()> {
    if path.is_dir() {
        for entry in
            fs::read_dir(path).with_context(|| format!("unable to read {}", path.display()))?
        {
            collect_public_any_counts(&entry?.path(), target_python, counts)?;
        }
        return Ok(());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("pyi") {
        return Ok(());
    }
    let source =
        fs::read_to_string(path).with_context(|| format!("unable to read {}", path.display()))?;
    let parsed = parse_module(&source)
        .with_context(|| format!("unable to parse stub syntax in {}", path.display()))?;
    let mut import_counter = TypingExtensionsImportCounter { target_python, count: 0 };
    import_counter.visit_body(parsed.suite());
    counts.unsupported_typing_extensions_imports += import_counter.count;

    let mut visitor = PublicStubVisitor::new(counts, parsed.suite());
    visitor.visit_body(parsed.suite());
    visitor.finish_module();
    Ok(())
}

struct TypingExtensionsImportCounter {
    target_python: PythonTarget,
    count: usize,
}

impl<'ast> Visitor<'ast> for TypingExtensionsImportCounter {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if let Stmt::ImportFrom(import) = statement
            && import.level == 0
            && import.module.as_ref().is_some_and(|module| module.as_str() == "typing_extensions")
        {
            self.count += import
                .names
                .iter()
                .filter(|alias| {
                    !is_known_typing_extensions_symbol(alias.name.as_str(), self.target_python)
                })
                .count();
        }
        visitor::walk_stmt(self, statement);
    }
}

fn is_known_typing_extensions_symbol(symbol: &str, target_python: PythonTarget) -> bool {
    target_python.stdlib_owner(symbol).is_some()
        || matches!(
            symbol,
            "Any"
                | "Callable"
                | "ClassVar"
                | "Final"
                | "Generic"
                | "Literal"
                | "Never"
                | "NewType"
                | "NoReturn"
                | "Optional"
                | "ParamSpec"
                | "Protocol"
                | "TypeAlias"
                | "TypeGuard"
                | "TypeVar"
                | "TypedDict"
                | "Union"
                | "overload"
        )
}

#[derive(Debug, Clone)]
struct TypingBindings {
    any_names: BTreeSet<String>,
    overload_names: BTreeSet<String>,
    typing_modules: BTreeSet<String>,
    type_factory_names: BTreeSet<String>,
    type_alias_marker_names: BTreeSet<String>,
    type_reference_names: BTreeSet<String>,
}

impl Default for TypingBindings {
    fn default() -> Self {
        Self {
            any_names: BTreeSet::from([String::from("Any")]),
            overload_names: BTreeSet::from([String::from("overload")]),
            typing_modules: BTreeSet::from([
                String::from("typing"),
                String::from("typing_extensions"),
            ]),
            type_factory_names: [
                "NamedTuple",
                "NewType",
                "ParamSpec",
                "TypeAliasType",
                "TypedDict",
                "TypeVar",
                "TypeVarTuple",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            type_alias_marker_names: BTreeSet::from([String::from("TypeAlias")]),
            type_reference_names: [
                "Any",
                "bool",
                "bytearray",
                "bytes",
                "complex",
                "dict",
                "float",
                "frozenset",
                "int",
                "list",
                "memoryview",
                "object",
                "range",
                "set",
                "slice",
                "str",
                "tuple",
                "type",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

impl TypingBindings {
    fn record_import(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let source = alias.name.as_str();
                    if !matches!(source, "typing" | "typing_extensions") {
                        continue;
                    }
                    let local =
                        alias.asname.as_ref().map_or(source, ruff_python_ast::Identifier::as_str);
                    self.typing_modules.insert(local.to_owned());
                }
            }
            Stmt::ImportFrom(import)
                if import.level == 0
                    && import.module.as_ref().is_some_and(|module| {
                        matches!(module.as_str(), "typing" | "typing_extensions")
                    }) =>
            {
                for alias in &import.names {
                    let source = alias.name.as_str();
                    if source == "*" {
                        continue;
                    }
                    let local = alias
                        .asname
                        .as_ref()
                        .map_or(source, ruff_python_ast::Identifier::as_str)
                        .to_owned();
                    match source {
                        "Any" => {
                            self.any_names.insert(local.clone());
                        }
                        "overload" => {
                            self.overload_names.insert(local.clone());
                        }
                        "NamedTuple" | "NewType" | "ParamSpec" | "TypeAliasType" | "TypedDict"
                        | "TypeVar" | "TypeVarTuple" => {
                            self.type_factory_names.insert(local.clone());
                        }
                        "TypeAlias" => {
                            self.type_alias_marker_names.insert(local.clone());
                        }
                        _ => {}
                    }
                    if typing_symbol_is_type(source) {
                        self.type_reference_names.insert(local);
                    }
                }
            }
            _ => {}
        }
    }
}

fn precollect_scope_bindings(suite: &[Stmt], mut bindings: TypingBindings) -> TypingBindings {
    ScopeBindingCollector { bindings: &mut bindings }.visit_body(suite);
    loop {
        let previous = bindings.type_reference_names.len();
        ScopeTypeDeclarationCollector { bindings: &mut bindings }.visit_body(suite);
        if bindings.type_reference_names.len() == previous {
            break;
        }
    }
    bindings
}

struct ScopeBindingCollector<'bindings> {
    bindings: &'bindings mut TypingBindings,
}

impl<'ast> Visitor<'ast> for ScopeBindingCollector<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        match statement {
            Stmt::Import(_) | Stmt::ImportFrom(_) => self.bindings.record_import(statement),
            Stmt::FunctionDef(_) => {}
            Stmt::ClassDef(class_def) => {
                self.bindings.type_reference_names.insert(class_def.name.as_str().to_owned());
            }
            Stmt::TypeAlias(type_alias) => {
                if let Expr::Name(name) = type_alias.name.as_ref() {
                    self.bindings.type_reference_names.insert(name.id.as_str().to_owned());
                }
            }
            _ => visitor::walk_stmt(self, statement),
        }
    }
}

struct ScopeTypeDeclarationCollector<'bindings> {
    bindings: &'bindings mut TypingBindings,
}

impl<'ast> Visitor<'ast> for ScopeTypeDeclarationCollector<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        match statement {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            Stmt::Assign(assignment)
                if assignment_is_type_declaration(assignment.value.as_ref(), self.bindings) =>
            {
                self.bindings.type_reference_names.extend(
                    assignment.targets.iter().flat_map(simple_assignment_names).map(str::to_owned),
                );
            }
            Stmt::AnnAssign(assignment)
                if annotation_is_type_alias_marker(
                    assignment.annotation.as_ref(),
                    self.bindings,
                ) =>
            {
                if let Expr::Name(name) = assignment.target.as_ref() {
                    self.bindings.type_reference_names.insert(name.id.as_str().to_owned());
                }
            }
            _ => visitor::walk_stmt(self, statement),
        }
    }
}

#[derive(Debug, Default)]
struct OverloadGroup {
    saw_overload: bool,
    last_overload_returns_any: bool,
    implementation_returns_any: Option<bool>,
}

impl OverloadGroup {
    fn record(&mut self, is_overload: bool, returns_any: bool) -> usize {
        if is_overload {
            let completed = if self.saw_overload && self.implementation_returns_any.is_some() {
                usize::from(self.fallback_returns_any())
            } else {
                0
            };
            if self.implementation_returns_any.is_some() {
                *self = Self::default();
            }
            self.saw_overload = true;
            self.last_overload_returns_any = returns_any;
            completed
        } else {
            if self.saw_overload && self.implementation_returns_any.is_none() {
                self.implementation_returns_any = Some(returns_any);
            }
            0
        }
    }

    fn fallback_returns_any(&self) -> bool {
        self.saw_overload
            && self.implementation_returns_any.unwrap_or(self.last_overload_returns_any)
    }
}

#[derive(Debug)]
struct PublicScopeState {
    bindings: TypingBindings,
    overload_groups: BTreeMap<String, OverloadGroup>,
}

impl PublicScopeState {
    fn new(bindings: TypingBindings) -> Self {
        Self { bindings, overload_groups: BTreeMap::new() }
    }

    fn fallback_count(&self) -> usize {
        self.overload_groups.values().filter(|group| group.fallback_returns_any()).count()
    }
}

struct PublicStubVisitor<'counts> {
    counts: &'counts mut PublicAnyCounts,
    scopes: Vec<PublicScopeState>,
}

impl<'counts> PublicStubVisitor<'counts> {
    fn new(counts: &'counts mut PublicAnyCounts, suite: &[Stmt]) -> Self {
        let bindings = precollect_scope_bindings(suite, TypingBindings::default());
        Self { counts, scopes: vec![PublicScopeState::new(bindings)] }
    }

    fn current_scope(&self) -> &PublicScopeState {
        self.scopes.last().expect("public stub visitor must have an active scope")
    }

    fn current_scope_mut(&mut self) -> &mut PublicScopeState {
        self.scopes.last_mut().expect("public stub visitor must have an active scope")
    }

    fn finish_class(&mut self) {
        let scope = self.scopes.pop().expect("public class scope must exist");
        self.counts.overload_fallbacks += scope.fallback_count();
    }

    fn finish_module(&mut self) {
        let scope = self.scopes.pop().expect("public module scope must exist");
        self.counts.overload_fallbacks += scope.fallback_count();
        debug_assert!(self.scopes.is_empty());
    }

    fn record_function(&mut self, function: &ruff_python_ast::StmtFunctionDef) {
        let (returns_any, is_overload) = {
            let bindings = &self.current_scope().bindings;
            (
                function
                    .returns
                    .as_deref()
                    .is_some_and(|annotation| annotation_is_any(annotation, bindings)),
                function
                    .decorator_list
                    .iter()
                    .any(|decorator| expression_is_overload(&decorator.expression, bindings)),
            )
        };
        if returns_any {
            self.counts.returns += 1;
        }
        let name = function.name.as_str().to_owned();
        let completed = if is_overload {
            self.current_scope_mut()
                .overload_groups
                .entry(name)
                .or_default()
                .record(true, returns_any)
        } else {
            self.current_scope_mut()
                .overload_groups
                .get_mut(&name)
                .map_or(0, |group| group.record(false, returns_any))
        };
        self.counts.overload_fallbacks += completed;
    }

    fn record_assignment(&mut self, assignment: &ruff_python_ast::StmtAssign) {
        let type_declaration = assignment_is_type_declaration(
            assignment.value.as_ref(),
            &self.current_scope().bindings,
        );
        let names = assignment
            .targets
            .iter()
            .flat_map(simple_assignment_names)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if type_declaration {
            self.current_scope_mut().bindings.type_reference_names.extend(names);
        } else {
            self.counts.untyped_attributes +=
                names.iter().filter(|name| public_stub_name(name)).count();
        }
    }

    fn record_annotated_assignment(&mut self, assignment: &ruff_python_ast::StmtAnnAssign) {
        let Expr::Name(name) = assignment.target.as_ref() else {
            return;
        };
        let is_type_alias = annotation_is_type_alias_marker(
            assignment.annotation.as_ref(),
            &self.current_scope().bindings,
        );
        if is_type_alias {
            self.current_scope_mut()
                .bindings
                .type_reference_names
                .insert(name.id.as_str().to_owned());
        } else if public_stub_name(name.id.as_str())
            && annotation_is_any(assignment.annotation.as_ref(), &self.current_scope().bindings)
        {
            self.counts.attributes += 1;
        }
    }
}

impl<'ast> Visitor<'ast> for PublicStubVisitor<'_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        match statement {
            Stmt::Import(_) | Stmt::ImportFrom(_) => {
                self.current_scope_mut().bindings.record_import(statement);
            }
            Stmt::FunctionDef(function) => {
                if public_stub_name(function.name.as_str()) {
                    self.record_function(function);
                }
            }
            Stmt::ClassDef(class_def) => {
                self.current_scope_mut()
                    .bindings
                    .type_reference_names
                    .insert(class_def.name.as_str().to_owned());
                if public_stub_name(class_def.name.as_str()) {
                    let bindings = precollect_scope_bindings(
                        &class_def.body,
                        self.current_scope().bindings.clone(),
                    );
                    self.scopes.push(PublicScopeState::new(bindings));
                    self.visit_body(&class_def.body);
                    self.finish_class();
                }
            }
            Stmt::Assign(assignment) => self.record_assignment(assignment),
            Stmt::AnnAssign(assignment) => self.record_annotated_assignment(assignment),
            Stmt::TypeAlias(type_alias) => {
                if let Expr::Name(name) = type_alias.name.as_ref() {
                    self.current_scope_mut()
                        .bindings
                        .type_reference_names
                        .insert(name.id.as_str().to_owned());
                }
            }
            _ => {
                visitor::walk_stmt(self, statement);
            }
        }
    }
}

fn public_stub_name(name: &str) -> bool {
    !name.starts_with('_')
}

fn simple_assignment_names(expression: &Expr) -> Vec<&str> {
    match expression {
        Expr::Name(name) => vec![name.id.as_str()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(simple_assignment_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(simple_assignment_names).collect(),
        _ => Vec::new(),
    }
}

fn annotation_is_any(annotation: &Expr, bindings: &TypingBindings) -> bool {
    match annotation {
        Expr::Name(name) => bindings.any_names.contains(name.id.as_str()),
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "Any"
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(module) if bindings.typing_modules.contains(module.id.as_str())
                )
        }
        Expr::BinOp(binary) if binary.op == Operator::BitOr => {
            annotation_is_any(binary.left.as_ref(), bindings)
                || annotation_is_any(binary.right.as_ref(), bindings)
        }
        _ => false,
    }
}

fn expression_is_overload(expression: &Expr, bindings: &TypingBindings) -> bool {
    match expression {
        Expr::Name(name) => bindings.overload_names.contains(name.id.as_str()),
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(module) if bindings.typing_modules.contains(module.id.as_str())
                )
        }
        _ => false,
    }
}

fn annotation_is_type_alias_marker(annotation: &Expr, bindings: &TypingBindings) -> bool {
    match annotation {
        Expr::Name(name) => bindings.type_alias_marker_names.contains(name.id.as_str()),
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "TypeAlias"
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(module) if bindings.typing_modules.contains(module.id.as_str())
                )
        }
        _ => false,
    }
}

fn assignment_is_type_declaration(value: &Expr, bindings: &TypingBindings) -> bool {
    match value {
        Expr::Call(call) => expression_is_type_factory(call.func.as_ref(), bindings),
        Expr::Subscript(subscript) => type_expression_head(subscript.value.as_ref(), bindings),
        Expr::BinOp(binary) if binary.op == Operator::BitOr => {
            type_expression_operand(binary.left.as_ref(), bindings)
                && type_expression_operand(binary.right.as_ref(), bindings)
        }
        Expr::Name(name) => bindings.type_reference_names.contains(name.id.as_str()),
        Expr::Attribute(_) => type_expression_head(value, bindings),
        _ => false,
    }
}

fn expression_is_type_factory(expression: &Expr, bindings: &TypingBindings) -> bool {
    match expression {
        Expr::Name(name) => bindings.type_factory_names.contains(name.id.as_str()),
        Expr::Attribute(attribute) => {
            matches!(
                attribute.attr.as_str(),
                "NamedTuple"
                    | "NewType"
                    | "ParamSpec"
                    | "TypeAliasType"
                    | "TypedDict"
                    | "TypeVar"
                    | "TypeVarTuple"
            ) && matches!(
                attribute.value.as_ref(),
                Expr::Name(module) if bindings.typing_modules.contains(module.id.as_str())
            )
        }
        _ => false,
    }
}

fn type_expression_head(expression: &Expr, bindings: &TypingBindings) -> bool {
    match expression {
        Expr::Name(name) => bindings.type_reference_names.contains(name.id.as_str()),
        Expr::Attribute(attribute) => {
            typing_symbol_is_type(attribute.attr.as_str())
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(module) if bindings.typing_modules.contains(module.id.as_str())
                )
        }
        _ => false,
    }
}

fn typing_symbol_is_type(symbol: &str) -> bool {
    matches!(
        symbol,
        "Annotated"
            | "Any"
            | "Callable"
            | "ClassVar"
            | "Concatenate"
            | "DefaultDict"
            | "Deque"
            | "Dict"
            | "Final"
            | "FrozenSet"
            | "Generic"
            | "Iterable"
            | "Iterator"
            | "List"
            | "Literal"
            | "Mapping"
            | "MutableMapping"
            | "MutableSequence"
            | "NamedTuple"
            | "Never"
            | "NewType"
            | "NoReturn"
            | "NotRequired"
            | "Optional"
            | "ParamSpec"
            | "Protocol"
            | "Required"
            | "Self"
            | "Sequence"
            | "Set"
            | "TypeAlias"
            | "TypeAliasType"
            | "TypeGuard"
            | "TypeIs"
            | "Tuple"
            | "TypedDict"
            | "Type"
            | "TypeVar"
            | "TypeVarTuple"
            | "Union"
            | "Unpack"
    )
}

fn type_expression_operand(expression: &Expr, bindings: &TypingBindings) -> bool {
    match expression {
        Expr::NoneLiteral(_) | Expr::StringLiteral(_) => true,
        Expr::Name(_) | Expr::Attribute(_) => type_expression_head(expression, bindings),
        Expr::Subscript(subscript) => type_expression_head(subscript.value.as_ref(), bindings),
        Expr::BinOp(binary) if binary.op == Operator::BitOr => {
            type_expression_operand(binary.left.as_ref(), bindings)
                && type_expression_operand(binary.right.as_ref(), bindings)
        }
        _ => false,
    }
}

fn distribution_version(site_root: &Path, distribution_name: &str) -> Result<Option<String>> {
    if !site_root.is_dir() {
        return Ok(None);
    }
    let normalized_distribution = normalize_distribution_name(distribution_name);
    let mut matches = Vec::new();
    for entry in fs::read_dir(site_root)
        .with_context(|| format!("unable to read {}", site_root.display()))?
    {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(stem) = file_name.strip_suffix(".dist-info") else {
            continue;
        };
        let Some((filename_distribution, _)) = stem.rsplit_once('-') else {
            continue;
        };
        if normalize_distribution_name(filename_distribution) != normalized_distribution {
            continue;
        }
        let metadata_path = path.join("METADATA");
        let metadata = fs::read_to_string(&metadata_path)
            .with_context(|| format!("unable to read {}", metadata_path.display()))?;
        let (metadata_name, version) =
            distribution_metadata_identity(&metadata).with_context(|| {
                format!("unable to identify distribution metadata in {}", metadata_path.display())
            })?;
        if normalize_distribution_name(&metadata_name) != normalized_distribution {
            anyhow::bail!(
                "distribution metadata `{}` declares Name `{metadata_name}`, expected `{distribution_name}`",
                metadata_path.display()
            );
        }
        matches.push((metadata_path, version));
    }
    match matches.as_slice() {
        [] => Ok(None),
        [(_, version)] => Ok(Some(version.clone())),
        matches => anyhow::bail!(
            "multiple metadata directories match distribution `{distribution_name}`: {}",
            matches
                .iter()
                .map(|(path, _)| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn normalize_distribution_name(name: &str) -> String {
    name.to_ascii_lowercase()
        .split(['-', '_', '.'])
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn distribution_metadata_identity(metadata: &str) -> Result<(String, String)> {
    let mut names = Vec::new();
    let mut versions = Vec::new();
    for line in metadata.lines() {
        if line.is_empty() {
            break;
        }
        let Some((header, value)) = line.split_once(':') else {
            continue;
        };
        if header.eq_ignore_ascii_case("Name") {
            names.push(value.trim().to_owned());
        } else if header.eq_ignore_ascii_case("Version") {
            versions.push(value.trim().to_owned());
        }
    }
    match (names.as_slice(), versions.as_slice()) {
        ([name], [version]) if !name.is_empty() && !version.is_empty() => {
            Ok((name.clone(), version.clone()))
        }
        _ => anyhow::bail!("metadata must contain exactly one non-empty Name and Version header"),
    }
}

fn write_type_lock(path: &Path, report: &TypeHealthReport, inputs: &TypeLockInputs) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create {}", parent.display()))?;
    }
    let mut rendered = String::from("# Generated by typepython type-health --write-lock\n");
    rendered.push_str(&format!("score = {}\n\n", report.score));
    rendered.push_str("[inputs]\n");
    rendered.push_str(&format!("target_python = \"{}\"\n", toml_string(&inputs.target_python)));
    rendered.push_str(&format!("analysis_python = \"{}\"\n", toml_string(&inputs.analysis_python)));
    if let Some(version) = &inputs.typing_extensions_version {
        rendered.push_str(&format!("typing_extensions_version = \"{}\"\n", toml_string(version)));
    }
    if let Some(commit) = &inputs.typeshed_commit {
        rendered.push_str(&format!("typeshed_commit = \"{}\"\n", toml_string(commit)));
    }
    rendered.push('\n');
    for checker in &inputs.checker_versions {
        rendered.push_str("[[checker]]\n");
        rendered.push_str(&format!("name = \"{}\"\n", toml_string(&checker.name)));
        if let Some(version) = &checker.version {
            rendered.push_str(&format!("version = \"{}\"\n", toml_string(version)));
        }
        rendered.push('\n');
    }
    for package in &report.packages {
        rendered.push_str("[[package]]\n");
        rendered.push_str(&format!("name = \"{}\"\n", toml_string(&package.name)));
        rendered.push_str(&format!("root = \"{}\"\n", toml_string(&package.root)));
        rendered.push_str(&format!("has_py_typed = {}\n", package.has_py_typed));
        rendered.push_str(&format!("is_stub_only = {}\n", package.is_stub_only));
        rendered.push_str(&format!("is_partial_stub = {}\n", package.is_partial_stub));
        rendered.push_str(&format!("public_any_returns = {}\n", package.public_any_returns));
        rendered.push_str(&format!("public_any_attributes = {}\n", package.public_any_attributes));
        rendered
            .push_str(&format!("overload_any_fallbacks = {}\n", package.overload_any_fallbacks));
        rendered.push_str(&format!(
            "public_untyped_attributes = {}\n",
            package.public_untyped_attributes
        ));
        rendered.push_str(&format!(
            "unsupported_typing_extensions_imports = {}\n",
            package.unsupported_typing_extensions_imports
        ));
        rendered.push_str(&format!("precision_debt = {}\n", package.precision_debt));
        if let Some(version) = &package.runtime_version {
            rendered.push_str(&format!("runtime_version = \"{}\"\n", toml_string(version)));
        }
        if let Some(version) = &package.stub_version {
            rendered.push_str(&format!("stub_version = \"{}\"\n", toml_string(version)));
        }
        if let Some(matches) = package.stub_version_matches_runtime {
            rendered.push_str(&format!("stub_version_matches_runtime = {matches}\n"));
        }
        rendered.push('\n');
    }
    fs::write(path, rendered).with_context(|| format!("unable to write {}", path.display()))
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn print_type_health_text(report: &TypeHealthReport) {
    println!("type-health:");
    println!("  score: {}", report.score);
    for package in &report.packages {
        println!(
            "  package: {} py.typed={} stub_only={} partial={} public_any_returns={} public_any_attributes={} overload_any_fallbacks={} public_untyped_attributes={} unsupported_typing_extensions_imports={} precision_debt={} runtime_version={} stub_version={} version_match={}",
            package.name,
            package.has_py_typed,
            package.is_stub_only,
            package.is_partial_stub,
            package.public_any_returns,
            package.public_any_attributes,
            package.overload_any_fallbacks,
            package.public_untyped_attributes,
            package.unsupported_typing_extensions_imports,
            package.precision_debt,
            package.runtime_version.as_deref().unwrap_or("?"),
            package.stub_version.as_deref().unwrap_or("?"),
            package
                .stub_version_matches_runtime
                .map(|matches| matches.to_string())
                .unwrap_or_else(|| String::from("?"))
        );
    }
    if let Some(lock_path) = &report.lock_path {
        println!("  lock: {lock_path}");
    }
}
