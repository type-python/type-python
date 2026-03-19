//! Symbol binding boundary for TypePython.

use std::path::PathBuf;

use typepython_lowering::LoweredModule;

/// Placeholder bound symbol table.
#[derive(Debug, Clone, Default)]
pub struct BindingTable {
    /// Module path for the symbol table.
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
}

/// Binds a lowered module into a symbol table.
#[must_use]
pub fn bind(module: &LoweredModule) -> BindingTable {
    BindingTable {
        module_path: module.source_path.clone(),
        declarations: module
            .python_source
            .lines()
            .scan(false, |previous_was_overload, line| {
                let declaration = bind_top_level_declaration(line, *previous_was_overload);
                *previous_was_overload = line.trim() == "@overload";
                Some(declaration)
            })
            .flatten()
            .collect(),
    }
}

fn bind_top_level_declaration(line: &str, previous_was_overload: bool) -> Option<Declaration> {
    if line.trim_start() != line {
        return None;
    }

    if let Some(rest) = line.strip_prefix("class ") {
        return extract_identifier(rest).map(|name| Declaration {
            name,
            kind: DeclarationKind::Class,
        });
    }
    if let Some(rest) = line.strip_prefix("def ") {
        return extract_identifier(rest).map(|name| Declaration {
            name,
            kind: if previous_was_overload {
                DeclarationKind::Overload
            } else {
                DeclarationKind::Function
            },
        });
    }

    let (name, remainder) = line.split_once(':')?;
    if !remainder.trim_start().starts_with("TypeAlias =") {
        return None;
    }

    extract_identifier(name).map(|name| Declaration {
        name,
        kind: DeclarationKind::TypeAlias,
    })
}

fn extract_identifier(source: &str) -> Option<String> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }

    let end = source
        .find(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        .unwrap_or(source.len());
    let candidate = &source[..end];
    is_valid_identifier(candidate).then(|| candidate.to_owned())
}

fn is_valid_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    match characters.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{Declaration, DeclarationKind, bind};
    use std::path::PathBuf;
    use typepython_lowering::{LoweredModule, SourceMapEntry};
    use typepython_syntax::SourceKind;

    #[test]
    fn bind_collects_top_level_aliases_classes_and_functions() {
        let table = bind(&LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from typing import TypeAlias\nUserId: TypeAlias = int\nclass User:\n    pass\ndef helper():\n    return 1\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
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
    fn bind_ignores_indented_local_definitions() {
        let table = bind(&LoweredModule {
            source_path: PathBuf::from("src/app/helpers.py"),
            source_kind: SourceKind::Python,
            python_source: String::from(
                "def outer():\n    class Inner:\n        pass\n    def nested():\n        return 1\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        });

        assert_eq!(
            table.declarations,
            vec![Declaration {
                name: String::from("outer"),
                kind: DeclarationKind::Function,
            }]
        );
    }

    #[test]
    fn bind_marks_overload_definitions_separately() {
        let table = bind(&LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from typing import overload\n@overload\ndef parse(x: str) -> int: ...\ndef parse(x):\n    return 0\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
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
}
