use super::*;

pub(in super::super) fn with_active_annotated_lambda_sites<T>(
    sites: Vec<AnnotatedLambdaSite>,
    action: impl FnOnce() -> T,
) -> T {
    struct LambdaSiteGuard {
        previous: Vec<AnnotatedLambdaSite>,
    }

    impl Drop for LambdaSiteGuard {
        fn drop(&mut self) {
            ACTIVE_ANNOTATED_LAMBDA_SITES.with(|active| {
                active.replace(std::mem::take(&mut self.previous));
            });
        }
    }

    let previous = ACTIVE_ANNOTATED_LAMBDA_SITES.with(|active| active.replace(sites));
    let _guard = LambdaSiteGuard { previous };
    action()
}

pub(in super::super) fn annotated_lambda_site_at(
    line: usize,
    column: usize,
) -> Option<AnnotatedLambdaSite> {
    ACTIVE_ANNOTATED_LAMBDA_SITES.with(|active| {
        active.borrow().iter().find(|site| site.line == line && site.column == column).cloned()
    })
}

pub(in super::super) fn normalize_annotated_lambda_source(
    source: &str,
) -> (String, Vec<AnnotatedLambdaSite>) {
    let mut normalized = source.as_bytes().to_vec();
    let mut sites = Vec::new();
    let mut search_from = 0usize;

    while let Some(lambda_start) = find_next_lambda_keyword(source, search_from) {
        search_from = lambda_start + "lambda".len();
        let Some(candidate) = parse_annotated_lambda_at(source, lambda_start) else {
            continue;
        };

        normalized[candidate.open_paren] = b' ';
        normalized[candidate.close_paren] = b' ';
        for (start, end) in candidate.annotation_spans {
            normalized[start..end].fill(b' ');
        }

        sites.push(AnnotatedLambdaSite {
            line: candidate.line,
            column: candidate.column,
            param_names: candidate.param_names,
            annotations: candidate.annotations,
        });
        search_from = candidate.close_paren + 1;
    }

    (String::from_utf8_lossy(&normalized).into_owned(), sites)
}

pub(in super::super) fn normalize_annotated_lambda_source_lossy(source: &str) -> String {
    normalize_annotated_lambda_source(source).0
}

pub(in super::super) struct AnnotatedLambdaCandidate {
    open_paren: usize,
    close_paren: usize,
    line: usize,
    column: usize,
    param_names: Vec<String>,
    annotations: Vec<Option<String>>,
    annotation_spans: Vec<(usize, usize)>,
}

pub(in super::super) fn find_next_lambda_keyword(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start;
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }

        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }

            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            _ if source.get(index..).is_some_and(|s| s.starts_with("lambda"))
                && is_lambda_keyword_boundary(bytes, index, index + "lambda".len()) =>
            {
                return Some(index);
            }
            _ => index += 1,
        }
    }

    None
}

pub(in super::super) fn is_lambda_keyword_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_identifier_byte(bytes[end]);
    before_ok && after_ok
}

pub(in super::super) fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

pub(in super::super) fn parse_annotated_lambda_at(
    source: &str,
    lambda_start: usize,
) -> Option<AnnotatedLambdaCandidate> {
    let bytes = source.as_bytes();
    let mut cursor = lambda_start + "lambda".len();
    while let Some(byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() {
            cursor += 1;
        } else {
            break;
        }
    }
    if bytes.get(cursor) != Some(&b'(') {
        return None;
    }

    let close_paren = find_matching_delimiter(source, cursor, b'(', b')')?;
    let mut body_colon = close_paren + 1;
    while let Some(byte) = bytes.get(body_colon) {
        if byte.is_ascii_whitespace() {
            body_colon += 1;
        } else {
            break;
        }
    }
    if bytes.get(body_colon) != Some(&b':') {
        return None;
    }

    let params_source = &source[cursor + 1..close_paren];
    let parsed = parse_annotated_lambda_params(params_source, cursor + 1);
    let (line, column) = offset_to_line_column(source, lambda_start);
    Some(AnnotatedLambdaCandidate {
        open_paren: cursor,
        close_paren,
        line,
        column,
        param_names: parsed.param_names,
        annotations: parsed.annotations,
        annotation_spans: parsed.annotation_spans,
    })
}

pub(in super::super) fn find_matching_delimiter(
    source: &str,
    open_index: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open_index;
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            _ if byte == open => {
                depth += 1;
                index += 1;
            }
            _ if byte == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }

    None
}

pub(in super::super) struct ParsedAnnotatedLambdaParams {
    param_names: Vec<String>,
    annotations: Vec<Option<String>>,
    annotation_spans: Vec<(usize, usize)>,
}

pub(in super::super) fn parse_annotated_lambda_params(
    params_source: &str,
    absolute_start: usize,
) -> ParsedAnnotatedLambdaParams {
    let mut param_names = Vec::new();
    let mut annotations = Vec::new();
    let mut annotation_spans = Vec::new();

    for (start, end) in top_level_comma_ranges(params_source) {
        let item = &params_source[start..end];
        let trimmed = item.trim();
        if trimmed.is_empty() || trimmed == "/" || trimmed == "*" {
            continue;
        }

        let default_index = find_top_level_char(item, b'=');
        let header_end = default_index.unwrap_or(item.len());
        let annotation_index = find_top_level_char(&item[..header_end], b':');
        let name_end = annotation_index.unwrap_or(header_end);
        let mut name = item[..name_end].trim();
        if let Some(rest) = name.strip_prefix("**") {
            name = rest.trim();
        } else if let Some(rest) = name.strip_prefix('*') {
            name = rest.trim();
        }

        let annotation = annotation_index.and_then(|index| {
            let annotation_end = default_index.unwrap_or(item.len());
            let annotation = item[index + 1..annotation_end].trim();
            (!annotation.is_empty()).then(|| annotation.to_owned())
        });

        if let Some(index) = annotation_index {
            annotation_spans.push((
                absolute_start + start + index,
                absolute_start + start + default_index.unwrap_or(item.len()),
            ));
        }

        param_names.push(name.to_owned());
        annotations.push(annotation);
    }

    ParsedAnnotatedLambdaParams { param_names, annotations, annotation_spans }
}

pub(in super::super) fn top_level_comma_ranges(input: &str) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut index = 0usize;
    let bytes = input.as_bytes();
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            b'(' => {
                paren_depth += 1;
                index += 1;
            }
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                index += 1;
            }
            b'[' => {
                bracket_depth += 1;
                index += 1;
            }
            b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                index += 1;
            }
            b'{' => {
                brace_depth += 1;
                index += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 1;
            }
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                parts.push((start, index));
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    parts.push((start, input.len()));
    parts
}

pub(in super::super) fn find_top_level_char(input: &str, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut index = 0usize;
    let mut in_comment = false;
    let mut string_quote = None::<u8>;
    let mut string_triple = false;
    let mut escaped = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_comment {
            if byte == b'\n' {
                in_comment = false;
            }
            index += 1;
            continue;
        }
        if let Some(quote) = string_quote {
            if string_triple {
                if byte == quote
                    && bytes.get(index + 1) == Some(&quote)
                    && bytes.get(index + 2) == Some(&quote)
                {
                    string_quote = None;
                    string_triple = false;
                    index += 3;
                    continue;
                }
                index += 1;
                continue;
            }
            if escaped {
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == quote {
                string_quote = None;
            }
            index += 1;
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                index += 1;
            }
            b'\'' | b'"' => {
                string_quote = Some(byte);
                string_triple =
                    bytes.get(index + 1) == Some(&byte) && bytes.get(index + 2) == Some(&byte);
                index += if string_triple { 3 } else { 1 };
            }
            b'(' => {
                paren_depth += 1;
                index += 1;
            }
            b')' => {
                paren_depth = paren_depth.saturating_sub(1);
                index += 1;
            }
            b'[' => {
                bracket_depth += 1;
                index += 1;
            }
            b']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                index += 1;
            }
            b'{' => {
                brace_depth += 1;
                index += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 1;
            }
            _ if byte == target && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return Some(index);
            }
            _ => index += 1,
        }
    }

    None
}
