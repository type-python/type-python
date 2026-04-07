fn normalize_typepython_source(source: &str, statements: &[SyntaxStatement]) -> String {
    let statement_lines: std::collections::BTreeMap<usize, &SyntaxStatement> =
        statements.iter().map(|statement| (statement_line(statement), statement)).collect();

    let mut normalized_lines = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let normalized = if let Some(statement) = statement_lines.get(&line_number) {
            normalize_typepython_statement_line(line, statement)
        } else {
            normalize_generic_python_header_line(line)
        };
        normalized_lines.push(normalized);
    }

    let mut normalized = normalized_lines.join("\n");
    if source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn statement_line(statement: &SyntaxStatement) -> usize {
    match statement {
        SyntaxStatement::TypeAlias(statement) => statement.line,
        SyntaxStatement::Interface(statement) => statement.line,
        SyntaxStatement::DataClass(statement) => statement.line,
        SyntaxStatement::SealedClass(statement) => statement.line,
        SyntaxStatement::OverloadDef(statement) => statement.line,
        SyntaxStatement::ClassDef(statement) => statement.line,
        SyntaxStatement::FunctionDef(statement) => statement.line,
        SyntaxStatement::Import(statement) => statement.line,
        SyntaxStatement::Value(statement) => statement.line,
        SyntaxStatement::Call(statement) => statement.line,
        SyntaxStatement::MethodCall(statement) => statement.line,
        SyntaxStatement::MemberAccess(statement) => statement.line,
        SyntaxStatement::Return(statement) => statement.line,
        SyntaxStatement::Yield(statement) => statement.line,
        SyntaxStatement::If(statement) => statement.line,
        SyntaxStatement::Assert(statement) => statement.line,
        SyntaxStatement::Invalidate(statement) => statement.line,
        SyntaxStatement::Match(statement) => statement.line,
        SyntaxStatement::For(statement) => statement.line,
        SyntaxStatement::With(statement) => statement.line,
        SyntaxStatement::ExceptHandler(statement) => statement.line,
        SyntaxStatement::Unsafe(statement) => statement.line,
    }
}

fn normalize_typepython_statement_line(line: &str, statement: &SyntaxStatement) -> String {
    match statement {
        SyntaxStatement::TypeAlias(statement) => {
            let indentation = leading_indent(line);
            format!("{indentation}{} = {}", statement.name, statement.value)
        }
        SyntaxStatement::Interface(statement)
        | SyntaxStatement::DataClass(statement)
        | SyntaxStatement::SealedClass(statement)
        | SyntaxStatement::ClassDef(statement) => {
            let indentation = leading_indent(line);
            format!(
                "{indentation}class {}{}{}:",
                statement.name,
                render_type_params(&statement.type_params),
                statement.header_suffix
            )
        }
        SyntaxStatement::OverloadDef(_) => {
            let indentation = leading_indent(line);
            let trimmed = line.trim_start();
            let rest = trimmed.strip_prefix("overload ").unwrap_or(trimmed);
            format!("{indentation}{rest}")
        }
        SyntaxStatement::FunctionDef(_) => line.to_owned(),
        SyntaxStatement::Import(_)
        | SyntaxStatement::Value(_)
        | SyntaxStatement::Call(_)
        | SyntaxStatement::MethodCall(_)
        | SyntaxStatement::MemberAccess(_)
        | SyntaxStatement::Return(_)
        | SyntaxStatement::Yield(_)
        | SyntaxStatement::If(_)
        | SyntaxStatement::Assert(_)
        | SyntaxStatement::Invalidate(_)
        | SyntaxStatement::Match(_)
        | SyntaxStatement::For(_)
        | SyntaxStatement::With(_)
        | SyntaxStatement::ExceptHandler(_) => line.to_owned(),
        SyntaxStatement::Unsafe(_) => {
            let indentation = leading_indent(line);
            format!("{indentation}if True:")
        }
    }
}

fn normalize_generic_python_header_line(line: &str) -> String {
    line.to_owned()
}

fn leading_indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn render_type_params(type_params: &[TypeParam]) -> String {
    if type_params.is_empty() {
        return String::new();
    }

    format!("[{}]", type_params.iter().map(render_type_param).collect::<Vec<_>>().join(", "))
}

fn render_type_param(type_param: &TypeParam) -> String {
    let prefix = match type_param.kind {
        TypeParamKind::TypeVar => "",
        TypeParamKind::ParamSpec => "**",
        TypeParamKind::TypeVarTuple => "*",
    };
    let mut rendered = if !type_param.constraints.is_empty() {
        format!(
            "{}: ({})",
            type_param.name,
            if type_param.constraint_exprs.is_empty() {
                type_param.constraints.join(", ")
            } else {
                type_param
                    .constraint_exprs
                    .iter()
                    .map(TypeExpr::render)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )
    } else {
        match &type_param.bound_expr {
            Some(bound) => format!("{}: {}", type_param.name, bound.render()),
            None => match &type_param.bound {
                Some(bound) => format!("{}: {}", type_param.name, bound),
                None => type_param.name.clone(),
            },
        }
    };
    rendered.insert_str(0, prefix);
    if let Some(default) = &type_param.default_expr {
        rendered.push_str(" = ");
        rendered.push_str(&default.render());
    } else if let Some(default) = &type_param.default {
        rendered.push_str(" = ");
        rendered.push_str(default);
    }
    rendered
}

pub fn normalize_source_variadic_type_syntax(text: &str) -> String {
    let mut normalized = String::new();
    let mut index = 0usize;

    while index < text.len() {
        let mut characters = text[index..].chars();
        let Some(character) = characters.next() else {
            break;
        };
        if character != '*' {
            normalized.push(character);
            index += character.len_utf8();
            continue;
        }
        if text[index + character.len_utf8()..].starts_with('*') {
            normalized.push(character);
            index += character.len_utf8();
            continue;
        }

        let previous = previous_significant_char(text, index);
        let Some((operand_start, operand_end, delimiter)) =
            variadic_unpack_operand_bounds(text, index + character.len_utf8())
        else {
            normalized.push(character);
            index += character.len_utf8();
            continue;
        };
        let operand = text[operand_start..operand_end].trim();
        if operand.is_empty()
            || matches!(delimiter, Some(':')) && matches!(previous, None | Some('(') | Some(','))
            || !matches!(previous, None | Some('[') | Some(',') | Some(':') | Some('>') | Some('='))
        {
            normalized.push(character);
            index += character.len_utf8();
            continue;
        }

        normalized.push_str("Unpack[");
        normalized.push_str(operand);
        normalized.push(']');
        index = operand_end;
    }

    normalized
}

const FORMAT_MARKER_PREFIX: &str = "__typepython_format__:";

/// Prepared source text that can be sent to an external Python formatter.
#[derive(Debug, Clone)]
pub struct ExternalFormattingSource {
    formatter_input: String,
    restore_custom_syntax: bool,
}

impl ExternalFormattingSource {
    /// Returns the formatter-ready source text.
    #[must_use]
    pub fn formatter_input(&self) -> &str {
        &self.formatter_input
    }

    /// Restores TypePython-only syntax after the external formatter has run.
    #[must_use]
    pub fn restore(&self, formatted: &str) -> String {
        if self.restore_custom_syntax {
            restore_typepython_formatting_markers(formatted)
        } else {
            formatted.to_owned()
        }
    }
}

/// Converts a source file into Python-compatible text for external formatters.
pub fn prepare_source_for_external_formatter(
    source: &SourceFile,
) -> Result<ExternalFormattingSource, DiagnosticReport> {
    if source.kind != SourceKind::TypePython {
        return Ok(ExternalFormattingSource {
            formatter_input: source.text.clone(),
            restore_custom_syntax: false,
        });
    }

    let syntax = parse_with_options(source.clone(), ParseOptions::default());
    prepare_syntax_tree_for_external_formatter(&syntax)
}

/// Converts an already-parsed syntax tree into Python-compatible text for external formatters.
pub fn prepare_syntax_tree_for_external_formatter(
    syntax: &SyntaxTree,
) -> Result<ExternalFormattingSource, DiagnosticReport> {
    if syntax.source.kind != SourceKind::TypePython {
        return Ok(ExternalFormattingSource {
            formatter_input: syntax.source.text.clone(),
            restore_custom_syntax: false,
        });
    }
    if syntax.diagnostics.has_errors() {
        return Err(syntax.diagnostics.clone());
    }

    Ok(ExternalFormattingSource {
        formatter_input: prepare_typepython_source_for_external_formatter(
            &syntax.source.text,
            &syntax.statements,
        ),
        restore_custom_syntax: true,
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum FormattingSyntaxKind {
    TypeAlias,
    Interface,
    DataClass,
    SealedClass,
    OverloadDef,
    Unsafe,
}

impl FormattingSyntaxKind {
    fn marker(self) -> &'static str {
        match self {
            Self::TypeAlias => "typealias",
            Self::Interface => "interface",
            Self::DataClass => "data_class",
            Self::SealedClass => "sealed_class",
            Self::OverloadDef => "overload_def",
            Self::Unsafe => "unsafe",
        }
    }

    fn from_marker(marker: &str) -> Option<Self> {
        Some(match marker {
            "typealias" => Self::TypeAlias,
            "interface" => Self::Interface,
            "data_class" => Self::DataClass,
            "sealed_class" => Self::SealedClass,
            "overload_def" => Self::OverloadDef,
            "unsafe" => Self::Unsafe,
            _ => return None,
        })
    }
}

fn prepare_typepython_source_for_external_formatter(
    source: &str,
    statements: &[SyntaxStatement],
) -> String {
    let mut custom_lines = BTreeMap::new();
    for statement in statements {
        if let Some(kind) = formatting_syntax_kind(statement) {
            custom_lines.insert(statement_line(statement), kind);
        }
    }

    let mut prepared_lines = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        if let Some(kind) = custom_lines.get(&line_number).copied() {
            let indentation = leading_indent(line);
            prepared_lines.push(format!("{indentation}# {FORMAT_MARKER_PREFIX}{}", kind.marker()));
            prepared_lines.push(normalize_typepython_formatting_line(line, kind));
        } else {
            prepared_lines.push(line.to_owned());
        }
    }

    let mut prepared = prepared_lines.join("\n");
    if source.ends_with('\n') {
        prepared.push('\n');
    }
    prepared
}

fn formatting_syntax_kind(statement: &SyntaxStatement) -> Option<FormattingSyntaxKind> {
    Some(match statement {
        SyntaxStatement::TypeAlias(_) => FormattingSyntaxKind::TypeAlias,
        SyntaxStatement::Interface(_) => FormattingSyntaxKind::Interface,
        SyntaxStatement::DataClass(_) => FormattingSyntaxKind::DataClass,
        SyntaxStatement::SealedClass(_) => FormattingSyntaxKind::SealedClass,
        SyntaxStatement::OverloadDef(_) => FormattingSyntaxKind::OverloadDef,
        SyntaxStatement::Unsafe(_) => FormattingSyntaxKind::Unsafe,
        _ => return None,
    })
}

fn normalize_typepython_formatting_line(line: &str, kind: FormattingSyntaxKind) -> String {
    let indentation = leading_indent(line);
    let trimmed = line.trim_start();
    match kind {
        FormattingSyntaxKind::TypeAlias => strip_soft_keyword(trimmed, "typealias")
            .map(|rest| format!("{indentation}{}", rest.trim_start()))
            .unwrap_or_else(|| line.to_owned()),
        FormattingSyntaxKind::Interface => strip_soft_keyword(trimmed, "interface")
            .map(|rest| format!("{indentation}class{rest}"))
            .unwrap_or_else(|| line.to_owned()),
        FormattingSyntaxKind::DataClass => trimmed
            .strip_prefix("data class ")
            .map(|rest| format!("{indentation}class {rest}"))
            .unwrap_or_else(|| line.to_owned()),
        FormattingSyntaxKind::SealedClass => trimmed
            .strip_prefix("sealed class ")
            .map(|rest| format!("{indentation}class {rest}"))
            .unwrap_or_else(|| line.to_owned()),
        FormattingSyntaxKind::OverloadDef => trimmed
            .strip_prefix("overload def ")
            .map(|rest| format!("{indentation}def {rest}"))
            .unwrap_or_else(|| line.to_owned()),
        FormattingSyntaxKind::Unsafe => trimmed
            .strip_prefix("unsafe:")
            .map(|rest| format!("{indentation}if True:{rest}"))
            .unwrap_or_else(|| line.to_owned()),
    }
}

fn restore_typepython_formatting_markers(formatted: &str) -> String {
    let mut restored_lines = Vec::new();
    let mut pending_kind = None;

    for line in formatted.lines() {
        let trimmed = line.trim_start();
        if let Some(marker) = trimmed.strip_prefix('#').map(str::trim_start)
            && let Some(marker) = marker.strip_prefix(FORMAT_MARKER_PREFIX)
            && let Some(kind) = FormattingSyntaxKind::from_marker(marker.trim())
        {
            pending_kind = Some(kind);
            continue;
        }

        if pending_kind.is_some() && trimmed.is_empty() {
            continue;
        }

        if let Some(kind) = pending_kind.take() {
            restored_lines.push(restore_typepython_formatting_line(line, kind));
        } else {
            restored_lines.push(line.to_owned());
        }
    }

    let mut restored = restored_lines.join("\n");
    if formatted.ends_with('\n') {
        restored.push('\n');
    }
    restored
}

fn restore_typepython_formatting_line(line: &str, kind: FormattingSyntaxKind) -> String {
    let indentation = leading_indent(line);
    let trimmed = line.trim_start();
    match kind {
        FormattingSyntaxKind::TypeAlias => split_top_level_once(trimmed, '=')
            .map(|(head, tail)| {
                format!("{indentation}typealias {} = {}", head.trim_end(), tail.trim_start())
            })
            .unwrap_or_else(|| format!("{indentation}typealias {trimmed}")),
        FormattingSyntaxKind::Interface => trimmed
            .strip_prefix("class ")
            .map(|rest| format!("{indentation}interface {rest}"))
            .unwrap_or_else(|| format!("{indentation}interface {trimmed}")),
        FormattingSyntaxKind::DataClass => trimmed
            .strip_prefix("class ")
            .map(|rest| format!("{indentation}data class {rest}"))
            .unwrap_or_else(|| format!("{indentation}data class {trimmed}")),
        FormattingSyntaxKind::SealedClass => trimmed
            .strip_prefix("class ")
            .map(|rest| format!("{indentation}sealed class {rest}"))
            .unwrap_or_else(|| format!("{indentation}sealed class {trimmed}")),
        FormattingSyntaxKind::OverloadDef => trimmed
            .strip_prefix("def ")
            .map(|rest| format!("{indentation}overload def {rest}"))
            .unwrap_or_else(|| format!("{indentation}overload def {trimmed}")),
        FormattingSyntaxKind::Unsafe => trimmed
            .strip_prefix("if True:")
            .map(|rest| format!("{indentation}unsafe:{rest}"))
            .unwrap_or_else(|| format!("{indentation}unsafe:")),
    }
}

fn previous_significant_char(text: &str, end: usize) -> Option<char> {
    text[..end].chars().rev().find(|character| !character.is_whitespace())
}

fn variadic_unpack_operand_bounds(
    text: &str,
    start: usize,
) -> Option<(usize, usize, Option<char>)> {
    let mut operand_start = start;
    while let Some(character) = text[operand_start..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        operand_start += character.len_utf8();
    }
    if operand_start >= text.len() {
        return None;
    }

    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut operand_end = text.len();
    let mut delimiter = None;

    for (offset, character) in text[operand_start..].char_indices() {
        if bracket_depth == 0 && paren_depth == 0 && brace_depth == 0 {
            match character {
                ',' | ']' | ')' | ':' | '=' => {
                    operand_end = operand_start + offset;
                    delimiter = Some(character);
                    break;
                }
                _ => {}
            }
        }
        match character {
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }

    Some((operand_start, operand_end, delimiter))
}
