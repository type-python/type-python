use super::*;

// ─── TypedDict utility transform expansion ───────────────────────────────────

/// Known TypedDict utility transforms.
pub(super) const TYPEDICT_TRANSFORMS: &[&str] =
    &["Partial", "Required_", "Readonly", "Mutable", "Pick", "Omit", "MapValues"];

type SharedShapeProjection = typepython_syntax::ShapeProjection;

/// If `value` is a TypedDict utility transform, returns the expanded class lines.
/// Otherwise returns None.
pub(super) fn try_expand_typeddict_transform(
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    experimental_shape_transforms: bool,
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
    if transform == "MapValues" && !map_values_wrapper_supported(key_args) {
        return None;
    }

    // Handle nested transforms: if target_arg is itself a transform, recursively expand it
    // inner_args[0]=transform name, [1]=target TypedDict, [2..]=key args
    let base_shape = if let Some((inner_transform, inner_args)) = parse_transform_expr(target_arg) {
        if TYPEDICT_TRANSFORMS.contains(&inner_transform) && inner_args.len() >= 2 {
            let inner_target = resolve_transform_shape(
                inner_args[1],
                typed_dicts,
                data_classes,
                experimental_shape_transforms,
            )?;
            apply_transform_to_shape(inner_transform, &inner_target, &inner_args[2..])
        } else {
            return None;
        }
    } else {
        resolve_transform_shape(
            target_arg,
            typed_dicts,
            data_classes,
            experimental_shape_transforms,
        )?
    };

    let indentation = source_line.len() - source_line.trim_start().len();
    let indent = &source_line[..indentation];

    let members = apply_transform_to_shape(transform, &base_shape, key_args).materialize_members();
    // Extract alias name from source line: "typealias Name = ..."
    let alias_name = source_line
        .trim_start()
        .trim_end()
        .strip_prefix("typealias")?
        .split('=')
        .next()?
        .trim()
        .to_owned();

    let mut lines = Vec::with_capacity(1 + members.len());
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

/// Apply a field-bearing utility transform to the shared syntax shape substrate.
fn apply_transform_to_shape(
    transform: &str,
    shape: &SharedShapeProjection,
    key_args: &[&str],
) -> SharedShapeProjection {
    match transform {
        "Partial" => shape.partial(),
        "Required_" => shape.required_fields(),
        "Readonly" => shape.readonly_fields(),
        "Mutable" => shape.mutable_fields(),
        "Pick" => {
            let keys = key_args
                .iter()
                .flat_map(|key_arg| transform_key_names(key_arg))
                .collect::<Vec<_>>();
            let keys = keys.iter().map(String::as_str).collect::<Vec<_>>();
            shape.pick(&keys)
        }
        "Omit" => {
            let keys = key_args
                .iter()
                .flat_map(|key_arg| transform_key_names(key_arg))
                .collect::<Vec<_>>();
            let keys = keys.iter().map(String::as_str).collect::<Vec<_>>();
            shape.omit(&keys)
        }
        "MapValues" => shape.map_values(key_args.first().copied().unwrap_or_default()),
        _ => shape.clone(),
    }
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

pub(super) fn has_optional_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import Optional"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("Optional"))
            || trimmed == "import typing"
    })
}

pub(super) fn has_typeddict_import(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "from typing import TypedDict"
            || trimmed == "from typing_extensions import TypedDict"
            || (trimmed.starts_with("from typing import ") && trimmed.contains("TypedDict"))
            || (trimmed.starts_with("from typing_extensions import ")
                && trimmed.contains("TypedDict"))
            || trimmed == "import typing"
            || trimmed == "import typing_extensions"
    })
}

pub(super) fn transform_targets_data_class(
    value: &str,
    data_classes: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
) -> bool {
    let Some((transform, args)) = parse_transform_expr(value.trim()) else {
        return false;
    };
    TYPEDICT_TRANSFORMS.contains(&transform)
        && args.len() >= 2
        && transform_expression_reaches_data_class(args[1], data_classes)
}

fn transform_expression_reaches_data_class(
    value: &str,
    data_classes: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
) -> bool {
    if data_classes.contains_key(value.trim()) {
        return true;
    }
    let Some((transform, args)) = parse_transform_expr(value.trim()) else {
        return false;
    };
    TYPEDICT_TRANSFORMS.contains(&transform)
        && args.len() >= 2
        && transform_expression_reaches_data_class(args[1], data_classes)
}

pub(super) fn collect_lowering_diagnostics_with_options(
    tree: &SyntaxTree,
    options: &LoweringOptions,
) -> DiagnosticReport {
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
    let data_classes_by_name: std::collections::BTreeMap<_, _> = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::DataClass(statement) if is_lowerable_named_block(statement) => {
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
                    &data_classes_by_name,
                    options.experimental_shape_transforms,
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
    data_classes: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    experimental_shape_transforms: bool,
) -> Vec<Diagnostic> {
    let Some((transform, args)) = parse_transform_expr(value.trim()) else {
        return Vec::new();
    };
    if !TYPEDICT_TRANSFORMS.contains(&transform) || args.len() < 2 {
        return Vec::new();
    }

    let target_arg = args[1];
    let key_args = &args[2..];
    let target_shape = match resolve_transform_shape(
        target_arg,
        typed_dicts,
        data_classes,
        experimental_shape_transforms,
    ) {
        Some(result) => result,
        None => {
            return vec![typed_dict_transform_error(
                path,
                line,
                format!(
                    "type transform `{}` targets `{}` which is not a known TypedDict or experimental shape source",
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

    let field_names: BTreeSet<_> = target_shape
        .fields
        .iter()
        .flat_map(|field| [field.name.as_str(), field.public_alias.as_str()])
        .collect();
    key_args
        .iter()
        .flat_map(|key_arg| transform_key_specs(key_arg))
        .filter_map(|(key, raw_key_arg)| {
            (!field_names.contains(key.as_str())).then(|| {
                typed_dict_transform_error(
                    path,
                    line,
                    format!(
                        "type transform `{}` references unknown key `{}` on shape source `{}`",
                        transform, key, target_shape.name
                    ),
                    Some((&key, raw_key_arg, &field_names)),
                )
            })
        })
        .collect()
}

fn resolve_transform_shape(
    value: &str,
    typed_dicts: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    data_classes: &std::collections::BTreeMap<&str, &typepython_syntax::NamedBlockStatement>,
    experimental_shape_transforms: bool,
) -> Option<SharedShapeProjection> {
    if let Some((transform, args)) = parse_transform_expr(value.trim())
        && TYPEDICT_TRANSFORMS.contains(&transform)
        && args.len() >= 2
    {
        let target_arg = args[1];
        let key_args = &args[2..];
        if transform == "MapValues" && !map_values_wrapper_supported(key_args) {
            return None;
        }
        let base_shape = resolve_transform_shape(
            target_arg,
            typed_dicts,
            data_classes,
            experimental_shape_transforms,
        )?;
        return Some(apply_transform_to_shape(transform, &base_shape, key_args));
    }

    if let Some(target) = typed_dicts.get(value.trim()) {
        return Some(SharedShapeProjection::from_named_block(
            target,
            typepython_syntax::ShapeProjectionSourceKind::TypedDict,
        ));
    }
    if experimental_shape_transforms && let Some(target) = data_classes.get(value.trim()) {
        return Some(SharedShapeProjection::from_named_block(
            target,
            typepython_syntax::ShapeProjectionSourceKind::DataClass,
        ));
    }
    None
}

fn map_values_wrapper_supported(key_args: &[&str]) -> bool {
    matches!(key_args.first().map(|wrapper| wrapper.trim()), Some("Optional" | "Readonly"))
}

fn transform_key_name(key: &str) -> String {
    key.trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(']')
        .trim_end_matches(')')
        .trim_end_matches('>')
        .to_owned()
}

fn transform_key_names(key: &str) -> Vec<String> {
    transform_key_specs(key).into_iter().map(|(name, _)| name).collect()
}

fn transform_key_specs(key: &str) -> Vec<(String, &str)> {
    if let Some((head, args)) = parse_transform_expr(key.trim())
        && is_literal_transform_head(head)
        && args.len() >= 2
    {
        return args[1..]
            .iter()
            .map(|literal_arg| (transform_key_name(literal_arg), *literal_arg))
            .collect();
    }
    vec![(transform_key_name(key), key)]
}

fn is_literal_transform_head(head: &str) -> bool {
    matches!(head.trim(), "Literal" | "typing.Literal" | "typing_extensions.Literal")
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
            value_type_expr: None,
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

    fn shape_from_members(members: &[ClassMember]) -> SharedShapeProjection {
        SharedShapeProjection {
            name: String::from("Generated"),
            source_kind: typepython_syntax::ShapeProjectionSourceKind::TypedDict,
            fields: members
                .iter()
                .map(typepython_syntax::ShapeProjectionField::from_class_member)
                .collect(),
        }
    }

    fn apply_members_transform(
        transform: &str,
        members: &[ClassMember],
        key_args: &[&str],
    ) -> Vec<ClassMember> {
        apply_transform_to_shape(transform, &shape_from_members(members), key_args)
            .materialize_members()
    }

    #[test]
    fn lowering_shape_transform_operations_preserve_projection_metadata() {
        let shape =
            shape_from_members(&[field_member("id", "int", 1), field_member("name", "str", 2)]);

        let projected = apply_transform_to_shape("Partial", &shape, &[]);

        assert_eq!(projected.name, "Generated");
        assert!(projected.fields.iter().all(|field| !field.required));
        assert!(projected.fields.iter().all(|field| {
            field.source_kind
                == typepython_syntax::ShapeProjectionFieldSourceKind::ProjectionGenerated
        }));
        assert_eq!(projected.fields[0].annotation.as_deref(), Some("NotRequired[int]"));
    }

    #[test]
    fn lowering_shape_pick_uses_public_aliases() {
        let mut aliased = typepython_syntax::ShapeProjectionField::from_class_member(
            &field_member("internal_name", "str", 1),
        );
        aliased.public_alias = String::from("externalName");
        let shape = SharedShapeProjection {
            name: String::from("Model"),
            source_kind: typepython_syntax::ShapeProjectionSourceKind::DataClass,
            fields: vec![
                aliased,
                typepython_syntax::ShapeProjectionField::from_class_member(&field_member(
                    "id", "int", 2,
                )),
            ],
        };

        let projected = apply_transform_to_shape("Pick", &shape, &["\"externalName\""]);

        assert_eq!(projected.fields.len(), 1);
        assert_eq!(projected.fields[0].name, "internal_name");
    }

    proptest! {
        #[test]
        fn required_after_partial_restores_field_annotations(fields in fields_strategy()) {
            let members = field_members(&fields);
            let partial = apply_members_transform("Partial", &members, &[]);
            let required = apply_members_transform("Required_", &partial, &[]);

            prop_assert_eq!(field_annotations(&required), field_annotations(&members));
        }

        #[test]
        fn mutable_after_readonly_restores_field_annotations(fields in fields_strategy()) {
            let members = field_members(&fields);
            let readonly = apply_members_transform("Readonly", &members, &[]);
            let mutable = apply_members_transform("Mutable", &readonly, &[]);

            prop_assert_eq!(field_annotations(&mutable), field_annotations(&members));
        }

        #[test]
        fn pick_and_omit_partition_fields((members, selected) in fields_and_subset_strategy()) {
            let key_args = selected.iter().map(String::as_str).collect::<Vec<_>>();
            let picked = apply_members_transform("Pick", &members, &key_args);
            let omitted = apply_members_transform("Omit", &members, &key_args);

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
