//! Symbol binding boundary for TypePython.

use std::path::PathBuf;

use typepython_syntax::{MethodKind, SourceKind, SyntaxStatement, SyntaxTree};

#[derive(Debug, Clone)]
pub struct BindingTable {
    pub module_path: PathBuf,
    pub module_key: String,
    pub module_kind: SourceKind,
    pub declarations: Vec<Declaration>,
    pub calls: Vec<CallSite>,
}

impl Default for BindingTable {
    fn default() -> Self {
        Self {
            module_path: PathBuf::new(),
            module_key: String::new(),
            module_kind: SourceKind::TypePython,
            declarations: Vec::new(),
            calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CallSite {
    pub callee: String,
    pub arg_count: usize,
    pub keyword_names: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub detail: String,
    pub method_kind: Option<MethodKind>,
    pub class_kind: Option<DeclarationOwnerKind>,
    pub owner: Option<DeclarationOwner>,
    pub is_override: bool,
    pub is_abstract_method: bool,
    pub is_final_decorator: bool,
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
        module_key: tree.source.logical_module.clone(),
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
                SyntaxStatement::Call(statement) => Some(CallSite {
                    callee: statement.callee.clone(),
                    arg_count: statement.arg_count,
                    keyword_names: statement.keyword_names.clone(),
                }),
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
            detail: statement.value.clone(),
            method_kind: None,
            class_kind: None,
            owner: None,
            is_override: false,
            is_abstract_method: false,
            is_final_decorator: false,
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
            detail: format_signature(&statement.params, statement.returns.as_deref()),
            method_kind: None,
            class_kind: None,
            owner: None,
            is_override: false,
            is_abstract_method: false,
            is_final_decorator: false,
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
        }],
        SyntaxStatement::FunctionDef(statement) => vec![Declaration {
            name: statement.name.clone(),
            kind: DeclarationKind::Function,
            detail: format_signature(&statement.params, statement.returns.as_deref()),
            method_kind: None,
            class_kind: None,
            owner: None,
            is_override: statement.is_override,
            is_abstract_method: false,
            is_final_decorator: false,
            is_final: false,
            is_class_var: false,
            bases: Vec::new(),
        }],
        SyntaxStatement::Import(statement) => statement
            .bindings
            .iter()
            .map(|binding| Declaration {
                name: binding.local_name.clone(),
                kind: DeclarationKind::Import,
                detail: binding.source_path.clone(),
                method_kind: None,
                class_kind: None,
                owner: None,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
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
                detail: statement.annotation.clone().unwrap_or_default(),
                method_kind: None,
                class_kind: None,
                owner: None,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
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
        detail: statement.bases.join(","),
        method_kind: None,
        class_kind: Some(owner_kind),
        owner: None,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: statement.is_final_decorator,
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
        detail: match member.kind {
            typepython_syntax::ClassMemberKind::Field => member.annotation.clone().unwrap_or_default(),
            typepython_syntax::ClassMemberKind::Method | typepython_syntax::ClassMemberKind::Overload => {
                format_signature(&member.params, member.returns.as_deref())
            }
        },
        method_kind: member.method_kind,
        class_kind: None,
        owner: Some(owner.clone()),
        is_override: member.is_override,
        is_abstract_method: member.is_abstract_method,
        is_final_decorator: member.is_final_decorator,
        is_final: member.is_final,
        is_class_var: member.is_class_var,
        bases: Vec::new(),
    }));
    declarations
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

#[cfg(test)]
mod tests {
    use super::{Declaration, DeclarationKind, DeclarationOwner, DeclarationOwnerKind, bind};
    use std::path::PathBuf;
    use typepython_diagnostics::DiagnosticReport;
    use typepython_syntax::{
        ClassMember, ClassMemberKind, FunctionStatement, ImportStatement, MethodKind,
        NamedBlockStatement, SourceFile, SourceKind, SyntaxStatement, SyntaxTree,
        TypeAliasStatement, TypeParam,
        ValueStatement,
    };

    #[test]
    fn bind_collects_top_level_aliases_classes_and_functions() {
        let table = bind(&SyntaxTree {
            source: SourceFile {
                path: PathBuf::from("src/app/__init__.tpy"),
                kind: SourceKind::TypePython,
                logical_module: String::from("app"),
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
                    is_final_decorator: false,
                    is_abstract_class: false,
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

        println!("{} {:?}", table.module_key, table.declarations);
        assert_eq!(table.module_key, "app");
        assert_eq!(
            table.declarations,
            vec![
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::from("int"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("helper"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
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
                logical_module: String::new(),
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
                    detail: String::from("()->"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("parse"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
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
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![
                SyntaxStatement::Import(ImportStatement {
                    bindings: vec![
                        typepython_syntax::ImportBinding {
                            local_name: String::from("local_foo"),
                            source_path: String::from("pkg.foo"),
                        },
                        typepython_syntax::ImportBinding {
                            local_name: String::from("bar"),
                            source_path: String::from("pkg.bar"),
                        },
                    ],
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
                    detail: String::from("pkg.foo"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("bar"),
                    kind: DeclarationKind::Import,
                    detail: String::from("pkg.bar"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("count"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
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
                logical_module: String::new(),
                text: String::new(),
            },
            statements: vec![SyntaxStatement::Interface(NamedBlockStatement {
                name: String::from("SupportsClose"),
                type_params: Vec::new(),
                header_suffix: String::new(),
                bases: Vec::new(),
                is_final_decorator: false,
                is_abstract_class: false,
                members: vec![
                    ClassMember {
                        name: String::from("value"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        line: 2,
                    },
                    ClassMember {
                        name: String::from("close"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
                        is_final: false,
                        is_class_var: false,
                        line: 3,
                    },
                    ClassMember {
                        name: String::from("close"),
                        kind: ClassMemberKind::Overload,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
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
                    detail: String::new(),
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Interface),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("value"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    method_kind: Some(MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("close"),
                    kind: DeclarationKind::Overload,
                    detail: String::from("()->"),
                    method_kind: Some(MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("SupportsClose"),
                        kind: DeclarationOwnerKind::Interface,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
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
                logical_module: String::new(),
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
                    is_final_decorator: false,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("limit"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
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
                    detail: String::from("Final"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: true,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("limit"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Box"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
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
                logical_module: String::new(),
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
                    is_final_decorator: false,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("cache"),
                        kind: ClassMemberKind::Field,
                        method_kind: None,
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: false,
                        is_abstract_method: false,
                        is_final_decorator: false,
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
                    detail: String::from("ClassVar[int]"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: true,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("Box"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("cache"),
                    kind: DeclarationKind::Value,
                    detail: String::new(),
                    method_kind: None,
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Box"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
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
                logical_module: String::new(),
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
                    is_final_decorator: false,
                    is_abstract_class: false,
                    members: vec![ClassMember {
                        name: String::from("run"),
                        kind: ClassMemberKind::Method,
                        method_kind: Some(MethodKind::Instance),
                        annotation: None,
                        params: Vec::new(),
                        returns: None,
                        is_override: true,
                        is_abstract_method: false,
                        is_final_decorator: false,
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
                    detail: String::from("()->"),
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_override: true,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
                Declaration {
                    name: String::from("Child"),
                    kind: DeclarationKind::Class,
                    detail: String::from("Base"),
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: vec![String::from("Base")],
                },
                Declaration {
                    name: String::from("run"),
                    kind: DeclarationKind::Function,
                    detail: String::from("()->"),
                    method_kind: Some(MethodKind::Instance),
                    class_kind: None,
                    owner: Some(DeclarationOwner {
                        name: String::from("Child"),
                        kind: DeclarationOwnerKind::Class,
                    }),
                    is_override: true,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                },
            ]
        );
    }
}
