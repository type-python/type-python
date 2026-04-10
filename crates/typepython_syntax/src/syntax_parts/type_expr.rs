use super::*;

#[derive(Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TypeExpr {
    Name(String),
    Generic { head: String, args: Vec<TypeExpr> },
    Callable { params: Box<CallableParamExpr>, return_type: Box<TypeExpr> },
    Union { branches: Vec<TypeExpr>, style: UnionStyle },
    Annotated { value: Box<TypeExpr>, metadata: Vec<String> },
    Unpack(Box<TypeExpr>),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum UnionStyle {
    Explicit,
    Shorthand,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CallableParamExpr {
    Ellipsis,
    ParamList(Vec<TypeExpr>),
    Concatenate(Vec<TypeExpr>),
    Single(Box<TypeExpr>),
}

impl TypeExpr {
    pub fn parse(text: &str) -> Option<Self> {
        let normalized = normalize_source_variadic_type_syntax(text);
        parse_type_expr(&normalized)
    }

    pub fn render(&self) -> String {
        match self {
            Self::Name(name) => normalize_name(name),
            Self::Generic { head, args } => {
                let head = normalize_type_head(head);
                let args = args.iter().map(Self::render).collect::<Vec<_>>().join(", ");
                format!("{head}[{args}]")
            }
            Self::Callable { params, return_type } => {
                format!("Callable[{}, {}]", params.render(), return_type.render())
            }
            Self::Union { branches, style } => match style {
                UnionStyle::Explicit => {
                    let branches = branches.iter().map(Self::render).collect::<Vec<_>>().join(", ");
                    format!("Union[{branches}]")
                }
                UnionStyle::Shorthand => {
                    branches.iter().map(Self::render).collect::<Vec<_>>().join(" | ")
                }
            },
            Self::Annotated { value, metadata } => {
                if metadata.is_empty() {
                    format!("Annotated[{}]", value.render())
                } else {
                    format!("Annotated[{}, {}]", value.render(), metadata.join(", "))
                }
            }
            Self::Unpack(inner) => format!("Unpack[{}]", inner.render()),
        }
    }
}

impl CallableParamExpr {
    pub fn render(&self) -> String {
        match self {
            Self::Ellipsis => String::from("..."),
            Self::ParamList(types) => {
                let types = types.iter().map(TypeExpr::render).collect::<Vec<_>>().join(", ");
                format!("[{types}]")
            }
            Self::Concatenate(types) => {
                let types = types.iter().map(TypeExpr::render).collect::<Vec<_>>().join(", ");
                format!("Concatenate[{types}]")
            }
            Self::Single(expr) => expr.render(),
        }
    }
}

pub fn parse_callable_annotation(text: &str) -> Option<(Option<Vec<String>>, String)> {
    let (params, return_type) = parse_callable_annotation_parts(text)?;
    if params == "..." {
        return Some((None, return_type));
    }
    let params = params.strip_prefix('[').and_then(|inner| inner.strip_suffix(']'))?;
    let param_types = if params.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level_type_args(params).into_iter().map(normalize_type_text).collect()
    };
    Some((Some(param_types), return_type))
}

pub fn parse_callable_annotation_parts(text: &str) -> Option<(String, String)> {
    match TypeExpr::parse(text)? {
        TypeExpr::Callable { params, return_type } => Some((params.render(), return_type.render())),
        _ => None,
    }
}

pub fn normalize_callable_param_expr(params: &str) -> String {
    parse_callable_param_expr(params)
        .map(|params| params.render())
        .unwrap_or_else(|| normalize_type_text_legacy(params))
}

pub fn normalize_type_text(text: &str) -> String {
    let normalized_source = normalize_source_variadic_type_syntax(text);
    let text = normalized_source.trim();
    if text.is_empty() {
        return String::new();
    }
    TypeExpr::parse(text)
        .map(|expr| expr.render())
        .unwrap_or_else(|| normalize_type_text_legacy(text))
}

pub fn union_branches(text: &str) -> Option<Vec<String>> {
    match TypeExpr::parse(text)? {
        TypeExpr::Annotated { value, .. } => {
            union_branches(&value.render()).or(Some(vec![value.render()]))
        }
        TypeExpr::Generic { head, args } if head == "Optional" && args.len() == 1 => {
            Some(vec![args[0].render(), String::from("None")])
        }
        TypeExpr::Union { branches, .. } => {
            Some(branches.into_iter().map(|branch| branch.render()).collect())
        }
        _ => None,
    }
}

pub fn split_top_level_union_branches(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

pub fn annotated_inner(text: &str) -> Option<String> {
    match TypeExpr::parse(text)? {
        TypeExpr::Annotated { value, .. } => Some(value.render()),
        _ => None,
    }
}

pub fn unpack_inner(text: &str) -> Option<String> {
    match TypeExpr::parse(text)? {
        TypeExpr::Unpack(inner) => Some(inner.render()),
        _ => None,
    }
}

pub fn normalize_type_head(head: &str) -> &str {
    match head.trim() {
        "List" => "list",
        "Dict" => "dict",
        "Tuple" => "tuple",
        "Set" => "set",
        "FrozenSet" => "frozenset",
        "Type" => "type",
        "Callable" => "Callable",
        "Literal" => "Literal",
        "NewType" => "NewType",
        other => other,
    }
}

pub fn split_top_level_type_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;

    for (index, ch) in args.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }

    let tail = args[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }

    parts
}

pub(super) fn parse_type_expr(text: &str) -> Option<TypeExpr> {
    let text = normalize_name(text);
    if text.is_empty() {
        return None;
    }

    let union_branches = split_top_level_union_branches(&text);
    if union_branches.len() > 1 {
        let branches = union_branches
            .into_iter()
            .map(|branch| parse_type_expr(branch).unwrap_or(TypeExpr::Name(normalize_name(branch))))
            .collect();
        return Some(TypeExpr::Union { branches, style: UnionStyle::Shorthand });
    }

    if let Some(inner) = text.strip_prefix("Annotated[").and_then(|inner| inner.strip_suffix(']')) {
        let mut args = split_top_level_type_args(inner).into_iter();
        let value = parse_type_expr(args.next()?)?;
        let metadata = args.map(normalize_type_text_legacy).collect();
        return Some(TypeExpr::Annotated { value: Box::new(value), metadata });
    }

    if let Some(inner) = text.strip_prefix("Unpack[").and_then(|inner| inner.strip_suffix(']')) {
        return Some(TypeExpr::Unpack(Box::new(parse_type_expr(inner)?)));
    }

    if let Some(inner) = text.strip_prefix("Callable[").and_then(|inner| inner.strip_suffix(']')) {
        let args = split_top_level_type_args(inner);
        if args.len() == 2 {
            return Some(TypeExpr::Callable {
                params: Box::new(parse_callable_param_expr(args[0])?),
                return_type: Box::new(parse_type_expr(args[1])?),
            });
        }
    }

    if let Some(inner) = text.strip_prefix("Union[").and_then(|inner| inner.strip_suffix(']')) {
        let branches = split_top_level_type_args(inner)
            .into_iter()
            .map(|branch| parse_type_expr(branch).unwrap_or(TypeExpr::Name(normalize_name(branch))))
            .collect();
        return Some(TypeExpr::Union { branches, style: UnionStyle::Explicit });
    }

    if let Some(open_index) = text.find('[')
        && let Some(inner) = text.strip_suffix(']')
    {
        let head = normalize_type_head(&inner[..open_index]).to_owned();
        let args = split_top_level_type_args(&inner[open_index + 1..])
            .into_iter()
            .map(|arg| parse_type_expr(arg).unwrap_or(TypeExpr::Name(normalize_name(arg))))
            .collect();
        return Some(TypeExpr::Generic { head, args });
    }

    Some(TypeExpr::Name(text))
}

pub(super) fn parse_callable_param_expr(text: &str) -> Option<CallableParamExpr> {
    let text = normalize_name(text);
    if text.is_empty() {
        return None;
    }
    if text == "..." {
        return Some(CallableParamExpr::Ellipsis);
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')) {
        let params = if inner.trim().is_empty() {
            Vec::new()
        } else {
            split_top_level_type_args(inner)
                .into_iter()
                .map(|arg| parse_type_expr(arg).unwrap_or(TypeExpr::Name(normalize_name(arg))))
                .collect()
        };
        return Some(CallableParamExpr::ParamList(params));
    }
    if let Some(inner) = text.strip_prefix("Concatenate[").and_then(|inner| inner.strip_suffix(']'))
    {
        let params = split_top_level_type_args(inner)
            .into_iter()
            .map(|arg| parse_type_expr(arg).unwrap_or(TypeExpr::Name(normalize_name(arg))))
            .collect();
        return Some(CallableParamExpr::Concatenate(params));
    }
    Some(CallableParamExpr::Single(Box::new(parse_type_expr(&text)?)))
}

pub(super) fn normalize_name(text: &str) -> String {
    let trimmed = text.trim();
    trimmed
        .strip_prefix("typing.")
        .or_else(|| trimmed.strip_prefix("typing_extensions."))
        .unwrap_or(trimmed)
        .trim()
        .to_owned()
}

pub(super) fn normalize_type_text_legacy(text: &str) -> String {
    let text = normalize_name(text);
    if text.is_empty() {
        return text;
    }

    if let Some(open_index) = text.find('[')
        && let Some(inner) = text.strip_suffix(']')
    {
        let head = normalize_type_head(&inner[..open_index]);
        let args = split_top_level_type_args(&inner[open_index + 1..])
            .into_iter()
            .map(normalize_type_text_legacy)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{head}[{args}]");
    }

    normalize_type_head(&text).to_owned()
}

#[cfg(test)]
mod type_expr_tests {
    use super::{
        CallableParamExpr, TypeExpr, UnionStyle, annotated_inner, normalize_callable_param_expr,
        normalize_type_text, parse_callable_annotation_parts, union_branches,
    };

    #[test]
    fn type_expr_parses_shorthand_union_without_reformatting_to_explicit_union() {
        let parsed = TypeExpr::parse("int | typing.List[str]").expect("parsed union");
        assert_eq!(
            parsed,
            TypeExpr::Union {
                branches: vec![
                    TypeExpr::Name(String::from("int")),
                    TypeExpr::Generic {
                        head: String::from("list"),
                        args: vec![TypeExpr::Name(String::from("str"))],
                    },
                ],
                style: UnionStyle::Shorthand,
            }
        );
        assert_eq!(parsed.render(), "int | list[str]");
    }

    #[test]
    fn normalize_type_text_normalizes_nested_generics_through_type_expr_ir() {
        assert_eq!(
            normalize_type_text("typing.Dict[str, typing.List[int | None]]"),
            "dict[str, list[int | None]]"
        );
    }

    #[test]
    fn parse_callable_annotation_parts_normalizes_params_and_return_type() {
        assert_eq!(
            parse_callable_annotation_parts(
                "typing.Callable[Concatenate[typing.List[int], P], Tuple[str, int]]"
            ),
            Some((String::from("Concatenate[list[int], P]"), String::from("tuple[str, int]")))
        );
    }

    #[test]
    fn callable_param_expr_normalization_preserves_supported_forms() {
        assert_eq!(normalize_callable_param_expr("[typing.List[int], P]"), "[list[int], P]");
        assert_eq!(normalize_callable_param_expr("Concatenate[int, P]"), "Concatenate[int, P]");
        assert_eq!(normalize_callable_param_expr("..."), "...");
        assert_eq!(normalize_callable_param_expr("P"), "P");
    }

    #[test]
    fn annotated_inner_and_union_branches_use_type_expr_ir() {
        assert_eq!(
            annotated_inner("Annotated[typing.Tuple[int, str], tag]"),
            Some(String::from("tuple[int, str]"))
        );
        assert_eq!(
            union_branches("Annotated[int | None, tag]"),
            Some(vec![String::from("int"), String::from("None")])
        );
        assert_eq!(
            union_branches("Optional[typing.List[int]]"),
            Some(vec![String::from("list[int]"), String::from("None")])
        );
    }

    #[test]
    fn callable_param_expr_single_variant_round_trips() {
        let parsed = CallableParamExpr::Single(Box::new(TypeExpr::Name(String::from("P"))));
        assert_eq!(parsed.render(), "P");
    }
}
