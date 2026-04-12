use super::{
    LoweringMetadata, LoweringOptions, LoweringSegmentKind, SourceMapEntry, SpanMapEntry,
    SpanMapRange, lower, lower_with_options,
};
use std::path::PathBuf;
use typepython_diagnostics::DiagnosticReport;
use typepython_syntax::{
    ClassMember, ClassMemberKind, FunctionParam, FunctionStatement, NamedBlockStatement,
    SourceFile, SourceKind, SyntaxStatement, SyntaxTree, TypeAliasStatement, TypeParam,
    TypeParamKind, UnsafeStatement, parse,
};
use typepython_target::{EmitStyle, PythonTarget};

fn compat_options(version: &str) -> LoweringOptions {
    LoweringOptions {
        target_python: PythonTarget::parse(version).expect("test target should parse"),
        emit_style: EmitStyle::Compat,
    }
}

fn native_options(version: &str) -> LoweringOptions {
    LoweringOptions {
        target_python: PythonTarget::parse(version).expect("test target should parse"),
        emit_style: EmitStyle::Native,
    }
}

#[test]
fn lower_rewrites_top_level_unsafe_blocks() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("unsafe.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("unsafe:\n    x = 1\n"),
        },
        statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 1 })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    println!("{}", lowered.module.python_source);
    println!("OUTPUT:\n{}", lowered.module.python_source);
    println!("DIAGNOSTICS: {:?}", lowered.diagnostics);
    assert!(lowered.diagnostics.is_empty());
    assert_eq!(lowered.module.python_source, "if True:\n    x = 1\n");
    assert_eq!(
        lowered.module.source_map,
        vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
        ]
    );
}

#[test]
fn lower_rewrites_nested_unsafe_blocks_with_indentation() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("nested-unsafe.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("def update():\n    unsafe:\n        x = 1\n"),
        },
        statements: vec![SyntaxStatement::Unsafe(UnsafeStatement { line: 2 })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    eprintln!("DIAGNOSTICS: {:?}", lowered.diagnostics);
    eprintln!("OUTPUT:\n{}", lowered.module.python_source);
    assert!(lowered.diagnostics.is_empty());
    assert_eq!(lowered.module.python_source, "def update():\n    if True:\n        x = 1\n");
}

#[test]
fn lower_normalizes_annotated_lambda_runtime_syntax() {
    let tree = parse(SourceFile {
        path: PathBuf::from("lambda-annotation.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("handler = lambda (value: int): value + 1\n"),
    });

    let lowered = lower(&tree);

    assert!(lowered.diagnostics.is_empty());
    assert!(!lowered.module.python_source.contains("(value: int)"));
    assert!(lowered.module.python_source.contains("lambda"));
    assert_eq!(lowered.module.python_source, "handler = lambda  value      : value + 1\n");
}

#[test]
fn lower_strips_runtime_only_typeddict_keywords() {
    let tree = parse(SourceFile {
        path: PathBuf::from("typed-dict-runtime.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "from typing import TypedDict\n\nclass User(TypedDict, total=False, closed=True, extra_items=int):\n    name: str\n",
        ),
    });

    let lowered = lower(&tree);

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class User(TypedDict, total=False):"));
    assert!(!lowered.module.python_source.contains("closed=True"));
    assert!(!lowered.module.python_source.contains("extra_items=int"));
}

#[test]
fn lower_reports_unimplemented_typepython_constructs() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("unsupported.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("unknown feature\n"),
        },
        statements: vec![],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
}

#[test]
fn lower_rewrites_non_generic_typealias_with_import() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("typealias.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias UserId = int\n"),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("UserId"),
            type_params: Vec::new(),
            value: String::from("int"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    println!("{}", lowered.module.python_source);
    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeAlias\nUserId: TypeAlias = int\n"
    );
}

#[test]
fn lower_rewrites_non_generic_interface_with_protocol_import() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("interface.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("interface SupportsClose:\n    def close(self): ...\n"),
        },
        statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
            name: String::from("SupportsClose"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
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
            logical_module: String::new(),
            text: String::from("interface SupportsClose(Closable):\n    def close(self): ...\n"),
        },
        statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
            name: String::from("SupportsClose"),
            type_params: Vec::new(),
            header_suffix: String::from("(Closable)"),
            bases: vec![String::from("Closable")],
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import Protocol\nclass SupportsClose(Closable, Protocol):\n    def close(self): ...\n"
    );
}

#[test]
fn lower_rewrites_generic_interface_with_protocol_and_generic_base() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("generic-interface.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "interface SupportsClose[T]:\n    def close(self, value: T) -> T: ...\n",
            ),
        },
        statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
            name: String::from("SupportsClose"),
            type_params: vec![TypeParam {
                name: String::from("T"),
                kind: TypeParamKind::TypeVar,
                bound: None,
                constraints: Vec::new(),
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            }],
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\")\nfrom typing import Protocol\nclass SupportsClose(Protocol, Generic[T]):\n    def close(self, value: T) -> T: ...\n"
    );
}

#[test]
fn lower_rewrites_non_generic_data_class_with_dataclass_import() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("data-class.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("data class Point:\n    x: float\n    y: float\n"),
        },
        statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
            name: String::from("Point"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
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
            SourceMapEntry { original_line: 1, lowered_line: 2 },
            SourceMapEntry { original_line: 2, lowered_line: 4 },
            SourceMapEntry { original_line: 3, lowered_line: 5 },
        ]
    );
}

#[test]
fn lower_rewrites_data_class_with_existing_bases() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("data-class-bases.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("data class Point(Base):\n    x: float\n"),
        },
        statements: vec![SyntaxStatement::DataClass(NamedBlockStatement {
            name: String::from("Point"),
            type_params: Vec::new(),
            header_suffix: String::from("(Base)"),
            bases: vec![String::from("Base")],
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from dataclasses import dataclass\n@dataclass\nclass Point(Base):\n    x: float\n"
    );
}

#[test]
fn lower_rewrites_generic_data_class_and_sealed_class_with_generic_base() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("generic-classlikes.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "data class Point[T]:\n    x: T\n\nsealed class Expr[T](Base):\n    ...\n",
            ),
        },
        statements: vec![
            SyntaxStatement::DataClass(NamedBlockStatement {
                name: String::from("Point"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            }),
            SyntaxStatement::SealedClass(NamedBlockStatement {
                name: String::from("Expr"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                header_suffix: String::from("(Base)"),
                bases: vec![String::from("Base")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\")\nfrom dataclasses import dataclass\n@dataclass\nclass Point(Generic[T]):\n    x: T\n\nclass Expr(Base, Generic[T]):  # tpy:sealed\n    ...\n"
    );
}

#[test]
fn lower_rewrites_non_generic_sealed_class_with_marker() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("sealed-class.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("sealed class Expr:\n    ...\n"),
        },
        statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
            name: String::from("Expr"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    println!("{}", lowered.module.python_source);
    assert!(lowered.diagnostics.is_empty());
    assert_eq!(lowered.module.python_source, "class Expr:  # tpy:sealed\n    ...\n");
}

#[test]
fn lower_normalizes_intrinsic_boundary_types_for_runtime_output() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("intrinsics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "typealias Alias = dynamic\n\ndef take(value: unknown, other: dynamic) -> unknown:\n    return value\n",
            ),
        },
        statements: vec![
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Alias"),
                type_params: Vec::new(),
                value: String::from("dynamic"),
                value_expr: None,
                line: 1,
            }),
            SyntaxStatement::FunctionDef(FunctionStatement {
                name: String::from("take"),
                type_params: Vec::new(),
                params: vec![
                    FunctionParam {
                        name: String::from("value"),
                        annotation: Some(String::from("unknown")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                    FunctionParam {
                        name: String::from("other"),
                        annotation: Some(String::from("dynamic")),
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    },
                ],
                returns: Some(String::from("unknown")),
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 3,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("from typing import Any"));
    assert!(lowered.module.python_source.contains("Alias: TypeAlias = Any"));
    assert!(
        lowered.module.python_source.contains("def take(value: object, other: Any) -> object:")
    );
    assert!(!lowered.module.python_source.contains("unknown"));
    assert!(!lowered.module.python_source.contains("dynamic"));
}

#[test]
fn lower_rewrites_sealed_class_with_existing_bases() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("sealed-class-bases.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("sealed class Expr(Base):\n    ...\n"),
        },
        statements: vec![SyntaxStatement::SealedClass(NamedBlockStatement {
            name: String::from("Expr"),
            type_params: Vec::new(),
            header_suffix: String::from("(Base)"),
            bases: vec![String::from("Base")],
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: Vec::new(),
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
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
            logical_module: String::new(),
            text: String::from("overload def parse(x: str) -> int: ...\n"),
        },
        statements: vec![SyntaxStatement::OverloadDef(typepython_syntax::FunctionStatement {
            name: String::from("parse"),
            type_params: Vec::new(),
            params: Vec::new(),
            returns: None,
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
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
        vec![SourceMapEntry { original_line: 1, lowered_line: 2 }]
    );
    assert_eq!(
        lowered.module.span_map,
        vec![
            SpanMapEntry {
                source_path: PathBuf::from("overload.tpy"),
                emitted_path: PathBuf::from("overload.py"),
                original: SpanMapRange { line: 0, start_col: 0, end_col: 0 },
                emitted: SpanMapRange { line: 1, start_col: 1, end_col: 28 },
                kind: LoweringSegmentKind::Inserted,
            },
            SpanMapEntry {
                source_path: PathBuf::from("overload.tpy"),
                emitted_path: PathBuf::from("overload.py"),
                original: SpanMapRange { line: 1, start_col: 1, end_col: 39 },
                emitted: SpanMapRange { line: 2, start_col: 1, end_col: 10 },
                kind: LoweringSegmentKind::Rewritten,
            },
            SpanMapEntry {
                source_path: PathBuf::from("overload.tpy"),
                emitted_path: PathBuf::from("overload.py"),
                original: SpanMapRange { line: 1, start_col: 1, end_col: 39 },
                emitted: SpanMapRange { line: 3, start_col: 1, end_col: 30 },
                kind: LoweringSegmentKind::Rewritten,
            },
        ]
    );
    assert_eq!(lowered.module.required_imports, vec![String::from("from typing import overload")]);
    assert_eq!(
        lowered.module.metadata,
        LoweringMetadata {
            has_generic_type_params: false,
            has_typed_dict_transforms: false,
            has_sealed_classes: false,
        }
    );
}

#[test]
fn lower_still_blocks_generic_typealias() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("generic-typealias.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias Pair[T] = tuple[T, T]\n"),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("Pair"),
            type_params: vec![TypeParam {
                name: String::from("T"),
                kind: TypeParamKind::TypeVar,
                bound: None,
                constraints: Vec::new(),
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            }],
            value: String::from("tuple[T, T]"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nT = TypeVar(\"T\")\nfrom typing import TypeAlias\nPair: TypeAlias = tuple[T, T]\n"
    );
}

#[test]
fn lower_rewrites_type_param_constraints_and_defaults() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("generic-default-typealias.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias Pair[T: (str, bytes) = str] = tuple[T, T]\n"),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("Pair"),
            type_params: vec![TypeParam {
                name: String::from("T"),
                kind: TypeParamKind::TypeVar,
                bound: None,
                constraints: vec![String::from("str"), String::from("bytes")],
                default: Some(String::from("str")),
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            }],
            value: String::from("tuple[T, T]"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing_extensions import TypeVar\nT = TypeVar(\"T\", \"str\", \"bytes\", default=\"str\")\nfrom typing import TypeAlias\nPair: TypeAlias = tuple[T, T]\n"
    );
    assert_eq!(
        lowered.module.required_imports,
        vec![
            String::from("from typing_extensions import TypeVar"),
            String::from("from typing import TypeAlias"),
        ]
    );
}

#[test]
fn lower_still_blocks_generic_overload_def() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("generic-overload.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("overload def parse[T](x: T) -> T: ...\n"),
        },
        statements: vec![SyntaxStatement::OverloadDef(typepython_syntax::FunctionStatement {
            name: String::from("parse"),
            type_params: vec![TypeParam {
                name: String::from("T"),
                kind: TypeParamKind::TypeVar,
                bound: None,
                constraints: Vec::new(),
                default: None,
                bound_expr: None,
                constraint_exprs: Vec::new(),
                default_expr: None,
            }],
            params: Vec::new(),
            returns: None,
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nT = TypeVar(\"T\")\nfrom typing import overload\n@overload\ndef parse(x: T) -> T: ...\n"
    );
}

#[test]
fn lower_rewrites_generic_ordinary_class_and_function_headers() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("ordinary-generics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Box[T](Base):\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Box"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                header_suffix: String::from("(Base)"),
                bases: vec![String::from("Base")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            }),
            SyntaxStatement::FunctionDef(typepython_syntax::FunctionStatement {
                name: String::from("first"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                params: Vec::new(),
                returns: None,
                returns_expr: None,
                is_async: false,
                is_override: false,
                is_deprecated: false,
                deprecation_message: None,
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    println!("{}", lowered.module.python_source);
    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\")\nclass Box(Base, Generic[T]):\n    pass\n\ndef first(value: T) -> T:\n    return value\n"
    );
    assert_eq!(
        lowered.module.source_map,
        vec![
            SourceMapEntry { original_line: 1, lowered_line: 4 },
            SourceMapEntry { original_line: 2, lowered_line: 5 },
            SourceMapEntry { original_line: 3, lowered_line: 6 },
            SourceMapEntry { original_line: 4, lowered_line: 7 },
            SourceMapEntry { original_line: 5, lowered_line: 8 },
        ]
    );
}

#[test]
fn lower_native_mode_preserves_pep_695_syntax() {
    let lowered = lower_with_options(
        &parse(SourceFile {
            path: PathBuf::from("native-generics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "typealias Pair[T] = tuple[T, T]\n\nclass Box[T](Base):\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
            ),
        }),
        &native_options("3.13"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "type Pair[T] = tuple[T, T]\n\nclass Box[T](Base):\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n"
    );
    assert!(lowered.module.required_imports.is_empty());
}

#[test]
fn lower_native_mode_falls_back_for_generic_defaults_before_313() {
    let lowered = lower_with_options(
        &parse(SourceFile {
            path: PathBuf::from("native-default-fallback.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "typealias Pair[T = int] = tuple[T, T]\n\ndef first[T = int](value: T = 1) -> T:\n    return value\n",
            ),
        }),
        &native_options("3.12"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("TypeVar(\"T\", default=\"int\")"));
    assert!(lowered.module.python_source.contains("Pair: TypeAlias = tuple[T, T]"));
    assert!(!lowered.module.python_source.contains("type Pair[T = int]"));
    assert!(!lowered.module.python_source.contains("def first[T = int]"));
}

#[test]
fn lower_rewrites_paramspec_headers() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("paramspec.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "def invoke[**P, R](cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n    return cb(*args, **kwargs)\n",
            ),
        },
        statements: vec![SyntaxStatement::FunctionDef(typepython_syntax::FunctionStatement {
            name: String::from("invoke"),
            type_params: vec![
                TypeParam {
                    name: String::from("P"),
                    kind: TypeParamKind::ParamSpec,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                },
                TypeParam {
                    name: String::from("R"),
                    kind: TypeParamKind::TypeVar,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                },
            ],
            params: Vec::new(),
            returns: None,
            returns_expr: None,
            is_async: false,
            is_override: false,
            is_deprecated: false,
            deprecation_message: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nfrom typing import ParamSpec\nP = ParamSpec(\"P\")\nR = TypeVar(\"R\")\ndef invoke(cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n    return cb(*args, **kwargs)\n"
    );
}

#[test]
fn lower_rewrites_typevartuple_helpers_for_target_python_310() {
    let lowered = lower_with_options(
        &SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("typevartuple-310.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("typealias Pack = tuple[Unpack[Ts]]\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Pack"),
                type_params: vec![TypeParam {
                    name: String::from("Ts"),
                    kind: TypeParamKind::TypeVarTuple,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                value: String::from("tuple[Unpack[Ts]]"),
                value_expr: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        },
        &compat_options("3.10"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing_extensions import TypeVarTuple\nfrom typing_extensions import Unpack\nTs = TypeVarTuple(\"Ts\")\nfrom typing import TypeAlias\nPack: TypeAlias = tuple[Unpack[Ts]]\n"
    );
    assert_eq!(
        lowered.module.required_imports,
        vec![
            String::from("from typing_extensions import TypeVarTuple"),
            String::from("from typing_extensions import Unpack"),
            String::from("from typing import TypeAlias"),
        ]
    );
}

#[test]
fn lower_rewrites_typevartuple_helpers_for_target_python_311() {
    let lowered = lower_with_options(
        &SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("typevartuple-311.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from("typealias Pack = tuple[Unpack[Ts]]\n"),
            },
            statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("Pack"),
                type_params: vec![TypeParam {
                    name: String::from("Ts"),
                    kind: TypeParamKind::TypeVarTuple,
                    bound: None,
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                value: String::from("tuple[Unpack[Ts]]"),
                value_expr: None,
                line: 1,
            })],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        },
        &compat_options("3.11"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVarTuple\nfrom typing import Unpack\nTs = TypeVarTuple(\"Ts\")\nfrom typing import TypeAlias\nPack: TypeAlias = tuple[Unpack[Ts]]\n"
    );
    assert_eq!(
        lowered.module.required_imports,
        vec![
            String::from("from typing import TypeVarTuple"),
            String::from("from typing import Unpack"),
            String::from("from typing import TypeAlias"),
        ]
    );
}

#[test]
fn lower_rewrites_source_authored_typevartuple_headers() {
    let lowered = lower(&parse(SourceFile {
        path: PathBuf::from("source-typevartuple.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("def collect[*Ts](*args: *Ts) -> tuple[*Ts]:\n    return args\n"),
    }));

    assert!(lowered.diagnostics.is_empty(), "{}", lowered.diagnostics.as_text());
    assert_eq!(
        lowered.module.python_source,
        "from typing_extensions import TypeVarTuple\nfrom typing_extensions import Unpack\nTs = TypeVarTuple(\"Ts\")\ndef collect(*args: Unpack[Ts]) -> tuple[Unpack[Ts]]:\n    return args\n"
    );
}

#[test]
fn lower_rewrites_source_authored_typevartuple_typealias() {
    let lowered = lower(&parse(SourceFile {
        path: PathBuf::from("source-typevartuple-alias.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("typealias Pack[*Ts] = tuple[*Ts]\n"),
    }));

    assert!(lowered.diagnostics.is_empty(), "{}", lowered.diagnostics.as_text());
    assert_eq!(
        lowered.module.python_source,
        "from typing_extensions import TypeVarTuple\nfrom typing_extensions import Unpack\nTs = TypeVarTuple(\"Ts\")\nfrom typing import TypeAlias\nPack: TypeAlias = tuple[Unpack[Ts]]\n"
    );
}

#[test]
fn lower_preserves_runtime_star_unpack_expressions() {
    let lowered = lower(&parse(SourceFile {
        path: PathBuf::from("runtime-star-unpack.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("def build(items: list[int]) -> list[int]:\n    return [*items]\n"),
    }));

    assert!(lowered.diagnostics.is_empty(), "{}", lowered.diagnostics.as_text());
    assert!(lowered.module.python_source.contains("return [*items]"));
}

#[test]
fn lower_quotes_hoisted_type_param_bounds_for_runtime_imports() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("bounded-generic.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "interface Serializable:\n    def to_json(self) -> str: ...\n\nclass Box[T: Serializable]:\n    pass\n",
            ),
        },
        statements: vec![
            SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("Serializable"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 1,
            }),
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Box"),
                type_params: vec![TypeParam {
                    name: String::from("T"),
                    kind: TypeParamKind::TypeVar,
                    bound: Some(String::from("Serializable")),
                    constraints: Vec::new(),
                    default: None,
                    bound_expr: None,
                    constraint_exprs: Vec::new(),
                    default_expr: None,
                }],
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: Vec::new(),
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert_eq!(
        lowered.module.python_source,
        "from typing import TypeVar\nfrom typing import Generic\nT = TypeVar(\"T\", bound=\"Serializable\")\nfrom typing import Protocol\nclass Serializable(Protocol):\n    def to_json(self) -> str: ...\n\nclass Box(Generic[T]):\n    pass\n"
    );
}

// ─── TypedDict utility transform tests ───────────────────────────────────

#[test]
fn lower_expands_partial_typeddict_transform() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("partial.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserCreate = Partial[User]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("id"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("name"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                ],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserCreate"),
                type_params: Vec::new(),
                value: String::from("Partial[User]"),
                value_expr: None,
                line: 5,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("# tpy:derived Partial[User]"));
    assert!(lowered.module.python_source.contains("class UserCreate(TypedDict):"));
    assert!(lowered.module.python_source.contains("id: NotRequired[int]"));
    assert!(lowered.module.python_source.contains("name: NotRequired[str]"));
    assert!(lowered.module.python_source.contains("from typing_extensions import NotRequired"));
    assert_eq!(
        lowered.module.required_imports,
        vec![
            String::from("from typing import TypeAlias"),
            String::from("from typing_extensions import NotRequired"),
        ]
    );
    assert!(lowered.module.metadata.has_typed_dict_transforms);
}

#[test]
fn lower_prefers_typing_notrequired_for_target_python_311() {
    let lowered = lower_with_options(
        &SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("partial-311.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: String::from(
                    "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserUpdate = Partial[User]\n",
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(TypedDict)"),
                    bases: vec![String::from("TypedDict")],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("id"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("int")),
                            annotation_expr: None,
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            annotation_expr: None,
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 3,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserUpdate"),
                    type_params: Vec::new(),
                    value: String::from("Partial[User]"),
                    value_expr: None,
                    line: 5,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        },
        &compat_options("3.11"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("from typing import NotRequired"));
    assert!(!lowered.module.python_source.contains("from typing_extensions import NotRequired"));
    assert_eq!(
        lowered.module.required_imports,
        vec![
            String::from("from typing import TypeAlias"),
            String::from("from typing import NotRequired"),
        ]
    );
    assert!(lowered.module.metadata.has_typed_dict_transforms);
}

#[test]
fn lower_rewrites_compat_qualified_names_for_target_python_310() {
    let lowered = lower_with_options(
        &parse(SourceFile {
            path: PathBuf::from("compat-qualified.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "import typing\nimport warnings\n\n@warnings.deprecated(\"use new_api\")\nclass Box:\n    @typing.override\n    def clone(self) -> typing.Self:\n        ...\n\nclass Config(typing.TypedDict):\n    flag: typing.ReadOnly[bool]\n\ndef accepts(value: object) -> typing.TypeIs[int]:\n    ...\n",
            ),
        }),
        &compat_options("3.10"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert!(
        lowered.module.python_source.contains("@typing_extensions.deprecated(\"use new_api\")")
    );
    assert!(lowered.module.python_source.contains("import typing_extensions"));
    assert!(lowered.module.python_source.contains("@typing_extensions.override"));
    assert!(lowered.module.python_source.contains("-> typing_extensions.Self"));
    assert!(lowered.module.python_source.contains("typing_extensions.ReadOnly[bool]"));
    assert!(lowered.module.python_source.contains("-> typing_extensions.TypeIs[int]"));
}

#[test]
fn lower_rewrites_compat_import_sources_for_target_python_312() {
    let lowered = lower_with_options(
        &parse(SourceFile {
            path: PathBuf::from("compat-imports.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "from typing_extensions import Self, Required, NotRequired, dataclass_transform, override, ReadOnly, TypeIs\nfrom warnings import deprecated\n\n@deprecated(\"use new_api\")\nclass Box:\n    @override\n    def clone(self) -> Self:\n        ...\n",
            ),
        }),
        &compat_options("3.12"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert!(
        lowered.module.python_source.contains(
            "from typing import Self, Required, NotRequired, dataclass_transform, override"
        )
    );
    assert!(
        lowered.module.python_source.contains("from typing_extensions import ReadOnly, TypeIs")
    );
    assert!(lowered.module.python_source.contains("from typing_extensions import deprecated"));
    assert!(!lowered.module.python_source.contains("from warnings import deprecated"));
}

#[test]
fn lower_rewrites_variadic_generic_compat_import_sources_for_target_python_310() {
    let lowered = lower_with_options(
        &parse(SourceFile {
            path: PathBuf::from("compat-variadics.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "from typing import TypeVarTuple, Unpack\nTs = TypeVarTuple(\"Ts\")\nvalue: typing.Unpack[Ts]\n",
            ),
        }),
        &compat_options("3.10"),
    );

    assert!(lowered.diagnostics.is_empty());
    assert!(
        lowered.module.python_source.contains("from typing_extensions import TypeVarTuple, Unpack")
    );
    assert!(lowered.module.python_source.contains("value: typing_extensions.Unpack[Ts]"));
}

#[test]
fn lower_expands_partial_typeddict_transform_for_qualified_bases() {
    for base in ["typing.TypedDict", "typing_extensions.TypedDict"] {
        let lowered = lower(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("partial-qualified.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::new(),
                text: format!(
                    "class User({base}):\n    id: int\n    name: str\n\ntypealias UserCreate = Partial[User]\n"
                ),
            },
            statements: vec![
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: format!("({base})"),
                    bases: vec![String::from(base)],
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_abstract_class: false,
                    members: vec![
                        ClassMember {
                            name: String::from("id"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("int")),
                            annotation_expr: None,
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 2,
                        },
                        ClassMember {
                            name: String::from("name"),
                            kind: ClassMemberKind::Field,
                            method_kind: None,
                            annotation: Some(String::from("str")),
                            annotation_expr: None,
                            value_type: None,
                            params: Vec::new(),
                            returns: None,
                            returns_expr: None,
                            is_async: false,
                            is_override: false,
                            is_abstract_method: false,
                            is_final_decorator: false,
                            is_deprecated: false,
                            deprecation_message: None,
                            is_final: false,
                            is_class_var: false,
                            line: 3,
                        },
                    ],
                    line: 1,
                }),
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserCreate"),
                    type_params: Vec::new(),
                    value: String::from("Partial[User]"),
                    value_expr: None,
                    line: 5,
                }),
            ],
            type_ignore_directives: Vec::new(),
            diagnostics: DiagnosticReport::default(),
        });

        assert!(lowered.diagnostics.is_empty(), "{}", lowered.diagnostics.as_text());
        assert!(lowered.module.python_source.contains("class UserCreate(TypedDict):"));
        assert!(lowered.module.python_source.contains("id: NotRequired[int]"));
        assert!(lowered.module.python_source.contains("name: NotRequired[str]"));
    }
}

#[test]
fn lower_expands_pick_typeddict_transform() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("pick.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class User(TypedDict):\n    id: int\n    name: str\n    email: str\n\ntypealias UserPublic = Pick[User, \"id\", \"name\"]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("id"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("name"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("email"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserPublic"),
                type_params: Vec::new(),
                value: String::from("Pick[User, \"id\", \"name\"]"),
                value_expr: None,
                line: 6,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    println!(
        "OUTPUT:
{}",
        lowered.module.python_source
    );
    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class UserPublic(TypedDict):"));
    assert!(lowered.module.python_source.contains("id: int"));
    assert!(lowered.module.python_source.contains("name: str"));
    // email should NOT appear in the UserPublic transform (it's in the original User class)
    let all_lines: Vec<_> = lowered.module.python_source.lines().collect();
    let user_public_start = all_lines
        .iter()
        .position(|l| l.contains("class UserPublic"))
        .expect("UserPublic class should be emitted");
    let mut section = String::new();
    for l in &all_lines[user_public_start..] {
        if l.trim().is_empty() || l.trim().starts_with("class ") {
            break;
        }
        section.push_str(l);
        section.push('\n');
    }
    assert!(!section.contains("email"), "email should not appear in UserPublic Pick transform");
}

#[test]
fn lower_expands_omit_typeddict_transform() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("omit.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserUpdate = Omit[User, \"id\"]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("id"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("name"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                ],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserUpdate"),
                type_params: Vec::new(),
                value: String::from("Omit[User, \"id\"]"),
                value_expr: None,
                line: 5,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class UserUpdate(TypedDict):"));
    assert!(lowered.module.python_source.contains("name: str"));
    // id should NOT appear in the UserUpdate transform (it's in the original User class)
    let all_lines: Vec<_> = lowered.module.python_source.lines().collect();
    let user_update_start = all_lines
        .iter()
        .position(|l| l.contains("class UserUpdate"))
        .expect("UserUpdate class should be emitted");
    let mut section = String::new();
    for l in &all_lines[user_update_start..] {
        if l.trim().is_empty() || l.trim().starts_with("class ") {
            break;
        }
        section.push_str(l);
        section.push('\n');
    }
    assert!(!section.contains("id:"), "id should not appear in UserUpdate Omit transform");
}

#[test]
fn lower_expands_readonly_typeddict_transform() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("readonly.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Config(TypedDict):\n    debug: bool\n\ntypealias ImmutableConfig = Readonly[Config]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Config"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![ClassMember {
                    name: String::from("debug"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("bool")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("ImmutableConfig"),
                type_params: Vec::new(),
                value: String::from("Readonly[Config]"),
                value_expr: None,
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class ImmutableConfig(TypedDict):"));
    assert!(lowered.module.python_source.contains("debug: ReadOnly[bool]"));
    assert!(lowered.module.python_source.contains("from typing_extensions import ReadOnly"));
}

#[test]
fn lower_expands_required_typeddict_transform() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("required.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class UserUpdate(TypedDict):\n    name: NotRequired[str]\n\ntypealias RequiredUpdate = Required_[UserUpdate]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("UserUpdate"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![ClassMember {
                    name: String::from("name"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("NotRequired[str]")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("RequiredUpdate"),
                type_params: Vec::new(),
                value: String::from("Required_[UserUpdate]"),
                value_expr: None,
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class RequiredUpdate(TypedDict):"));
    assert!(lowered.module.python_source.contains("name: str"));
}

#[test]
fn lower_expands_required_typeddict_transform_with_nested_annotation() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("required-nested.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class UserUpdate(TypedDict):\n    value: NotRequired[list[int]]\n\ntypealias RequiredUpdate = Required_[UserUpdate]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("UserUpdate"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![ClassMember {
                    name: String::from("value"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("NotRequired[list[int]]")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("RequiredUpdate"),
                type_params: Vec::new(),
                value: String::from("Required_[UserUpdate]"),
                value_expr: None,
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class RequiredUpdate(TypedDict):"));
    assert!(lowered.module.python_source.contains("value: list[int]"));
}

#[test]
fn lower_expands_composed_typeddict_transform() {
    // Partial[Omit[User, "id"]]: Omit removes "id", Partial makes rest optional
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("composed.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class User(TypedDict):\n    id: int\n    name: str\n\ntypealias UserUpdate = Partial[Omit[User, \"id\"]]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("id"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("int")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("name"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: Some(String::from("str")),
                        annotation_expr: None,
                        value_type: None,
                        params: Vec::new(),
                        returns: None,
                        returns_expr: None,
                        is_async: false,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_deprecated: false,
                        deprecation_message: None,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                ],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserUpdate"),
                type_params: Vec::new(),
                value: String::from("Partial[Omit[User, \"id\"]]"),
                value_expr: None,
                line: 5,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class UserUpdate(TypedDict):"));
    // Omit removes id, then Partial makes name optional
    assert!(lowered.module.python_source.contains("name: NotRequired[str]"));
    // id should NOT appear in the UserUpdate transform
    let all_lines: Vec<_> = lowered.module.python_source.lines().collect();
    let user_update_start = all_lines
        .iter()
        .position(|l| l.contains("class UserUpdate"))
        .expect("UserUpdate class should be emitted");
    let mut section = String::new();
    for l in &all_lines[user_update_start..] {
        if l.trim().is_empty() || l.trim().starts_with("class ") {
            break;
        }
        section.push_str(l);
        section.push('\n');
    }
    assert!(!section.contains("id:"), "id should not appear in composed transform");
}

#[test]
fn lower_expands_mutable_typeddict_transform() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("mutable.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class Config(TypedDict):\n    debug: ReadOnly[bool]\n\ntypealias MutableConfig = Mutable[Config]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("Config"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![ClassMember {
                    name: String::from("debug"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("ReadOnly[bool]")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("MutableConfig"),
                type_params: Vec::new(),
                value: String::from("Mutable[Config]"),
                value_expr: None,
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("class MutableConfig(TypedDict):"));
    // ReadOnly wrapper should be stripped
    assert!(lowered.module.python_source.contains("debug: bool"));
}

#[test]
fn lower_keeps_decorated_class_header_singleton() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("decorated-class.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("@model\nclass User:\n    name: str\n    age: int\n"),
        },
        statements: vec![SyntaxStatement::ClassDef(NamedBlockStatement {
            name: String::from("User"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: vec![
                ClassMember {
                    name: String::from("name"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 3,
                },
                ClassMember {
                    name: String::from("age"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 4,
                },
            ],
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    let lines = lowered.module.python_source.lines().collect::<Vec<_>>();
    assert_eq!(lines, vec!["@model", "class User:", "    name: str", "    age: int"]);
}

#[test]
fn lower_reports_unknown_pick_key_as_tpy4017() {
    let root = std::env::temp_dir().join(format!(
        "typepython-lowering-pick-invalid-key-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("temp lowering test directory should be created");
    let source_path = root.join("pick-invalid-key.tpy");
    std::fs::write(
        &source_path,
        "class User(TypedDict):\n    id: int\n\ntypealias UserPublic = Pick[User, \"name\"]\n",
    )
    .expect("temp lowering source should be written");
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: source_path.clone(),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from(
                "class User(TypedDict):\n    id: int\n\ntypealias UserPublic = Pick[User, \"name\"]\n",
            ),
        },
        statements: vec![
            SyntaxStatement::ClassDef(NamedBlockStatement {
                name: String::from("User"),
                type_params: Vec::new(),
                header_suffix: String::from("(TypedDict)"),
                bases: vec![String::from("TypedDict")],
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_abstract_class: false,
                members: vec![ClassMember {
                    name: String::from("id"),
                    kind: ClassMemberKind::Field,
                    method_kind: None,
                    annotation: Some(String::from("int")),
                    annotation_expr: None,
                    value_type: None,
                    params: Vec::new(),
                    returns: None,
                    returns_expr: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    line: 2,
                }],
                line: 1,
            }),
            SyntaxStatement::TypeAlias(TypeAliasStatement {
                name: String::from("UserPublic"),
                type_params: Vec::new(),
                value: String::from("Pick[User, \"name\"]"),
                value_expr: None,
                line: 4,
            }),
        ],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });
    let _ = std::fs::remove_dir_all(&root);

    let rendered = lowered.diagnostics.as_text();
    assert!(rendered.contains("TPY4017"));
    assert!(rendered.contains("unknown key `name`"));
    let diagnostic = lowered
        .diagnostics
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPY4017")
        .expect("unknown transform key diagnostic should be present");
    assert_eq!(diagnostic.suggestions.len(), 1);
    assert!(diagnostic.suggestions[0].message.contains("Replace `name` with `id`"));
    assert_eq!(diagnostic.suggestions[0].replacement, "\"id\"");
}

#[test]
fn lower_reports_non_typeddict_transform_target_as_tpy4017() {
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("pick-invalid-target.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias UserPublic = Pick[Config, \"name\"]\n"),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("UserPublic"),
            type_params: Vec::new(),
            value: String::from("Pick[Config, \"name\"]"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    let rendered = lowered.diagnostics.as_text();
    assert!(rendered.contains("TPY4017"));
    assert!(rendered.contains("not a known TypedDict"));
}

#[test]
fn lower_non_transform_typealias_unchanged() {
    // Regular type alias (not a transform) should still work as before
    let lowered = lower(&SyntaxTree {
        source: SourceFile {
            path: PathBuf::from("regular.tpy"),
            kind: SourceKind::TypePython,
            logical_module: String::new(),
            text: String::from("typealias UserId = int\n"),
        },
        statements: vec![SyntaxStatement::TypeAlias(TypeAliasStatement {
            name: String::from("UserId"),
            type_params: Vec::new(),
            value: String::from("int"),
            value_expr: None,
            line: 1,
        })],
        type_ignore_directives: Vec::new(),
        diagnostics: DiagnosticReport::default(),
    });

    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.module.python_source.contains("from typing import TypeAlias"));
    assert!(lowered.module.python_source.contains("UserId: TypeAlias = int"));
}

// ─── Snapshot (golden) tests ────────────────────────────────────────────

#[test]
fn snapshot_lower_typealias() {
    let tree = parse(SourceFile {
        path: PathBuf::from("typealias.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("typealias UserId = int\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_generic_typealias() {
    let tree = parse(SourceFile {
        path: PathBuf::from("generic-typealias.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("typealias Pair[T] = tuple[T, T]\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_interface() {
    let tree = parse(SourceFile {
        path: PathBuf::from("interface.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("interface Closable:\n    def close(self) -> None: ...\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_interface_with_bases() {
    let tree = parse(SourceFile {
        path: PathBuf::from("interface-bases.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("interface SupportsClose(Closable):\n    def close(self): ...\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_generic_interface() {
    let tree = parse(SourceFile {
        path: PathBuf::from("generic-interface.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "interface SupportsClose[T]:\n    def close(self, value: T) -> T: ...\n",
        ),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_data_class() {
    let tree = parse(SourceFile {
        path: PathBuf::from("data-class.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("data class Point:\n    x: float\n    y: float\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_data_class_with_bases() {
    let tree = parse(SourceFile {
        path: PathBuf::from("data-class-bases.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("data class Point(Base):\n    x: float\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_sealed_class() {
    let tree = parse(SourceFile {
        path: PathBuf::from("sealed-class.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("sealed class Expr:\n    ...\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_sealed_class_with_bases() {
    let tree = parse(SourceFile {
        path: PathBuf::from("sealed-class-bases.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("sealed class Expr(Base):\n    ...\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_overload_def() {
    let tree = parse(SourceFile {
        path: PathBuf::from("overload.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("overload def parse(x: str) -> int: ...\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_unsafe_block() {
    let tree = parse(SourceFile {
        path: PathBuf::from("unsafe.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("unsafe:\n    x = eval('1')\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_nested_unsafe_in_function() {
    let tree = parse(SourceFile {
        path: PathBuf::from("nested-unsafe.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("def update():\n    unsafe:\n        x = eval('1')\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_lambda_annotation() {
    let tree = parse(SourceFile {
        path: PathBuf::from("lambda-annotation.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("handler = lambda (value: int): value + 1\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_typeddict_keyword_stripping() {
    let tree = parse(SourceFile {
        path: PathBuf::from("typed-dict-runtime.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "from typing import TypedDict\n\nclass User(TypedDict, total=False, closed=True, extra_items=int):\n    name: str\n",
        ),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_generic_class_and_function() {
    let tree = parse(SourceFile {
        path: PathBuf::from("ordinary-generics.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "class Box[T](Base):\n    pass\n\ndef first[T](value: T) -> T:\n    return value\n",
        ),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_paramspec() {
    let tree = parse(SourceFile {
        path: PathBuf::from("paramspec.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "def invoke[**P, R](cb: Callable[P, R], *args: P.args, **kwargs: P.kwargs) -> R:\n    return cb(*args, **kwargs)\n",
        ),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_type_param_with_bounds_constraints_defaults() {
    let tree = parse(SourceFile {
        path: PathBuf::from("generic-default-typealias.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from("typealias Pair[T: (str, bytes) = str] = tuple[T, T]\n"),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_combined_typepython_constructs() {
    let tree = parse(SourceFile {
        path: PathBuf::from("combined.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "typealias UserId = int\n\ninterface Closable:\n    def close(self) -> None: ...\n\ndata class Point:\n    x: float\n    y: float\n\nsealed class Expr:\n    ...\n\noverload def parse(x: str) -> int: ...\n\ndef helper() -> int:\n    return 1\n",
        ),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_compat_qualified_names_310() {
    let tree = parse(SourceFile {
        path: PathBuf::from("compat-qualified.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "import typing\nimport warnings\n\n@warnings.deprecated(\"use new_api\")\nclass Box:\n    @typing.override\n    def clone(self) -> typing.Self:\n        ...\n",
        ),
    });
    let lowered =
        lower_with_options(&tree, &compat_options("3.10"));
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_compat_imports_312() {
    let tree = parse(SourceFile {
        path: PathBuf::from("compat-imports.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: String::from(
            "from typing_extensions import Self, Required, NotRequired, dataclass_transform, override, ReadOnly, TypeIs\nfrom warnings import deprecated\n\n@deprecated(\"use new_api\")\nclass Box:\n    @override\n    def clone(self) -> Self:\n        ...\n",
        ),
    });
    let lowered =
        lower_with_options(&tree, &compat_options("3.12"));
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}

#[test]
fn snapshot_lower_passthrough_python_source() {
    let tree = parse(SourceFile {
        path: PathBuf::from("passthrough.py"),
        kind: SourceKind::Python,
        logical_module: String::new(),
        text: String::from(
            "def hello(name: str) -> str:\n    return f\"Hello, {name}\"\n\nclass User:\n    name: str\n",
        ),
    });
    let lowered = lower(&tree);
    assert!(lowered.diagnostics.is_empty());
    insta::assert_snapshot!(lowered.module.python_source);
}
