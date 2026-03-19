//! Symbol binding boundary for TypePython.

use std::path::PathBuf;

use typepython_syntax::{SyntaxStatement, SyntaxTree};

#[derive(Debug, Clone, Default)]
pub struct BindingTable {
    pub module_path: PathBuf,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DeclarationKind {
    TypeAlias,
    Class,
    Function,
    Overload,
    Value,
    Import,
}

#[must_use]
pub fn bind(tree: &SyntaxTree) -> BindingTable {
    BindingTable {
        module_path: tree.source.path.clone(),
        declarations: tree
            .statements
            .iter()
            .flat_map(bind_statement)
            .collect(),
    }
}

fn bind_statement(statement: &SyntaxStatement) -> Vec<Declaration> {
    match statement {
        SyntaxStatement::TypeAlias(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::TypeAlias,
        }],
        SyntaxStatement::Interface(statement)
        | SyntaxStatement::DataClass(statement)
        | SyntaxStatement::SealedClass(statement)
        | SyntaxStatement::ClassDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Class,
        }],
        SyntaxStatement::OverloadDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Overload,
        }],
        SyntaxStatement::FunctionDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Function,
        }],
        SyntaxStatement::Import(statement) => statement
            .names
            .iter()
            .cloned()
            .map(|name| Declaration {
                name,
                kind: DeclarationKind::Import,
            })
            .collect(),
        SyntaxStatement::Value(statement) => statement
            .names
            .iter()
            .cloned()
            .map(|name| Declaration {
                name,
                kind: DeclarationKind::Value,
            })
            .collect(),
        SyntaxStatement::Unsafe(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Declaration, DeclarationKind, bind};
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        FunctionStatement, ImportStatement, NamedBlockStatement, SourceFile, SourceKind,
        SyntaxStatement, SyntaxTree, TypeAliasStatement, TypeParam, ValueStatement,
    };

    #[test]
    fn bind_collects_top_level_aliases_classes_and_functions() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::TypeAlias(TypeAliasStatement {
                    name: String::from("UserId"),
                    type_params: Vec::new(),
                    value: String::from("int"),
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("User"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("helper"),
                    type_params: Vec::new(),
                    line: 3,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        println!("{:?}", table.declarations);
        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                },
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                },
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                },
            ]
        );
    }

    #[test]
    fn bind_marks_overload_definitions_separately() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::OverloadDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: vec![TypeParam {
                        name: String::from("T"),
                        bound: None,
                    }],
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Overload,
                },
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                },
            ]
        );
    }

    #[test]
    fn bind_collects_imports_and_values_from_syntax_tree() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/helpers.py"),
                kind: SourceKind::Python,
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Import(ImportStatement {
                    names: vec![String::from("local_foo"), String::from("bar")],
                    line: 1,
                }),
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("value"), String::from("count")],
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("local_foo"),
                    kind: DeclarationKind::Import,
                },
                Declaration {
                    name: String::from("bar"),
                    kind: DeclarationKind::Import,
                },
                Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                },
                Declaration {
                    name: String::from("count"),
                    kind: DeclarationKind::Value,
                },
            ]
        );
    }
}
