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

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TypeLevelShapeField {
    pub owner: String,
    pub name: String,
    pub annotation: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ShapeProjectionSourceKind {
    TypedDict,
    DataClass,
    FrameworkTransform,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ShapeProjectionFieldSourceKind {
    SourceField,
    ProjectionGenerated,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ShapeProjectionField {
    pub name: String,
    pub public_alias: String,
    pub annotation: Option<String>,
    pub required: bool,
    pub readonly: bool,
    pub source_kind: ShapeProjectionFieldSourceKind,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ShapeProjection {
    pub name: String,
    pub source_kind: ShapeProjectionSourceKind,
    pub fields: Vec<ShapeProjectionField>,
}

impl ShapeProjectionField {
    pub fn from_class_member(member: &ClassMember) -> Self {
        let annotation = member
            .annotation
            .clone()
            .or_else(|| member.annotation_expr.as_ref().map(TypeExpr::render));
        Self {
            name: member.name.clone(),
            public_alias: member.name.clone(),
            required: !annotation.as_deref().is_some_and(shape_annotation_is_notrequired),
            readonly: annotation.as_deref().is_some_and(shape_annotation_is_readonly),
            annotation,
            source_kind: ShapeProjectionFieldSourceKind::SourceField,
        }
    }

    pub fn projected(mut self) -> Self {
        self.source_kind = ShapeProjectionFieldSourceKind::ProjectionGenerated;
        self
    }

    pub fn normalized_annotation(&self) -> String {
        let mut rendered = self.annotation.as_deref().unwrap_or("object").trim().to_owned();
        loop {
            if let Some(inner) = bracket_inner(&rendered, "NotRequired") {
                rendered = inner.trim().to_owned();
            } else if let Some(inner) = bracket_inner(&rendered, "Required") {
                rendered = inner.trim().to_owned();
            } else if let Some(inner) = bracket_inner(&rendered, "Required_") {
                rendered = inner.trim().to_owned();
            } else if let Some(inner) = bracket_inner(&rendered, "ReadOnly") {
                rendered = inner.trim().to_owned();
            } else {
                break;
            }
        }
        rendered
    }
}

impl ShapeProjection {
    pub fn from_named_block(
        block: &NamedBlockStatement,
        source_kind: ShapeProjectionSourceKind,
    ) -> Self {
        Self {
            name: block.name.clone(),
            source_kind,
            fields: block
                .members
                .iter()
                .filter(|member| member.kind == ClassMemberKind::Field)
                .map(ShapeProjectionField::from_class_member)
                .collect(),
        }
    }

    pub fn materialize_members(&self) -> Vec<ClassMember> {
        self.fields
            .iter()
            .map(|field| ClassMember {
                kind: ClassMemberKind::Field,
                name: field.name.clone(),
                annotation: field.annotation.clone(),
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
                method_kind: None,
                line: 0,
            })
            .collect()
    }

    pub fn partial(&self) -> Self {
        self.with_projected_fields(self.fields.iter().cloned().map(|mut field| {
            let annotation = field.annotation.as_deref().unwrap_or("object");
            field.annotation = Some(if annotation.contains("NotRequired[") {
                annotation.to_owned()
            } else if annotation.starts_with("Required_[") {
                annotation.replace("Required_[", "NotRequired[")
            } else {
                format!("NotRequired[{annotation}]")
            });
            field.required = false;
            field.projected()
        }))
    }

    pub fn required_fields(&self) -> Self {
        self.with_projected_fields(self.fields.iter().cloned().map(|mut field| {
            let annotation = field.annotation.as_deref().unwrap_or("object");
            field.annotation = Some(
                bracket_inner(annotation, "NotRequired")
                    .map(str::trim)
                    .unwrap_or(annotation)
                    .to_owned(),
            );
            field.required = true;
            field.projected()
        }))
    }

    pub fn readonly_fields(&self) -> Self {
        self.with_projected_fields(self.fields.iter().cloned().map(|mut field| {
            let annotation = field.annotation.as_deref().unwrap_or("object");
            field.annotation = Some(if annotation.contains("ReadOnly[") {
                annotation.to_owned()
            } else {
                format!("ReadOnly[{annotation}]")
            });
            field.readonly = true;
            field.projected()
        }))
    }

    pub fn mutable_fields(&self) -> Self {
        self.with_projected_fields(self.fields.iter().cloned().map(|mut field| {
            let annotation = field.annotation.as_deref().unwrap_or("object");
            field.annotation = Some(
                bracket_inner(annotation, "ReadOnly")
                    .map(str::trim)
                    .unwrap_or(annotation)
                    .to_owned(),
            );
            field.readonly = false;
            field.projected()
        }))
    }

    pub fn pick(&self, names: &[&str]) -> Self {
        self.with_projected_fields(
            self.fields
                .iter()
                .filter(|field| {
                    names.iter().any(|name| *name == field.name || *name == field.public_alias)
                })
                .cloned()
                .map(ShapeProjectionField::projected),
        )
    }

    pub fn omit(&self, names: &[&str]) -> Self {
        self.with_projected_fields(
            self.fields
                .iter()
                .filter(|field| {
                    !names.iter().any(|name| *name == field.name || *name == field.public_alias)
                })
                .cloned()
                .map(ShapeProjectionField::projected),
        )
    }

    pub fn map_values(&self, wrapper: &str) -> Self {
        self.with_projected_fields(self.fields.iter().cloned().map(|mut field| {
            let annotation = field.annotation.as_deref().unwrap_or("object");
            field.annotation = Some(match wrapper.trim() {
                "Optional" => format!("Optional[{annotation}]"),
                "Readonly" => {
                    field.readonly = true;
                    if annotation.contains("ReadOnly[") {
                        annotation.to_owned()
                    } else {
                        format!("ReadOnly[{annotation}]")
                    }
                }
                _ => annotation.to_owned(),
            });
            field.projected()
        }))
    }

    fn with_projected_fields(
        &self,
        fields: impl IntoIterator<Item = ShapeProjectionField>,
    ) -> Self {
        Self {
            name: self.name.clone(),
            source_kind: self.source_kind,
            fields: fields.into_iter().collect(),
        }
    }
}

fn shape_annotation_is_notrequired(annotation: &str) -> bool {
    annotation.trim_start().starts_with("NotRequired[")
}

fn shape_annotation_is_readonly(annotation: &str) -> bool {
    annotation.trim_start().starts_with("ReadOnly[")
}

fn bracket_inner<'a>(expression: &'a str, transform: &str) -> Option<&'a str> {
    let prefix = format!("{transform}[");
    expression.trim().strip_prefix(&prefix)?.strip_suffix(']')
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

pub fn type_expr_is_assignable_text(expected: &str, actual: &str) -> bool {
    let expected = TypeExpr::parse(expected)
        .map(|expr| expr.render())
        .unwrap_or_else(|| normalize_type_text(expected));
    let actual = TypeExpr::parse(actual)
        .map(|expr| expr.render())
        .unwrap_or_else(|| normalize_type_text(actual));
    type_expr_is_assignable_normalized(&expected, &actual)
}

pub fn reduce_typeif_is_subtype_text(value: &str) -> Option<String> {
    let TypeExpr::Generic { head, args } = TypeExpr::parse(value.trim())? else {
        return None;
    };
    if head != "TypeIf" || args.len() != 3 {
        return None;
    }
    let TypeExpr::Generic { head: condition_head, args: condition_args } = &args[0] else {
        return None;
    };
    if condition_head != "IsSubtype" || condition_args.len() != 2 {
        return None;
    }
    Some(
        if type_expr_is_assignable_text(&condition_args[1].render(), &condition_args[0].render()) {
            args[1].render()
        } else {
            args[2].render()
        },
    )
}

pub fn reduce_key_set_alias_text(
    value: &str,
    fields: &[TypeLevelShapeField],
) -> Option<Vec<String>> {
    let TypeExpr::Generic { head, args } = TypeExpr::parse(value.trim())? else {
        return None;
    };
    if !matches!(head.as_str(), "KeyOf" | "RequiredKeys" | "OptionalKeys") || args.len() != 1 {
        return None;
    }
    let owner = args[0].render();
    let mut keys = fields
        .iter()
        .filter(|field| field.owner == owner)
        .filter(|field| match head.as_str() {
            "RequiredKeys" => !field
                .annotation
                .as_deref()
                .is_some_and(|ann| ann.trim_start().starts_with("NotRequired[")),
            "OptionalKeys" => field
                .annotation
                .as_deref()
                .is_some_and(|ann| ann.trim_start().starts_with("NotRequired[")),
            _ => true,
        })
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return None;
    }
    keys.sort();
    Some(keys)
}

pub fn contains_restricted_type_level_alias_text(value: &str) -> bool {
    TypeExpr::parse(value.trim()).is_some_and(|expr| contains_restricted_type_level_form(&expr))
}

fn contains_restricted_type_level_form(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Generic { head, args } => {
            matches!(
                head.as_str(),
                "TypeIf"
                    | "IsSubtype"
                    | "KeyOf"
                    | "Pick"
                    | "Omit"
                    | "RequiredKeys"
                    | "OptionalKeys"
                    | "MapValues"
            ) || args.iter().any(contains_restricted_type_level_form)
        }
        TypeExpr::Callable { params, return_type } => {
            contains_restricted_callable_params(params)
                || contains_restricted_type_level_form(return_type)
        }
        TypeExpr::Union { branches, .. } => {
            branches.iter().any(contains_restricted_type_level_form)
        }
        TypeExpr::Annotated { value, .. } | TypeExpr::Unpack(value) => {
            contains_restricted_type_level_form(value)
        }
        TypeExpr::Name(_) => false,
    }
}

fn contains_restricted_callable_params(params: &CallableParamExpr) -> bool {
    match params {
        CallableParamExpr::Ellipsis => false,
        CallableParamExpr::ParamList(params) | CallableParamExpr::Concatenate(params) => {
            params.iter().any(contains_restricted_type_level_form)
        }
        CallableParamExpr::Single(param) => contains_restricted_type_level_form(param),
    }
}

fn type_expr_is_assignable_normalized(expected: &str, actual: &str) -> bool {
    if expected == actual || matches!(expected, "Any" | "object") || actual == "Any" {
        return true;
    }
    if expected == "float" && actual == "int" {
        return true;
    }
    if let Some(expected_branches) = union_branches(expected) {
        if let Some(actual_branches) = union_branches(actual) {
            return actual_branches.iter().all(|actual_branch| {
                expected_branches.iter().any(|expected_branch| {
                    type_expr_is_assignable_normalized(expected_branch, actual_branch)
                })
            });
        }
        return expected_branches
            .iter()
            .any(|expected_branch| type_expr_is_assignable_normalized(expected_branch, actual));
    }
    if let Some(actual_branches) = union_branches(actual) {
        return actual_branches
            .iter()
            .all(|actual_branch| type_expr_is_assignable_normalized(expected, actual_branch));
    }
    match (TypeExpr::parse(expected), TypeExpr::parse(actual)) {
        (
            Some(TypeExpr::Generic { head: expected_head, args: expected_args }),
            Some(TypeExpr::Generic { head: actual_head, args: actual_args }),
        ) if normalize_type_head(&expected_head) == normalize_type_head(&actual_head)
            && expected_args.len() == actual_args.len() =>
        {
            expected_args
                .iter()
                .zip(actual_args.iter())
                .all(|(expected_arg, actual_arg)| expected_arg.render() == actual_arg.render())
        }
        _ => false,
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
        CallableParamExpr, ClassMember, ClassMemberKind, NamedBlockStatement, ShapeProjection,
        ShapeProjectionFieldSourceKind, ShapeProjectionSourceKind, TypeExpr, TypeLevelShapeField,
        UnionStyle, annotated_inner, normalize_callable_param_expr, normalize_type_text,
        parse_callable_annotation_parts, reduce_key_set_alias_text, reduce_typeif_is_subtype_text,
        type_expr_is_assignable_text, union_branches,
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
    fn type_expr_parses_variadic_unpack_after_unicode_whitespace() {
        let parsed = TypeExpr::parse("tuple[\u{a0}*Ts]").expect("parsed variadic tuple");
        assert_eq!(parsed.render(), "tuple[Unpack[Ts]]");
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

    #[test]
    fn type_expr_assignability_covers_top_and_union_cases() {
        assert!(type_expr_is_assignable_text("object", "int"));
        assert!(type_expr_is_assignable_text("int | str", "int"));
        assert!(!type_expr_is_assignable_text("int", "object"));
    }

    #[test]
    fn reduce_typeif_is_subtype_uses_type_expr_ir() {
        assert_eq!(
            reduce_typeif_is_subtype_text("TypeIf[IsSubtype[int, object], str, bytes]"),
            Some(String::from("str"))
        );
    }

    #[test]
    fn reduce_key_set_alias_uses_shared_shape_fields() {
        let fields = vec![
            TypeLevelShapeField {
                owner: String::from("User"),
                name: String::from("id"),
                annotation: Some(String::from("int")),
            },
            TypeLevelShapeField {
                owner: String::from("User"),
                name: String::from("nickname"),
                annotation: Some(String::from("NotRequired[str]")),
            },
        ];

        assert_eq!(
            reduce_key_set_alias_text("KeyOf[User]", &fields),
            Some(vec![String::from("id"), String::from("nickname")])
        );
        assert_eq!(
            reduce_key_set_alias_text("RequiredKeys[User]", &fields),
            Some(vec![String::from("id")])
        );
        assert_eq!(
            reduce_key_set_alias_text("OptionalKeys[User]", &fields),
            Some(vec![String::from("nickname")])
        );
    }

    #[test]
    fn shape_projection_substrate_applies_field_transforms() {
        let source = NamedBlockStatement {
            name: String::from("User"),
            type_params: Vec::new(),
            header_suffix: String::new(),
            bases: Vec::new(),
            is_final_decorator: false,
            is_deprecated: false,
            deprecation_message: None,
            is_abstract_class: false,
            members: vec![
                ClassMember {
                    kind: ClassMemberKind::Field,
                    name: String::from("id"),
                    annotation: Some(String::from("int")),
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
                    method_kind: None,
                    line: 2,
                },
                ClassMember {
                    kind: ClassMemberKind::Field,
                    name: String::from("name"),
                    annotation: Some(String::from("ReadOnly[str]")),
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
                    method_kind: None,
                    line: 3,
                },
            ],
            line: 1,
        };

        let projected =
            ShapeProjection::from_named_block(&source, ShapeProjectionSourceKind::TypedDict)
                .partial()
                .mutable_fields()
                .pick(&["name"]);

        assert_eq!(projected.fields.len(), 1);
        assert_eq!(projected.fields[0].name, "name");
        assert!(!projected.fields[0].required);
        assert!(!projected.fields[0].readonly);
        assert_eq!(projected.fields[0].normalized_annotation(), "str");
        assert_eq!(
            projected.fields[0].source_kind,
            ShapeProjectionFieldSourceKind::ProjectionGenerated
        );
    }
}
