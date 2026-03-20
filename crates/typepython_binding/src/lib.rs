//! Symbol binding boundary for TypePython.

use std::path::PathBuf;

use typepython_syntax::{SourceKind, SyntaxStatement, SyntaxTree};

#[derive(Debug, Clone)]
pub struct BindingTable {
    pub module_path: PathBuf,
    pub module_kind: SourceKind,
    pub declarations: Vec<Declaration>,
    pub calls: Vec<String>,
}

impl Default for BindingTable {
    fn default() -> Self {
        Self {
            module_path: PathBuf::new(),
            module_kind: SourceKind::TypePython,
            declarations: Vec::new(),
            calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub class_kind: Option<DeclarationOwnerKind>,
    pub owner: Option<DeclarationOwner>,
    pub is_override: bool,
    pub is_abstract_method: bool,
    pub is_final: bool,
    pub is_class_var: bool,
    pub bases: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DeclarationOwner {
    pub name: String,
    pub kind: DeclarationOwnerKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum DeclarationOwnerKind {
    Class,
    Interface,
    DataClass,
    SealedClass,
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
        module_kind: tree.source.kind,
        declarations: tree
            .statements
            .iter()
            .flat_map(bind_statement)
            .collect(),
        calls: tree
            .statements
            .iter()
            .filter_map(|statement| match statement {
                SyntaxStatement::Call(statement) => Some(statement.callee.clone()),
                _ => None,
            })
            .collect(),
    }
}

fn bind_statement(statement: &SyntaxStatement) -> Vec<Declaration> {
    match statement {
        SyntaxStatement::TypeAlias(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::TypeAlias,
            class_kind: None,
            owner: None,
            is_override: false,
            is_abstract_method: false,
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
        }],
        SyntaxStatement::Interface(statement) => bind_named_block(statement, DeclarationOwnerKind::Interface),
        SyntaxStatement::DataClass(statement) => bind_named_block(statement, DeclarationOwnerKind::DataClass),
        SyntaxStatement::SealedClass(statement) => bind_named_block(statement, DeclarationOwnerKind::SealedClass),
        SyntaxStatement::ClassDef(statement) => bind_named_block(statement, DeclarationOwnerKind::Class),
        SyntaxStatement::OverloadDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Overload,
            class_kind: None,
            owner: None,
            is_override: false,
            is_abstract_method: false,
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
        }],
        SyntaxStatement::FunctionDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Function,
            class_kind: None,
            owner: None,
            is_override: statement.is_override,
            is_abstract_method: false,
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
        }],
        SyntaxStatement::Import(statement) => statement
            .names
            .iter()
            .cloned()
            .map(|name| Declaration {
                name,
                kind: DeclarationKind::Import,
                class_kind: None,
                owner: None,
                is_override: false,
                is_abstract_method: false,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
            })
            .collect(),
        SyntaxStatement::Value(statement) => statement
            .names
            .iter()
            .cloned()
            .map(|name| Declaration {
                name,
                kind: DeclarationKind::Value,
                class_kind: None,
                owner: None,
                is_override: false,
                is_abstract_method: false,
                is_final: statement.is_final,
                is_class_var: statement.is_class_var,
                bases: Vec::new(),
            })
            .collect(),
        SyntaxStatement::Call(_) => Vec::new(),
        SyntaxStatement::Unsafe(_) => Vec::new(),
    }
}

fn bind_named_block(
    statement: &typepython_syntax::NamedBlockStatement,
    owner_kind: DeclarationOwnerKind,
) -> Vec<Declaration> {
    let owner = DeclarationOwner {
        name: statement.name.clone(),
        kind: owner_kind,
    };
    let mut declarations = vec![Declaration {
        name: statement.name.clone(),
        kind: DeclarationKind::Class,
        class_kind: Some(owner_kind),
        owner: None,
        is_override: false,
        is_abstract_method: false,
        is_final: false,
        is_class_var: false,
        bases: statement.bases.clone(),
    }];
    declarations.extend(statement.members.iter().map(|member| Declaration {
        name: member.name.clone(),
        kind: match member.kind {
            typepython_syntax::ClassMemberKind::Field => DeclarationKind::Value,
            typepython_syntax::ClassMemberKind::Method => DeclarationKind::Function,
            typepython_syntax::ClassMemberKind::Overload => DeclarationKind::Overload,
        },
        class_kind: None,
        owner: Some(owner.clone()),
        is_override: member.is_override,
        is_abstract_method: member.is_abstract_method,
        is_final: member.is_final,
        is_class_var: member.is_class_var,
        bases: Vec::new(),
    }));
    declarations
}

#[cfg(test)]
mod tests {
    use super::{Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind, bind};
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        ClassMember, ClassMemberKind, FunctionStatement, ImportStatement, NamedBlockStatement,
        SourceFile, SourceKind, SyntaxStatement, SyntaxTree, TypeAliasStatement, TypeParam,
        ValueStatement,
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
                    bases: Vec::new(),
                    members: Vec::new(),
                    line: 2,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("helper"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_override: false,
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
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
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
                    params: Vec::new(),
                    returns: None,
                    is_override: false,
                    line: 1,
                }),
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("parse"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_override: false,
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
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
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
                    annotation: None,
                    is_final: false,
                    is_class_var: false,
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
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("bar"),
                    kind: DeclarationKind::Import,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("count"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_collects_class_like_member_declarations_with_owner() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/models.tpy"),
                kind: SourceKind::TypePython,
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                members: vec![
                    ClassMember {
                        name: String::from("value"),
                        kind: ClassMemberKind::Field,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("close"),
                        kind: ClassMemberKind::Method,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("close"),
                        kind: ClassMemberKind::Overload,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final: false,
                        is_class_var: false,
                        line: 4,
                    },
                ],
                line: 1,
            })],
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("SupportsClose"),
                    kind: DeclarationKind::Class,
                    class_kind: Some(DeclarationOwnerKind::Interface),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Overload,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_marks_final_values_and_fields() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/finals.py"),
                kind: SourceKind::Python,
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("MAX_SIZE")],
                    annotation: Some(String::from("Final")),
                    is_final: true,
                    is_class_var: false,
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    members: vec![ClassMember {
                        name: String::from("limit"),
                        kind: ClassMemberKind::Field,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final: true,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("MAX_SIZE"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Box"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_marks_classvar_values_and_fields() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/classvars.py"),
                kind: SourceKind::Python,
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Value(ValueStatement {
                    names: vec![String::from("VALUE")],
                    annotation: Some(String::from("ClassVar[int]")),
                    is_final: false,
                    is_class_var: true,
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Box"),
                    type_params: Vec::new(),
                    header_suffix: String::new(),
                    bases: Vec::new(),
                    members: vec![ClassMember {
                        name: String::from("cache"),
                        kind: ClassMemberKind::Field,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final: false,
                        is_class_var: true,
                        line: 2,
                    }],
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("VALUE"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("cache"),
                    kind: DeclarationKind::Value,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Box"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn bind_marks_override_functions_and_members() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/override.py"),
                kind: SourceKind::Python,
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::FunctionDef(FunctionStatement {
                    name: String::from("top_level"),
                    type_params: Vec::new(),
                    params: Vec::new(),
                    returns: None,
                    is_override: true,
                    line: 1,
                }),
                SyntaxStatement::ClassDef(NamedBlockStatement {
                    name: String::from("Child"),
                    type_params: Vec::new(),
                    header_suffix: String::from("(Base)"),
                    bases: vec![String::from("Base")],
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: true,
                        is_abstract_method: false,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    }],
                    line: 2,
                }),
            ],
            diagnostics: DiagnosticReport::default(),
        });

        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("top_level"),
                    kind: DeclarationKind::Function,
                    class_kind: None,
                    owner: None,
                    is_override: true,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: vec![String::from("Base")],
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: true,
                    is_abstract_method: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
            ]
        );
    }
}
