use super::*;

pub(in super::super) fn parse_extension_statement(
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

pub(in super::super) fn strip_soft_keyword<'source>(
    line: &'source str,
    keyword: &str,
) -> Option<&'source str> {
    let rest = line.strip_prefix(keyword)?;
    match rest.chars().next() {
        Some(character) if character == '_' || character.is_ascii_alphanumeric() => None,
        _ => Some(rest),
    }
}

pub(in super::super) fn parse_typealias(
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
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, "typealias declaration")?;

    Some(SyntaxStatement::TypeAlias(TypeAliasStatement {
        name,
        type_params: parsed_type_params.type_params,
        value: tail.trim().to_owned(),
        value_expr: TypeExpr::parse(tail.trim()),
        line: line_number,
    }))
}

pub(in super::super) fn parse_named_block(
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
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)?;

    Some(constructor(NamedBlockStatement {
        name,
        type_params: parsed_type_params.type_params,
        header_suffix: parsed_type_params.remainder.trim().to_owned(),
        bases: Vec::new(),
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_abstract_class: false,
        members: Vec::new(),
        line: line_number,
    }))
}

pub(in super::super) fn parse_overload(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
) -> Option<SyntaxStatement> {
    parse_function(
        path,
        line,
        rest,
        line_number,
        diagnostics,
        "overload declaration",
        SyntaxStatement::OverloadDef,
    )
}

pub(in super::super) fn parse_function(
    path: &Path,
    line: &str,
    rest: &str,
    line_number: usize,
    diagnostics: &mut DiagnosticReport,
    label: &str,
    constructor: fn(FunctionStatement) -> SyntaxStatement,
) -> Option<SyntaxStatement> {
    let Some((signature, _suite)) = split_top_level_once(rest.trim_end(), ':') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must end with `:`"),
        ));
        return None;
    };

    let Some((name_part, _)) = split_top_level_once(signature.trim_end(), '(') else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a parameter list"),
        ));
        return None;
    };
    let Some((name, suffix)) = extract_decl_head(name_part) else {
        diagnostics.push(parse_error(
            path,
            line_number,
            line,
            format!("{label} must include a function name"),
        ));
        return None;
    };
    let parsed_type_params =
        parse_type_params(path, line_number, line, suffix, diagnostics, label)?;

    Some(constructor(FunctionStatement {
        name,
        type_params: parsed_type_params.type_params,
        params: Vec::new(),
        returns: None,
        returns_expr: None,
        is_async: false,
        is_override: false,
        is_deprecated: false,
        deprecation_message: None,
        line: line_number,
    }))
}

pub(in super::super) fn parse_unsafe(
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

pub(in super::super) fn extract_decl_head(header: &str) -> Option<(String, &str)> {
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

pub(in super::super) fn parse_type_params<'source>(
    path: &Path,
    line_number: usize,
    line: &str,
    suffix: &'source str,
    diagnostics: &mut DiagnosticReport,
    label: &str,
) -> Option<ParsedTypeParams<'source>> {
    let suffix = suffix.trim_start();
    if suffix.is_empty() || !suffix.starts_with('[') {
        return Some(ParsedTypeParams { type_params: Vec::new(), remainder: suffix });
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
                diagnostics.push(*diagnostic);
                return None;
            }
        }
    }

    if !validate_type_param_names(path, line_number, label, &type_params, diagnostics)
        || !validate_type_param_default_order(path, line_number, label, &type_params, diagnostics)
    {
        return None;
    }

    Some(ParsedTypeParams { type_params, remainder })
}

pub(in super::super) fn validate_type_param_names(
    path: &Path,
    line_number: usize,
    label: &str,
    type_params: &[TypeParam],
    diagnostics: &mut DiagnosticReport,
) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    for type_param in type_params {
        if !seen.insert(type_param.name.as_str()) {
            diagnostics.push(
                Diagnostic::error(
                    "TPY4004",
                    format!("{label} declares type parameter `{}` more than once", type_param.name),
                )
                .with_span(Span::new(
                    path.display().to_string(),
                    line_number,
                    1,
                    line_number,
                    1,
                )),
            );
            return false;
        }
    }
    true
}

pub(in super::super) fn validate_type_param_default_order(
    path: &Path,
    line_number: usize,
    label: &str,
    type_params: &[TypeParam],
    diagnostics: &mut DiagnosticReport,
) -> bool {
    let mut seen_default = false;
    for type_param in type_params {
        if type_param.default.is_some() {
            seen_default = true;
            continue;
        }
        if seen_default {
            diagnostics.push(
                Diagnostic::error(
                    "TPY2001",
                    format!(
                        "{label} type parameter `{}` without a default cannot follow a parameter with a default",
                        type_param.name
                    ),
                )
                .with_span(Span::new(path.display().to_string(), line_number, 1, line_number, 1)),
            );
            return false;
        }
    }
    true
}

pub(in super::super) fn split_top_level_once(input: &str, separator: char) -> Option<(&str, &str)> {
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

pub(in super::super) fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut start = 0usize;

    for (index, character) in input.char_indices() {
        if character == ',' && bracket_depth == 0 && paren_depth == 0 {
            parts.push(input[start..index].trim());
            start = index + character.len_utf8();
            continue;
        }

        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ => {}
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

pub(in super::super) fn parse_type_param(
    path: &Path,
    line_number: usize,
    line: &str,
    item: &str,
    label: &str,
) -> Result<TypeParam, Box<Diagnostic>> {
    let item = item.trim();
    if item.is_empty() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an empty type parameter entry"),
        )));
    }

    let (item, default) = match split_top_level_once(item, '=') {
        Some((head, default)) => (head.trim(), Some(default.trim())),
        None => (item, None),
    };
    let (kind, item) = if let Some(item) = item.strip_prefix("**") {
        (TypeParamKind::ParamSpec, item.trim())
    } else if let Some(item) = item.strip_prefix('*') {
        (TypeParamKind::TypeVarTuple, item.trim())
    } else {
        (TypeParamKind::TypeVar, item)
    };
    let (name_part, bound_or_constraints) = match split_top_level_once(item, ':') {
        Some((name_part, bound)) => (name_part.trim(), Some(bound.trim())),
        None => (item, None),
    };
    if !is_valid_identifier(name_part) {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} contains an invalid type parameter name"),
        )));
    }

    if kind == TypeParamKind::ParamSpec && bound_or_constraints.is_some() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} ParamSpec `{name_part}` must not declare bounds or constraints"),
        )));
    }
    if kind == TypeParamKind::TypeVarTuple && bound_or_constraints.is_some() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} TypeVarTuple `{name_part}` must not declare bounds or constraints"),
        )));
    }

    let (bound, constraints) = match bound_or_constraints {
        Some("") => {
            return Err(Box::new(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter bound must not be empty"),
            )));
        }
        Some(bound) if bound.starts_with('(') => {
            (None, parse_type_param_constraints(path, line_number, line, bound, label)?)
        }
        Some(bound) => (Some(bound.to_owned()), Vec::new()),
        None => (None, Vec::new()),
    };
    let default = match default {
        Some("") => {
            return Err(Box::new(parse_error(
                path,
                line_number,
                line,
                format!("{label} type parameter default must not be empty"),
            )));
        }
        Some(default) => Some(default.to_owned()),
        None => None,
    };

    Ok(TypeParam {
        kind,
        name: name_part.to_owned(),
        bound_expr: bound.as_deref().and_then(TypeExpr::parse),
        bound,
        constraint_exprs: constraints
            .iter()
            .filter_map(|constraint| TypeExpr::parse(constraint))
            .collect(),
        constraints,
        default_expr: default.as_deref().and_then(TypeExpr::parse),
        default,
    })
}

pub(in super::super) fn parse_type_param_constraints(
    path: &Path,
    line_number: usize,
    line: &str,
    constraints: &str,
    label: &str,
) -> Result<Vec<String>, Box<Diagnostic>> {
    let Some(inner) = constraints.strip_prefix('(').and_then(|inner| inner.strip_suffix(')'))
    else {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter constraint list must be parenthesized"),
        )));
    };
    let parsed = split_top_level_commas(inner);
    if parsed.is_empty() {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter constraint list must not be empty"),
        )));
    }
    if parsed.iter().any(|constraint| constraint.is_empty()) {
        return Err(Box::new(parse_error(
            path,
            line_number,
            line,
            format!("{label} type parameter constraint list must not contain empty entries"),
        )));
    }
    Ok(parsed.into_iter().map(str::to_owned).collect())
}

pub(in super::super) fn split_bracketed(input: &str) -> Option<(&str, &str)> {
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

pub(in super::super) fn split_top_level(input: &str, separator: char) -> Vec<&str> {
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

pub(in super::super) fn is_valid_identifier(candidate: &str) -> bool {
    let mut characters = candidate.chars();
    match characters.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(in super::super) fn parse_error(
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

pub(in super::super) fn parse_error_span(
    path: &Path,
    source: &str,
    start: usize,
    end: usize,
) -> Span {
    let start = start.min(source.len());
    let end = end.max(start).min(source.len());
    let (line, column) = offset_to_line_column(source, start);
    let (end_line, end_column) = offset_to_line_column(source, end);

    Span::new(path.display().to_string(), line, column, end_line, end_column)
}

pub(in super::super) fn offset_to_line_column(source: &str, offset: usize) -> (usize, usize) {
    let active_lookup = ACTIVE_SOURCE_LINE_INDICES.with(|active| {
        active
            .borrow()
            .iter()
            .rev()
            .find(|index| index.ptr == source.as_ptr() as usize && index.len == source.len())
            .map(|index| offset_to_line_column_from_line_starts(source, offset, &index.line_starts))
    });
    if let Some(line_and_column) = active_lookup {
        return line_and_column;
    }

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
