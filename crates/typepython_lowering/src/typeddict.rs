use super::*;

// ─── TypedDict utility transform expansion ───────────────────────────────────

/// Known TypedDict utility transforms.
pub(super) const TYPEDICT_TRANSFORMS: &[&str] =
    &["Partial", "Required_", "Readonly", "Mutable", "Pick", "Omit"];

/// If `value` is a TypedDict utility transform, returns the expanded class lines.
/// Otherwise returns None.
pub(super) fn try_expand_typeddict_transform(
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    source_line: &str,
) -> Option<Vec<String>> {
    let value = value.trim();
    let (transform, args) = parse_transform_expr(value)?;
    if !TYPEDICT_TRANSFORMS.contains(&transform) {
        return None;
    }

    // args = [transform_name, target_or_key, key2, ...]
    // For Partial[T]: target_name = T, key_args = []
    // For Pick[T, "k1", "k2"]: target_name = T, key_args = ["k1", "k2"]
    // args = [transform_name, target_or_key, key2, ...]
    // For Partial[T]: target_name = T, key_args = []
    // For Pick[T, "k1", "k2"]: target_name = T, key_args = ["k1", "k2"]
    let (transform, target_arg, key_args) = if args.len() < 2 {
        return None;
    } else {
        (args[0], args[1], &args[2..])
    };

    // Handle nested transforms: if target_arg is itself a transform, recursively expand it
    // inner_args[0]=transform name, [1]=target TypedDict, [2..]=key args
    let base_members = if let Some((inner_transform, inner_args)) = parse_transform_expr(target_arg)
    {
        if TYPEDICT_TRANSFORMS.contains(&inner_transform) && inner_args.len() >= 2 {
            // Recursively expand the inner transform
            let inner_target_name = inner_args[1]; // [1] is the target TypedDict name
            let inner_key_args = &inner_args[2..]; // [2..] are the key args
            let inner_target = typed_dicts.get(inner_target_name)?;
            // inner_target is &&NamedBlockStatement, dereference to &
            apply_transform_to_members(inner_transform, &inner_target.members, inner_key_args)
        } else {
            return None;
        }
    } else {
        // target_arg is a regular TypedDict name
        let target = typed_dicts.get(target_arg)?;
        target.members.to_vec()
    };

    let indentation = source_line.len() - source_line.trim_start().len();
    let indent = &source_line[..indentation];

    let members = apply_transform_to_members(transform, &base_members, key_args);
    // Extract alias name from source line: "typealias Name = ..."
    let alias_name = source_line
        .trim_start()
        .trim_end()
        .strip_prefix("typealias")?
        .split('=')
        .next()?
        .trim()
        .to_owned();

    let mut lines = Vec::with_capacity(2 + members.len());
    lines.push(format!("{}# tpy:derived {}", indent, value));
    lines.push(format!("{}class {}(TypedDict):", indent, alias_name));
    for member in members {
        let ann = member.annotation.as_deref().unwrap_or("object");
        lines.push(format!("{}    {}: {}", indent, member.name, ann));
    }

    Some(lines)
}

/// Parse "TransformName[T]" or "TransformName[T, 'k1', 'k2', ...]"
/// Returns (transform_name, vec![target_type, "k1", "k2", ...])
/// Handles nested transforms and quoted key names like "id".
pub(super) fn parse_transform_expr(value: &str) -> Option<(&str, Vec<&str>)> {
    let value = value.trim();
    let bracket_start = value.find('[')?;
    let transform = value[..bracket_start].trim();

    // Find the matching closing bracket using depth counting,
    // respecting quoted strings so ["id"] doesn't confuse the parser.
    let rest = &value[bracket_start + 1..];
    let closing_pos = find_matching_bracket(rest)?;

    // inner: everything between the opening '[' and its matching ']'
    let inner = &rest[..closing_pos];

    // Split on top-level commas only (inside nested brackets)
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_string = false;
    let mut string_char = ' ';
    for (i, c) in inner.char_indices() {
        if !in_string && (c == '"' || c == '\'') {
            in_string = true;
            string_char = c;
        } else if in_string && c == string_char && (i == 0 || !inner[..i].ends_with('\\')) {
            in_string = false;
        } else if !in_string {
            match c {
                '[' | '<' | '(' => depth += 1,
                ']' | '>' | ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    args.push(inner[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
    }
    args.push(inner[start..].trim());

    if args.is_empty() {
        return None;
    }
    // Prepend transform name so args = [transform_name, target_or_key, key2, ...]
    let mut full_args = Vec::with_capacity(1 + args.len());
    full_args.push(transform);
    full_args.extend(args);
    Some((transform, full_args))
}

/// Find the position of the matching closing bracket for a string
/// that starts with '['. Returns the position of the closing ']' (exclusive).
/// Returns None if no matching bracket is found.
fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut string_char = ' ';
    for (i, c) in s.char_indices() {
        if in_string {
            if c == '\\' && i + 1 < s.len() {
                // Skip escaped character
                continue;
            }
            if c == string_char {
                in_string = false;
            }
        } else {
            match c {
                '"' | '\'' => {
                    in_string = true;
                    string_char = c;
                }
                '[' => depth += 1,
                ']' => {
                    if depth == 0 {
                        return Some(i);
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    None
}

/// Apply a TypedDict utility transform to TypedDict field members.
fn apply_transform_to_members<'a>(
    transform: &str,
    members: &'a [typepython_syntax::ClassMember],
    key_args: &[&str],
) -> Vec<typepython_syntax::ClassMember> {
    let fields: Vec<&'a typepython_syntax::ClassMember> =
        members.iter().filter(|m| m.kind == typepython_syntax::ClassMemberKind::Field).collect();

    match transform {
        "Partial" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = if ann.contains("NotRequired[") {
                    ann.to_owned()
                } else if ann.starts_with("Required_[") {
                    ann.replace("Required_[", "NotRequired[").to_owned()
                } else {
                    format!("NotRequired[{}]", ann)
                };
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Required_" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = if let Some(inner) =
                    ann.strip_prefix("NotRequired[").and_then(|inner| inner.strip_suffix(']'))
                {
                    inner.trim().to_owned()
                } else {
                    ann.to_owned()
                };
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Readonly" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = if ann.contains("ReadOnly[") {
                    ann.to_owned()
                } else {
                    format!("ReadOnly[{}]", ann)
                };
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Mutable" => fields
            .into_iter()
            .map(|m| {
                let ann = m.annotation.as_deref().unwrap_or("object");
                let new_ann = strip_readonly(ann);
                let mut m = m.clone();
                m.annotation = Some(new_ann);
                m
            })
            .collect(),
        "Pick" => {
            let keys: std::collections::BTreeSet<_> = key_args
                .iter()
                .map(|s| {
                    s.trim_matches('"')
                        .trim_matches('\'')
                        .trim_end_matches(']')
                        .trim_end_matches(')')
                        .trim_end_matches('>')
                        .to_owned()
                })
                .collect();
            fields.into_iter().filter(|m| keys.contains(&m.name)).cloned().collect()
        }
        "Omit" => {
            let keys: std::collections::BTreeSet<_> = key_args
                .iter()
                .map(|s| {
                    s.trim_matches('"')
                        .trim_matches('\'')
                        .trim_end_matches(']')
                        .trim_end_matches(')')
                        .trim_end_matches('>')
                        .to_owned()
                })
                .collect();
            fields.into_iter().filter(|m| !keys.contains(&m.name)).cloned().collect()
        }
        _ => fields.into_iter().cloned().collect(),
    }
}

/// Strip ReadOnly[...] wrappers from a type string (one level).
fn strip_readonly(ann: &str) -> String {
    let ann = ann.trim();
    if let Some(inner) = ann.strip_prefix("ReadOnly[").and_then(|s| s.strip_suffix(']')) {
        return inner.to_owned();
    }
    ann.to_owned()
}

pub(super) fn has_notrequired_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing_extensions import NotRequired"
            || (trimmed.starts_with("from typing_extensions import ")
                && trimmed.contains("NotRequired"))
            || (trimmed.starts_with("from typing import ") && trimmed.contains("NotRequired"))
    })
}

pub(super) fn has_readonly_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing_extensions import ReadOnly"
            || (trimmed.starts_with("from typing_extensions import ")
                && trimmed.contains("ReadOnly"))
            || (trimmed.starts_with("from typing import ") && trimmed.contains("ReadOnly"))
    })
}

pub(super) fn collect_lowering_diagnostics(tree: &SyntaxTree) -> DiagnosticReport {
    let mut diagnostics = DiagnosticReport::default();
    let typed_dicts_by_name: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::ClassDef(statement)
                if statement.bases.iter().any(|base| is_typed_dict_base(base)) =>
            {
                Some((statement.name.as_str(), statement))
            }
            _ => None,
        })
        .collect();

    for statement in &tree.statements {
        match statement {
            SyntaxStatement::Unsafe(_) => {}
            SyntaxStatement::TypeAlias(statement) => {
                for diagnostic in collect_typed_dict_transform_diagnostics(
                    &tree.source.path,
                    statement.line,
                    &statement.value,
                    &typed_dicts_by_name,
                ) {
                    diagnostics.push(diagnostic);
                }
            }
            SyntaxStatement::Interface(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::Interface(statement) => {
                diagnostics.push(lowering_error(&tree.source.path, statement.line, "interface"))
            }
            SyntaxStatement::DataClass(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::DataClass(statement) => {
                diagnostics.push(lowering_error(&tree.source.path, statement.line, "data class"))
            }
            SyntaxStatement::SealedClass(statement) if is_lowerable_named_block(statement) => {}
            SyntaxStatement::SealedClass(statement) => {
                diagnostics.push(lowering_error(&tree.source.path, statement.line, "sealed class"))
            }
            SyntaxStatement::ClassDef(_) => {}
            SyntaxStatement::FunctionDef(_) => {}
            SyntaxStatement::Import(_) => {}
            SyntaxStatement::Value(_) => {}
            SyntaxStatement::Call(_) => {}
            SyntaxStatement::MethodCall(_) => {}
            SyntaxStatement::MemberAccess(_) => {}
            SyntaxStatement::Return(_) => {}
            SyntaxStatement::Yield(_) => {}
            SyntaxStatement::If(_) => {}
            SyntaxStatement::Assert(_) => {}
            SyntaxStatement::Invalidate(_) => {}
            SyntaxStatement::Match(_) => {}
            SyntaxStatement::For(_) => {}
            SyntaxStatement::With(_) => {}
            SyntaxStatement::ExceptHandler(_) => {}
            SyntaxStatement::OverloadDef(_) => {}
        }
    }

    diagnostics
}

fn collect_typed_dict_transform_diagnostics(
    path: &std::path::Path,
    line: usize,
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
) -> Vec<Diagnostic> {
    let Some((transform, args)) = parse_transform_expr(value.trim()) else {
        return Vec::new();
    };
    if !TYPEDICT_TRANSFORMS.contains(&transform) || args.len() < 2 {
        return Vec::new();
    }

    let target_arg = args[1];
    let key_args = &args[2..];
    let (target_name, members) = match resolve_transform_members(target_arg, typed_dicts) {
        Some(result) => result,
        None => {
            return vec![typed_dict_transform_error(
                path,
                line,
                format!(
                    "type transform `{}` targets `{}` which is not a known TypedDict",
                    transform,
                    target_arg.trim()
                ),
                None,
            )];
        }
    };

    if !matches!(transform, "Pick" | "Omit") {
        return Vec::new();
    }

    let field_names: BTreeSet<_> = members
        .iter()
        .filter(|member| member.kind == typepython_syntax::ClassMemberKind::Field)
        .map(|member| member.name.as_str())
        .collect();
    key_args
        .iter()
        .filter_map(|key_arg| {
            let key = transform_key_name(key_arg);
            (!field_names.contains(key.as_str())).then(|| {
                typed_dict_transform_error(
                    path,
                    line,
                    format!(
                        "type transform `{}` references unknown key `{}` on TypedDict `{}`",
                        transform, key, target_name
                    ),
                    Some((&key, key_arg, &field_names)),
                )
            })
        })
        .collect()
}

fn resolve_transform_members(
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
) -> Option<(String, Vec<typepython_syntax::ClassMember>)> {
    if let Some((transform, args)) = parse_transform_expr(value.trim()) {
        if TYPEDICT_TRANSFORMS.contains(&transform) && args.len() >= 2 {
            let target_arg = args[1];
            let key_args = &args[2..];
            let (target_name, base_members) = resolve_transform_members(target_arg, typed_dicts)?;
            return Some((
                target_name,
                apply_transform_to_members(transform, &base_members, key_args),
            ));
        }
    }

    let target = typed_dicts.get(value.trim())?;
    Some((target.name.clone(), target.members.to_vec()))
}

fn transform_key_name(key: &str) -> String {
    key.trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(']')
        .trim_end_matches(')')
        .trim_end_matches('>')
        .to_owned()
}

fn typed_dict_transform_error(
    path: &std::path::Path,
    line: usize,
    message: String,
    unknown_key: Option<(&str, &str, &BTreeSet<&str>)>,
) -> Diagnostic {
    let diagnostic = Diagnostic::error("TPY4017", message).with_span(Span::new(
        path.display().to_string(),
        line,
        1,
        line,
        1,
    ));
    if let Some((unknown_key, raw_key_arg, known_keys)) = unknown_key
        && let Some(candidate) = closest_known_key(unknown_key, known_keys)
        && let Some(suggestion) =
            typed_dict_transform_key_suggestion(path, line, raw_key_arg, candidate)
    {
        return diagnostic.with_suggestion(
            format!("Replace `{unknown_key}` with `{candidate}`"),
            suggestion.0,
            suggestion.1,
            SuggestionApplicability::MachineApplicable,
        );
    }
    diagnostic
}

fn closest_known_key<'a>(unknown_key: &str, known_keys: &'a BTreeSet<&str>) -> Option<&'a str> {
    known_keys
        .iter()
        .map(|candidate| (*candidate, edit_distance(unknown_key, candidate)))
        .min_by_key(|(_, distance)| *distance)
        .and_then(|(candidate, distance)| (distance <= 5).then_some(candidate))
}

fn typed_dict_transform_key_suggestion(
    path: &std::path::Path,
    line: usize,
    raw_key_arg: &str,
    candidate: &str,
) -> Option<(Span, String)> {
    let source = std::fs::read_to_string(path).ok()?;
    let line_text = source.lines().nth(line.saturating_sub(1))?;
    let trimmed_key = raw_key_arg.trim();
    let start = line_text.find(trimmed_key)? + 1;
    let replacement = if trimmed_key.starts_with('"') && trimmed_key.ends_with('"') {
        format!("\"{candidate}\"")
    } else if trimmed_key.starts_with('\'') && trimmed_key.ends_with('\'') {
        format!("'{candidate}'")
    } else {
        candidate.to_owned()
    };
    Some((
        Span::new(path.display().to_string(), line, start, line, start + trimmed_key.len()),
        replacement,
    ))
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    let mut prev = (0..=right.len()).collect::<Vec<_>>();
    let mut curr = vec![0usize; right.len() + 1];

    for (i, left_char) in left.iter().enumerate() {
        curr[0] = i + 1;
        for (j, right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.clone_from(&curr);
    }

    prev[right.len()]
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
    use super::*;
    use proptest::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};
    use typepython_syntax::{ClassMember, ClassMemberKind};

    fn annotation_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(String::from("int")),
            Just(String::from("str")),
            Just(String::from("bool")),
            Just(String::from("list[int]")),
            Just(String::from("tuple[str, int]")),
            Just(String::from("dict[str, int]")),
            Just(String::from("int | None")),
        ]
    }

    fn fields_strategy() -> impl Strategy<Value = BTreeMap<String, String>> {
        prop::collection::btree_map("[a-z][a-z0-9_]{0,6}", annotation_strategy(), 1..=6)
    }

    prop_compose! {
        fn fields_and_subset_strategy()
            (fields in fields_strategy())
            (selected in prop::sample::subsequence(
                fields.keys().cloned().collect::<Vec<_>>(),
                0..=fields.len(),
            ), fields in Just(fields)) -> (Vec<ClassMember>, BTreeSet<String>) {
                (field_members(&fields), selected.into_iter().collect())
            }
    }

    fn field_members(fields: &BTreeMap<String, String>) -> Vec<ClassMember> {
        fields
            .iter()
            .enumerate()
            .map(|(index, (name, annotation))| field_member(name, annotation, index + 1))
            .collect()
    }

    fn field_member(name: &str, annotation: &str, line: usize) -> ClassMember {
        ClassMember {
            name: name.to_owned(),
            kind: ClassMemberKind::Field,
            method_kind: None,
            annotation: Some(annotation.to_owned()),
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
            line,
        }
    }

    fn field_annotations(members: &[ClassMember]) -> Vec<(String, String)> {
        members
            .iter()
            .map(|member| {
                (
                    member.name.clone(),
                    member.annotation.clone().unwrap_or_else(|| String::from("object")),
                )
            })
            .collect()
    }

    fn field_names(members: &[ClassMember]) -> Vec<String> {
        members.iter().map(|member| member.name.clone()).collect()
    }

    proptest! {
        #[test]
        fn required_after_partial_restores_field_annotations(fields in fields_strategy()) {
            let members = field_members(&fields);
            let partial = apply_transform_to_members("Partial", &members, &[]);
            let required = apply_transform_to_members("Required_", &partial, &[]);

            prop_assert_eq!(field_annotations(&required), field_annotations(&members));
        }

        #[test]
        fn mutable_after_readonly_restores_field_annotations(fields in fields_strategy()) {
            let members = field_members(&fields);
            let readonly = apply_transform_to_members("Readonly", &members, &[]);
            let mutable = apply_transform_to_members("Mutable", &readonly, &[]);

            prop_assert_eq!(field_annotations(&mutable), field_annotations(&members));
        }

        #[test]
        fn pick_and_omit_partition_fields((members, selected) in fields_and_subset_strategy()) {
            let key_args = selected.iter().map(String::as_str).collect::<Vec<_>>();
            let picked = apply_transform_to_members("Pick", &members, &key_args);
            let omitted = apply_transform_to_members("Omit", &members, &key_args);

            let expected_picked = members
                .iter()
                .filter(|member| selected.contains(&member.name))
                .map(|member| member.name.clone())
                .collect::<Vec<_>>();
            let expected_omitted = members
                .iter()
                .filter(|member| !selected.contains(&member.name))
                .map(|member| member.name.clone())
                .collect::<Vec<_>>();
            let picked_names = field_names(&picked);
            let omitted_names = field_names(&omitted);

            prop_assert_eq!(&picked_names, &expected_picked);
            prop_assert_eq!(&omitted_names, &expected_omitted);

            let picked_set = picked_names.iter().cloned().collect::<BTreeSet<_>>();
            let omitted_set = omitted_names.iter().cloned().collect::<BTreeSet<_>>();
            let original_set = field_names(&members).into_iter().collect::<BTreeSet<_>>();

            prop_assert!(picked_set.is_disjoint(&omitted_set));
            prop_assert_eq!(
                picked_set.union(&omitted_set).cloned().collect::<BTreeSet<_>>(),
                original_set
            );
        }
    }
}
