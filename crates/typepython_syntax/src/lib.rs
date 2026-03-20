//! Source classification and parser boundary for TypePython.

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_ast::{Expr, Stmt, TypeParam as AstTypeParam};
use ruff_python_parser::parse_module;
use ruff_text_size::Ranged;
use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};

/// Supported input file kinds from the spec.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceKind {
    /// `.tpy` TypePython source.
    TypePython,
    /// `.py` pass-through Python.
    Python,
    /// `.pyi` stub input.
    Stub,
}

impl SourceKind {
    /// Infers the source kind from a path suffix.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(OsStr::to_str) {
            Some("tpy") => Some(Self::TypePython),
            Some("py") => Some(Self::Python),
            Some("pyi") => Some(Self::Stub),
            _ => None,
        }
    }
}

/// An in-memory source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Filesystem location.
    pub path: PathBuf,
    /// Classified input kind.
    pub kind: SourceKind,
    /// Source text.
    pub text: String,
}

impl SourceFile {
    /// Reads a source file from disk.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let path = path.as_ref().to_path_buf();
        let text = fs::read_to_string(&path)?;
        let kind = SourceKind::from_path(&path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported source suffix for {}", path.display()),
            )
        })?;

        Ok(Self { path, kind, text })
    }
}

/// Parser output placeholder.
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    /// Original source file.
    pub source: SourceFile,
    pub statements: Vec<SyntaxStatement>,
    pub diagnostics: DiagnosticReport,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SyntaxStatement {
    TypeAlias(TypeAliasStatement),
    Interface(NamedBlockStatement),
    DataClass(NamedBlockStatement),
    SealedClass(NamedBlockStatement),
    OverloadDef(FunctionStatement),
    ClassDef(NamedBlockStatement),
    FunctionDef(FunctionStatement),
    Import(ImportStatement),
    Value(ValueStatement),
    Unsafe(UnsafeStatement),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeAliasStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NamedBlockStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub header_suffix: String,
    pub members: Vec<ClassMember>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionParam {
    pub name: String,
    pub annotation: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ImportStatement {
    pub names: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ValueStatement {
    pub names: Vec<String>,
    pub is_final: bool,
    pub is_class_var: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ClassMember {
    pub name: String,
    pub kind: ClassMemberKind,
    pub annotation: Option<String>,
    pub params: Vec<FunctionParam>,
    pub returns: Option<String>,
    pub is_final: bool,
    pub is_class_var: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClassMemberKind {
    Field,
    Method,
    Overload,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnsafeStatement {
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub bound: Option<String>,
}

struct ParsedTypeParams<'source> {
    type_params: Vec<TypeParam>,
    remainder: &'source str,
}

/// Parses a source file into a syntax tree.
#[must_use]
pub fn parse(source: SourceFile) -> SyntaxTree {
    match source.kind {
        SourceKind::TypePython => parse_typepython_source(source),
        SourceKind::Python | SourceKind::Stub => parse_python_source(source),
    }
}

fn parse_python_source(source: SourceFile) -> SyntaxTree {
    let mut statements = Vec::new();
    let mut diagnostics = DiagnosticReport::default();

    match parse_module(&source.text) {
        Ok(parsed) => {
            collect_invalid_annotation_placement_diagnostics(
                &source.path,
                &source.text,
                parsed.suite(),
                false,
                &mut diagnostics,
            );
            statements.extend(extract_ast_backed_statements(
                &source.path,
                &source.text,
                &source.text,
                parsed.suite(),
                &[],
                &mut diagnostics,
            ));
            statements.sort_by_key(statement_line);
        }
        Err(error) => {
            diagnostics.push(
                Diagnostic::error("TPY2001", format!("Python syntax error: {}", error.error))
                    .with_span(parse_error_span(
                        &source.path,
                        &source.text,
                        error.location.start().to_usize(),
                        error.location.end().to_usize(),
                    )),
            );
        }
    }

    SyntaxTree { source, statements, diagnostics }
}

fn parse_typepython_source(source: SourceFile) -> SyntaxTree {
    let mut statements = Vec::new();
    let mut diagnostics = DiagnosticReport::default();

    for (index, line) in source.text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(statement) = parse_extension_statement(&source.path, trimmed, line_number, &mut diagnostics) {
            statements.push(statement);
        }
    }

    if !diagnostics.has_errors() {
        let normalized = normalize_typepython_source(&source.text, &statements);
        match parse_module(&normalized) {
            Ok(parsed) => {
                collect_invalid_annotation_placement_diagnostics(
                    &source.path,
                    &normalized,
                    parsed.suite(),
                    false,
                    &mut diagnostics,
                );
                refresh_custom_statements_from_ast(
                    &source.path,
                    &normalized,
                    parsed.suite(),
                    &mut statements,
                    &mut diagnostics,
                );
            statements.extend(extract_ast_backed_statements(
                &source.path,
                &normalized,
                &normalized,
                parsed.suite(),
                &statements,
                    &mut diagnostics,
                ));
                statements.sort_by_key(statement_line);
            }
            Err(error) => {
                diagnostics.push(
                    Diagnostic::error("TPY2001", format!("TypePython syntax error: {}", error.error))
                        .with_span(parse_error_span(
                            &source.path,
                            &source.text,
                            error.location.start().to_usize(),
                            error.location.end().to_usize(),
                        )),
                );
            }
        }
    }

    SyntaxTree { source, statements, diagnostics }
}

fn collect_invalid_annotation_placement_diagnostics(
    path: &Path,
    source: &str,
    suite: &[Stmt],
    in_function_body: bool,
    diagnostics: &mut DiagnosticReport,
) {
    for statement in suite {
        match statement {
            Stmt::AnnAssign(assign) if in_function_body && is_classvar_annotation(&assign.annotation) => {
                let line = offset_to_line_column(source, assign.range.start().to_usize()).0;
                diagnostics.push(
                    Diagnostic::error(
                        "TPY4001",
                        "ClassVar[...] is not allowed inside function or method bodies",
                    )
                    .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
                );
            }
            Stmt::FunctionDef(function) => {
                collect_invalid_parameter_annotation_diagnostics(path, source, &function.parameters, diagnostics);
                collect_invalid_annotation_placement_diagnostics(path, source, &function.body, true, diagnostics)
            }
            Stmt::ClassDef(class_def) => {
                collect_invalid_annotation_placement_diagnostics(path, source, &class_def.body, false, diagnostics)
            }
            _ => {}
        }
    }
}

fn collect_invalid_parameter_annotation_diagnostics(
    path: &Path,
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    diagnostics: &mut DiagnosticReport,
) {
    for parameter in parameters.iter() {
        let Some(annotation) = parameter.annotation() else {
            continue;
        };
        let line = offset_to_line_column(source, parameter.range().start().to_usize()).0;

        if is_classvar_annotation(annotation) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4001",
                    "ClassVar[...] is not allowed in function or method parameter annotations",
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
        }
        if is_final_annotation(annotation) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4010",
                    "Final[...] in function or method parameter position is deferred beyond v1",
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
        }
    }
}

fn refresh_custom_statements_from_ast(
    path: &Path,
    normalized: &str,
    suite: &[Stmt],
    statements: &mut [SyntaxStatement],
    diagnostics: &mut DiagnosticReport,
) {
    for statement in statements.iter_mut() {
        match statement {
            SyntaxStatement::Interface(existing) => {
                if let Some(ast_statement) = ast_class_def_for_line(normalized, suite, existing.line) {
                    for body_statement in &ast_statement.body {
                        if !is_valid_interface_body_statement(body_statement) {
                            diagnostics.push(
                                Diagnostic::error(
                                    "TPY2001",
                                    format!(
                                        "interface `{}` body must not contain executable statements",
                                        existing.name
                                    ),
                                )
                                .with_span(Span::new(
                                    path.display().to_string(),
                                    offset_to_line_column(
                                        normalized,
                                        body_statement.range().start().to_usize(),
                                    )
                                    .0,
                                    1,
                                    offset_to_line_column(
                                        normalized,
                                        body_statement.range().end().to_usize(),
                                    )
                                    .0,
                                    1,
                                )),
                            );
                            break;
                        }
                    }
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "interface declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                    }
                }
            }
            SyntaxStatement::DataClass(existing) => {
                if let Some(ast_statement) = ast_class_def_for_line(normalized, suite, existing.line) {
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "data class declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                    }
                }
            }
            SyntaxStatement::SealedClass(existing) => {
                if let Some(ast_statement) = ast_class_def_for_line(normalized, suite, existing.line) {
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "sealed class declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.header_suffix = ast_statement
                            .arguments
                            .as_ref()
                            .and_then(|arguments| slice_range(normalized, arguments.range()))
                            .map(str::to_owned)
                            .unwrap_or_default();
                        existing.members = extract_class_members(normalized, &ast_statement.body);
                    }
                }
            }
            SyntaxStatement::OverloadDef(existing) => {
                if let Some(ast_statement) = ast_function_def_for_line(normalized, suite, existing.line) {
                    if !is_stub_like_function_body(&ast_statement.body) {
                        diagnostics.push(
                            Diagnostic::error(
                                "TPY2001",
                                format!(
                                    "overload declaration `{}` body must not contain executable statements",
                                    existing.name
                                ),
                            )
                            .with_span(Span::new(
                                path.display().to_string(),
                                offset_to_line_column(
                                    normalized,
                                    ast_statement.range.start().to_usize(),
                                )
                                .0,
                                1,
                                offset_to_line_column(
                                    normalized,
                                    ast_statement.range.end().to_usize(),
                                )
                                .0,
                                1,
                            )),
                        );
                    }
                    if let Some(type_params) = extract_ast_type_params(
                        path,
                        normalized,
                        ast_statement.type_params.as_deref(),
                        existing.line,
                        "overload declaration",
                        diagnostics,
                    ) {
                        existing.name = ast_statement.name.as_str().to_owned();
                        existing.type_params = type_params;
                        existing.params = extract_function_params(normalized, &ast_statement.parameters);
                        existing.returns = ast_statement
                            .returns
                            .as_ref()
                            .and_then(|returns| slice_range(normalized, returns.range()))
                            .map(str::to_owned);
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_valid_interface_body_statement(statement: &Stmt) -> bool {
    match statement {
        Stmt::AnnAssign(_) | Stmt::Pass(_) => true,
        Stmt::Expr(expr) => {
            matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_))
        }
        Stmt::FunctionDef(function) => is_stub_like_function_body(&function.body),
        _ => false,
    }
}

fn is_stub_like_function_body(body: &[Stmt]) -> bool {
    body.iter().all(|statement| {
        matches!(statement, Stmt::Pass(_))
            || matches!(statement, Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)))
    })
}

fn extract_class_members(normalized: &str, body: &[Stmt]) -> Vec<ClassMember> {
    let mut members = Vec::new();

    for statement in body {
        match statement {
            Stmt::FunctionDef(function) => members.push(ClassMember {
                name: function.name.as_str().to_owned(),
                kind: if function.decorator_list.iter().any(is_overload_decorator) {
                    ClassMemberKind::Overload
                } else {
                    ClassMemberKind::Method
                },
                annotation: None,
                params: extract_function_params(normalized, &function.parameters),
                returns: function
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(normalized, returns.range()))
                    .map(str::to_owned),
                is_final: false,
                is_class_var: false,
                line: offset_to_line_column(normalized, function.range.start().to_usize()).0,
            }),
            Stmt::AnnAssign(assign) => {
                let is_final = is_final_annotation(&assign.annotation);
                let is_class_var = is_classvar_annotation(&assign.annotation);
                members.extend(extract_assignment_names(&assign.target).into_iter().map(|name| ClassMember {
                    name,
                    kind: ClassMemberKind::Field,
                    annotation: slice_range(normalized, assign.annotation.range()).map(str::to_owned),
                    params: Vec::new(),
                    returns: None,
                    is_final,
                    is_class_var,
                    line: offset_to_line_column(normalized, assign.range.start().to_usize()).0,
                }));
            }
            Stmt::Assign(assign) => {
                let line = offset_to_line_column(normalized, assign.range.start().to_usize()).0;
                members.extend(assign.targets.iter().flat_map(extract_assignment_names).map(|name| ClassMember {
                    name,
                    kind: ClassMemberKind::Field,
                    annotation: None,
                    params: Vec::new(),
                    returns: None,
                    is_final: false,
                    is_class_var: false,
                    line,
                }));
            }
            _ => {}
        }
    }

    members
}

fn ast_class_def_for_line<'a>(normalized: &str, suite: &'a [Stmt], line: usize) -> Option<&'a ruff_python_ast::StmtClassDef> {
    suite.iter().find_map(|stmt| match stmt {
        Stmt::ClassDef(class_def)
            if offset_to_line_column(normalized, class_def.range.start().to_usize()).0 == line =>
        {
            Some(class_def)
        }
        _ => None,
    })
}

fn ast_function_def_for_line<'a>(normalized: &str, suite: &'a [Stmt], line: usize) -> Option<&'a ruff_python_ast::StmtFunctionDef> {
    suite.iter().find_map(|stmt| match stmt {
        Stmt::FunctionDef(function_def)
            if offset_to_line_column(normalized, function_def.range.start().to_usize()).0 == line =>
        {
            Some(function_def)
        }
        _ => None,
    })
}

fn normalize_typepython_source(source: &str, statements: &[SyntaxStatement]) -> String {
    let statement_lines: std::collections::BTreeMap<usize, &SyntaxStatement> = statements
        .iter()
        .map(|statement| (statement_line(statement), statement))
        .collect();

    let mut normalized_lines = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let normalized = if let Some(statement) = statement_lines.get(&line_number) {
            normalize_typepython_statement_line(line, statement)
        } else {
            normalize_generic_python_header_line(line)
        };
        normalized_lines.push(normalized);
    }

    let mut normalized = normalized_lines.join("\n");
    if source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn statement_line(statement: &SyntaxStatement) -> usize {
    match statement {
        SyntaxStatement::TypeAlias(statement) => statement.line,
        SyntaxStatement::Interface(statement) => statement.line,
        SyntaxStatement::DataClass(statement) => statement.line,
        SyntaxStatement::SealedClass(statement) => statement.line,
        SyntaxStatement::OverloadDef(statement) => statement.line,
        SyntaxStatement::ClassDef(statement) => statement.line,
        SyntaxStatement::FunctionDef(statement) => statement.line,
        SyntaxStatement::Import(statement) => statement.line,
        SyntaxStatement::Value(statement) => statement.line,
        SyntaxStatement::Unsafe(statement) => statement.line,
    }
}

fn normalize_typepython_statement_line(line: &str, statement: &SyntaxStatement) -> String {
    match statement {
        SyntaxStatement::TypeAlias(statement) => {
            let indentation = leading_indent(line);
            format!("{indentation}{} = {}", statement.name, statement.value)
        }
        SyntaxStatement::Interface(statement)
        | SyntaxStatement::DataClass(statement)
        | SyntaxStatement::SealedClass(statement)
        | SyntaxStatement::ClassDef(statement) => {
            let indentation = leading_indent(line);
            format!(
                "{indentation}class {}{}{}:",
                statement.name,
                render_type_params(&statement.type_params),
                statement.header_suffix
            )
        }
        SyntaxStatement::OverloadDef(_) => {
            let indentation = leading_indent(line);
            let trimmed = line.trim_start();
            let rest = trimmed.strip_prefix("overload ").unwrap_or(trimmed);
            format!("{indentation}{rest}")
        }
        SyntaxStatement::FunctionDef(_) => line.to_owned(),
        SyntaxStatement::Import(_) | SyntaxStatement::Value(_) => line.to_owned(),
        SyntaxStatement::Unsafe(_) => {
            let indentation = leading_indent(line);
            format!("{indentation}if True:")
        }
    }
}

fn normalize_generic_python_header_line(line: &str) -> String {
    line.to_owned()
}

fn leading_indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn render_type_params(type_params: &[TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }

    format!(
        "[{}]",
        type_params
            .iter()
            .map(|type_param| match &type_param.bound {
                Some(bound) => format!("{}: {}", type_param.name, bound),
                None => type_param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn extract_ast_backed_statements(
    path: &Path,
    source: &str,
    normalized: &str,
    suite: &[Stmt],
    existing: &[SyntaxStatement],
    diagnostics: &mut DiagnosticReport,
) -> Vec<SyntaxStatement> {
    let existing_lines: std::collections::BTreeSet<_> = existing.iter().map(statement_line).collect();
    let mut statements = Vec::new();

    for stmt in suite {
        let line = offset_to_line_column(normalized, stmt.range().start().to_usize()).0;
        if existing_lines.contains(&line) {
            continue;
        }
        if let Some(statement) =
            extract_ast_backed_statement(path, source, normalized, stmt, line, diagnostics)
        {
            statements.push(statement);
        }
    }

    statements
}

fn extract_ast_backed_statement(
    path: &Path,
    source: &str,
    normalized: &str,
    stmt: &Stmt,
    line: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    match stmt {
        Stmt::ClassDef(stmt) => Some(SyntaxStatement::ClassDef(NamedBlockStatement {
            name: stmt.name.as_str().to_owned(),
            type_params: extract_ast_type_params(
                path,
                source,
                stmt.type_params.as_deref(),
                line,
                "class declaration",
                diagnostics,
            )?,
            header_suffix: stmt
                .arguments
                .as_ref()
                .and_then(|arguments| slice_range(source, arguments.range()))
                .map(str::to_owned)
                .unwrap_or_default(),
            members: extract_class_members(normalized, &stmt.body),
            line,
        })),
        Stmt::FunctionDef(stmt) => {
            let is_overload = stmt.decorator_list.iter().any(is_overload_decorator);
            let statement = FunctionStatement {
                name: stmt.name.as_str().to_owned(),
                type_params: extract_ast_type_params(
                    path,
                    source,
                    stmt.type_params.as_deref(),
                    line,
                    if is_overload {
                        "overload declaration"
                    } else {
                        "function declaration"
                    },
                    diagnostics,
                )?,
                params: extract_function_params(source, &stmt.parameters),
                returns: stmt
                    .returns
                    .as_ref()
                    .and_then(|returns| slice_range(source, returns.range()))
                    .map(str::to_owned),
                line,
            };

            Some(if is_overload {
                SyntaxStatement::OverloadDef(statement)
            } else {
                SyntaxStatement::FunctionDef(statement)
            })
        }
        Stmt::Import(stmt) => {
            let names = stmt
                .names
                .iter()
                .map(|alias| {
                    alias
                        .asname
                        .as_ref()
                        .map(|identifier| identifier.as_str().to_owned())
                        .unwrap_or_else(|| {
                            alias
                                .name
                                .as_str()
                                .split('.')
                                .next()
                                .unwrap_or(alias.name.as_str())
                                .to_owned()
                        })
                })
                .collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Import(ImportStatement { names, line }))
        }
        Stmt::ImportFrom(stmt) => {
            let names = stmt
                .names
                .iter()
                .map(|alias| alias.asname.as_ref().unwrap_or(&alias.name).as_str().to_owned())
                .collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Import(ImportStatement { names, line }))
        }
        Stmt::Assign(stmt) => {
            let names = stmt
                .targets
                .iter()
                .flat_map(extract_assignment_names)
                .collect::<Vec<_>>();
            (!names.is_empty()).then_some(SyntaxStatement::Value(ValueStatement {
                names,
                is_final: false,
                is_class_var: false,
                line,
            }))
        }
        Stmt::AnnAssign(stmt) => {
            let names = extract_assignment_names(&stmt.target);
            (!names.is_empty()).then_some(SyntaxStatement::Value(ValueStatement {
                names,
                is_final: is_final_annotation(&stmt.annotation),
                is_class_var: is_classvar_annotation(&stmt.annotation),
                line,
            }))
        }
        _ => None,
    }
}

fn extract_ast_type_params(
    path: &Path,
    source: &str,
    type_params: Option<&ruff_python_ast::TypeParams>,
    line: usize,
    label: &str,
    diagnostics: &mut DiagnosticReport,
) -> Option<Vec<TypeParam>> {
    let mut parsed = Vec::new();

    for type_param in type_params.into_iter().flat_map(|type_params| type_params.iter()) {
        match type_param {
            AstTypeParam::TypeVar(type_var) => {
                if type_var.default.is_some() {
                    diagnostics.push(
                        Diagnostic::error(
                            "TPY4010",
                            format!("{label} uses deferred-beyond-v1 type parameter defaults"),
                        )
                        .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
                    );
                    return None;
                }
                parsed.push(TypeParam {
                    name: type_var.name.as_str().to_owned(),
                    bound: type_var
                        .bound
                        .as_ref()
                        .and_then(|bound| slice_range(source, bound.range()))
                        .map(str::to_owned),
                });
            }
            AstTypeParam::TypeVarTuple(_) | AstTypeParam::ParamSpec(_) => {
                diagnostics.push(
                    Diagnostic::error(
                        "TPY4010",
                        format!("{label} uses deferred-beyond-v1 type parameter syntax"),
                    )
                    .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
                );
                return None;
            }
        }
    }

    let mut seen = std::collections::BTreeSet::new();
    for type_param in &parsed {
        if !seen.insert(type_param.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4004",
                    format!("{label} declares type parameter `{}` more than once", type_param.name),
                )
                .with_span(Span::new(path.display().to_string(), line, 1, line, 1)),
            );
            return None;
        }
    }

    Some(parsed)
}

fn extract_function_params(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
) -> Vec<FunctionParam> {
    parameters
        .iter()
        .map(|parameter| FunctionParam {
            name: parameter.name().as_str().to_owned(),
            annotation: parameter
                .annotation()
                .and_then(|annotation| slice_range(source, annotation.range()))
                .map(str::to_owned),
        })
        .collect()
}

fn extract_assignment_names(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Name(name) => vec![name.id.as_str().to_owned()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(extract_assignment_names).collect(),
        Expr::Starred(starred) => extract_assignment_names(&starred.value),
        _ => Vec::new(),
    }
}

fn is_overload_decorator(decorator: &ruff_python_ast::Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id.as_str() == "overload",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "overload"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if name.id.as_str() == "typing")
        }
        _ => false,
    }
}

fn is_final_annotation(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "Final",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "Final"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        Expr::Subscript(subscript) => is_final_annotation(&subscript.value),
        _ => false,
    }
}

fn is_classvar_annotation(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "ClassVar",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "ClassVar"
                && matches!(attribute.value.as_ref(), Expr::Name(name) if matches!(name.id.as_str(), "typing" | "typing_extensions"))
        }
        Expr::Subscript(subscript) => is_classvar_annotation(&subscript.value),
        _ => false,
    }
}

fn slice_range(source: &str, range: ruff_text_size::TextRange) -> Option<&str> {
    source.get(range.start().to_usize()..range.end().to_usize())
}

fn parse_extension_statement(
    path: &Path,
    trimmed_line: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    if let Some(rest) = strip_soft_keyword(trimmed_line, "typealias") {
        return parse_typealias(path, trimmed_line, rest, line_number, diagnostics);
    }
    if let Some(rest) = strip_soft_keyword(trimmed_line, "interface") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "interface declaration",
            SyntaxStatement::Interface,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("data class ") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "data class declaration",
            SyntaxStatement::DataClass,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("sealed class ") {
        return parse_named_block(
            path,
            trimmed_line,
            rest,
            line_number,
            diagnostics,
            "sealed class declaration",
            SyntaxStatement::SealedClass,
        );
    }
    if let Some(rest) = trimmed_line.strip_prefix("overload def ") {
        return parse_overload(path, trimmed_line, rest, line_number, diagnostics);
    }
    if trimmed_line.starts_with("unsafe") {
        return parse_unsafe(path, trimmed_line, line_number, diagnostics);
    }

    None
}

fn strip_soft_keyword<'source>(line: &'source str, keyword: &str) -> Option<&'source str> {
    let rest = line.strip_prefix(keyword)?;
    match rest.chars().next() {
        Some(character) if character == '_' || character.is_ascii_alphanumeric() => None,
        _ => Some(rest),
    }
}

fn parse_typealias(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    let (head, tail) = match split_top_level_once(rest, '=') {
        Some(parts) => parts,
        None => {
            diagnostics.push(parse_error(
                path,
                line_number,
                line,
                "typealias declaration must contain `=`",
            ));
            return None;
        }
    };

    if tail.trim().is_empty() {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "typealias declaration must define a target type expression",
        ));
        return None;
    }

    let Some((name, suffix)) = extract_decl_head(head) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "typealias declaration must name an alias before `=`",
        ));
        return None;
    };
    let Some(parsed_type_params) = parse_type_params(
        path,
        line_number,
        line,
        suffix,
        diagnostics,
        "typealias declaration",
    ) else {
        return None;
    };

    Some(SyntaxStatement::TypeAlias(TypeAliasStatement {
        name,
        type_params: parsed_type_params.type_params,
        value: tail.trim().to_owned(),
        line: line_number,
    }))
}

fn parse_named_block(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(NamedBlockStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    if !line.ends_with(':') {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    }

    let header = &rest[..rest.len().saturating_sub(1)].trim_end();
    let Some((name, suffix)) = extract_decl_head(header) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a valid name"),
        ));
        return None;
    };
    let Some(parsed_type_params) =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)
    else {
        return None;
    };

    Some(constructor(NamedBlockStatement {
        name,
        type_params: parsed_type_params.type_params,
        header_suffix: parsed_type_params.remainder.trim().to_owned(),
        members: Vec::new(),
        line: line_number,
    }))
}

fn parse_overload(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    parse_function(
        path,
        line,
        rest,
        line_number,
        diagnostics,
        "overload declaration",
        SyntaxStatement::OverloadDef,
    )
}

fn parse_function(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(FunctionStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    let Some((signature, _suite)) = split_top_level_once(rest.trim_end(), ':') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    };

    let Some((name_part, _)) = split_top_level_once(signature.trim_end(), '(') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a parameter list"),
        ));
        return None;
    };
    let Some((name, suffix)) = extract_decl_head(name_part) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a function name"),
        ));
        return None;
    };
    let Some(parsed_type_params) = parse_type_params(
        path,
        line_number,
        line,
        suffix,
        diagnostics,
        label,
    ) else {
        return None;
    };

    Some(constructor(FunctionStatement {
        name,
        type_params: parsed_type_params.type_params,
        params: Vec::new(),
        returns: None,
        line: line_number,
    }))
}

fn parse_unsafe(
    path: &Path,
    line: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    if line == "unsafe:" {
        return Some(SyntaxStatement::Unsafe(UnsafeStatement { line: line_number }));
    }

    if line.starts_with("unsafe:") {
        return Some(SyntaxStatement::Unsafe(UnsafeStatement { line: line_number }));
    }

    diagnostics.push(parse_error(
        path,
        line_number,
        line,
        "unsafe block must start with `unsafe:`",
    ));
    None
}

fn extract_decl_head(header: &str) -> Option<(String, &str)> {
    let header = header.trim();
    if header.is_empty() {
        return None;
    }

    let end = header
        .find(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .unwrap_or(header.len());
    let candidate = &header[..end];
    is_valid_identifier(candidate).then(|| (candidate.to_owned(), &header[end..]))
}

fn parse_type_params<'source>(
    path: &Path,
    line_number: usize,
    line: &str,
    suffix: &'source str,
    diagnostics: &mut DiagnosticReport,
    label: &str,
) -> Option<ParsedTypeParams<'source>> {
    let suffix = suffix.trim_start();
    if suffix.is_empty() || !suffix.starts_with('[') {
        return Some(ParsedTypeParams {
            type_params: Vec::new(),
            remainder: suffix,
        });
    }

    let Some((content, remainder)) = split_bracketed(suffix) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} has an unterminated type parameter list"),
        ));
        return None;
    };
    if remainder.trim_start().starts_with('[') {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must not contain multiple type parameter lists"),
        ));
        return None;
    }

    let mut type_params = Vec::new();
    for item in split_top_level(content, ',') {
        match parse_type_param(path, line_number, line, item, label) {
            Ok(type_param) => type_params.push(type_param),
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                return None;
            }
        }
    }

    let mut seen = std::collections::BTreeSet::new();
    for type_param in &type_params {
        if !seen.insert(type_param.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4004",
                    format!(
                        "{label} declares type parameter `{}` more than once",
                        type_param.name
                    ),
                )
                .with_span(Span::new(
                    path.display().to_string(),
                    line_number,
                    1,
                    line_number,
                    line.chars().count().max(1),
                )),
            );
            return None;
        }
    }

    Some(ParsedTypeParams {
        type_params,
        remainder,
    })
}

fn split_top_level_once(input: &str, separator: char) -> Option<(&str, &str)> {
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, character) in input.char_indices() {
        if character == separator && bracket_depth == 0 && paren_depth == 0 {
            let tail_start = index + character.len_utf8();
            return Some((&input[..index], &input[tail_start..]));
        }

        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

fn parse_type_param(
    path: &Path,
    line_number: usize,
    line: &str,
    item: &str,
    label: &str,
) -> Result<TypeParam, Diagnostic> {
    let item = item.trim();
    if item.is_empty() {
        return Err(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an empty type parameter entry"),
        ));
    }
    if item.contains('=') {
        return Err(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter defaults are deferred beyond v1"),
        ));
    }

    let (name_part, bound) = match item.split_once(':') {
        Some((name_part, bound)) => (name_part.trim(), Some(bound.trim())),
        None => (item, None),
    };
    if !is_valid_identifier(name_part) {
        return Err(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an invalid type parameter name"),
        ));
    }

    let bound = match bound {
        Some(bound) if bound.is_empty() => {
            return Err(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter bound must not be empty"),
            ));
        }
        Some(bound) if bound.starts_with('(') => {
            return Err(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter constraint lists are deferred beyond v1"),
            ));
        }
        Some(bound) => Some(bound.to_owned()),
        None => None,
    };

    Ok(TypeParam {
        name: name_part.to_owned(),
        bound,
    })
}

fn split_bracketed(input: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((&input[1..index], &input[index + 1..]));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;

    for (index, character) in input.char_indices() {
        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ if character == separator && bracket_depth == 0 && paren_depth == 0 => {
                items.push(&input[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }

    items.push(&input[start..]);
    items
}

fn is_valid_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    match characters.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn parse_error(
    path: &Path,
    line_number: usize,
    line: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error("TPY2001", message.into()).with_span(Span::new(
        path.display().to_string(),
        line_number,
        1,
        line_number,
        line.chars().count().max(1),
    ))
}

fn parse_error_span(path: &Path, source: &str, start: usize, end: usize) -> Span {
    let start = start.min(source.len());
    let end = end.max(start).min(source.len());
    let (line, column) = offset_to_line_column(source, start);
    let (end_line, end_column) = offset_to_line_column(source, end);

    Span::new(path.display().to_string(), line, column, end_line, end_column)
}

fn offset_to_line_column(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut column = 1usize;

    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

#[cfg(test)]
mod tests {
    use super::{
        ClassMember, ClassMemberKind, FunctionStatement, ImportStatement, NamedBlockStatement,
        FunctionParam, SourceFile, SourceKind, SyntaxStatement, TypeAliasStatement, TypeParam,
        UnsafeStatement, ValueStatement, parse,
    };
    use std::path::PathBuf;

    #[test]
    fn parse_recognizes_typepython_extension_headers() {
        let tree = parse(SourceFile {
            path: PathBuf::from("example.tpy"),
            kind: SourceKind::TypePython,
            text: concat!(
                "typealias Pair[T] = tuple[T, T]\n",
                "interface Service:\n",
                "    pass\n",
                "data class Box:\n",
                "    pass\n",
                "sealed class Result:\n",
                "    pass\n",
                "overload def parse(value):\n",
                "    ...\n",
                "unsafe:\n",
                "    pass\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    value: String::from("tuple[T, T]"),
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Service"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 4,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 6,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                    }],
                    returns: None,
                    line: 8,
                }),
                SyntaxStatement::Unsafe(UnsafeStatement { line: 10 }),
            ]
        );
    }

    #[test]
    fn parse_captures_type_params_and_bounds() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic.tpy"),
            kind: SourceKind::TypePython,
            text: concat!(
                "typealias Pair[T: Hashable] = tuple[T, T]\n",
                "interface Box[T]:\n",
                "    pass\n",
                "data class Node[T: Sequence[str]]:\n",
                "    pass\n",
                "sealed class Result[T]:\n",
                "    pass\n",
                "overload def first[T: Sequence[str]](value):\n",
                "    ...\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("Pair"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Hashable")),
                    }],
                    value: String::from("tuple[T, T]"),
                    line: 1,
                }),
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Node"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Sequence[str]")),
                    }],
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 4,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 6,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Sequence[str]")),
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: None,
                    }],
                    returns: None,
                    line: 8,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_malformed_extension_headers() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            text: concat!(
                "typealias Pair tuple[int, int]\n",
                "interface:\n",
                "overload def parse\n",
                "unsafe\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("typealias declaration must contain `=`"));
        assert!(rendered.contains("interface declaration must include a valid name"));
        assert!(rendered.contains("overload declaration must end with `:`"));
        assert!(rendered.contains("unsafe block must start with `unsafe:`"));
    }

    #[test]
    fn parse_reports_malformed_type_parameter_lists() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken-generics.tpy"),
            kind: SourceKind::TypePython,
            text: concat!(
                "typealias Pair[T = int] = tuple[T, T]\n",
                "interface Box[T:] :\n",
                "overload def first[T: (A, B)](value):\n"
            )
            .to_owned(),
        });

        assert!(tree.diagnostics.has_errors());
        let rendered = tree.diagnostics.as_text();
        assert!(rendered.contains("type parameter defaults are deferred beyond v1"));
        assert!(rendered.contains("type parameter bound must not be empty"));
        assert!(rendered.contains("type parameter constraint lists are deferred beyond v1"));
    }

    #[test]
    fn parse_reports_duplicate_type_parameter_names() {
        let tree = parse(SourceFile {
            path: PathBuf::from("duplicate-generics.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("class Box[T, T]:\n    pass\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4004"));
        assert!(rendered.contains("declares type parameter `T` more than once"));
    }

    #[test]
    fn parse_captures_interface_base_list_suffix() {
        let tree = parse(SourceFile {
            path: PathBuf::from("interface-bases.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("interface SupportsClose(Closable):\n    pass\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::from("(Closable)"),
                members: Vec::new(),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_rejects_executable_interface_bodies() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-interface.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("interface SupportsClose:\n    value = 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("body must not contain executable statements"));
    }

    #[test]
    fn parse_accepts_overload_simple_suite_form() {
        let tree = parse(SourceFile {
            path: PathBuf::from("overload-simple-suite.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("overload def parse(x: str) -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::OverloadDef(FunctionStatement {
                name: String::from("parse"),
                type_params: Vec::new(),
                params: vec![FunctionParam {
                    name: String::from("x"),
                    annotation: Some(String::from("str")),
                }],
                returns: Some(String::from("int")),
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_rejects_executable_overload_bodies() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-overload.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("overload def parse(x: str) -> int:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("body must not contain executable statements"));
    }

    #[test]
    fn parse_leaves_python_files_without_extension_analysis() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            text: String::from("def unsafe(value):\n    return value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("unsafe"),
                type_params: Vec::new(),
                params: vec![FunctionParam {
                    name: String::from("value"),
                    annotation: None,
                }],
                returns: None,
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_reports_invalid_python_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.py"),
            kind: SourceKind::Python,
            text: String::from("def broken(:\n    return 1\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("Python syntax error"));
    }

    #[test]
    fn parse_accepts_valid_stub_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.pyi"),
            kind: SourceKind::Stub,
            text: String::from("def helper() -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
    }

    #[test]
    fn parse_classifies_decorated_overloads_in_stub_sources() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.pyi"),
            kind: SourceKind::Stub,
            text: String::from("from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("overload")],
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("x"),
                        annotation: Some(String::from("str")),
                    }],
                    returns: Some(String::from("int")),
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_reports_invalid_typepython_body_syntax_after_normalization() {
        let tree = parse(SourceFile {
            path: PathBuf::from("broken.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("typealias UserId = int\ndef broken():\n    return )\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY2001"));
        assert!(rendered.contains("TypePython syntax error"));
    }

    #[test]
    fn parse_accepts_generic_python_headers_in_typepython_source() {
        let tree = parse(SourceFile {
            path: PathBuf::from("generic.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("class Box[T]:\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::new(),
                    members: Vec::new(),
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("T")),
                    }],
                    returns: Some(String::from("T")),
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_imports_and_values_from_ast_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.tpy"),
            kind: SourceKind::TypePython,
            text: String::from(
                "from pkg import foo, bar as baz\nimport tools.helpers, more.tools as alias\nvalue: int = 1\na = b = 2\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("foo"), String::from("baz")],
                    line: 1,
                }),
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("tools"), String::from("alias")],
                    line: 2,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value")],
                    is_final: false,
                    is_class_var: false,
                    line: 3,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("a"), String::from("b")],
                    is_final: false,
                    is_class_var: false,
                    line: 4,
                }),
            ]
        );
    }

    #[test]
    fn parse_extracts_class_like_members_from_ast_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("members.tpy"),
            kind: SourceKind::TypePython,
            text: String::from(
                "class Box:\n    value: int\n    total = 1\n    def get(self) -> int: ...\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Box"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                members: vec![
                    ClassMember {
                        name: String::from("value"),
                        kind: ClassMemberKind::Field,
                        annotation: Some(String::from("int")),
                        params: Vec::new(),
                        returns: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("total"),
                        kind: ClassMemberKind::Field,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("get"),
                        kind: ClassMemberKind::Method,
                        annotation: None,
                        params: vec![FunctionParam {
                            name: String::from("self"),
                            annotation: None,
                        }],
                        returns: Some(String::from("int")),
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_marks_decorated_class_methods_as_overload_members() {
        let tree = parse(SourceFile {
            path: PathBuf::from("class-overloads.tpy"),
            kind: SourceKind::TypePython,
            text: String::from(
                "from typing import overload\n\nclass Parser:\n    @overload\n    def parse(self, x: str) -> int: ...\n\n    def parse(self, x):\n        return 0\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("overload")],
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Parser"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    members: vec![
                        ClassMember {
                            name: String::from("parse"),
                            kind: ClassMemberKind::Overload,
                            annotation: None,
                            params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                },
                                FunctionParam {
                                    name: String::from("x"),
                                    annotation: Some(String::from("str")),
                                },
                            ],
                            returns: Some(String::from("int")),
                            is_final: false,
                            is_class_var: false,
                            line: 4,
                        },
                        ClassMember {
                            name: String::from("parse"),
                            kind: ClassMemberKind::Method,
                            annotation: None,
                            params: vec![
                                FunctionParam {
                                    name: String::from("self"),
                                    annotation: None,
                                },
                                FunctionParam {
                                    name: String::from("x"),
                                    annotation: None,
                                },
                            ],
                            returns: None,
                            is_final: false,
                            is_class_var: false,
                            line: 7,
                        },
                    ],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_final_value_declarations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("finals.py"),
            kind: SourceKind::Python,
            text: String::from(
                "from typing import Final\nMAX_SIZE: Final = 100\nclass Box:\n    limit: Final[int] = 1\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("Final")],
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("MAX_SIZE")],
                    is_final: true,
                    is_class_var: false,
                    line: 2,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    members: vec![ClassMember {
                        name: String::from("limit"),
                        kind: ClassMemberKind::Field,
                        annotation: Some(String::from("Final[int]")),
                        params: Vec::new(),
                        returns: None,
                        is_final: true,
                        is_class_var: false,
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_marks_classvar_value_declarations() {
        let tree = parse(SourceFile {
            path: PathBuf::from("classvars.py"),
            kind: SourceKind::Python,
            text: String::from(
                "from typing import ClassVar\nVALUE: ClassVar[int] = 1\nclass Box:\n    cache: ClassVar[int] = 2\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("ClassVar")],
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("VALUE")],
                    is_final: false,
                    is_class_var: true,
                    line: 2,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    members: vec![ClassMember {
                        name: String::from("cache"),
                        kind: ClassMemberKind::Field,
                        annotation: Some(String::from("ClassVar[int]")),
                        params: Vec::new(),
                        returns: None,
                        is_final: false,
                        is_class_var: true,
                        line: 4,
                    }],
                    line: 3,
                }),
            ]
        );
    }

    #[test]
    fn parse_rejects_classvar_inside_function_body() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-classvar.py"),
            kind: SourceKind::Python,
            text: String::from(
                "from typing import ClassVar\n\ndef build() -> None:\n    value: ClassVar[int] = 1\n",
            ),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("ClassVar[...] is not allowed inside function or method bodies"));
    }

    #[test]
    fn parse_rejects_classvar_in_parameter_position() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-classvar-param.py"),
            kind: SourceKind::Python,
            text: String::from("from typing import ClassVar\n\ndef build(value: ClassVar[int]) -> None:\n    pass\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4001"));
        assert!(rendered.contains("parameter annotations"));
    }

    #[test]
    fn parse_rejects_final_in_parameter_position() {
        let tree = parse(SourceFile {
            path: PathBuf::from("bad-final-param.py"),
            kind: SourceKind::Python,
            text: String::from("from typing import Final\n\ndef build(value: Final[int]) -> None:\n    pass\n"),
        });

        let rendered = tree.diagnostics.as_text();
        assert!(tree.diagnostics.has_errors());
        assert!(rendered.contains("TPY4010"));
        assert!(rendered.contains("deferred beyond v1"));
    }

    #[test]
    fn parse_retains_function_signature_shapes() {
        let tree = parse(SourceFile {
            path: PathBuf::from("signatures.py"),
            kind: SourceKind::Python,
            text: String::from(
                "from typing import overload\n\n@overload\ndef parse(value: str) -> int: ...\n\ndef build(value: int) -> str:\n    return \"x\"\n",
            ),
        });

        assert!(tree.diagnostics.is_empty());
        println!("{:?}", tree.statements);
        assert_eq!(
            tree.statements,
            vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("overload")],
                    line: 1,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("str")),
                    }],
                    returns: Some(String::from("int")),
                    line: 3,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("build"),
                    type_params: Vec::new(),
                    params: vec![FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("int")),
                    }],
                    returns: Some(String::from("str")),
                    line: 6,
                }),
            ]
        );
    }
}
