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
    Value,
    Import,
}

/// Binds a lowered module into a symbol table.
#[must_use]
pub fn bind(module: &LoweredModule) -> BindingTable {
    let first_source_line = module
        .source_map
        .iter()
        .map(|entry| entry.lowered_line)
        .min()
        .unwrap_or(1);

    BindingTable {
        module_path: module.source_path.clone(),
        declarations: module
            .python_source
            .lines()
            .enumerate()
            .scan(false, |previous_was_overload, (index, line)| {
                let line_number = index + 1;
                if line_number < first_source_line {
                    return Some(Vec::new());
                }
                let declarations = bind_top_level_declarations(line, *previous_was_overload);
                *previous_was_overload = line.trim() == "@overload";
                Some(declarations)
            })
            .flatten()
            .collect(),
    }
}

fn bind_top_level_declarations(line: &str, previous_was_overload: bool) -> Vec<Declaration> {
    if line.trim_start() != line {
        return Vec::new();
    }

    if let Some(rest) = line.strip_prefix("class ") {
        return extract_identifier(rest)
            .map(|name| Declaration {
            name,
            kind: DeclarationKind::Class,
        })
            .into_iter()
            .collect();
    }
    if let Some(rest) = line.strip_prefix("def ") {
        return extract_identifier(rest)
            .map(|name| Declaration {
            name,
            kind: if previous_was_overload {
                DeclarationKind::Overload
            } else {
                DeclarationKind::Function
            },
        })
            .into_iter()
            .collect();
    }
    if let Some(rest) = line.strip_prefix("import ") {
        return bind_import_declaration(rest);
    }
    if let Some(rest) = line.strip_prefix("from ") {
        return bind_from_import_declaration(rest);
    }

    if let Some((name, remainder)) = line.split_once(':') {
        if remainder.trim_start().starts_with("TypeAlias =") {
            return extract_identifier(name)
                .map(|name| Declaration {
                name,
                kind: DeclarationKind::TypeAlias,
            })
                .into_iter()
                .collect();
        }

        if remainder.contains('=') || !remainder.trim().is_empty() {
            return extract_identifier(name)
                .map(|name| Declaration {
                name,
                kind: DeclarationKind::Value,
            })
                .into_iter()
                .collect();
        }
    }

    bind_assignment_declaration(line).into_iter().collect()
}

fn bind_import_declaration(rest: &str) -> Vec<Declaration> {
    rest.split(',')
        .filter_map(|item| {
            let item = item.trim();
            if let Some((_, alias)) = item.split_once(" as ") {
                return extract_identifier(alias).map(|name| Declaration {
                    name,
                    kind: DeclarationKind::Import,
                });
            }

            let root = item.split('.').next()?.trim();
            extract_identifier(root).map(|name| Declaration {
                name,
                kind: DeclarationKind::Import,
            })
        })
        .collect()
}

fn bind_from_import_declaration(rest: &str) -> Vec<Declaration> {
    let Some((_, imports)) = rest.split_once(" import ") else {
        return Vec::new();
    };

    imports
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            if let Some((_, alias)) = item.split_once(" as ") {
                return extract_identifier(alias).map(|name| Declaration {
                    name,
                    kind: DeclarationKind::Import,
                });
            }

            extract_identifier(item).map(|name| Declaration {
                name,
                kind: DeclarationKind::Import,
            })
        })
        .collect()
}

fn bind_assignment_declaration(line: &str) -> Option<Declaration> {
    let (head, tail) = line.split_once('=')?;
    let tail = tail.trim_start();
    if tail.starts_with('=') {
        return None;
    }
    let head = head.trim_end();
    if matches!(head.chars().last(), Some('!' | '<' | '>' | '=')) {
        return None;
    }

    extract_identifier(head).map(|name| Declaration {
        name,
        kind: DeclarationKind::Value,
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
            source_map: vec![
                SourceMapEntry {
                    original_line: 1,
                    lowered_line: 2,
                },
                SourceMapEntry {
                    original_line: 2,
                    lowered_line: 3,
                },
                SourceMapEntry {
                    original_line: 3,
                    lowered_line: 5,
                },
            ],
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
            source_map: vec![
                SourceMapEntry {
                    original_line: 1,
                    lowered_line: 2,
                },
                SourceMapEntry {
                    original_line: 2,
                    lowered_line: 4,
                },
            ],
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
    fn bind_ignores_synthetic_prelude_imports_and_typevars() {
        let table = bind(&LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from typing import TypeVar\nfrom typing import TypeAlias\nT = TypeVar(\"T\")\nPair: TypeAlias = tuple[T, T]\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 4 }],
        });

        assert_eq!(
            table.declarations,
            vec![Declaration {
                name: String::from("Pair"),
                kind: DeclarationKind::TypeAlias,
            }]
        );
    }

    #[test]
    fn bind_collects_top_level_values_and_imports() {
        let table = bind(&LoweredModule {
            source_path: PathBuf::from("src/app/helpers.py"),
            source_kind: SourceKind::Python,
            python_source: String::from(
                "from pkg import foo as local_foo, bar\nimport tools.helpers, more.tools as alias\nvalue: int = 1\ncount = 2\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
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
                    name: String::from("tools"),
                    kind: DeclarationKind::Import,
                },
                Declaration {
                    name: String::from("alias"),
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
