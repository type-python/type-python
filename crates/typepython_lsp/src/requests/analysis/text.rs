pub(crate) fn full_document_range(text: &str) -> LspRange {
    LspRange {
        start: LspPosition { line: 0, character: 0 },
        end: position_at_byte_offset(text, text.len()).unwrap_or(LspPosition {
            line: 0,
            character: 0,
        }),
    }
}

pub(crate) fn token_at_position(text: &str, position: LspPosition) -> Option<TokenOccurrence> {
    tokenize_identifiers(text).into_iter().find(|token| range_contains(token.range, position))
}

pub(crate) fn resolve_owner_canonical(
    workspace: &WorkspaceState,
    document: &DocumentState,
    declarations_by_canonical: &BTreeMap<String, SymbolOccurrence>,
    owner_name: &str,
    through_instance: bool,
) -> Option<String> {
    if !through_instance {
        return document
            .local_value_types
            .get(owner_name)
            .cloned()
            .or_else(|| document.local_symbols.get(owner_name).cloned());
    }

    let callable_canonical = document.local_symbols.get(owner_name)?.clone();
    let _callable = declarations_by_canonical.get(&callable_canonical)?;
    let return_type =
        resolve_callable_return_type_in_scope(workspace, document, None, None, owner_name)?;
    document.local_symbols.get(&return_type).cloned().or_else(|| Some(return_type.to_owned()))
}

pub(crate) fn member_receiver_name(text: &str, position: LspPosition) -> Option<String> {
    let prefix = line_prefix_at_position(text, position)?;
    let mut chars = prefix.chars().collect::<Vec<_>>();
    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }
    if chars.pop()? != '.' {
        return None;
    }
    while chars.last().is_some_and(|ch| ch.is_whitespace()) {
        chars.pop();
    }
    let end = chars.len();
    let mut start = end;
    while start > 0 {
        let ch = chars[start - 1];
        if ch.is_ascii_alphanumeric() || ch == '_' {
            start -= 1;
        } else {
            break;
        }
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

pub(crate) fn collect_local_value_types(
    document: &DocumentState,
    local_symbols: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut types = BTreeMap::new();
    for statement in &document.syntax.statements {
        let SyntaxStatement::Value(statement) = statement else {
            continue;
        };
        let resolved_type = statement
            .annotation
            .as_ref()
            .and_then(|annotation| local_symbols.get(annotation))
            .cloned()
            .or_else(|| {
                statement
                    .value_callee
                    .as_ref()
                    .and_then(|callee| local_symbols.get(callee))
                    .cloned()
            })
            .or_else(|| {
                statement
                    .rendered_value_type()
                    .as_ref()
                    .and_then(|value_type| local_symbols.get(value_type))
                    .cloned()
            });
        let Some(resolved_type) = resolved_type else {
            continue;
        };
        for name in &statement.names {
            types.insert(name.clone(), resolved_type.clone());
        }
    }
    types
}

pub(crate) fn dedupe_occurrences(occurrences: &mut Vec<SymbolOccurrence>) {
    let mut indexes_by_location = BTreeMap::<_, usize>::new();
    let mut deduped = Vec::<SymbolOccurrence>::with_capacity(occurrences.len());
    for occurrence in occurrences.drain(..) {
        let location = (
            occurrence.canonical.clone(),
            occurrence.uri.clone(),
            occurrence.range.start.line,
            occurrence.range.start.character,
            occurrence.range.end.line,
            occurrence.range.end.character,
        );
        if let Some(index) = indexes_by_location.get(&location).copied() {
            if occurrence.declaration && !deduped[index].declaration {
                deduped[index] = occurrence;
            }
            continue;
        }
        indexes_by_location.insert(location, deduped.len());
        deduped.push(occurrence);
    }
    *occurrences = deduped;
}

#[derive(Debug)]
pub(crate) struct TokenOccurrence {
    pub(crate) name: String,
    pub(crate) range: LspRange,
    pub(crate) preceded_by_dot: bool,
}

pub(crate) fn tokenize_identifiers(text: &str) -> Vec<TokenOccurrence> {
    use ruff_python_ast::token::TokenKind;
    use ruff_python_parser::{Mode, ParseOptions as RuffParseOptions, parse_unchecked};

    let parsed = parse_unchecked(text, RuffParseOptions::from(Mode::Module));
    let mut tokens = Vec::new();
    let mut previous_significant_kind = None;
    for token in parsed.tokens().iter() {
        let (kind, range) = token.as_tuple();
        let preceded_by_dot = previous_significant_kind == Some(TokenKind::Dot);
        if !kind.is_trivia() {
            previous_significant_kind = Some(kind);
        }
        if kind != TokenKind::Name {
            continue;
        }

        let start_offset = usize::from(range.start());
        let end_offset = usize::from(range.end());
        let (Some(name), Some(start), Some(end)) = (
            text.get(start_offset..end_offset),
            position_at_byte_offset(text, start_offset),
            position_at_byte_offset(text, end_offset),
        ) else {
            continue;
        };
        tokens.push(TokenOccurrence {
            name: name.to_owned(),
            range: LspRange { start, end },
            preceded_by_dot,
        });
    }
    tokens
}

pub(crate) fn position_at_byte_offset(text: &str, offset: usize) -> Option<LspPosition> {
    let prefix = text.get(..offset)?;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    Some(LspPosition {
        line: prefix.bytes().filter(|byte| *byte == b'\n').count() as u32,
        character: utf16_len(text.get(line_start..offset)?),
    })
}

pub(crate) fn find_name_range(text: &str, line: usize, name: &str) -> Option<LspRange> {
    let line_text = text.lines().nth(line.saturating_sub(1))?;
    let byte_column = line_text.find(name)?;
    let column = utf16_len(line_text.get(..byte_column)?);
    let end_column = column.saturating_add(utf16_len(name));
    Some(LspRange {
        start: LspPosition { line: line.saturating_sub(1) as u32, character: column },
        end: LspPosition {
            line: line.saturating_sub(1) as u32,
            character: end_column,
        },
    })
}

pub(crate) fn line_prefix(text: &str, position: LspPosition) -> String {
    line_prefix_at_position(text, position).unwrap_or_default().to_owned()
}

pub(crate) fn utf16_len(text: &str) -> u32 {
    text.chars().fold(0u32, |length, character| {
        length.saturating_add(character.len_utf16() as u32)
    })
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum PositionConversionError {
    LineOutOfBounds,
    CharacterOutOfBounds,
    SplitsCodePoint,
}

pub(crate) fn byte_offset_at_position(
    text: &str,
    position: LspPosition,
) -> Result<usize, PositionConversionError> {
    let mut line_start = 0usize;
    let mut line_number = 0u32;

    loop {
        let remainder = text
            .get(line_start..)
            .ok_or(PositionConversionError::LineOutOfBounds)?;
        let newline = remainder.find('\n');
        let raw_line = newline.map_or(remainder, |newline| &remainder[..newline]);
        let line_text = raw_line.strip_suffix('\r').unwrap_or(raw_line);

        if line_number == position.line {
            return byte_offset_at_utf16_column(line_text, position.character)
                .map(|column| line_start + column);
        }

        let Some(newline) = newline else {
            break;
        };
        line_start = line_start.saturating_add(newline).saturating_add(1);
        line_number = line_number.saturating_add(1);
    }

    Err(PositionConversionError::LineOutOfBounds)
}

fn byte_offset_at_utf16_column(
    line_text: &str,
    character: u32,
) -> Result<usize, PositionConversionError> {
    let mut utf16_offset = 0u32;
    for (byte_offset, ch) in line_text.char_indices() {
        if utf16_offset == character {
            return Ok(byte_offset);
        }
        utf16_offset = utf16_offset.saturating_add(ch.len_utf16() as u32);
        if utf16_offset > character {
            return Err(PositionConversionError::SplitsCodePoint);
        }
    }

    if utf16_offset == character {
        Ok(line_text.len())
    } else {
        Err(PositionConversionError::CharacterOutOfBounds)
    }
}

pub(crate) fn lsp_position_from_scalar_column(
    text: &str,
    one_based_line: usize,
    one_based_column: usize,
) -> LspPosition {
    let line = one_based_line.saturating_sub(1) as u32;
    let scalar_column = one_based_column.saturating_sub(1);
    let character = text
        .split('\n')
        .nth(line as usize)
        .map(|line_text| line_text.strip_suffix('\r').unwrap_or(line_text))
        .map(|line_text| {
            let byte_column = line_text
                .char_indices()
                .nth(scalar_column)
                .map_or(line_text.len(), |(offset, _)| offset);
            utf16_len(&line_text[..byte_column])
        })
        .unwrap_or(scalar_column as u32);
    LspPosition { line, character }
}

fn line_prefix_at_position(text: &str, position: LspPosition) -> Option<&str> {
    let offset = byte_offset_at_position(text, position).ok()?;
    let line_start = text.get(..offset)?.rfind('\n').map_or(0, |index| index + 1);
    text.get(line_start..offset)
}

pub(crate) fn format_signature(
    params: &[typepython_syntax::FunctionParam],
    returns: Option<&str>,
) -> String {
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
