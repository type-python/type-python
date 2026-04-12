use super::*;

pub(crate) fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, LspError> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                    LspError::invalid_request(format!("invalid `Content-Length` header: {error}"))
                })?);
            }
        }
    }

    let Some(content_length) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    let message = serde_json::from_slice(&body)
        .map_err(|error| LspError::parse_error(format!("invalid JSON-RPC payload: {error}")))?;
    Ok(Some(message))
}

pub(crate) fn write_message<W: Write>(writer: &mut W, value: &Value) -> Result<(), LspError> {
    let payload = serde_json::to_vec(value).map_err(|error| {
        LspError::internal(format!("unable to encode JSON-RPC payload: {error}"))
    })?;
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
    writer.write_all(&payload)?;
    Ok(())
}

pub(crate) fn publish_diagnostics_notification(
    uri: &str,
    diagnostics: Vec<LspDiagnostic>,
) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics,
        }
    })
}

pub(crate) fn text_document_position(params: &Value) -> Result<(String, LspPosition), LspError> {
    let uri = params
        .get("textDocument")
        .and_then(|document| document.get("uri"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            LspError::invalid_params(String::from(
                "textDocument/position request missing `params.textDocument.uri`",
            ))
        })?;
    let raw_position = params.get("position").cloned().ok_or_else(|| {
        LspError::invalid_params(String::from(
            "textDocument/position request missing `params.position`",
        ))
    })?;
    let position: LspPosition = serde_json::from_value(raw_position).map_err(|error| {
        LspError::invalid_params(format!(
            "textDocument/position request has invalid `params.position`: {error}"
        ))
    })?;
    Ok((uri.to_owned(), position))
}

pub(crate) fn text_document_range(params: &Value) -> Result<(String, LspRange), LspError> {
    let uri = params
        .get("textDocument")
        .and_then(|document| document.get("uri"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            LspError::invalid_params(String::from(
                "textDocument/range request missing `params.textDocument.uri`",
            ))
        })?;
    let raw_range = params.get("range").cloned().ok_or_else(|| {
        LspError::invalid_params(String::from("textDocument/range request missing `params.range`"))
    })?;
    let range: LspRange = serde_json::from_value(raw_range).map_err(|error| {
        LspError::invalid_params(format!(
            "textDocument/range request has invalid `params.range`: {error}"
        ))
    })?;
    Ok((uri.to_owned(), range))
}

pub(crate) fn apply_content_changes(
    current_text: &str,
    content_changes: &[LspContentChangeEvent],
    uri: &str,
) -> Result<String, LspError> {
    let mut text = current_text.to_owned();
    for change in content_changes {
        match change.range {
            Some(range) => apply_ranged_change(&mut text, range, &change.text, uri)?,
            None => {
                if change.range_length.is_some() {
                    return Err(LspError::content_modified(format!(
                        "TPY6002: didChange for `{}` provided `rangeLength` without `range`",
                        uri
                    ))
                    .with_tpy_code("TPY6002"));
                }
                text = change.text.clone();
            }
        }
    }
    Ok(text)
}

pub(crate) fn apply_ranged_change(
    text: &mut String,
    range: LspRange,
    replacement: &str,
    uri: &str,
) -> Result<(), LspError> {
    let start = lsp_position_to_byte_offset(text, range.start, uri)?;
    let end = lsp_position_to_byte_offset(text, range.end, uri)?;
    if start > end {
        return Err(LspError::content_modified(format!(
            "TPY6002: didChange for `{}` uses an invalid range with start after end",
            uri
        ))
        .with_tpy_code("TPY6002"));
    }
    text.replace_range(start..end, replacement);
    Ok(())
}

pub(crate) fn lsp_position_to_byte_offset(
    text: &str,
    position: LspPosition,
    uri: &str,
) -> Result<usize, LspError> {
    let mut line_start = 0usize;
    for (line_index, line) in text.split_inclusive('\n').enumerate() {
        let line_number = line_index as u32;
        let line_text = line.strip_suffix('\n').unwrap_or(line);
        if line_number == position.line {
            return utf16_column_to_byte_offset(line_text, line_start, position, uri);
        }
        line_start += line.len();
    }

    let total_lines = text.lines().count() as u32;
    if position.line == total_lines {
        return utf16_column_to_byte_offset(&text[line_start..], line_start, position, uri);
    }

    Err(LspError::content_modified(format!(
        "TPY6002: didChange for `{}` references line {} beyond the current document",
        uri, position.line
    ))
    .with_tpy_code("TPY6002"))
}

pub(crate) fn utf16_column_to_byte_offset(
    line_text: &str,
    line_start: usize,
    position: LspPosition,
    uri: &str,
) -> Result<usize, LspError> {
    let mut utf16_offset = 0u32;
    for (byte_offset, ch) in line_text.char_indices() {
        if utf16_offset == position.character {
            return Ok(line_start + byte_offset);
        }
        utf16_offset += ch.len_utf16() as u32;
        if utf16_offset > position.character {
            return Err(LspError::content_modified(format!(
                "TPY6002: didChange for `{}` splits a UTF-16 code point at line {}, character {}",
                uri, position.line, position.character
            ))
            .with_tpy_code("TPY6002"));
        }
    }

    if utf16_offset == position.character {
        Ok(line_start + line_text.len())
    } else {
        Err(LspError::content_modified(format!(
            "TPY6002: didChange for `{}` references character {} beyond line {}",
            uri, position.character, position.line
        ))
        .with_tpy_code("TPY6002"))
    }
}

pub(crate) fn resolve_symbol<'a>(
    workspace: &'a WorkspaceState,
    uri: &str,
    position: LspPosition,
) -> Option<&'a SymbolOccurrence> {
    workspace
        .queries
        .occurrences_by_uri
        .get(uri)?
        .iter()
        .find(|occurrence| range_contains(occurrence.range, position))
}

pub(crate) fn range_contains(range: LspRange, position: LspPosition) -> bool {
    (position.line > range.start.line
        || (position.line == range.start.line && position.character >= range.start.character))
        && (position.line < range.end.line
            || (position.line == range.end.line && position.character <= range.end.character))
}

pub(crate) fn range_intersects(left: LspRange, right: LspRange) -> bool {
    !(left.end.line < right.start.line
        || right.end.line < left.start.line
        || (left.end.line == right.start.line && left.end.character < right.start.character)
        || (right.end.line == left.start.line && right.end.character < left.start.character))
}

pub(crate) fn diagnostics_by_uri(
    documents: &[DocumentState],
    diagnostics: &DiagnosticReport,
) -> BTreeMap<String, Vec<LspDiagnostic>> {
    let mut by_uri = documents
        .iter()
        .map(|document| (document.uri.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let path_to_uri = documents
        .iter()
        .map(|document| (normalize_path_string(&document.path), document.uri.clone()))
        .collect::<BTreeMap<_, _>>();

    for diagnostic in &diagnostics.diagnostics {
        let Some(span) = &diagnostic.span else {
            continue;
        };
        let normalized = normalize_path_string(Path::new(&span.path));
        let Some(uri) = path_to_uri.get(&normalized) else {
            continue;
        };
        by_uri.entry(uri.clone()).or_default().push(LspDiagnostic {
            range: LspRange {
                start: LspPosition {
                    line: span.line.saturating_sub(1) as u32,
                    character: span.column.saturating_sub(1) as u32,
                },
                end: LspPosition {
                    line: span.end_line.saturating_sub(1) as u32,
                    character: span.end_column.saturating_sub(1) as u32,
                },
            },
            severity: match diagnostic.severity {
                Severity::Error => 1,
                Severity::Warning => 2,
                Severity::Note => 3,
            },
            code: diagnostic.code.clone(),
            message: if diagnostic.notes.is_empty() {
                diagnostic.message.clone()
            } else {
                format!("{} ({})", diagnostic.message, diagnostic.notes.join("; "))
            },
            data: (!diagnostic.suggestions.is_empty()).then(|| {
                json!({
                    "suggestions": diagnostic.suggestions,
                })
            }),
        });
    }

    by_uri
}
