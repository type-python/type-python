//! Source classification and parser boundary for TypePython.

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use ruff_python_parser::parse_module;
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
    pub line: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FunctionStatement {
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub line: usize,
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
    let mut diagnostics = DiagnosticReport::default();

    if let Err(error) = parse_module(&source.text) {
        diagnostics.push(
            Diagnostic::error("TPY2001", format!("Python syntax error: {}", error.error))
                .with_span(parse_error_span(&source.path, &source.text, error.location.start().to_usize(), error.location.end().to_usize())),
        );
    }

    SyntaxTree {
        source,
        statements: Vec::new(),
        diagnostics,
    }
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

    SyntaxTree { source, statements, diagnostics }
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
    let Some((signature, _suite)) = split_top_level_once(rest.trim_end(), ':') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "overload declaration must end with `:`",
        ));
        return None;
    };

    let Some((name_part, _)) = split_top_level_once(signature.trim_end(), '(') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "overload declaration must include a parameter list",
        ));
        return None;
    };
    let Some((name, suffix)) = extract_decl_head(name_part) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            "overload declaration must include a function name",
        ));
        return None;
    };
    let Some(parsed_type_params) = parse_type_params(
        path,
        line_number,
        line,
        suffix,
        diagnostics,
        "overload declaration",
    ) else {
        return None;
    };

    Some(SyntaxStatement::OverloadDef(FunctionStatement {
        name,
        type_params: parsed_type_params.type_params,
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
        FunctionStatement, NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement,
        TypeAliasStatement, TypeParam, UnsafeStatement, parse,
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
                "data class Box:\n",
                "sealed class Result:\n",
                "overload def parse(value):\n",
                "unsafe:\n"
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
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    line: 3,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    line: 4,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    line: 5,
                }),
                SyntaxStatement::Unsafe(UnsafeStatement { line: 6 }),
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
                "data class Node[T: Sequence[str]]:\n",
                "sealed class Result[T]:\n",
                "overload def first[T: Sequence[str]](value):\n"
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
                    line: 2,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("Node"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Sequence[str]")),
                    }],
                    header_suffix: String::new(),
                    line: 3,
                }),
                SyntaxStatement::SealedClass(NamedBlockStatement {
                    name: String::from("Result"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::new(),
                    line: 4,
                }),
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: Some(String::from("Sequence[str]")),
                    }],
                    line: 5,
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
    fn parse_captures_interface_base_list_suffix() {
        let tree = parse(SourceFile {
            path: PathBuf::from("interface-bases.tpy"),
            kind: SourceKind::TypePython,
            text: String::from("interface SupportsClose(Closable):\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert_eq!(
            tree.statements,
            vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::from("(Closable)"),
                line: 1,
            })]
        );
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
                line: 1,
            })]
        );
    }

    #[test]
    fn parse_leaves_python_files_without_extension_analysis() {
        let tree = parse(SourceFile {
            path: PathBuf::from("module.py"),
            kind: SourceKind::Python,
            text: String::from("def unsafe(value):\n    return value\n"),
        });

        assert!(tree.diagnostics.is_empty());
        assert!(tree.statements.is_empty());
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
}
