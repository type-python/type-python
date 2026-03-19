//! Lowering boundary for TypePython.

use std::{collections::BTreeSet, path::PathBuf};

use typepython_diagnostics::{Diagnostic, DiagnosticReport, Span};
use typepython_syntax::{SourceKind, SyntaxStatement, SyntaxTree};

/// A single source-map row.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SourceMapEntry {
    /// Original source line.
    pub original_line: usize,
    /// Lowered output line.
    pub lowered_line: usize,
}

/// Lowered representation consumed by later phases.
#[derive(Debug, Clone)]
pub struct LoweredModule {
    /// Original module path.
    pub source_path: PathBuf,
    /// Source kind of the module.
    pub source_kind: SourceKind,
    /// Lowered Python text.
    pub python_source: String,
    /// Placeholder source-map rows.
    pub source_map: Vec<SourceMapEntry>,
}

#[derive(Debug, Clone)]
pub struct LoweringResult {
    pub module: LoweredModule,
    pub diagnostics: DiagnosticReport,
}

struct LoweredText {
    python_source: String,
    source_map: Vec<SourceMapEntry>,
}

/// Lowers a parsed module into its Python-facing form.
#[must_use]
pub fn lower(tree: &SyntaxTree) -> LoweringResult {
    let lowered_text = match tree.source.kind {
        SourceKind::TypePython => lower_typepython(tree),
        SourceKind::Python | SourceKind::Stub => lower_passthrough(&tree.source.text),
    };
    let diagnostics = collect_lowering_diagnostics(tree);

    LoweringResult {
        module: LoweredModule {
            source_path: tree.source.path.clone(),
            source_kind: tree.source.kind,
            python_source: lowered_text.python_source,
            source_map: lowered_text.source_map,
        },
        diagnostics,
    }
}

fn lower_passthrough(source: &str) -> LoweredText {
    LoweredText {
        python_source: source.to_owned(),
        source_map: source
            .lines()
            .enumerate()
            .map(|(index, _)| SourceMapEntry {
                original_line: index + 1,
                lowered_line: index + 1,
            })
            .collect(),
    }
}

fn lower_typepython(tree: &SyntaxTree) -> LoweredText {
    let unsafe_lines: BTreeSet<_> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Unsafe(statement) => Some(statement.line),
            _ => None,
        })
        .collect();
    let type_aliases: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement) if statement.type_params.is_empty() => {
                Some((statement.line, statement))
            }
            _ => None,
        })
        .collect();
    let interfaces: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Interface(statement) if is_lowerable_interface(statement) => {
                Some((statement.line, statement))
            }
            _ => None,
        })
        .collect();
    let data_classes: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::DataClass(statement) if is_lowerable_named_block(statement) => {
                Some((statement.line, statement))
            }
            _ => None,
        })
        .collect();
    let overloads: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::OverloadDef(statement) if statement.type_params.is_empty() => {
                Some((statement.line, statement))
            }
            _ => None,
        })
        .collect();
    let sealed_classes: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::SealedClass(statement) if is_lowerable_named_block(statement) => {
                Some((statement.line, statement))
            }
            _ => None,
        })
        .collect();
    let class_defs: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::ClassDef(statement) => Some((statement.line, statement)),
            _ => None,
        })
        .collect();
    let function_defs: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::FunctionDef(statement) => Some((statement.line, statement)),
            _ => None,
        })
        .collect();

    let mut lowered_lines = Vec::new();
    let mut lowered_line_number = 1usize;
    let mut source_map = Vec::new();
    if !type_aliases.is_empty() && !has_typealias_import(&tree.source.text) {
        lowered_lines.push(String::from("from typing import TypeAlias"));
        lowered_line_number += 1;
    }
    if !interfaces.is_empty() && !has_protocol_import(&tree.source.text) {
        lowered_lines.push(String::from("from typing import Protocol"));
        lowered_line_number += 1;
    }
    if !data_classes.is_empty() && !has_dataclass_import(&tree.source.text) {
        lowered_lines.push(String::from("from dataclasses import dataclass"));
        lowered_line_number += 1;
    }
    if !overloads.is_empty() && !has_overload_import(&tree.source.text) {
        lowered_lines.push(String::from("from typing import overload"));
        lowered_line_number += 1;
    }

    for (index, line) in tree.source.text.lines().enumerate() {
        let line_number = index + 1;
        let replacement_lines = if let Some(statement) = type_aliases.get(&line_number) {
            vec![rewrite_typealias_line(line, statement)]
        } else if let Some(statement) = interfaces.get(&line_number) {
            vec![rewrite_interface_line(line, statement)]
        } else if let Some(statement) = data_classes.get(&line_number) {
            rewrite_data_class_lines(line, statement).into_iter().collect()
        } else if overloads.contains_key(&line_number) {
            rewrite_overload_lines(line).into_iter().collect()
        } else if let Some(statement) = sealed_classes.get(&line_number) {
            vec![rewrite_sealed_class_line(line, statement)]
        } else if let Some(statement) = class_defs.get(&line_number) {
            vec![rewrite_class_def_line(line, statement)]
        } else if function_defs.contains_key(&line_number) {
            vec![rewrite_function_def_line(line)]
        } else if unsafe_lines.contains(&line_number) {
            vec![rewrite_unsafe_line(line)]
        } else {
            vec![line.to_owned()]
        };

        source_map.push(SourceMapEntry {
            original_line: line_number,
            lowered_line: lowered_line_number,
        });
        lowered_line_number += replacement_lines.len();
        lowered_lines.extend(replacement_lines);
    }

    let mut lowered = lowered_lines.join("\n");
    if tree.source.text.ends_with('\n') {
        lowered.push('\n');
    }

    LoweredText { python_source: lowered, source_map }
}

fn rewrite_unsafe_line(line: &str) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!("{indentation}if True:")
}

fn rewrite_typealias_line(
    line: &str,
    statement: &typepython_syntax::TypeAliasStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!(
        "{indentation}{}: TypeAlias = {}",
        statement.name, statement.value
    )
}

fn has_typealias_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import TypeAlias"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("TypeAlias"))
    })
}

fn rewrite_interface_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = if statement.header_suffix.is_empty() {
        String::from("(Protocol)")
    } else {
        append_protocol_base(&statement.header_suffix)
    };
    format!("{indentation}class {}{}:", statement.name, bases)
}

fn rewrite_data_class_lines(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> [String; 2] {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = if statement.header_suffix.is_empty() {
        String::new()
    } else {
        statement.header_suffix.clone()
    };

    [
        format!("{indentation}@dataclass"),
        format!("{indentation}class {}{}:", statement.name, bases),
    ]
}

fn rewrite_sealed_class_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let bases = if statement.header_suffix.is_empty() {
        String::new()
    } else {
        statement.header_suffix.clone()
    };

    format!("{indentation}class {}{}:  # tpy:sealed", statement.name, bases)
}

fn rewrite_class_def_line(
    line: &str,
    statement: &typepython_syntax::NamedBlockStatement,
) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    format!("{indentation}class {}{}:", statement.name, statement.header_suffix)
}

fn rewrite_function_def_line(line: &str) -> String {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let trimmed = line.trim_start();
    format!("{indentation}{}", strip_generic_type_params(trimmed))
}

fn strip_generic_type_params(source: &str) -> String {
    let mut bracket_index = None;
    let mut paren_depth = 0usize;

    for (index, character) in source.char_indices() {
        match character {
            '[' if paren_depth == 0 => {
                bracket_index = Some(index);
                break;
            }
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    let Some(bracket_index) = bracket_index else {
        return source.to_owned();
    };
    let (head, tail) = source.split_at(bracket_index);
    let Some((_params, remainder)) = split_bracketed(tail) else {
        return source.to_owned();
    };
    format!("{head}{remainder}")
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

fn append_protocol_base(header_suffix: &str) -> String {
    let trimmed = header_suffix.trim();
    if trimmed == "()" {
        return String::from("(Protocol)");
    }

    let inner = trimmed.trim_start_matches('(').trim_end_matches(')').trim();
    if inner.is_empty() {
        String::from("(Protocol)")
    } else {
        format!("({inner}, Protocol)")
    }
}

fn has_protocol_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import Protocol"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("Protocol"))
    })
}

fn has_dataclass_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from dataclasses import dataclass"
            || (trimmed.starts_with("from dataclasses import ") && trimmed.contains("dataclass"))
    })
}

fn rewrite_overload_lines(line: &str) -> [String; 2] {
    let indentation_width = line.len() - line.trim_start().len();
    let indentation = &line[..indentation_width];
    let rewritten = line
        .trim_start()
        .strip_prefix("overload ")
        .unwrap_or_else(|| line.trim_start())
        .to_owned();

    [format!("{indentation}@overload"), format!("{indentation}{rewritten}")]
}

fn has_overload_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import overload"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("overload"))
    })
}

fn is_lowerable_interface(statement: &typepython_syntax::NamedBlockStatement) -> bool {
    is_lowerable_named_block(statement)
}

fn is_lowerable_named_block(statement: &typepython_syntax::NamedBlockStatement) -> bool {
    statement.type_params.is_empty()
        && (statement.header_suffix.is_empty()
            || (statement.header_suffix.starts_with('(')
                && statement.header_suffix.ends_with(')')))
}

fn collect_lowering_diagnostics(tree: &SyntaxTree) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();

    for statement in &tree.statements {
        match statement {
            SyntaxStatement::Unsafe(_) => {}
            SyntaxStatement::TypeAlias(statement) if statement.type_params.is_empty() => {}
            SyntaxStatement::TypeAlias(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "generic typealias",
            )),
            SyntaxStatement::Interface(statement) if is_lowerable_interface(statement) => {}
            SyntaxStatement::Interface(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "interface",
            )),
            SyntaxStatement::DataClass(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::DataClass(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "data class",
            )),
            SyntaxStatement::SealedClass(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::SealedClass(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "sealed class",
            )),
            SyntaxStatement::ClassDef(_) => {}
            SyntaxStatement::FunctionDef(_) => {}
            SyntaxStatement::OverloadDef(statement) if statement.type_params.is_empty() => {}
            SyntaxStatement::OverloadDef(statement) => diagnostics.push(lowering_error(
                &tree.source.path,
                statement.line,
                "generic overload def",
            )),
        }
    }

    diagnostics
}

fn lowering_error(path: &std::path::Path, line: usize, construct: &str) -> Diagnostic {
    Diagnostic::error(
        "TPY2002",
        format!("TypePython-only syntax `{construct}` is recognized but not lowerable yet"),
    )
    .with_span(Span::new(path.display().to_string(), line, 1, line, 1))
}

#[cfg(test)]
mod tests {
    use super::{SourceMapEntry, lower};
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement, SyntaxTree,
        TypeAliasStatement, TypeParam, UnsafeStatement,
    };

    #[test]
    fn lower_rewrites_top_level_unsafe_blocks() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("unsafe.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("unsafe:\n    x = 1\n"),
            },
            statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 1 })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "if True:\n    x = 1\n");
        assert_eq!(
            lowered.module.source_map,
            vec![
                SourceMapEntry {
                    original_line: 1,
                    lowered_line: 1,
                },
                SourceMapEntry {
                    original_line: 2,
                    lowered_line: 2,
                },
            ]
        );
    }

    #[test]
    fn lower_rewrites_nested_unsafe_blocks_with_indentation() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("nested-unsafe.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("def update():\n    unsafe:\n        x = 1\n"),
            },
            statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 2 })],
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "def update():\n    if True:\n        x = 1\n");
    }

    #[test]
    fn lower_reports_unimplemented_typepython_constructs() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("unsupported.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("interface Service[T]:\ndata class User[T]:\n"),
            },
            statements: vec![
                SyntaxStatement::Interface(NamedBlockStatement {
                    name: String::from("Service"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::new(),
                    line: 1,
                }),
                SyntaxStatement::DataClass(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::new(),
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(lowered.diagnostics.has_errors());
        assert!(rendered.contains("TPY2002"));
        assert!(rendered.contains("`interface`"));
        assert!(rendered.contains("`data class`"));
    }

    #[test]
    fn lower_rewrites_non_generic_typealias_with_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("typealias.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("typealias UserId = int\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserId"),
                type_params: Vec::new(),
                value: String::from("int"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "from typing import TypeAlias\nUserId: TypeAlias = int\n");
    }

    #[test]
    fn lower_rewrites_non_generic_interface_with_protocol_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("interface.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("interface SupportsClose:\n    def close(self): ...\n"),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import Protocol\nclass SupportsClose(Protocol):\n    def close(self): ...\n"
        );
    }

    #[test]
    fn lower_rewrites_interface_with_existing_bases() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("interface-bases.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("interface SupportsClose(Closable):\n    def close(self): ...\n"),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::from("(Closable)"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import Protocol\nclass SupportsClose(Closable, Protocol):\n    def close(self): ...\n"
        );
    }

    #[test]
    fn lower_rewrites_non_generic_data_class_with_dataclass_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("data-class.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("data class Point:\n    x: float\n    y: float\n"),
            },
            statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
                name: String::from("Point"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from dataclasses import dataclass\n@dataclass\nclass Point:\n    x: float\n    y: float\n"
        );
        assert_eq!(
            lowered.module.source_map,
            vec![
                SourceMapEntry {
                    original_line: 1,
                    lowered_line: 2,
                },
                SourceMapEntry {
                    original_line: 2,
                    lowered_line: 4,
                },
                SourceMapEntry {
                    original_line: 3,
                    lowered_line: 5,
                },
            ]
        );
    }

    #[test]
    fn lower_rewrites_data_class_with_existing_bases() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("data-class-bases.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("data class Point(Base):\n    x: float\n"),
            },
            statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
                name: String::from("Point"),
                type_params: Vec::new(),
                header_suffix: String::from("(Base)"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from dataclasses import dataclass\n@dataclass\nclass Point(Base):\n    x: float\n"
        );
    }

    #[test]
    fn lower_rewrites_non_generic_sealed_class_with_marker() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("sealed-class.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("sealed class Expr:\n    ...\n"),
            },
            statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
                name: String::from("Expr"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "class Expr:  # tpy:sealed\n    ...\n");
    }

    #[test]
    fn lower_rewrites_sealed_class_with_existing_bases() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("sealed-class-bases.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("sealed class Expr(Base):\n    ...\n"),
            },
            statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
                name: String::from("Expr"),
                type_params: Vec::new(),
                header_suffix: String::from("(Base)"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty());
        assert_eq!(lowered.module.python_source, "class Expr(Base):  # tpy:sealed\n    ...\n");
    }

    #[test]
    fn lower_rewrites_non_generic_overload_with_import() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("overload.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("overload def parse(x: str) -> int: ...\n"),
            },
            statements: vec![SyntaxStatement::OverloadDef(typepython_syntax::FunctionStatement {
                name: String::from("parse"),
                type_params: Vec::new(),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "from typing import overload\n@overload\ndef parse(x: str) -> int: ...\n"
        );
        assert_eq!(
            lowered.module.source_map,
            vec![SourceMapEntry {
                original_line: 1,
                lowered_line: 2,
            }]
        );
    }

    #[test]
    fn lower_still_blocks_generic_typealias() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-typealias.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("typealias Pair[T] = tuple[T, T]\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Pair"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    bound: None,
                }],
                value: String::from("tuple[T, T]"),
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(lowered.diagnostics.has_errors());
        assert!(rendered.contains("TPY2002"));
        assert!(rendered.contains("`generic typealias`"));
    }

    #[test]
    fn lower_still_blocks_generic_overload_def() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("generic-overload.tpy"),
                kind: SourceKind::TypePython,
                text: String::from("overload def parse[T](x: T) -> T: ...\n"),
            },
            statements: vec![SyntaxStatement::OverloadDef(typepython_syntax::FunctionStatement {
                name: String::from("parse"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    bound: None,
                }],
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        let rendered = lowered.diagnostics.as_text();
        assert!(lowered.diagnostics.has_errors());
        assert!(rendered.contains("TPY2002"));
        assert!(rendered.contains("`generic overload def`"));
    }

    #[test]
    fn lower_rewrites_generic_ordinary_class_and_function_headers() {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("ordinary-generics.tpy"),
                kind: SourceKind::TypePython,
                text: String::from(
                    "class Box[T](Base):\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    header_suffix: String::from("(Base)"),
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(typepython_syntax::FunctionStatement {
                    name: String::from("first"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    line: 4,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{}", lowered.module.python_source);
        assert!(lowered.diagnostics.is_empty());
        assert_eq!(
            lowered.module.python_source,
            "class Box(Base):\n    pass\n\ndef first(value: T) -> T:\n    return value\n"
        );
        assert_eq!(
            lowered.module.source_map,
            vec![
                SourceMapEntry {
                    original_line: 1,
                    lowered_line: 1,
                },
                SourceMapEntry {
                    original_line: 2,
                    lowered_line: 2,
                },
                SourceMapEntry {
                    original_line: 3,
                    lowered_line: 3,
                },
                SourceMapEntry {
                    original_line: 4,
                    lowered_line: 4,
                },
                SourceMapEntry {
                    original_line: 5,
                    lowered_line: 5,
                },
            ]
        );
    }
}
