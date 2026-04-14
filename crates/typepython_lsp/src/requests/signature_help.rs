use super::*;
use std::cmp::Reverse;

#[derive(Debug, Clone)]
pub(crate) struct ActiveCall {
    pub(crate) callee: String,
    pub(crate) active_parameter: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SignatureCandidate {
    pub(crate) info: LspSignatureInformation,
    pub(crate) params: Vec<typepython_syntax::FunctionParam>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveCallSite {
    pub(crate) arg_types: Vec<String>,
    pub(crate) keyword_names: Vec<String>,
    pub(crate) keyword_arg_types: Vec<String>,
}

fn rendered_arg_types(values: &[typepython_syntax::DirectExprMetadata]) -> Vec<String> {
    values.iter().map(|value| value.rendered_value_type().unwrap_or_default()).collect()
}

pub(crate) fn active_call(
    document: &DocumentState,
    position: LspPosition,
    uri: &str,
) -> Result<Option<ActiveCall>, LspError> {
    let offset = lsp_position_to_byte_offset(&document.text, position, uri)?;
    let prefix = &document.text[..offset];
    let Some((open_offset, active_parameter)) = active_call_open(prefix) else {
        return Ok(None);
    };
    let Some(callee) = call_callee_before_offset(prefix, open_offset) else {
        return Ok(None);
    };
    Ok(Some(ActiveCall { callee, active_parameter }))
}

pub(crate) fn active_call_open(prefix: &str) -> Option<(usize, usize)> {
    let mut paren_stack = Vec::<(usize, usize)>::new();
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    for (offset, ch) in prefix.char_indices() {
        match ch {
            '(' => paren_stack.push((offset, 0)),
            ')' => {
                paren_stack.pop();
            }
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if bracket_depth == 0 && brace_depth == 0 => {
                if let Some((_, active_parameter)) = paren_stack.last_mut() {
                    *active_parameter += 1;
                }
            }
            _ => {}
        }
    }

    paren_stack.pop()
}

pub(crate) fn call_callee_before_offset(prefix: &str, open_offset: usize) -> Option<String> {
    let before = prefix[..open_offset].trim_end();
    if before.is_empty() {
        return None;
    }

    let mut start = before.len();
    let mut generic_depth = 0usize;
    for (offset, ch) in before.char_indices().rev() {
        match ch {
            ']' => {
                generic_depth += 1;
                start = offset;
            }
            '[' => {
                if generic_depth == 0 {
                    break;
                }
                generic_depth -= 1;
                start = offset;
            }
            _ if generic_depth > 0 => {
                start = offset;
            }
            _ if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' => {
                start = offset;
            }
            _ => break,
        }
    }

    let callee = before[start..].trim();
    (!callee.is_empty()).then(|| callee.to_owned())
}

pub(crate) fn resolve_signature_candidates(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    callee: &str,
) -> Vec<SignatureCandidate> {
    let normalized = strip_generic_args(callee).trim();
    if normalized.is_empty() {
        return Vec::new();
    }

    if let Some((receiver, member_name)) = normalized.rsplit_once('.') {
        return resolve_member_signature_candidates(
            workspace,
            document,
            position,
            receiver.trim(),
            member_name.trim(),
        );
    }

    let mut signatures = if let Some(canonical) = document.local_symbols.get(normalized) {
        signature_candidates_for_canonical(workspace, canonical)
    } else {
        Vec::new()
    };
    if signatures.is_empty() {
        let (_, owner_type_name) = scope_context_at_position(document, position);
        if let Some(owner_type_name) = owner_type_name {
            signatures.extend(class_member_signature_candidates(
                document,
                &owner_type_name,
                normalized,
            ));
        }
    }
    signatures
}

pub(crate) fn resolve_member_signature_candidates(
    workspace: &WorkspaceState,
    document: &DocumentState,
    position: LspPosition,
    receiver: &str,
    member_name: &str,
) -> Vec<SignatureCandidate> {
    let mut owner_canonicals = Vec::new();
    if let Some(canonical) = document.local_symbols.get(receiver) {
        push_unique(&mut owner_canonicals, canonical.clone());
    }
    if let Some(type_text) =
        resolve_visible_name_type_text(workspace, document, position, receiver, 0)
    {
        for canonical in resolve_type_canonicals(workspace, document, &type_text) {
            push_unique(&mut owner_canonicals, canonical);
        }
    }

    owner_canonicals
        .into_iter()
        .flat_map(|owner_canonical| {
            signature_candidates_for_canonical(
                workspace,
                &format!("{owner_canonical}.{member_name}"),
            )
        })
        .collect()
}

pub(crate) fn signature_candidates_for_canonical(
    workspace: &WorkspaceState,
    canonical: &str,
) -> Vec<SignatureCandidate> {
    let Some(declaration) = workspace.declarations_by_canonical.get(canonical) else {
        return Vec::new();
    };
    let Some(document) = workspace.queries.documents_by_uri.get(&declaration.uri) else {
        return Vec::new();
    };
    let Some((owner_canonical, member_name)) = canonical.rsplit_once('.') else {
        return Vec::new();
    };
    if workspace.declarations_by_canonical.contains_key(owner_canonical) {
        let owner_name =
            owner_canonical.rsplit_once('.').map(|(_, name)| name).unwrap_or(owner_canonical);
        return class_member_signature_candidates(document, owner_name, member_name);
    }
    top_level_signature_candidates(document, member_name)
}

pub(crate) fn top_level_signature_candidates(
    document: &DocumentState,
    name: &str,
) -> Vec<SignatureCandidate> {
    let signatures = document
        .syntax
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::FunctionDef(function) | SyntaxStatement::OverloadDef(function)
                if function.name == name =>
            {
                Some(signature_candidate(
                    &function.name,
                    &function.params,
                    function.returns.as_deref(),
                    false,
                ))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if !signatures.is_empty() {
        return signatures;
    }

    document
        .syntax
        .statements
        .iter()
        .find_map(|statement| match statement {
            SyntaxStatement::Interface(class_like)
            | SyntaxStatement::DataClass(class_like)
            | SyntaxStatement::SealedClass(class_like)
            | SyntaxStatement::ClassDef(class_like)
                if class_like.name == name =>
            {
                Some(class_constructor_signature_candidate(class_like))
            }
            _ => None,
        })
        .into_iter()
        .collect()
}

pub(crate) fn class_member_signature_candidates(
    document: &DocumentState,
    owner_name: &str,
    member_name: &str,
) -> Vec<SignatureCandidate> {
    document
        .syntax
        .statements
        .iter()
        .find_map(|statement| match statement {
            SyntaxStatement::Interface(class_like)
            | SyntaxStatement::DataClass(class_like)
            | SyntaxStatement::SealedClass(class_like)
            | SyntaxStatement::ClassDef(class_like)
                if class_like.name == owner_name =>
            {
                Some(
                    class_like
                        .members
                        .iter()
                        .filter(|member| member.name == member_name)
                        .filter(|member| member.kind != typepython_syntax::ClassMemberKind::Field)
                        .map(|member| {
                            let drop_first = member
                                .method_kind
                                .is_some_and(|kind| kind != typepython_syntax::MethodKind::Static);
                            signature_candidate(
                                &format!("{owner_name}.{}", member.name),
                                &member.params,
                                member.returns.as_deref(),
                                drop_first,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            }
            _ => None,
        })
        .unwrap_or_default()
}

pub(crate) fn class_constructor_signature_candidate(
    class_like: &NamedBlockStatement,
) -> SignatureCandidate {
    let init_signatures = class_like
        .members
        .iter()
        .filter(|member| member.name == "__init__")
        .filter(|member| member.kind != typepython_syntax::ClassMemberKind::Field)
        .map(|member| {
            let drop_first = member
                .method_kind
                .is_some_and(|kind| kind != typepython_syntax::MethodKind::Static);
            signature_candidate(&class_like.name, &member.params, Some("None"), drop_first)
        })
        .collect::<Vec<_>>();
    if let Some(signature) = init_signatures.into_iter().next() {
        return signature;
    }

    let field_params = class_like
        .members
        .iter()
        .filter(|member| {
            member.kind == typepython_syntax::ClassMemberKind::Field && !member.is_class_var
        })
        .map(|member| typepython_syntax::FunctionParam {
            name: member.name.clone(),
            annotation: member.rendered_annotation().or_else(|| member.rendered_value_type()),
            annotation_expr: member.annotation_expr.clone().or_else(|| {
                member.rendered_annotation().as_deref().and_then(typepython_syntax::TypeExpr::parse)
            }),
            has_default: false,
            positional_only: false,
            keyword_only: false,
            variadic: false,
            keyword_variadic: false,
        })
        .collect::<Vec<_>>();
    signature_candidate(&class_like.name, &field_params, Some(&class_like.name), false)
}

pub(crate) fn signature_candidate(
    name: &str,
    params: &[typepython_syntax::FunctionParam],
    returns: Option<&str>,
    drop_first: bool,
) -> SignatureCandidate {
    let shown_params = if drop_first {
        params.iter().skip(1).collect::<Vec<_>>()
    } else {
        params.iter().collect::<Vec<_>>()
    };
    let parameter_labels =
        shown_params.iter().map(|param| render_parameter_label(param)).collect::<Vec<_>>();
    SignatureCandidate {
        info: LspSignatureInformation {
            label: format!(
                "{}({}){}",
                name,
                parameter_labels.join(", "),
                returns.map(|returns| format!(" -> {returns}")).unwrap_or_default()
            ),
            parameters: parameter_labels
                .into_iter()
                .map(|label| LspParameterInformation { label })
                .collect(),
        },
        params: shown_params.into_iter().cloned().collect(),
    }
}

pub(crate) fn render_parameter_label(param: &typepython_syntax::FunctionParam) -> String {
    let mut label = String::new();
    if param.keyword_variadic {
        label.push_str("**");
    } else if param.variadic {
        label.push('*');
    }
    label.push_str(&param.name);
    if let Some(annotation) = &param.annotation {
        label.push_str(": ");
        label.push_str(annotation);
    }
    if param.has_default {
        label.push_str(" = ...");
    }
    label
}

pub(crate) fn active_call_site(
    document: &DocumentState,
    position: LspPosition,
    callee: &str,
) -> Option<ActiveCallSite> {
    let line = position.line as usize + 1;
    let normalized = strip_generic_args(callee).trim();
    if let Some((owner_name, method_name)) = normalized.rsplit_once('.') {
        let call = document.syntax.statements.iter().find_map(|statement| match statement {
            SyntaxStatement::MethodCall(method_call)
                if method_call.line == line
                    && method_call.owner_name == owner_name
                    && method_call.method == method_name =>
            {
                Some(method_call)
            }
            _ => None,
        })?;
        return Some(ActiveCallSite {
            arg_types: rendered_arg_types(&call.arg_values),
            keyword_names: call.keyword_names.clone(),
            keyword_arg_types: rendered_arg_types(&call.keyword_arg_values),
        });
    }

    let call = document.syntax.statements.iter().find_map(|statement| match statement {
        SyntaxStatement::Call(call) if call.line == line && call.callee == normalized => Some(call),
        _ => None,
    })?;
    Some(ActiveCallSite {
        arg_types: rendered_arg_types(&call.arg_values),
        keyword_names: call.keyword_names.clone(),
        keyword_arg_types: rendered_arg_types(&call.keyword_arg_values),
    })
}

pub(crate) fn select_active_signature(
    candidates: &[SignatureCandidate],
    active_parameter: usize,
    call_site: Option<&ActiveCallSite>,
) -> usize {
    candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| {
            signature_match_sort_key(candidate, active_parameter, call_site)
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn signature_match_sort_key(
    candidate: &SignatureCandidate,
    active_parameter: usize,
    call_site: Option<&ActiveCallSite>,
) -> (usize, usize, usize, Reverse<usize>, usize) {
    let last_parameter_index = candidate.info.parameters.len().saturating_sub(1);
    let (mismatches, exact_matches) = call_site
        .map(|call_site| signature_type_match_score(candidate, call_site))
        .unwrap_or((0, 0));
    (
        usize::from(active_parameter > last_parameter_index),
        active_parameter.abs_diff(last_parameter_index),
        mismatches,
        Reverse(exact_matches),
        last_parameter_index,
    )
}

fn signature_type_match_score(
    candidate: &SignatureCandidate,
    call_site: &ActiveCallSite,
) -> (usize, usize) {
    let positional_params = candidate
        .params
        .iter()
        .filter(|param| !param.keyword_only && !param.keyword_variadic)
        .collect::<Vec<_>>();
    let variadic_param = candidate.params.iter().find(|param| param.variadic);
    let keyword_variadic_param = candidate.params.iter().find(|param| param.keyword_variadic);

    let mut mismatches = 0usize;
    let mut exact_matches = 0usize;

    for (index, actual) in call_site.arg_types.iter().enumerate() {
        let expected = positional_params.get(index).copied().or(variadic_param);
        score_signature_type_match(
            expected.and_then(|param| param.annotation.as_deref()),
            actual,
            &mut mismatches,
            &mut exact_matches,
        );
    }

    for (keyword, actual) in call_site.keyword_names.iter().zip(&call_site.keyword_arg_types) {
        let expected = candidate
            .params
            .iter()
            .find(|param| param.name == *keyword && !param.positional_only)
            .or(keyword_variadic_param);
        score_signature_type_match(
            expected.and_then(|param| param.annotation.as_deref()),
            actual,
            &mut mismatches,
            &mut exact_matches,
        );
    }

    (mismatches, exact_matches)
}

fn score_signature_type_match(
    expected: Option<&str>,
    actual: &str,
    mismatches: &mut usize,
    exact_matches: &mut usize,
) {
    if actual.trim().is_empty() {
        return;
    }
    let Some(expected) = expected.map(str::trim).filter(|expected| !expected.is_empty()) else {
        return;
    };
    if expected == actual.trim() {
        *exact_matches += 1;
    } else {
        *mismatches += 1;
    }
}
